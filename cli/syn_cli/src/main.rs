//! Syn: quality policy gate.
//!
//! Named after Old Norse "syn" — denial, refusal. The gatekeeper.
//!
//! Reads .qa sidecars (Saga's truth), filters via jq expressions (jaq-interpret),
//! returns decision (allow/warn/deny) + formatted output.
//!
//! Modes:
//!   syn --mode report <path>   — filter + format issues (informational)
//!   syn --mode gate <path>     — enforce policy, exit 0/1 (deterministic)
//!
//! Output:
//!   (default)   TOON when piped, --colored when tty
//!   --colored   ANSI terminal output
//!   --json      Machine-readable JSON
//!
//! Config:
//!   .syn/warn.toml   — noise filter (what's visible)
//!   .syn/deny.toml   — enforcement threshold (what blocks)

use std::io::{self, Read as IoRead};
use std::path::{Path, PathBuf};
use std::process;

use jaq_interpret::{Ctx, FilterT, ParseCtx, RcIter, Val};
use saga_runner::SanityReport;

use report_render_core::{CheckGroup, OutputMode, group_issues, total_issues, groups_to_json, format_output, severity_rank};

// =============================================================================
// Filter engine (jaq-interpret)
// =============================================================================

type CompiledFilter = jaq_interpret::Filter;

fn compile_filter(expr: &str) -> Result<CompiledFilter, String> {
    let mut defs = ParseCtx::new(Vec::new());
    let (parsed, errs) = jaq_parse::parse(expr, jaq_parse::main());
    if !errs.is_empty() {
        return Err(format!("jq parse error in '{}': {:?}", expr, errs));
    }
    let filter = defs.compile(parsed.ok_or_else(|| format!("jq parse returned None for '{}'", expr))?);
    if !defs.errs.is_empty() {
        return Err(format!("jq compile error in '{}': {} errors", expr, defs.errs.len()));
    }
    Ok(filter)
}

fn matches_filter(issue: &saga_runner::Issue, filter: &CompiledFilter) -> bool {
    let json = match serde_json::to_value(issue) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let inputs = RcIter::new(core::iter::empty());
    let val = Val::from(json);
    let results: Vec<_> = filter.run((Ctx::new([], &inputs), val)).collect();
    matches!(results.first(), Some(Ok(Val::Bool(true))))
}

// =============================================================================
// Config loading (.syn/warn.toml, .syn/deny.toml)
// =============================================================================

const DEFAULT_WARN_FILTER: &str = r#".tool == "gleipnir""#;
const DEFAULT_DENY_FILTER: &str = r#".severity == "blocked""#;

struct SynConfig {
    warn_filter: CompiledFilter,
    deny_filter: CompiledFilter,
    _warn_expr: String,
    _deny_expr: String,
}

fn load_filter_config(config_path: &Path, default_expr: &str) -> Result<(String, CompiledFilter), String> {
    let expr = if config_path.exists() {
        std::fs::read_to_string(config_path)
            .ok()
            .and_then(|content| {
                let table: toml::Table = content.parse().ok()?;
                table.get("filter")?.as_str().map(String::from)
            })
            .unwrap_or_else(|| default_expr.to_string())
    } else {
        default_expr.to_string()
    };

    let filter = compile_filter(&expr)
        .map_err(|e| format!("[syn] fatal: bad jq filter in {}: {}", config_path.display(), e))?;

    Ok((expr, filter))
}

fn load_config(project_dir: &Path) -> Result<SynConfig, String> {
    let syn_dir = project_dir.join(".syn");
    let (warn_expr, warn_filter) = load_filter_config(&syn_dir.join("warn.toml"), DEFAULT_WARN_FILTER)?;
    let (deny_expr, deny_filter) = load_filter_config(&syn_dir.join("deny.toml"), DEFAULT_DENY_FILTER)?;
    Ok(SynConfig { warn_filter, deny_filter, _warn_expr: warn_expr, _deny_expr: deny_expr })
}

// =============================================================================
// CLI args
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Report,
    Gate,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Target {
    Src,
    Tests,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Py,
    Rs,
    All,
}

struct Args {
    mode: Mode,
    output: OutputMode,
    silent: bool,
    stdin: bool,
    project_dir: Option<PathBuf>,
    target: Target,
    kind: Kind,
    tool_filter: Option<String>,
    level_filter: Option<String>,
    custom_filter: Option<String>,
    path: Option<PathBuf>,
}

