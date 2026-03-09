//! PreToolUse hook: LLM behavioral detection for file access tools.
//!
//! Detects probing (reading security infrastructure) and gaming (circumventing
//! constraints) in interactive LLM sessions. Not for subagents — those use
//! hook_pre_subagent_tool.
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_pre_llm_tool --gaming warn --probing block
//!
//! Env var:
//!     HOOK_LLM_ALLOW_PATHS=/path1:/path2  — exempt paths from probing/gaming checks

use std::process::ExitCode;

use hook_io::rules::{parse_rule_array, parse_severity, parse_toml_table, RawRule, Severity};
use hook_io::{HookDecision, HookInput};

static RULES_TOML: &str = include_str!("../rules.toml");

fn main() -> ExitCode {
    hook_io::run_hook(decide)
}

// ── Rules ──────────────────────────────────────────────────────────

struct Rules {
    floor: Vec<RawRule>,
    probing: Vec<RawRule>,
    gaming: Vec<RawRule>,
}

fn parse_rules(toml_str: &str) -> Result<Rules, String> {
    let table = parse_toml_table(toml_str)?;
    Ok(Rules {
        floor: parse_rule_array(&table, "floor"),
        probing: parse_rule_array(&table, "probing"),
        gaming: parse_rule_array(&table, "gaming"),
    })
}

// ── Config ─────────────────────────────────────────────────────────

#[derive(Debug)]
struct Config {
    probing: Option<Severity>,
    gaming: Option<Severity>,
    allow_paths: Vec<String>,
}

fn parse_config() -> Config {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut probing = None;
    let mut gaming = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--probing" if i + 1 < args.len() => {
                probing = parse_severity(&args[i + 1]);
                i += 2;
            }
            "--gaming" if i + 1 < args.len() => {
                gaming = parse_severity(&args[i + 1]);
                i += 2;
            }
            _ => i += 1,
        }
    }

    let allow_paths = std::env::var("HOOK_LLM_ALLOW_PATHS")
        .unwrap_or_default()
        .split(':')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    Config {
        probing,
        gaming,
        allow_paths,
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

    let tool_input = &input.tool_input;

    // Extract path from tool_input (same fields as subagent_tool)
    let target = tool_input
        .get("file_path")
        .and_then(|v| v.as_str())
        .or_else(|| tool_input.get("path").and_then(|v| v.as_str()));

    let target = match target {
        Some(t) => t,
        None => return HookDecision::Allow,
    };

    // Layer 1: Floor — always block, no override
    for rule in &rules.floor {
        if target.contains(&rule.pattern) {
            return HookDecision::Deny {
                category: "floor".into(),
                event: format!("access to {} blocked", rule.description.to_lowercase()),
                reason: format!(
                    "Access to '{}' is permanently blocked ({}).",
                    target, rule.description
                ),
            };
        }
    }

    // Check env var exemptions before probing/gaming
    if is_allowed_path(target, &config.allow_paths) {
        return HookDecision::Allow;
    }

    // Layer 2: Probing
    if let Some(severity) = config.probing {
        for rule in &rules.probing {
            if target.contains(&rule.pattern) {
                return make_decision(
                    severity,
                    "probing",
                    &rule.description,
                    target,
                );
            }
        }
    }

    // Layer 3: Gaming
    if let Some(severity) = config.gaming {
        for rule in &rules.gaming {
            if target.contains(&rule.pattern) {
                return make_decision(
                    severity,
                    "gaming",
                    &rule.description,
                    target,
                );
            }
        }
    }

    // Layer 4: Allow everything else
    HookDecision::Allow
}

fn is_allowed_path(target: &str, allow_paths: &[String]) -> bool {
    allow_paths.iter().any(|prefix| target.starts_with(prefix))
}

fn make_decision(
    severity: Severity,
    category: &str,
    description: &str,
    target: &str,
) -> HookDecision {
    match severity {
        Severity::Block => HookDecision::Deny {
            category: category.into(),
            event: format!("{} \u{2014} {}", description.to_lowercase(), target),
            reason: format!(
                "Access to '{}' blocked ({}: {}).",
                target, category, description
            ),
        },
        Severity::Warn => HookDecision::Warn {
            category: category.into(),
            event: format!("{} \u{2014} {}", description.to_lowercase(), target),
            user_reason: format!(
                "LLM accessed '{}' ({}: {}). Behavior flagged.",
                target, category, description
            ),
            llm_context: format!(
                "Your access to '{}' was flagged as {} behavior ({}). \
                 The user has been notified. If this access is necessary \
                 for your task, explain why to the user.",
                target, category, description
            ),
        },
    }
}
