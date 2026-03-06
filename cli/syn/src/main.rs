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

// =============================================================================
// Grouped structures (ported from qa_core)
// =============================================================================

use std::collections::BTreeMap;

#[derive(Debug, Clone)]
struct LocatedIssue {
    file: String,
    line: usize,
    message: String,
}

#[derive(Debug)]
struct CheckGroup {
    tool: String,
    code: String,
    severity: String,
    signal: String,
    direction: String,
    canary: String,
    representative_message: String,
    issues: Vec<LocatedIssue>,
    file_count: usize,
}

fn group_issues(reports: &[SanityReport]) -> Vec<CheckGroup> {
    let mut groups: BTreeMap<(String, String), (saga_core::Issue, Vec<LocatedIssue>)> =
        BTreeMap::new();

    for report in reports {
        for issue in &report.issues {
            let key = (issue.tool.clone(), issue.code.clone());
            let located = LocatedIssue {
                file: report.relative_path.clone(),
                line: issue.line,
                message: issue.message.clone(),
            };
            groups
                .entry(key)
                .or_insert_with(|| (issue.clone(), Vec::new()))
                .1
                .push(located);
        }
    }

    groups
        .into_iter()
        .map(|((tool, code), (rep, issues))| {
            let file_count = {
                let mut files: Vec<&str> = issues.iter().map(|li| li.file.as_str()).collect();
                files.sort();
                files.dedup();
                files.len()
            };
            let representative_message = issues.first()
                .map(|li| li.message.clone())
                .unwrap_or_default();
            CheckGroup {
                tool,
                code,
                severity: rep.severity,
                signal: rep.signal,
                direction: rep.direction,
                canary: rep.canary,
                representative_message,
                issues,
                file_count,
            }
        })
        .collect()
}

fn total_issues(groups: &[CheckGroup]) -> usize {
    groups.iter().map(|g| g.issues.len()).sum()
}

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

fn load_filter_config(config_path: &Path, default_expr: &str) -> (String, CompiledFilter) {
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

    let filter = match compile_filter(&expr) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[syn] fatal: bad jq filter in {}: {}", config_path.display(), e);
            process::exit(2);
        }
    };

    (expr, filter)
}

fn load_config(project_dir: &Path) -> SynConfig {
    let syn_dir = project_dir.join(".syn");
    let (warn_expr, warn_filter) = load_filter_config(&syn_dir.join("warn.toml"), DEFAULT_WARN_FILTER);
    let (deny_expr, deny_filter) = load_filter_config(&syn_dir.join("deny.toml"), DEFAULT_DENY_FILTER);
    SynConfig { warn_filter, deny_filter, _warn_expr: warn_expr, _deny_expr: deny_expr }
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
enum OutputMode {
    Toon,
    Colored,
    Json,
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
    // Report-mode-only overrides
    tool_filter: Option<String>,
    level_filter: Option<String>,
    custom_filter: Option<String>,
    path: Option<PathBuf>,
}

fn is_tty() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stdout())
}

fn parse_args() -> Args {
    let raw: Vec<String> = std::env::args().skip(1).collect();

    let mut mode = Mode::Report;
    let mut output = None; // None means auto-detect
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
                        eprintln!("[syn] --mode requires 'report' or 'gate'");
                        process::exit(2);
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
                        eprintln!("[syn] --output requires 'colored', 'json', or 'toon'");
                        process::exit(2);
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
                        eprintln!("[syn] --target requires 'src', 'tests', or 'all'");
                        process::exit(2);
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
            "--help" | "-h" => {
                print_usage();
                process::exit(0);
            }
            arg if arg.starts_with('-') => {
                eprintln!("[syn] unknown flag: {}", arg);
                process::exit(2);
            }
            arg => positional.push(arg.to_string()),
        }
        idx += 1;
    }

    // Gate mode rejects override flags
    if mode == Mode::Gate {
        if tool_filter.is_some() || level_filter.is_some() || custom_filter.is_some() {
            eprintln!("[syn] gate mode rejects --tool/--level/--filter (locked to config)");
            process::exit(2);
        }
    }

    // Auto-detect output mode
    let output = output.unwrap_or_else(|| {
        if is_tty() { OutputMode::Colored } else { OutputMode::Toon }
    });

    let path = positional.first().map(PathBuf::from);

    Args { mode, output, silent, stdin, project_dir, target, tool_filter, level_filter, custom_filter, path }
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

    // Directory walk — target excludes the opposite branch
    let mut results = Vec::new();
    let skip = match target {
        Target::Src => Some("tests"),
        Target::Tests => Some("src"),
        Target::All => None,
    };
    walk_qa_files(path, skip, &mut results);
    results
}

