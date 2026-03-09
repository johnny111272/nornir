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

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_rules: embedded TOML ────────────────────────────────

    #[test]
    fn parse_rules_succeeds_on_embedded_toml() {
        let rules = parse_rules(RULES_TOML);
        assert!(rules.is_ok(), "Embedded RULES_TOML must parse successfully");
    }

    #[test]
    fn parse_rules_contains_floor_rules() {
        let rules = parse_rules(RULES_TOML).unwrap();
        assert!(!rules.floor.is_empty(), "Floor rules must not be empty");

        let patterns: Vec<&str> = rules.floor.iter().map(|r| r.pattern.as_str()).collect();
        assert!(patterns.contains(&"/.ssh/"), "Must block SSH keys");
        assert!(patterns.contains(&"/.aws/"), "Must block AWS credentials");
        assert!(patterns.contains(&"/.gnupg/"), "Must block GPG keyrings");
    }

    #[test]
    fn parse_rules_contains_probing_rules() {
        let rules = parse_rules(RULES_TOML).unwrap();
        assert!(!rules.probing.is_empty(), "Probing rules must not be empty");

        let patterns: Vec<&str> = rules.probing.iter().map(|r| r.pattern.as_str()).collect();
        assert!(patterns.contains(&"/.claude/hooks/"), "Must detect hook probing");
        assert!(patterns.contains(&"/.gleipnir/"), "Must detect gleipnir probing");
        assert!(patterns.contains(&"/.claude/settings"), "Must detect settings probing");
    }

    #[test]
    fn parse_rules_invalid_toml_returns_error() {
        let result = parse_rules("[[[broken");
        assert!(result.is_err());
    }

    // ── is_allowed_path ───────────────────────────────────────────

    #[test]
    fn is_allowed_path_matches_prefix() {
        let allow = vec!["/home/user/project/".to_string()];
        assert!(is_allowed_path("/home/user/project/src/main.rs", &allow));
    }

    #[test]
    fn is_allowed_path_no_match() {
        let allow = vec!["/home/user/project/".to_string()];
        assert!(!is_allowed_path("/etc/passwd", &allow));
    }

    #[test]
    fn is_allowed_path_empty_list() {
        let allow: Vec<String> = vec![];
        assert!(!is_allowed_path("/any/path", &allow));
    }

    #[test]
    fn is_allowed_path_multiple_prefixes() {
        let allow = vec!["/path/a/".to_string(), "/path/b/".to_string()];
        assert!(is_allowed_path("/path/b/file.rs", &allow));
        assert!(!is_allowed_path("/path/c/file.rs", &allow));
    }

    #[test]
    fn is_allowed_path_exact_match() {
        let allow = vec!["/exact/path".to_string()];
        assert!(is_allowed_path("/exact/path", &allow));
    }

    #[test]
    fn is_allowed_path_partial_prefix_no_false_positive() {
        // "/home/user" should NOT match "/home/username"
        let allow = vec!["/home/user/".to_string()];
        assert!(!is_allowed_path("/home/username/secret", &allow));
    }

    // ── make_decision ─────────────────────────────────────────────

    #[test]
    fn make_decision_block_returns_deny() {
        let decision = make_decision(
            Severity::Block,
            "probing",
            "Hook scripts",
            "/home/.claude/hooks/myhook",
        );
        match decision {
            HookDecision::Deny { category, reason, .. } => {
                assert_eq!(category, "probing");
                assert!(reason.contains("blocked"));
                assert!(reason.contains("/home/.claude/hooks/myhook"));
            }
            _ => panic!("Block severity must produce Deny"),
        }
    }

    #[test]
    fn make_decision_warn_returns_warn() {
        let decision = make_decision(
            Severity::Warn,
            "gaming",
            "Circumvention attempt",
            "/some/path",
        );
        match decision {
            HookDecision::Warn {
                category,
                user_reason,
                llm_context,
                ..
            } => {
                assert_eq!(category, "gaming");
                assert!(user_reason.contains("flagged"));
                assert!(llm_context.contains("gaming"));
            }
            _ => panic!("Warn severity must produce Warn"),
        }
    }

    // ── decide: integration via constructed HookInput ─────────────

    fn make_hook_input_no_path() -> HookInput {
        HookInput {
            tool_name: Some("Read".to_string()),
            tool_input: serde_json::json!({}),
        }
    }

    #[test]
    fn decide_floor_ssh_always_denied() {
        // Floor rules fire regardless of config — test with direct parse_rules + logic
        let rules = parse_rules(RULES_TOML).unwrap();
        let target = "/home/user/.ssh/id_rsa";
        for rule in &rules.floor {
            if target.contains(&rule.pattern) {
                // Confirmed: floor rule matches SSH path
                return;
            }
        }
        panic!("Floor rules must match /.ssh/ path");
    }

    #[test]
    fn decide_floor_aws_always_denied() {
        let rules = parse_rules(RULES_TOML).unwrap();
        let target = "/home/user/.aws/credentials";
        let matched = rules.floor.iter().any(|r| target.contains(&r.pattern));
        assert!(matched, "Floor rules must match /.aws/ path");
    }

    #[test]
    fn decide_floor_gnupg_always_denied() {
        let rules = parse_rules(RULES_TOML).unwrap();
        let target = "/home/user/.gnupg/secring.gpg";
        let matched = rules.floor.iter().any(|r| target.contains(&r.pattern));
        assert!(matched, "Floor rules must match /.gnupg/ path");
    }

    #[test]
    fn decide_floor_kube_always_denied() {
        let rules = parse_rules(RULES_TOML).unwrap();
        let target = "/home/user/.kube/config";
        let matched = rules.floor.iter().any(|r| target.contains(&r.pattern));
        assert!(matched, "Floor rules must match /.kube/config path");
    }

    #[test]
    fn decide_floor_docker_always_denied() {
        let rules = parse_rules(RULES_TOML).unwrap();
        let target = "/home/user/.docker/config.json";
        let matched = rules.floor.iter().any(|r| target.contains(&r.pattern));
        assert!(matched, "Floor rules must match /.docker/config.json path");
    }

    #[test]
    fn decide_floor_netrc_always_denied() {
        let rules = parse_rules(RULES_TOML).unwrap();
        let target = "/home/user/.netrc";
        let matched = rules.floor.iter().any(|r| target.contains(&r.pattern));
        assert!(matched, "Floor rules must match /.netrc path");
    }

    #[test]
    fn decide_probing_hooks_detected() {
        let rules = parse_rules(RULES_TOML).unwrap();
        let target = "/home/user/.claude/hooks/pre_tool";
        let matched = rules.probing.iter().any(|r| target.contains(&r.pattern));
        assert!(matched, "Probing rules must match /.claude/hooks/ path");
    }

    #[test]
    fn decide_probing_gleipnir_detected() {
        let rules = parse_rules(RULES_TOML).unwrap();
        let target = "/project/.gleipnir/rules.toml";
        let matched = rules.probing.iter().any(|r| target.contains(&r.pattern));
        assert!(matched, "Probing rules must match /.gleipnir/ path");
    }

    #[test]
    fn decide_probing_settings_detected() {
        let rules = parse_rules(RULES_TOML).unwrap();
        let target = "/home/user/.claude/settings.json";
        let matched = rules.probing.iter().any(|r| target.contains(&r.pattern));
        assert!(matched, "Probing rules must match /.claude/settings path");
    }

    #[test]
    fn decide_benign_path_not_flagged() {
        let rules = parse_rules(RULES_TOML).unwrap();
        let target = "/home/user/project/src/main.rs";
        let floor_match = rules.floor.iter().any(|r| target.contains(&r.pattern));
        let probing_match = rules.probing.iter().any(|r| target.contains(&r.pattern));
        let gaming_match = rules.gaming.iter().any(|r| target.contains(&r.pattern));
        assert!(!floor_match, "Benign path must not match floor");
        assert!(!probing_match, "Benign path must not match probing");
        assert!(!gaming_match, "Benign path must not match gaming");
    }

    #[test]
    fn decide_no_path_in_input_allows() {
        // When tool_input has no file_path or path, decide() returns Allow
        let input = make_hook_input_no_path();
        // We can't call decide() directly due to parse_config() reading CLI args,
        // but we can verify the extraction logic:
        let target = input.tool_input
            .get("file_path")
            .and_then(|v| v.as_str())
            .or_else(|| input.tool_input.get("path").and_then(|v| v.as_str()));
        assert!(target.is_none(), "No path should be extractable");
    }

    #[test]
    fn decide_path_field_also_works() {
        // Glob/Grep tools use "path" instead of "file_path"
        let input = HookInput {
            tool_name: Some("Grep".to_string()),
            tool_input: serde_json::json!({ "path": "/home/user/.ssh/keys" }),
        };
        let target = input.tool_input
            .get("file_path")
            .and_then(|v| v.as_str())
            .or_else(|| input.tool_input.get("path").and_then(|v| v.as_str()));
        assert_eq!(target, Some("/home/user/.ssh/keys"));
    }
}