fn is_tty() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stdout())
}

fn parse_args() -> Result<Args, String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();

    let mut mode = Mode::Report;
    let mut output = None;
    let mut silent = false;
    let mut stdin = false;
    let mut project_dir = None;
    let mut target = Target::Src;
    let mut kind = Kind::All;
    let mut tool_filter = None;
    let mut level_filter = None;
    let mut custom_filter = None;
    let mut positional: Vec<String> = Vec::new();

    let mut idx = 0;
    while idx < raw.len() {
        match raw[idx].as_str() {
            "--mode" => {
                idx += 1;
                match raw.get(idx).map(|s| s.as_str()) {
                    Some("report") => mode = Mode::Report,
                    Some("gate") => mode = Mode::Gate,
                    _ => {
                        return Err("[syn] --mode requires 'report' or 'gate'".to_string());
                    }
                }
            }
            "--output" => {
                idx += 1;
                output = Some(match raw.get(idx).map(|s| s.as_str()) {
                    Some("colored") => OutputMode::Colored,
                    Some("json") => OutputMode::Json,
                    Some("toon") => OutputMode::Toon,
                    _ => {
                        return Err("[syn] --output requires 'colored', 'json', or 'toon'".to_string());
                    }
                });
            }
            "--silent" => silent = true,
            "--stdin" => stdin = true,
            "--project-dir" => {
                idx += 1;
                project_dir = raw.get(idx).map(PathBuf::from);
            }
            "--target" => {
                idx += 1;
                target = match raw.get(idx).map(|s| s.as_str()) {
                    Some("src") => Target::Src,
                    Some("tests") => Target::Tests,
                    Some("all") => Target::All,
                    _ => {
                        return Err("[syn] --target requires 'src', 'tests', or 'all'".to_string());
                    }
                };
            }
            "--kind" => {
                idx += 1;
                kind = match raw.get(idx).map(|s| s.as_str()) {
                    Some("py") => Kind::Py,
                    Some("rs") => Kind::Rs,
                    Some("all") => Kind::All,
                    Some(other) => return Err(format!("[syn] unknown kind: {} (use py, rs, or all)", other)),
                    None => return Err("[syn] --kind requires a value (py, rs, or all)".into()),
                };
            }
            "--tool" => {
                idx += 1;
                tool_filter = raw.get(idx).cloned();
            }
            "--level" => {
                idx += 1;
                level_filter = raw.get(idx).cloned();
            }
            "--filter" => {
                idx += 1;
                custom_filter = raw.get(idx).cloned();
            }
            arg if arg.starts_with('-') => {
                return Err(format!("[syn] unknown flag: {}", arg));
            }
            arg => positional.push(arg.to_string()),
        }
        idx += 1;
    }

    if mode == Mode::Gate {
        if tool_filter.is_some() || level_filter.is_some() || custom_filter.is_some() {
            return Err("[syn] gate mode rejects --tool/--level/--filter (locked to config)".to_string());
        }
    }

    let output = output.unwrap_or_else(|| {
        if is_tty() { OutputMode::Colored } else { OutputMode::Toon }
    });

    let path = positional.first().map(PathBuf::from);

    Ok(Args { mode, output, silent, stdin, project_dir, target, kind, tool_filter, level_filter, custom_filter, path })
}

fn print_usage() {
    eprintln!(
        "syn — quality policy gate

USAGE:
    syn [options] [path]

MODES:
    --mode report    Informational (default). CLI overrides allowed.
    --mode gate      Deterministic policy gate. Locked to config files.
                     Rejects --tool/--level/--filter.

INPUT:
    <path>           File (.py/.rs finds sidecar), directory, or omit for cwd
    --stdin          Read .qa JSON from stdin
    --project-dir    Project root
    --target         [src|tests|all] Subtree scope (default: src)
    --kind           [py|rs|all] File types to include (default: all)

OUTPUT:
    --output         [colored|json|toon] (default: colored on tty, toon on pipe)
    --silent         Suppress Hlidskjalf broadcast

FILTERS (report mode only):
    --tool           [gleipnir|ruff|basedpyright|all]
    --level          [info|warning|error|blocked] and above
    --filter         '<jq expression>'"
    );
}

// =============================================================================
// Input discovery
// =============================================================================