fn walk_qa_files(dir: &Path, skip: Option<&str>, results: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            if name.starts_with('.') || name == "__pycache__" || name == "node_modules" || name == ".venv" {
                continue;
            }
            if skip.is_some_and(|s| name == s) {
                continue;
            }
            walk_qa_files(&path, skip, results);
        } else if name.ends_with(".qa") && name.starts_with('.') {
            results.push(path);
        }
    }
}

fn load_reports_from_stdin() -> Vec<SanityReport> {
    let mut buf = String::new();
    if io::stdin().read_to_string(&mut buf).is_err() {
        eprintln!("[syn] failed to read stdin");
        process::exit(2);
    }

    // Try single report first, then array
    if let Ok(report) = serde_json::from_str::<SanityReport>(&buf) {
        return vec![report];
    }
    if let Ok(reports) = serde_json::from_str::<Vec<SanityReport>>(&buf) {
        return reports;
    }

    eprintln!("[syn] stdin is not valid .qa JSON");
    process::exit(2);
}

// =============================================================================
// Severity ordering
// =============================================================================

fn severity_rank(s: &str) -> u8 {
    match s {
        "info" => 0,
        "warning" => 1,
        "error" => 2,
        "blocked" => 3,
        _ => 0,
    }
}

// =============================================================================
// Three-tier filtering
// =============================================================================

struct FilteredOutput {
    warn_groups: Vec<CheckGroup>,   // visible issues (passed warn filter)
    deny_issues: usize,             // count that also passed deny filter
    decision: &'static str,         // "allow", "warn", "deny"
}

