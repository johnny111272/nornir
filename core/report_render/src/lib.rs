//! Report rendering and grouping for QA sanity reports.
//!
//! Provides pure functions to group, format, and serialize `saga_core::SanityReport`
//! issues into human-readable (colored terminal, TOON) and machine-readable (JSON)
//! output.
//!
//! All public API is pure — no I/O, no side effects.

use std::collections::BTreeMap;

use saga_core::SanityReport;

// =============================================================================
// Grouped structures
// =============================================================================

#[derive(Debug, Clone)]
pub struct LocatedIssue {
    pub file: String,
    pub line: usize,
    pub message: String,
}

#[derive(Debug)]
pub struct CheckGroup {
    pub tool: String,
    pub code: String,
    pub severity: String,
    pub signal: String,
    pub direction: String,
    pub canary: String,
    pub representative_message: String,
    pub issues: Vec<LocatedIssue>,
    pub file_count: usize,
}

// =============================================================================
// Grouping
// =============================================================================

pub fn group_issues(reports: &[SanityReport]) -> Vec<CheckGroup> {
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

pub fn total_issues(groups: &[CheckGroup]) -> usize {
    groups.iter().map(|g| g.issues.len()).sum()
}

// =============================================================================
// Severity ordering
// =============================================================================

pub fn severity_rank(s: &str) -> u8 {
    match s {
        "info" => 0,
        "warning" => 1,
        "error" => 2,
        "blocked" => 3,
        _ => 0,
    }
}

// =============================================================================
// Output mode
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputMode {
    Toon,
    Colored,
    Json,
}

// =============================================================================
// Formatting primitives
// =============================================================================

const MAX_WIDTH: usize = 79;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const WHITE: &str = "\x1b[37m";
const BG_RED: &str = "\x1b[41m";

pub fn wrap_adaptive(text: &str, first_width: usize, rest_width: usize) -> Vec<String> {
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

pub fn collapse_line_numbers(lines: &[usize]) -> String {
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

// =============================================================================
// Serialization
// =============================================================================

pub fn groups_to_json(groups: &[CheckGroup]) -> serde_json::Value {
    let json_groups: Vec<serde_json::Value> = groups
        .iter()
        .map(|group| {
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

// =============================================================================
// Formatters
// =============================================================================

fn format_toon(groups: &[CheckGroup]) -> String {
    if groups.is_empty() {
        return "All checks passed.".to_string();
    }

    let json = groups_to_json(groups);
    match format_core::serialize::to_toon(&json) {
        Ok(toon) => toon,
        Err(_) => {
            serde_json::to_string_pretty(&json).unwrap_or_default()
        }
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
            let avail = MAX_WIDTH - 3;

            if 3 + file.len() + 1 + line_list.len() <= MAX_WIDTH {
                let pad = avail - file.len() - line_list.len();
                sections.push(format!(
                    "{color}│{RESET}  {DIM}{file}{}{line_list}{RESET}",
                    " ".repeat(pad),
                ));
            } else {
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

fn format_json(groups: &[CheckGroup]) -> String {
    if groups.is_empty() {
        return r#"{"total":0,"check_types":0,"groups":[]}"#.to_string();
    }
    serde_json::to_string_pretty(&groups_to_json(groups)).unwrap_or_default()
}

pub fn format_output(groups: &[CheckGroup], output_mode: OutputMode) -> String {
    match output_mode {
        OutputMode::Toon => format_toon(groups),
        OutputMode::Colored => format_colored(groups),
        OutputMode::Json => format_json(groups),
    }
}