fn matches_kind(qa_name: &str, kind: Kind) -> bool {
    match kind {
        Kind::All => true,
        Kind::Py => qa_name.ends_with(".py.qa"),
        Kind::Rs => qa_name.ends_with(".rs.qa"),
    }
}

fn find_qa_files(path: &Path, target: Target, kind: Kind) -> Vec<PathBuf> {
    if path.is_file() && path.extension().map_or(false, |e| e == "qa") {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if matches_kind(&name, kind) {
            return vec![path.to_path_buf()];
        }
        return Vec::new();
    }

    if path.is_file() {
        let sidecar = saga_runner::qa_path(path);
        if sidecar.exists() {
            return vec![sidecar];
        }
        return Vec::new();
    }

    let mut results = Vec::new();
    let skip = match target {
        Target::Src => Some("tests"),
        Target::Tests => Some("src"),
        Target::All => None,
    };
    let extra_skip: Vec<&str> = skip.into_iter().collect();
    saga_runner::walk_files(
        path,
        &extra_skip,
        &|name| name.ends_with(".qa") && name.starts_with('.') && matches_kind(name, kind),
        &mut results,
    );
    results
}

fn load_reports_from_stdin() -> Result<Vec<SanityReport>, String> {
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)
        .map_err(|_| "[syn] failed to read stdin".to_string())?;

    if let Ok(report) = serde_json::from_str::<SanityReport>(&buf) {
        return Ok(vec![report]);
    }
    if let Ok(reports) = serde_json::from_str::<Vec<SanityReport>>(&buf) {
        return Ok(reports);
    }

    Err("[syn] stdin is not valid .qa JSON".to_string())
}

// =============================================================================
// Three-tier filtering
// =============================================================================

struct FilteredOutput {
    warn_groups: Vec<CheckGroup>,
    deny_issues: usize,
    decision: &'static str,
}

fn apply_filters(
    reports: &[SanityReport],
    config: &SynConfig,
    args: &Args,
) -> FilteredOutput {
    let mut visible_reports: Vec<SanityReport> = Vec::new();
    let mut deny_count: usize = 0;

    let has_cli_overrides = args.mode == Mode::Report
        && (args.tool_filter.is_some() || args.level_filter.is_some() || args.custom_filter.is_some());

    let custom_compiled = args.custom_filter.as_ref().and_then(|expr| compile_filter(expr).ok());

    for report in reports {
        let mut visible_issues = Vec::new();
        for issue in &report.issues {
            if has_cli_overrides {
                if let Some(ref tool) = args.tool_filter {
                    if tool != "all" && issue.tool != *tool {
                        continue;
                    }
                }
                if let Some(ref level) = args.level_filter {
                    if severity_rank(&issue.severity) < severity_rank(level) {
                        continue;
                    }
                }
                if let Some(ref f) = custom_compiled {
                    if !matches_filter(issue, f) {
                        continue;
                    }
                }
            } else {
                if !matches_filter(issue, &config.warn_filter) {
                    continue;
                }
            }
            visible_issues.push(issue.clone());

            if matches_filter(issue, &config.deny_filter) {
                deny_count += 1;
            }
        }

        if !visible_issues.is_empty() {
            visible_reports.push(SanityReport {
                file: report.file.clone(),
                relative_path: report.relative_path.clone(),
                issues: visible_issues,
                ..Default::default()
            });
        }
    }

    let warn_groups = group_issues(&visible_reports);
    let decision = if deny_count > 0 && args.mode == Mode::Gate {
        "deny"
    } else if !warn_groups.is_empty() {
        "warn"
    } else {
        "allow"
    };

    FilteredOutput { warn_groups, deny_issues: deny_count, decision }
}

// =============================================================================
// Hlidskjalf broadcast
// =============================================================================

fn broadcast(groups: &[CheckGroup], decision: &str, deny_count: usize, workspace: String) {
    let payload = groups_to_json(groups);

    let datagram = datagram::Datagram {
        timestamp: datagram::now(),
        source: "syn".to_string(),
        kind: datagram::DatagramKind::Quality,
        classifier: Some("directory".into()),
        priority: match decision {
            "deny" => datagram::Priority::High,
            "warn" => datagram::Priority::Normal,
            _ => datagram::Priority::Low,
        },
        workspace,
        detail: Some(format!(
            "{} issues, {} deny, decision: {}",
            total_issues(groups), deny_count, decision
        )),
        speech: None,
        payload: Some(payload),
    };
    datagram::emit_validated_or_alert(&datagram, "syn");
}