fn apply_filters(
    reports: &[SanityReport],
    config: &SynConfig,
    args: &Args,
) -> FilteredOutput {
    // Collect all issues that pass the warn filter
    let mut visible_reports: Vec<SanityReport> = Vec::new();
    let mut deny_count: usize = 0;

    // In report mode, CLI overrides REPLACE the warn filter (they define
    // what's visible instead). In gate mode, only the config warn filter applies.
    let has_cli_overrides = args.mode == Mode::Report
        && (args.tool_filter.is_some() || args.level_filter.is_some() || args.custom_filter.is_some());

    // Pre-compile custom filter once if present
    let custom_compiled = args.custom_filter.as_ref().and_then(|expr| compile_filter(expr).ok());

    for report in reports {
        let mut visible_issues = Vec::new();
        for issue in &report.issues {
            if has_cli_overrides {
                // CLI overrides replace the warn filter
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
                // No CLI overrides — use config warn filter
                if !matches_filter(issue, &config.warn_filter) {
                    continue;
                }
            }
            visible_issues.push(issue.clone());

            // Deny filter — subset of visible
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
// Output formatting
// =============================================================================

const MAX_WIDTH: usize = 79;

// -- Text wrapping helpers (ported from qa_core) --

fn wrap_adaptive(text: &str, first_width: usize, rest_width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut width = first_width;
    for word in text.split_whitespace() {
        if current.is_empty() {
            current = word.to_string();
        } else if current.len() + 1 + word.len() > width {
            lines.push(current);
            current = word.to_string();
            width = rest_width;
        } else {
            current.push(' ');
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

// -- TOON formatter --

fn format_toon(groups: &[CheckGroup]) -> String {
    if groups.is_empty() {
        return "All checks passed.".to_string();
    }

    // Build JSON structure, then convert via format_core
    let json = groups_to_json(groups);
    match format_core::serialize::to_toon(&json) {
        Ok(toon) => toon,
        Err(_) => {
            // Fallback to JSON if TOON encoding fails
            serde_json::to_string_pretty(&json).unwrap_or_default()
        }
    }
}

// -- Colored terminal formatter (ported from qa_core) --

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const WHITE: &str = "\x1b[37m";
const BG_RED: &str = "\x1b[41m";

/// Collapse duplicate line numbers: [14,14,21,22] → "14(2),21,22"
fn collapse_line_numbers(lines: &[usize]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let mut count = 1;
        while i + count < lines.len() && lines[i + count] == line {
            count += 1;
        }
        if count == 1 {
            parts.push(line.to_string());
        } else {
            parts.push(format!("{}/{}", line, count));
        }
        i += count;
    }
    parts.join(",")
}

/// Opaque codes are short letter+digit patterns (ruff: E501, S701, I001).
/// Descriptive codes contain lowercase or are long (reportMissingImports, no_any_types).
fn is_opaque_code(code: &str) -> bool {
    code.len() <= 8 && code.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

fn severity_color(severity: &str) -> &'static str {
    match severity {
        "error" | "blocked" => "\x1b[1;31m",
        "warning" => "\x1b[1;33m",
        _ => "\x1b[1;37m",
    }
}

fn format_colored(groups: &[CheckGroup]) -> String {
    if groups.is_empty() {
        return format!(
            "\n{BOLD}{CYAN}{bar}{RESET}\n  All checks passed.\n{BOLD}{CYAN}{bar}{RESET}",
            bar = "━".repeat(MAX_WIDTH),
        );
    }

    let total = total_issues(groups);
    let mut sections: Vec<String> = Vec::new();

    let bar = "━".repeat(MAX_WIDTH);
    sections.push(format!("\n{BOLD}{RED}{bar}{RESET}"));
    sections.push(format!(
        "{BOLD}{WHITE}{BG_RED}  SYN  {RESET}  {BOLD}{WHITE}{total} violations{RESET} across {BOLD}{WHITE}{count} check types{RESET}",
        count = groups.len(),
    ));
    sections.push(format!("{BOLD}{RED}{bar}{RESET}"));

    for group in groups {
        let color = severity_color(&group.severity);
        let files_word = if group.file_count == 1 { "file" } else { "files" };

        // Annotate opaque codes (ruff letter+number like S701) with a representative message.
        // Descriptive codes (basedpyright's reportMissingImports, gleipnir's check names) stand alone.
        let code_display = if is_opaque_code(&group.code) && !group.representative_message.is_empty() {
            let msg = &group.representative_message;
            let short = msg.find(". ")
                .map(|i| &msg[..i])
                .unwrap_or(msg);
            let short = if short.len() > 60 {
                let truncated = &short[..short[..57].rfind(' ').unwrap_or(57)];
                format!("{}...", truncated)
            } else {
                short.to_string()
            };
            format!("{} — {}", group.code, short)
        } else {
            group.code.clone()
        };

        sections.push(format!(
            "\n{color}┌─ {code_display}{RESET}  {DIM}{severity}{RESET}  {BOLD}{WHITE}{count}{RESET} in {file_count} {files_word}",
            severity = group.severity,
            count = group.issues.len(), file_count = group.file_count,
        ));
        sections.push(format!("{color}│{RESET}"));

        let gutter = format!("{color}│{RESET}  ");
        let continuation = format!("{color}│{RESET}    ");
        let cont_width = MAX_WIDTH - 5;

        if !group.signal.is_empty() {
            let lines = wrap_adaptive(&group.signal, MAX_WIDTH - 11, cont_width);
            sections.push(format!("{gutter}{BOLD}{CYAN}Signal:{RESET} {}", lines[0]));
            for line in &lines[1..] {
                sections.push(format!("{continuation}{line}"));
            }
        }
        if !group.direction.is_empty() {
            let lines = wrap_adaptive(&group.direction, MAX_WIDTH - 14, cont_width);
            sections.push(format!("{gutter}{BOLD}{CYAN}Direction:{RESET} {}", lines[0]));
            for line in &lines[1..] {
                sections.push(format!("{continuation}{line}"));
            }
        }
        if !group.canary.is_empty() {
            let lines = wrap_adaptive(&group.canary, MAX_WIDTH - 11, cont_width);
            sections.push(format!("{gutter}{BOLD}{YELLOW}Canary:{RESET} {}", lines[0]));
            for line in &lines[1..] {
                sections.push(format!("{continuation}{line}"));
            }
        }

        sections.push(format!("{color}│{RESET}"));
        // Collapse same-file issues into file:[line1,line2,line3]
        let mut file_lines: Vec<(&str, Vec<usize>)> = Vec::new();
        for issue in &group.issues {
            if let Some(last) = file_lines.last_mut() {
                if last.0 == issue.file {
                    last.1.push(issue.line);
                    continue;
                }
            }
            file_lines.push((&issue.file, vec![issue.line]));
        }
        for (file, lines) in &file_lines {
            let line_list = format!("[{}]", collapse_line_numbers(lines));
            let avail = MAX_WIDTH - 3; // after "│  "

            if 3 + file.len() + 1 + line_list.len() <= MAX_WIDTH {
                // Fits on one line: file left, line list right
                let pad = avail - file.len() - line_list.len();
                sections.push(format!(
                    "{color}│{RESET}  {DIM}{file}{}{line_list}{RESET}",
                    " ".repeat(pad),
                ));
            } else {
                // File on its own line, line list right-justified on next line(s)
                sections.push(format!(
                    "{color}│{RESET}  {DIM}{file}{RESET}",
                ));
                if line_list.len() <= avail {
                    let pad = avail - line_list.len();
                    sections.push(format!(
                        "{color}│{RESET}  {DIM}{}{line_list}{RESET}",
                        " ".repeat(pad),
                    ));
                } else {
                    // Line list itself needs wrapping, right-justify each row
                    let collapsed = collapse_line_numbers(lines);
                    let parts: Vec<&str> = collapsed.split(',').collect();
                    let mut rows: Vec<String> = Vec::new();
                    let mut current = String::from("[");
                    for part in &parts {
                        let entry = if current == "[" {
                            part.to_string()
                        } else {
                            format!(",{}", part)
                        };
                        if current.len() + entry.len() + 1 > avail {
                            current.push(',');
                            rows.push(current);
                            current = part.to_string();
                        } else {
                            current.push_str(&entry);
                        }
                    }
                    current.push(']');
                    rows.push(current);

                    for row in &rows {
                        let pad = if row.len() < avail { avail - row.len() } else { 0 };
                        sections.push(format!(
                            "{color}│{RESET}  {DIM}{}{row}{RESET}",
                            " ".repeat(pad),
                        ));
                    }
                }
            }
        }
        sections.push(format!("{color}└{bar}{RESET}", bar = "─".repeat(MAX_WIDTH - 1)));
    }

    sections.join("\n")
}

// -- JSON formatter --

fn groups_to_json(groups: &[CheckGroup]) -> serde_json::Value {
    let json_groups: Vec<serde_json::Value> = groups
        .iter()
        .map(|group| {
            // Collapse same-file issues into file:[lines]
            let mut file_lines: Vec<(&str, Vec<usize>)> = Vec::new();
            for li in &group.issues {
                if let Some(last) = file_lines.last_mut() {
                    if last.0 == li.file {
                        last.1.push(li.line);
                        continue;
                    }
                }
                file_lines.push((&li.file, vec![li.line]));
            }
            let locations: Vec<serde_json::Value> = file_lines.iter().map(|(file, lines)| {
                if lines.len() == 1 {
                    serde_json::json!({ "file": file, "line": lines[0] })
                } else {
                    serde_json::json!({ "file": file, "lines": lines })
                }
            }).collect();

            serde_json::json!({
                "tool": group.tool,
                "code": group.code,
                "message": group.representative_message,
                "severity": group.severity,
                "count": group.issues.len(),
                "file_count": group.file_count,
                "signal": group.signal,
                "direction": group.direction,
                "canary": group.canary,
                "locations": locations,
            })
        })
        .collect();

    serde_json::json!({
        "type": "syn_report",
        "total": total_issues(groups),
        "check_types": groups.len(),
        "groups": json_groups,
    })
}

fn format_json(groups: &[CheckGroup]) -> String {
    if groups.is_empty() {
        return r#"{"total":0,"check_types":0,"groups":[]}"#.to_string();
    }
    serde_json::to_string_pretty(&groups_to_json(groups)).unwrap_or_default()
}

fn format_output(groups: &[CheckGroup], output_mode: OutputMode) -> String {
    match output_mode {
        OutputMode::Toon => format_toon(groups),
        OutputMode::Colored => format_colored(groups),
        OutputMode::Json => format_json(groups),
    }
}

// =============================================================================
// Hlidskjalf broadcast
// =============================================================================

fn broadcast(groups: &[CheckGroup], decision: &str, deny_count: usize) {
    let payload = groups_to_json(groups);

    socket_emit::emit(&socket_emit::WatchtowerEvent {
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0),
        category: "quality".into(),
        decision: decision.to_string(),
        event_name: "syn_check".into(),
        workspace: socket_emit::workspace_name(),
        detail: format!(
            "{} issues, {} deny, decision: {}",
            total_issues(groups), deny_count, decision
        ),
        context_injected: String::new(),
        speech: None,
        payload: Some(payload),
    });
}

// =============================================================================
// Main
// =============================================================================

fn main() {
    let args = parse_args();

    // Resolve project directory
    let project_dir = args.project_dir.clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // Load config
    let config = load_config(&project_dir);

    // Load reports
    let reports = if args.stdin {
        load_reports_from_stdin()
    } else {
        let path = args.path.as_deref().unwrap_or_else(|| Path::new("."));
        let qa_files = find_qa_files(path, args.target);
        if qa_files.is_empty() {
            if args.output == OutputMode::Json {
                println!(r#"{{"total":0,"check_types":0,"groups":[]}}"#);
            } else if args.output == OutputMode::Colored {
                let bar = "━".repeat(MAX_WIDTH);
                println!("\n\x1b[1;36m{bar}\x1b[0m\n  All checks passed.\n\x1b[1;36m{bar}\x1b[0m");
            } else {
                println!("All checks passed.");
            }
            process::exit(0);
        }
        qa_files.iter().filter_map(|qf| saga_core::load_qa_file(qf)).collect()
    };

    if reports.is_empty() {
        println!("All checks passed.");
        process::exit(0);
    }

    // Filter
    let result = apply_filters(&reports, &config, &args);

    // Output
    let output = format_output(&result.warn_groups, args.output);
    if !output.is_empty() {
        println!("{}", output);
    }

    // Decision line on stderr (gate mode)
    if args.mode == Mode::Gate {
        let total = total_issues(&result.warn_groups);
        eprintln!(
            "[syn] {} — {} visible, {} deny",
            result.decision.to_uppercase(), total, result.deny_issues
        );
    }

    // Broadcast to Hlidskjalf
    if !args.silent {
        broadcast(&result.warn_groups, result.decision, result.deny_issues);
    }

    // Exit code
    match result.decision {
        "deny" => process::exit(1),
        _ => process::exit(0),
    }
}
