//! PreToolUse hook: LLM behavioral detection for Bash commands.
//!
//! Detects subversion (lock/constraint manipulation), truncation (partial reads
//! of constraint files), and evasion (adversarial shortcuts) in interactive
//! LLM sessions.
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_pre_llm_bash --subversion block --truncation warn --evasion warn
//!
//! Env var:
//!     HOOK_LLM_ALLOW_BASH=pattern_name1:pattern_name2  — exempt specific patterns

use std::process::ExitCode;

use hook_io::rules::{parse_rule_array, parse_severity, parse_toml_table, RawRule, Severity};
use hook_io::{HookDecision, HookInput};
use regex::Regex;

static RULES_TOML: &str = include_str!("../rules.toml");

fn main() -> ExitCode {
    hook_io::run_hook(decide)
}

// ── Rules ──────────────────────────────────────────────────────────

#[derive(Debug)]
struct CompiledRule {
    pattern: String,
    description: String,
    compiled: Regex,
}

impl CompiledRule {
    /// Compile a RawRule's pattern as a regex. Returns None if invalid.
    fn from_raw(raw: RawRule) -> Option<Self> {
        let compiled = Regex::new(&raw.pattern).ok()?;
        Some(Self {
            pattern: raw.pattern,
            description: raw.description,
            compiled,
        })
    }
}

struct Rules {
    subversion: Vec<CompiledRule>,
    truncation: Vec<CompiledRule>,
    evasion: Vec<CompiledRule>,
}

fn parse_rules(toml_str: &str) -> Result<Rules, String> {
    let table = parse_toml_table(toml_str)?;

    let compile_array = |key: &str| -> Vec<CompiledRule> {
        parse_rule_array(&table, key)
            .into_iter()
            .filter_map(CompiledRule::from_raw)
            .collect()
    };

    Ok(Rules {
        subversion: compile_array("subversion"),
        truncation: compile_array("truncation"),
        evasion: compile_array("evasion"),
    })
}

// ── Config ─────────────────────────────────────────────────────────

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

// ── Decision logic ─────────────────────────────────────────────────

fn decide(input: &HookInput) -> HookDecision {
    let config = parse_config();
    let rules = match parse_rules(RULES_TOML) {
        Ok(r) => r,
        Err(e) => {
            return HookDecision::Deny {
                category: "config".into(),
                event: "rules parse failure".into(),
                reason: format!("Cannot load rules \u{2014} {e}"),
            };
        }
    };

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
    rules: &[CompiledRule],
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
            event: format!("{} \u{2014} {}", description.to_lowercase(), short_cmd),
            reason: format!(
                "Command blocked ({}: {}).\n{}",
                category, description, short_cmd
            ),
        },
        Severity::Warn => HookDecision::Warn {
            category: category.into(),
            event: format!("{} \u{2014} {}", description.to_lowercase(), short_cmd),
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