// =============================================================================
// Main
// =============================================================================

fn run(args: &Args) -> Result<i32, String> {
    let project_dir = args.project_dir.clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let config = load_config(&project_dir)?;

    let reports = if args.stdin {
        load_reports_from_stdin()?
    } else {
        let path = args.path.as_deref().unwrap_or_else(|| Path::new("."));
        let qa_files = find_qa_files(path, args.target, args.kind);
        if qa_files.is_empty() {
            println!("{}", format_output(&[], args.output));
            return Ok(0);
        }
        qa_files.iter().filter_map(|qf| saga_runner::load_qa_file(qf)).collect()
    };

    if reports.is_empty() {
        println!("{}", format_output(&[], args.output));
        return Ok(0);
    }

    let result = apply_filters(&reports, &config, args);

    let output = format_output(&result.warn_groups, args.output);
    if !output.is_empty() {
        println!("{}", output);
    }

    if args.mode == Mode::Gate {
        let total = total_issues(&result.warn_groups);
        eprintln!(
            "[syn] {} — {} visible, {} deny",
            result.decision.to_uppercase(), total, result.deny_issues
        );
    }

    if !args.silent {
        let workspace = datagram::workspace_from_path(&project_dir);
        broadcast(&result.warn_groups, result.decision, result.deny_issues, workspace);
    }

    match result.decision {
        "deny" => Ok(1),
        _ => Ok(0),
    }
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        process::exit(0);
    }

    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(2);
        }
    };

    match run(&args) {
        Ok(code) => process::exit(code),
        Err(e) => {
            eprintln!("{}", e);
            process::exit(2);
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Test helpers
    // =========================================================================

    fn make_issue(tool: &str, code: &str, severity: &str, line: usize) -> saga_runner::Issue {
        saga_runner::Issue {
            tool: tool.into(),
            code: code.into(),
            severity: severity.into(),
            line,
            column: None,
            message: String::new(),
            category: String::new(),
            fixable: false,
            signal: String::new(),
            direction: String::new(),
            canary: String::new(),
        }
    }

    fn make_issue_fixable(tool: &str, code: &str, severity: &str, line: usize) -> saga_runner::Issue {
        saga_runner::Issue {
            fixable: true,
            ..make_issue(tool, code, severity, line)
        }
    }

    fn make_report(path: &str, issues: Vec<saga_runner::Issue>) -> saga_runner::SanityReport {
        saga_runner::SanityReport {
            file: path.into(),
            relative_path: path.into(),
            issues,
            ..Default::default()
        }
    }

    fn make_config(warn_expr: &str, deny_expr: &str) -> SynConfig {
        SynConfig {
            warn_filter: compile_filter(warn_expr).unwrap(),
            deny_filter: compile_filter(deny_expr).unwrap(),
            _warn_expr: warn_expr.into(),
            _deny_expr: deny_expr.into(),
        }
    }

    fn make_args_report() -> Args {
        Args {
            mode: Mode::Report,
            output: OutputMode::Json,
            silent: true,
            stdin: false,
            project_dir: None,
            target: Target::Src,
            kind: Kind::All,
            tool_filter: None,
            level_filter: None,
            custom_filter: None,
            path: None,
        }
    }

    fn make_args_gate() -> Args {
        Args {
            mode: Mode::Gate,
            output: OutputMode::Json,
            silent: true,
            stdin: false,
            project_dir: None,
            target: Target::Src,
            kind: Kind::All,
            tool_filter: None,
            level_filter: None,
            custom_filter: None,
            path: None,
        }
    }

    // =========================================================================
    // compile_filter
    // =========================================================================

    #[test]
    fn compile_filter_valid_tool_eq() {
        let result = compile_filter(r#".tool == "gleipnir""#);
        assert!(result.is_ok(), "valid jq filter should compile: {:?}", result.err());
    }

    #[test]
    fn compile_filter_valid_severity_eq() {
        let result = compile_filter(r#".severity == "blocked""#);
        assert!(result.is_ok(), "severity equality filter should compile");
    }

    #[test]
    fn compile_filter_invalid_syntax() {
        let result = compile_filter(".[[[");
        assert!(result.is_err(), "malformed jq expression should fail to compile");
    }

    #[test]
    fn compile_filter_complex_and_expression() {
        let result = compile_filter(r#".tool == "ruff" and .severity == "error""#);
        assert!(result.is_ok(), "complex 'and' filter should compile: {:?}", result.err());
    }

    #[test]
    fn compile_filter_boolean_field() {
        // jaq uses .fixable (identity truthy) rather than .fixable == true
        let result = compile_filter(".fixable");
        assert!(result.is_ok(), "boolean field filter should compile: {:?}", result.err());
    }

    #[test]
    fn compile_filter_string_interpolation() {
        // Test a filter pattern that uses string comparison — a real-world pattern
        let result = compile_filter(r#".category == "style""#);
        assert!(result.is_ok(), "category string comparison should compile: {:?}", result.err());
    }

    #[test]
    fn compile_filter_empty_string() {
        // An empty string is not valid jq
        let result = compile_filter("");
        // jaq may or may not parse empty — just verify it doesn't panic
        let _ = result;
    }

    // =========================================================================
    // matches_filter
    // =========================================================================

    #[test]
    fn matches_filter_tool_eq_matches() {
        let filter = compile_filter(r#".tool == "gleipnir""#).unwrap();
        let issue = make_issue("gleipnir", "G001", "error", 10);
        assert!(matches_filter(&issue, &filter), "gleipnir issue should match .tool == gleipnir");
    }

    #[test]
    fn matches_filter_tool_eq_no_match() {
        let filter = compile_filter(r#".tool == "gleipnir""#).unwrap();
        let issue = make_issue("ruff", "E501", "warning", 10);
        assert!(!matches_filter(&issue, &filter), "ruff issue should NOT match .tool == gleipnir");
    }

    #[test]
    fn matches_filter_severity_eq_matches() {
        let filter = compile_filter(r#".severity == "error""#).unwrap();
        let issue = make_issue("ruff", "E501", "error", 10);
        assert!(matches_filter(&issue, &filter), "error issue should match .severity == error");
    }

    #[test]
    fn matches_filter_severity_eq_no_match() {
        let filter = compile_filter(r#".severity == "blocked""#).unwrap();
        let issue = make_issue("ruff", "E501", "warning", 10);
        assert!(!matches_filter(&issue, &filter), "warning issue should NOT match .severity == blocked");
    }

    #[test]
    fn matches_filter_fixable_true() {
        // jaq: .fixable is truthy when true
        let filter = compile_filter(".fixable").unwrap();
        let issue = make_issue_fixable("ruff", "E501", "warning", 10);
        assert!(matches_filter(&issue, &filter), "fixable issue should match .fixable");
    }

    #[test]
    fn matches_filter_fixable_false_no_match() {
        let filter = compile_filter(".fixable").unwrap();
        let issue = make_issue("ruff", "E501", "warning", 10);
        assert!(!matches_filter(&issue, &filter), "non-fixable issue should NOT match .fixable");
    }

    #[test]
    fn matches_filter_complex_and_both_match() {
        let filter = compile_filter(r#".tool == "ruff" and .severity == "error""#).unwrap();
        let issue = make_issue("ruff", "E501", "error", 10);
        assert!(matches_filter(&issue, &filter), "ruff+error should match compound filter");
    }

    #[test]
    fn matches_filter_complex_and_one_fails() {
        let filter = compile_filter(r#".tool == "ruff" and .severity == "error""#).unwrap();
        let issue = make_issue("ruff", "E501", "warning", 10);
        assert!(!matches_filter(&issue, &filter), "ruff+warning should NOT match ruff+error filter");
    }

    #[test]
    fn matches_filter_or_expression() {
        let filter = compile_filter(r#".tool == "ruff" or .tool == "gleipnir""#).unwrap();
        let ruff_issue = make_issue("ruff", "E501", "warning", 1);
        let gleipnir_issue = make_issue("gleipnir", "G001", "error", 1);
        let other_issue = make_issue("basedpyright", "BP001", "info", 1);
        assert!(matches_filter(&ruff_issue, &filter));
        assert!(matches_filter(&gleipnir_issue, &filter));
        assert!(!matches_filter(&other_issue, &filter));
    }

    #[test]
    fn matches_filter_line_number() {
        let filter = compile_filter(".line > 50").unwrap();
        let issue_above = make_issue("ruff", "E501", "warning", 100);
        let issue_below = make_issue("ruff", "E501", "warning", 10);
        assert!(matches_filter(&issue_above, &filter));
        assert!(!matches_filter(&issue_below, &filter));
    }

    // =========================================================================
    // load_filter_config
    // =========================================================================

    #[test]
    fn load_filter_config_nonexistent_path_uses_default() {
        let path = std::path::Path::new("/nonexistent/path/warn.toml");
        let default_expr = r#".tool == "gleipnir""#;
        let (expr, _filter) = load_filter_config(path, default_expr).unwrap();
        assert_eq!(expr, default_expr, "non-existent config should fall back to default expression");
    }

    #[test]
    fn load_filter_config_valid_toml_file() {
        let dir = std::env::temp_dir().join(format!("syn_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("warn.toml");
        std::fs::write(&config_path, r#"filter = '.tool == "ruff"'"#).unwrap();

        let (expr, _filter) = load_filter_config(&config_path, DEFAULT_WARN_FILTER).unwrap();
        assert_eq!(expr, r#".tool == "ruff""#, "should read filter from TOML file");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_filter_config_toml_missing_filter_key_uses_default() {
        let dir = std::env::temp_dir().join(format!("syn_test_nokey_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("warn.toml");
        std::fs::write(&config_path, "something_else = 42\n").unwrap();

        let (expr, _filter) = load_filter_config(&config_path, DEFAULT_WARN_FILTER).unwrap();
        assert_eq!(expr, DEFAULT_WARN_FILTER, "TOML without 'filter' key should fall back to default");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_filter_config_invalid_jq_in_file_returns_error() {
        let dir = std::env::temp_dir().join(format!("syn_test_badjq_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("deny.toml");
        std::fs::write(&config_path, r#"filter = '.[[['"#).unwrap();

        let result = load_filter_config(&config_path, DEFAULT_DENY_FILTER);
        assert!(result.is_err(), "invalid jq in config file should return error");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // =========================================================================
    // apply_filters — THE core policy function
    // =========================================================================

    // --- Warn filter only shows matching issues ---

    #[test]
    fn apply_filters_warn_filter_selects_matching_tool() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let args = make_args_report();

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("gleipnir", "G001", "error", 10),
                make_issue("ruff", "E501", "warning", 20),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        let total = report_render_core::total_issues(&result.warn_groups);
        assert_eq!(total, 1, "warn filter .tool==gleipnir should show only gleipnir issues");
        assert_eq!(result.warn_groups[0].tool, "gleipnir");
    }

    #[test]
    fn apply_filters_warn_filter_excludes_nonmatching() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let args = make_args_report();

        // All ruff issues — none match warn filter
        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("ruff", "E501", "warning", 10),
                make_issue("ruff", "F401", "error", 20),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        assert!(result.warn_groups.is_empty(), "no issues match warn filter, groups should be empty");
        assert_eq!(result.decision, "allow");
    }

    // --- Deny filter counts subset ---

    #[test]
    fn apply_filters_deny_filter_counts_blocked() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let args = make_args_report();

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("gleipnir", "G001", "blocked", 10),
                make_issue("gleipnir", "G002", "error", 20),
                make_issue("gleipnir", "G003", "blocked", 30),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        assert_eq!(result.deny_issues, 2, "two blocked issues should yield deny_issues=2");
    }

    #[test]
    fn apply_filters_deny_filter_zero_when_no_match() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let args = make_args_report();

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("gleipnir", "G001", "error", 10),
                make_issue("gleipnir", "G002", "warning", 20),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        assert_eq!(result.deny_issues, 0, "no blocked issues should yield deny_issues=0");
    }

    // --- Gate mode with deny issues → decision "deny" ---

    #[test]
    fn apply_filters_gate_mode_deny_issues_returns_deny() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let args = make_args_gate();

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("gleipnir", "G001", "blocked", 10),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        assert_eq!(result.decision, "deny", "gate mode with blocked issues should deny");
        assert_eq!(result.deny_issues, 1);
    }

    // --- Gate mode no deny → decision "warn" ---

    #[test]
    fn apply_filters_gate_mode_no_deny_with_issues_returns_warn() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let args = make_args_gate();

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("gleipnir", "G001", "error", 10),
                make_issue("gleipnir", "G002", "warning", 20),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        assert_eq!(result.decision, "warn", "gate mode with non-blocked issues should warn");
        assert_eq!(result.deny_issues, 0);
    }

    // --- Report mode never returns "deny" ---

    #[test]
    fn apply_filters_report_mode_never_returns_deny() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let args = make_args_report();

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("gleipnir", "G001", "blocked", 10),
                make_issue("gleipnir", "G002", "blocked", 20),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        assert_ne!(result.decision, "deny", "report mode should never return 'deny'");
        assert_eq!(result.decision, "warn", "report mode with deny-matching issues should return 'warn'");
        assert_eq!(result.deny_issues, 2, "deny_issues count should still track matches");
    }

    // --- CLI override replaces warn filter ---

    #[test]
    fn apply_filters_cli_tool_override_replaces_warn_filter() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let mut args = make_args_report();
        args.tool_filter = Some("ruff".into());

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("gleipnir", "G001", "error", 10),
                make_issue("ruff", "E501", "warning", 20),
                make_issue("ruff", "F401", "error", 30),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        let total = report_render_core::total_issues(&result.warn_groups);
        assert_eq!(total, 2, "CLI --tool ruff should show only ruff issues (2), ignoring warn filter");
        for group in &result.warn_groups {
            assert_eq!(group.tool, "ruff", "all visible groups should be ruff");
        }
    }

    #[test]
    fn apply_filters_cli_tool_all_shows_everything() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let mut args = make_args_report();
        args.tool_filter = Some("all".into());

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("gleipnir", "G001", "error", 10),
                make_issue("ruff", "E501", "warning", 20),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        let total = report_render_core::total_issues(&result.warn_groups);
        assert_eq!(total, 2, "CLI --tool all should show all issues");
    }

    // --- CLI override level filter ---

    #[test]
    fn apply_filters_cli_level_filter_error_hides_lower() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let mut args = make_args_report();
        args.tool_filter = Some("all".into());
        args.level_filter = Some("error".into());

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("ruff", "E501", "warning", 10),
                make_issue("ruff", "F401", "error", 20),
                make_issue("gleipnir", "G001", "blocked", 30),
                make_issue("gleipnir", "G002", "info", 40),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        let total = report_render_core::total_issues(&result.warn_groups);
        assert_eq!(total, 2, "level filter 'error' should show error+blocked (severity >= error)");
    }

    #[test]
    fn apply_filters_cli_level_filter_blocked_most_restrictive() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let mut args = make_args_report();
        args.tool_filter = Some("all".into());
        args.level_filter = Some("blocked".into());

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("gleipnir", "G001", "error", 10),
                make_issue("gleipnir", "G002", "blocked", 20),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        let total = report_render_core::total_issues(&result.warn_groups);
        assert_eq!(total, 1, "level filter 'blocked' should show only blocked issues");
    }

    // --- CLI custom filter ---

    #[test]
    fn apply_filters_cli_custom_filter() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let mut args = make_args_report();
        args.custom_filter = Some(r#".code == "E501""#.into());

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("ruff", "E501", "warning", 10),
                make_issue("ruff", "F401", "error", 20),
                make_issue("gleipnir", "G001", "error", 30),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        let total = report_render_core::total_issues(&result.warn_groups);
        assert_eq!(total, 1, "custom filter .code==E501 should show only E501 issue");
        assert_eq!(result.warn_groups[0].code, "E501");
    }

    // --- Empty reports → decision "allow" ---

    #[test]
    fn apply_filters_empty_reports_returns_allow() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let args = make_args_report();
        let reports: Vec<saga_runner::SanityReport> = vec![];

        let result = apply_filters(&reports, &config, &args);
        assert_eq!(result.decision, "allow");
        assert!(result.warn_groups.is_empty());
        assert_eq!(result.deny_issues, 0);
    }

    #[test]
    fn apply_filters_gate_empty_reports_returns_allow() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let args = make_args_gate();
        let reports: Vec<saga_runner::SanityReport> = vec![];

        let result = apply_filters(&reports, &config, &args);
        assert_eq!(result.decision, "allow");
    }

    // --- Reports with no matching issues → allow ---

    #[test]
    fn apply_filters_no_matching_issues_returns_allow() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let args = make_args_gate();

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("ruff", "E501", "warning", 10),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        assert_eq!(result.decision, "allow", "no gleipnir issues → nothing visible → allow");
    }

    // --- Multiple reports, mixed tools ---

    #[test]
    fn apply_filters_multiple_reports_aggregates() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let args = make_args_gate();

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("gleipnir", "G001", "blocked", 10),
                make_issue("ruff", "E501", "warning", 20),
            ]),
            make_report("src/b.py", vec![
                make_issue("gleipnir", "G001", "error", 15),
                make_issue("gleipnir", "G002", "blocked", 25),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        let total = report_render_core::total_issues(&result.warn_groups);
        assert_eq!(total, 3, "should see 3 gleipnir issues across 2 reports (ruff excluded)");
        assert_eq!(result.deny_issues, 2, "2 blocked gleipnir issues");
        assert_eq!(result.decision, "deny", "gate mode with blocked → deny");
    }

    // --- Deny filter is applied to visible issues only ---

    #[test]
    fn apply_filters_deny_only_counts_visible() {
        // Warn filter shows only gleipnir. Ruff blocked issue is invisible.
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let args = make_args_gate();

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("ruff", "E501", "blocked", 10),       // invisible (not gleipnir)
                make_issue("gleipnir", "G001", "warning", 20),   // visible, not blocked
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        assert_eq!(result.deny_issues, 0, "ruff blocked issue should not count (not visible)");
        assert_eq!(result.decision, "warn", "visible issues but no deny → warn");
    }

    // --- Gate mode with CLI overrides is enforced at parse_args, not apply_filters ---

    #[test]
    fn apply_filters_gate_mode_note_cli_overrides_parsed_elsewhere() {
        // In apply_filters, gate mode with CLI overrides would actually apply the config
        // (has_cli_overrides is false because mode != Report).
        // This test confirms apply_filters uses config, not CLI overrides, in gate mode.
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let mut args = make_args_gate();
        // Even if we set tool_filter on gate args, has_cli_overrides will be false
        // because mode == Gate (the check is args.mode == Mode::Report).
        args.tool_filter = Some("ruff".into());

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("gleipnir", "G001", "error", 10),
                make_issue("ruff", "E501", "warning", 20),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        let total = report_render_core::total_issues(&result.warn_groups);
        assert_eq!(total, 1, "gate mode ignores CLI overrides, uses config warn filter");
        assert_eq!(result.warn_groups[0].tool, "gleipnir");
    }

    // --- Warn filter with pass-all expression ---

    #[test]
    fn apply_filters_passall_warn_shows_everything() {
        // Use a universally-true expression (line is always >= 1 in our test data)
        let config = make_config(".line > 0", r#".severity == "blocked""#);
        let args = make_args_report();

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("gleipnir", "G001", "error", 10),
                make_issue("ruff", "E501", "warning", 20),
                make_issue("basedpyright", "BP001", "info", 30),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        let total = report_render_core::total_issues(&result.warn_groups);
        assert_eq!(total, 3, "pass-all warn filter should show all issues");
    }

    // --- Deny filter with pass-all expression ---

    #[test]
    fn apply_filters_passall_deny_counts_all_visible() {
        let config = make_config(".line > 0", ".line > 0");
        let args = make_args_gate();

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("ruff", "E501", "warning", 10),
                make_issue("gleipnir", "G001", "error", 20),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        assert_eq!(result.deny_issues, 2, "pass-all deny filter should count all visible issues");
        assert_eq!(result.decision, "deny");
    }

    // --- Deny filter with pass-none expression ---

    #[test]
    fn apply_filters_deny_passnothing_never_denies() {
        // An expression that matches nothing: no tool has this name
        let config = make_config(".line > 0", r#".tool == "NONEXISTENT_TOOL_xyz""#);
        let args = make_args_gate();

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("gleipnir", "G001", "blocked", 10),
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        assert_eq!(result.deny_issues, 0, "deny filter matching nothing should yield 0");
        assert_eq!(result.decision, "warn", "visible issues but no deny matches → warn");
    }

    // --- Combined CLI overrides (tool + level) ---

    #[test]
    fn apply_filters_cli_tool_and_level_combined() {
        let config = make_config(r#".tool == "gleipnir""#, r#".severity == "blocked""#);
        let mut args = make_args_report();
        args.tool_filter = Some("ruff".into());
        args.level_filter = Some("error".into());

        let reports = vec![
            make_report("src/a.py", vec![
                make_issue("ruff", "E501", "warning", 10),   // ruff but warning < error → skip
                make_issue("ruff", "F401", "error", 20),     // ruff and error → show
                make_issue("gleipnir", "G001", "blocked", 30), // not ruff → skip
            ]),
        ];

        let result = apply_filters(&reports, &config, &args);
        let total = report_render_core::total_issues(&result.warn_groups);
        assert_eq!(total, 1, "only ruff+error should pass both tool and level filters");
    }
}
