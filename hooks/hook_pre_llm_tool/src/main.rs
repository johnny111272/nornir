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
use hook_io::{DecisionInput, HookDecision, HookInput};

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
    decide_inner(input, &config, &rules)
}

/// Core decision logic, separated from config/rules parsing for testability.
fn decide_inner(input: &HookInput, config: &Config, rules: &Rules) -> HookDecision {
    let target = match input.target_path() {
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
    if let Some(default_severity) = config.probing {
        for rule in &rules.probing {
            if target.contains(&rule.pattern) {
                return hook_io::make_decision(&DecisionInput {
                    severity: rule.severity.unwrap_or(default_severity),
                    category: "probing",
                    description: &rule.description,
                    subject: target,
                    verb_past: "accessed",
                    verb_present: "access",
                    max_subject_len: None,
                });
            }
        }
    }

    // Layer 3: Gaming
    if let Some(default_severity) = config.gaming {
        for rule in &rules.gaming {
            if target.contains(&rule.pattern) {
                return hook_io::make_decision(&DecisionInput {
                    severity: rule.severity.unwrap_or(default_severity),
                    category: "gaming",
                    description: &rule.description,
                    subject: target,
                    verb_past: "accessed",
                    verb_present: "access",
                    max_subject_len: None,
                });
            }
        }
    }

    // Layer 4: Allow everything else
    HookDecision::Allow
}

fn is_allowed_path(target: &str, allow_paths: &[String]) -> bool {
    allow_paths.iter().any(|prefix| target.starts_with(prefix))
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

    // ── make_decision (via hook_io) ─────────────────────────────────

    fn tool_decision(severity: Severity, category: &str, description: &str, target: &str) -> HookDecision {
        hook_io::make_decision(&DecisionInput {
            severity, category, description, subject: target,
            verb_past: "accessed", verb_present: "access", max_subject_len: None,
        })
    }

    #[test]
    fn make_decision_block_returns_deny() {
        let decision = tool_decision(Severity::Block, "probing", "Hook scripts", "/home/.claude/hooks/myhook");
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
        let decision = tool_decision(Severity::Warn, "gaming", "Circumvention attempt", "/some/path");
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

    // ── decide_inner: integration tests ─────────────────────────────

    fn default_rules() -> Rules {
        parse_rules(RULES_TOML).unwrap()
    }

    fn config_all_block() -> Config {
        Config { probing: Some(Severity::Block), gaming: Some(Severity::Block), allow_paths: vec![] }
    }

    fn config_all_warn() -> Config {
        Config { probing: Some(Severity::Warn), gaming: Some(Severity::Warn), allow_paths: vec![] }
    }

    fn config_disabled() -> Config {
        Config { probing: None, gaming: None, allow_paths: vec![] }
    }

    fn make_input(path: &str) -> HookInput {
        HookInput {
            tool_name: Some("Read".to_string()),
            tool_input: serde_json::json!({ "file_path": path }),
        }
    }

    fn make_input_path_field(path: &str) -> HookInput {
        HookInput {
            tool_name: Some("Grep".to_string()),
            tool_input: serde_json::json!({ "path": path }),
        }
    }

    // Layer 1: Floor — always deny regardless of config

    #[test]
    fn decide_floor_ssh_denied() {
        let d = decide_inner(&make_input("/home/user/.ssh/id_rsa"), &config_disabled(), &default_rules());
        match d {
            HookDecision::Deny { category, .. } => assert_eq!(category, "floor"),
            _ => panic!("Floor must deny SSH access"),
        }
    }

    #[test]
    fn decide_floor_aws_denied() {
        let d = decide_inner(&make_input("/home/user/.aws/credentials"), &config_disabled(), &default_rules());
        assert!(matches!(d, HookDecision::Deny { .. }));
    }

    #[test]
    fn decide_floor_gnupg_denied() {
        let d = decide_inner(&make_input("/home/user/.gnupg/secring.gpg"), &config_disabled(), &default_rules());
        assert!(matches!(d, HookDecision::Deny { .. }));
    }

    #[test]
    fn decide_floor_kube_denied() {
        let d = decide_inner(&make_input("/home/user/.kube/config"), &config_disabled(), &default_rules());
        assert!(matches!(d, HookDecision::Deny { .. }));
    }

    #[test]
    fn decide_floor_docker_denied() {
        let d = decide_inner(&make_input("/home/user/.docker/config.json"), &config_disabled(), &default_rules());
        assert!(matches!(d, HookDecision::Deny { .. }));
    }

    #[test]
    fn decide_floor_netrc_denied() {
        let d = decide_inner(&make_input("/home/user/.netrc"), &config_disabled(), &default_rules());
        assert!(matches!(d, HookDecision::Deny { .. }));
    }

    #[test]
    fn decide_floor_overrides_allow_paths() {
        // allow_paths cannot exempt floor rules
        let config = Config {
            probing: Some(Severity::Block),
            gaming: Some(Severity::Block),
            allow_paths: vec!["/home/user/.ssh/".to_string()],
        };
        let d = decide_inner(&make_input("/home/user/.ssh/id_rsa"), &config, &default_rules());
        match d {
            HookDecision::Deny { category, .. } => assert_eq!(category, "floor"),
            _ => panic!("Floor must deny even with allow_paths"),
        }
    }

    // Allow path exemption (probing/gaming only)

    #[test]
    fn decide_allow_path_exempts_probing() {
        let config = Config {
            probing: Some(Severity::Block),
            gaming: None,
            allow_paths: vec!["/home/user/.claude/hooks/".to_string()],
        };
        let d = decide_inner(&make_input("/home/user/.claude/hooks/pre_tool"), &config, &default_rules());
        assert!(matches!(d, HookDecision::Allow));
    }

    // Layer 2: Probing

    #[test]
    fn decide_probing_block_denies() {
        let config = Config { probing: Some(Severity::Block), gaming: None, allow_paths: vec![] };
        let d = decide_inner(&make_input("/home/user/.claude/hooks/pre_tool"), &config, &default_rules());
        match d {
            HookDecision::Deny { category, .. } => assert_eq!(category, "probing"),
            _ => panic!("Probing block must deny"),
        }
    }

    #[test]
    fn decide_probing_warn_warns() {
        let config = Config { probing: Some(Severity::Warn), gaming: None, allow_paths: vec![] };
        let d = decide_inner(&make_input("/home/user/.claude/hooks/pre_tool"), &config, &default_rules());
        match d {
            HookDecision::Warn { category, .. } => assert_eq!(category, "probing"),
            _ => panic!("Probing warn must warn"),
        }
    }

    #[test]
    fn decide_probing_disabled_allows() {
        let config = Config { probing: None, gaming: None, allow_paths: vec![] };
        let d = decide_inner(&make_input("/home/user/.claude/hooks/pre_tool"), &config, &default_rules());
        assert!(matches!(d, HookDecision::Allow));
    }

    #[test]
    fn decide_probing_gleipnir_detected() {
        let d = decide_inner(&make_input("/project/.gleipnir/rules.toml"), &config_all_block(), &default_rules());
        match d {
            HookDecision::Deny { category, .. } => assert_eq!(category, "probing"),
            _ => panic!("Gleipnir probing must be detected"),
        }
    }

    #[test]
    fn decide_probing_settings_uses_per_rule_ask() {
        // settings rule has severity = "ask", overriding the category default
        let d = decide_inner(&make_input("/home/user/.claude/settings.json"), &config_all_block(), &default_rules());
        match d {
            HookDecision::Ask { category, .. } => assert_eq!(category, "probing"),
            _ => panic!("Settings must use per-rule severity (ask), not category default"),
        }
    }

    // Layer 3: Gaming

    #[test]
    fn decide_gaming_block_denies() {
        // Use a gaming pattern — need to check what gaming rules exist
        let rules = default_rules();
        if rules.gaming.is_empty() {
            // No gaming rules currently defined — skip
            return;
        }
        let target = format!("/some/path/{}", rules.gaming[0].pattern);
        let config = Config { probing: None, gaming: Some(Severity::Block), allow_paths: vec![] };
        let d = decide_inner(&make_input(&target), &config, &rules);
        match d {
            HookDecision::Deny { category, .. } => assert_eq!(category, "gaming"),
            _ => panic!("Gaming block must deny"),
        }
    }

    #[test]
    fn decide_gaming_warn_warns() {
        let rules = default_rules();
        if rules.gaming.is_empty() {
            return;
        }
        let target = format!("/some/path/{}", rules.gaming[0].pattern);
        let config = Config { probing: None, gaming: Some(Severity::Warn), allow_paths: vec![] };
        let d = decide_inner(&make_input(&target), &config, &rules);
        match d {
            HookDecision::Warn { category, .. } => assert_eq!(category, "gaming"),
            _ => panic!("Gaming warn must warn"),
        }
    }

    // Layer 4: Allow

    #[test]
    fn decide_benign_path_allowed() {
        let d = decide_inner(&make_input("/home/user/project/src/main.rs"), &config_all_block(), &default_rules());
        assert!(matches!(d, HookDecision::Allow));
    }

    #[test]
    fn decide_no_path_in_input_allowed() {
        let input = HookInput {
            tool_name: Some("Read".to_string()),
            tool_input: serde_json::json!({}),
        };
        let d = decide_inner(&input, &config_all_block(), &default_rules());
        assert!(matches!(d, HookDecision::Allow));
    }

    #[test]
    fn decide_path_field_works() {
        // Glob/Grep use "path" instead of "file_path"
        let d = decide_inner(&make_input_path_field("/home/user/.ssh/keys"), &config_disabled(), &default_rules());
        match d {
            HookDecision::Deny { category, .. } => assert_eq!(category, "floor"),
            _ => panic!("path field must be recognized"),
        }
    }

    // Priority: probing checked before gaming

    #[test]
    fn decide_probing_takes_priority_over_gaming() {
        // A path matching probing should return probing decision even if gaming is also configured
        let d = decide_inner(
            &make_input("/home/user/.claude/hooks/pre_tool"),
            &config_all_warn(),
            &default_rules(),
        );
        match d {
            HookDecision::Warn { category, .. } => assert_eq!(category, "probing"),
            _ => panic!("Probing must take priority over gaming"),
        }
    }
}
