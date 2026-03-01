//! PreToolUse hook: LLM behavioral detection for Bash commands.
//!
//! Detects subversion (lock/constraint manipulation), truncation (partial reads
//! of constraint files), and evasion (adversarial shortcuts) in interactive
//! LLM sessions.
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_intercept_llm_bash --subversion block --truncation warn --evasion warn
//!
//! Env var:
//!     HOOK_LLM_ALLOW_BASH=pattern_name1:pattern_name2  — exempt specific patterns

use std::process::ExitCode;

use hook_io::{HookDecision, HookInput};
use regex::Regex;

static RULES_TOML: &str = include_str!("../rules.toml");

fn main() -> ExitCode {
    hook_io::run_hook(decide)
}

// ── Rules ──────────────────────────────────────────────────────────

#[derive(Debug)]
struct Rule {
    pattern: String,
    description: String,
    compiled: Regex,
}

#[derive(Debug)]
struct Rules {
    subversion: Vec<Rule>,
    truncation: Vec<Rule>,
    evasion: Vec<Rule>,
}

fn parse_rules(toml_str: &str) -> Rules {
    let table: toml::Table = toml_str.parse().expect("embedded rules.toml is invalid");

    let parse_array = |key: &str| -> Vec<Rule> {
        table
            .get(key)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let t = item.as_table()?;
                        let pattern = t.get("pattern")?.as_str()?.to_string();
                        let compiled = Regex::new(&pattern).ok()?;
                        Some(Rule {
                            pattern,
                            description: t.get("description")?.as_str()?.to_string(),
                            compiled,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    };

    Rules {
        subversion: parse_array("subversion"),
        truncation: parse_array("truncation"),
        evasion: parse_array("evasion"),
    }
}

// ── Config ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum Severity {
    Warn,
    Block,
}

#[derive(Debug)]
struct Config {
    subversion: Option<Severity>,
    truncation: Option<Severity>,
    evasion: Option<Severity>,
    allow_patterns: Vec<String>,
}

fn parse_config() -> Config {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut subversion = None;
    let mut truncation = None;
    let mut evasion = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--subversion" if i + 1 < args.len() => {
                subversion = parse_severity(&args[i + 1]);
                i += 2;
            }
            "--truncation" if i + 1 < args.len() => {
                truncation = parse_severity(&args[i + 1]);
                i += 2;
            }
            "--evasion" if i + 1 < args.len() => {
                evasion = parse_severity(&args[i + 1]);
                i += 2;
            }
            _ => i += 1,
        }
    }

    let allow_patterns = std::env::var("HOOK_LLM_ALLOW_BASH")
        .unwrap_or_default()
        .split(':')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    Config {
        subversion,
        truncation,
        evasion,
        allow_patterns,
    }
}

fn parse_severity(s: &str) -> Option<Severity> {
    match s {
        "warn" => Some(Severity::Warn),
        "block" => Some(Severity::Block),
        _ => None,
    }
}

// ── Decision logic ─────────────────────────────────────────────────

fn decide(input: &HookInput) -> HookDecision {
    let config = parse_config();
    let rules = parse_rules(RULES_TOML);

    let command = input
        .tool_input
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if command.trim().is_empty() {
        return HookDecision::Allow;
    }

    // Check each category
    if let Some(severity) = config.subversion {
        if let Some(decision) = check_category(
            command, &rules.subversion, severity, "subversion", &config.allow_patterns,
        ) {
            return decision;
        }
    }

    if let Some(severity) = config.truncation {
        if let Some(decision) = check_category(
            command, &rules.truncation, severity, "truncation", &config.allow_patterns,
        ) {
            return decision;
        }
    }

    if let Some(severity) = config.evasion {
        if let Some(decision) = check_category(
            command, &rules.evasion, severity, "evasion", &config.allow_patterns,
        ) {
            return decision;
        }
    }

    HookDecision::Allow
}

fn check_category(
    command: &str,
    rules: &[Rule],
    severity: Severity,
    category: &str,
    allow_patterns: &[String],
) -> Option<HookDecision> {
    for rule in rules {
        if rule.compiled.is_match(command) {
            // Check if this pattern is exempted
            if allow_patterns.iter().any(|p| rule.pattern.contains(p.as_str())) {
                return None;
            }
            return Some(make_decision(severity, category, &rule.description, command));
        }
    }
    None
}

fn make_decision(
    severity: Severity,
    category: &str,
    description: &str,
    command: &str,
) -> HookDecision {
    // Truncate command for display (first 60 chars)
    let short_cmd = if command.len() > 60 {
        format!("{}...", &command[..57])
    } else {
        command.to_string()
    };

    match severity {
        Severity::Block => HookDecision::Deny {
            category: category.into(),
            event: format!("{} — {}", description.to_lowercase(), short_cmd),
            reason: format!(
                "Command blocked ({}: {}).\n{}",
                category, description, short_cmd
            ),
        },
        Severity::Warn => HookDecision::Warn {
            category: category.into(),
            event: format!("{} — {}", description.to_lowercase(), short_cmd),
            user_reason: format!(
                "LLM ran '{}' ({}: {}). Behavior flagged.",
                short_cmd, category, description
            ),
            llm_context: format!(
                "Your command was flagged as {} behavior ({}). \
                 The user has been notified. If this is necessary \
                 for your task, explain why to the user.",
                category, description
            ),
        },
    }
}
