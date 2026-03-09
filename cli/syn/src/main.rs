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
use saga_core::SanityReport;

use report_render::{CheckGroup, OutputMode, group_issues, total_issues, groups_to_json, format_output, severity_rank};

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

fn matches_filter(issue: &saga_core::Issue, filter: &CompiledFilter) -> bool {
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

struct Args {
    mode: Mode,
    output: OutputMode,
    silent: bool,
    stdin: bool,
    project_dir: Option<PathBuf>,
    target: Target,
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

    Ok(Args { mode, output, silent, stdin, project_dir, target, tool_filter, level_filter, custom_filter, path })
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
    <path>           File (.py finds sidecar), directory, or omit for cwd
    --stdin          Read .qa JSON from stdin
    --project-dir    Project root
    --target         [src|tests|all] Subtree scope (default: src)

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

fn find_qa_files(path: &Path, target: Target) -> Vec<PathBuf> {
    if path.is_file() && path.extension().map_or(false, |e| e == "qa") {
        return vec![path.to_path_buf()];
    }

    if path.is_file() {
        let sidecar = saga_core::qa_path(path);
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
    saga_core::walk_files(
        path,
        &extra_skip,
        &|name| name.ends_with(".qa") && name.starts_with('.'),
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

fn broadcast(groups: &[CheckGroup], decision: &str, deny_count: usize) {
    let payload = groups_to_json(groups);

    let datagram = socket_emit::Datagram {
        timestamp: socket_emit::now(),
        source: "syn".to_string(),
        kind: socket_emit::DatagramKind::Report,
        priority: match decision {
            "deny" => socket_emit::Priority::High,
            "warn" => socket_emit::Priority::Normal,
            _ => socket_emit::Priority::Low,
        },
        workspace: socket_emit::workspace_name(),
        detail: Some(format!(
            "{} issues, {} deny, decision: {}",
            total_issues(groups), deny_count, decision
        )),
        speech: None,
        payload: Some(payload),
    };
    socket_emit::emit_datagram(&datagram);
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
        let qa_files = find_qa_files(path, args.target);
        if qa_files.is_empty() {
            println!("{}", format_output(&[], args.output));
            return Ok(0);
        }
        qa_files.iter().filter_map(|qf| saga_core::load_qa_file(qf)).collect()
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
        broadcast(&result.warn_groups, result.decision, result.deny_issues);
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
