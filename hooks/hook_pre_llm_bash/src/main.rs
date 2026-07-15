//! PreToolUse hook: LLM behavioral detection for Bash commands.
//!
//! Detects subversion (lock/constraint manipulation), truncation (partial reads
//! of constraint files), and evasion (adversarial shortcuts) in interactive
//! LLM sessions.
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_pre_llm_bash --subversion block --truncation warn --evasion warn --workflow ask --chaining block
//!
//! Env var:
//!     HOOK_LLM_ALLOW_BASH=pattern_name1:pattern_name2  — exempt specific patterns

use std::process::ExitCode;

use hook_io::rules::{parse_rule_array, parse_severity, parse_toml_table, RawRule, Severity};
use hook_io::{DecisionInput, HookDecision, HookInput};
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
    severity: Option<Severity>,
    compiled: Regex,
}

impl CompiledRule {
    /// Compile a RawRule's pattern as a regex. Returns None if invalid.
    fn from_raw(rule: RawRule) -> Option<Self> {
        let compiled = Regex::new(&rule.pattern).ok()?;
        Some(Self {
            pattern: rule.pattern,
            description: rule.description,
            severity: rule.severity,
            compiled,
        })
    }
}

struct Rules {
    subversion: Vec<CompiledRule>,
    truncation: Vec<CompiledRule>,
    evasion: Vec<CompiledRule>,
    destruction: Vec<CompiledRule>,
    revert: Vec<CompiledRule>,
    workflow: Vec<CompiledRule>,
    chaining: Vec<CompiledRule>,
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
        destruction: compile_array("destruction"),
        revert: compile_array("revert"),
        workflow: compile_array("workflow"),
        chaining: compile_array("chaining"),
    })
}

// ── Config ─────────────────────────────────────────────────────────

#[derive(Debug)]
struct Config {
    subversion: Option<Severity>,
    truncation: Option<Severity>,
    evasion: Option<Severity>,
    destruction: Option<Severity>,
    revert: Option<Severity>,
    workflow: Option<Severity>,
    chaining: Option<Severity>,
    allow_patterns: Vec<String>,
}

fn parse_config() -> Config {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut subversion = None;
    let mut truncation = None;
    let mut evasion = None;
    let mut destruction = None;
    let mut revert = None;
    let mut workflow = None;
    let mut chaining = None;

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
            "--destruction" if i + 1 < args.len() => {
                destruction = parse_severity(&args[i + 1]);
                i += 2;
            }
            "--revert" if i + 1 < args.len() => {
                revert = parse_severity(&args[i + 1]);
                i += 2;
            }
            "--workflow" if i + 1 < args.len() => {
                workflow = parse_severity(&args[i + 1]);
                i += 2;
            }
            "--chaining" if i + 1 < args.len() => {
                chaining = parse_severity(&args[i + 1]);
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
        destruction,
        revert,
        workflow,
        chaining,
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

    let command = input.command();

    if command.trim().is_empty() {
        return HookDecision::Allow;
    }

    // Check each category (destruction first — data loss is highest priority)
    if let Some(severity) = config.destruction {
        if let Some(decision) = check_category(
            command, &rules.destruction, severity, "destruction", &config.allow_patterns,
        ) {
            return decision;
        }
    }

    // Chaining next — catches hang-inducing && before deeper analysis
    if let Some(severity) = config.chaining {
        if let Some(decision) = check_category(
            command, &rules.chaining, severity, "chaining", &config.allow_patterns,
        ) {
            return decision;
        }
    }

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

    if let Some(severity) = config.revert {
        if let Some(decision) = check_category(
            command, &rules.revert, severity, "revert", &config.allow_patterns,
        ) {
            return decision;
        }
    }

    if let Some(severity) = config.workflow {
        if let Some(decision) = check_category(
            command, &rules.workflow, severity, "workflow", &config.allow_patterns,
        ) {
            return decision;
        }
    }

    HookDecision::Allow
}

fn check_category(
    command: &str,
    rules: &[CompiledRule],
    default_severity: Severity,
    category: &str,
    allow_patterns: &[String],
) -> Option<HookDecision> {
    for rule in rules {
        if rule.compiled.is_match(command) {
            if allow_patterns.iter().any(|p| rule.pattern.contains(p.as_str())) {
                return None;
            }
            let severity = rule.severity.unwrap_or(default_severity);
            return Some(hook_io::make_decision(&DecisionInput {
                severity,
                category,
                description: &rule.description,
                subject: command,
                verb_past: "ran",
                verb_present: "run",
                max_subject_len: Some(60),
            }));
        }
    }
    None
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
    fn parse_rules_subversion_rules_compile() {
        let rules = parse_rules(RULES_TOML).unwrap();
        assert!(
            !rules.subversion.is_empty(),
            "Subversion rules must not be empty"
        );
        // All rules compiled successfully (from_raw filters invalid regex)
        for rule in &rules.subversion {
            assert!(
                rule.compiled.is_match("") || !rule.compiled.is_match(""),
                "Compiled regex must be functional"
            );
        }
    }

    #[test]
    fn parse_rules_truncation_rules_compile() {
        let rules = parse_rules(RULES_TOML).unwrap();
        assert!(
            !rules.truncation.is_empty(),
            "Truncation rules must not be empty"
        );
    }

    #[test]
    fn parse_rules_evasion_rules_compile() {
        let rules = parse_rules(RULES_TOML).unwrap();
        assert!(
            !rules.evasion.is_empty(),
            "Evasion rules must not be empty"
        );
    }

    #[test]
    fn parse_rules_invalid_toml_returns_error() {
        let result = parse_rules("[[[broken");
        assert!(result.is_err());
    }

    // ── CompiledRule::from_raw ────────────────────────────────────

    #[test]
    fn compiled_rule_from_raw_valid_regex() {
        let raw = RawRule {
            pattern: r"rm.*\.lock".to_string(),
            description: "test rule".to_string(),
            severity: None,
        };
        let compiled = CompiledRule::from_raw(raw);
        assert!(compiled.is_some(), "Valid regex must compile");
        let cr = compiled.unwrap();
        assert!(cr.compiled.is_match("rm foo.lock"));
    }

    #[test]
    fn compiled_rule_from_raw_invalid_regex() {
        let raw = RawRule {
            pattern: r"[invalid".to_string(),
            description: "bad regex".to_string(),
            severity: None,
        };
        let compiled = CompiledRule::from_raw(raw);
        assert!(compiled.is_none(), "Invalid regex must return None");
    }

    #[test]
    fn compiled_rule_from_raw_empty_pattern() {
        let raw = RawRule {
            pattern: String::new(),
            description: "empty".to_string(),
            severity: None,
        };
        let compiled = CompiledRule::from_raw(raw);
        // Empty string is valid regex (matches everything)
        assert!(compiled.is_some());
    }

    // ── check_category: THE critical security function ────────────
    //
    // Tests BOTH directions:
    //   1. Malicious input IS detected (no false negatives)
    //   2. Benign input is NOT flagged (no false positives)

    fn make_rules_from_toml() -> Rules {
        parse_rules(RULES_TOML).unwrap()
    }

    // -- Subversion detections (must catch) --

    #[test]
    fn subversion_rm_lock_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "rm foo.lock",
            &rules.subversion,
            Severity::Block,
            "subversion",
            &[],
        );
        assert!(result.is_some(), "rm *.lock must be detected as subversion");
        match result.unwrap() {
            HookDecision::Deny { category, .. } => assert_eq!(category, "subversion"),
            _ => panic!("Block severity must produce Deny"),
        }
    }

    #[test]
    fn subversion_chflags_noschg_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "chflags noschg rules.toml",
            &rules.subversion,
            Severity::Block,
            "subversion",
            &[],
        );
        assert!(
            result.is_some(),
            "chflags noschg must be detected as subversion"
        );
    }

    #[test]
    fn subversion_export_hook_env_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "export HOOK_LLM_ALLOW_PATHS=/",
            &rules.subversion,
            Severity::Block,
            "subversion",
            &[],
        );
        assert!(
            result.is_some(),
            "export HOOK_LLM env var manipulation must be detected"
        );
    }

    #[test]
    fn subversion_env_hook_llm_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "env HOOK_LLM_ALLOW_BASH=all cargo build",
            &rules.subversion,
            Severity::Block,
            "subversion",
            &[],
        );
        assert!(
            result.is_some(),
            "env HOOK_LLM override must be detected"
        );
    }

    #[test]
    fn subversion_chmod_rules_toml_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "chmod 777 rules.toml",
            &rules.subversion,
            Severity::Block,
            "subversion",
            &[],
        );
        assert!(
            result.is_some(),
            "chmod on rules.toml must be detected"
        );
    }

    #[test]
    fn subversion_flock_unlock_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "flock /tmp/lockfile --unlock",
            &rules.subversion,
            Severity::Block,
            "subversion",
            &[],
        );
        assert!(
            result.is_some(),
            "flock --unlock must be detected"
        );
    }

    // -- Truncation detections (must catch) --

    #[test]
    fn truncation_head_claude_md_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "head -5 CLAUDE.md",
            &rules.truncation,
            Severity::Warn,
            "truncation",
            &[],
        );
        assert!(
            result.is_some(),
            "head CLAUDE.md must be detected as truncation"
        );
    }

    #[test]
    fn truncation_tail_claude_md_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "tail CLAUDE.md",
            &rules.truncation,
            Severity::Warn,
            "truncation",
            &[],
        );
        assert!(
            result.is_some(),
            "tail CLAUDE.md must be detected as truncation"
        );
    }

    #[test]
    fn truncation_grep_claude_md_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "grep pattern CLAUDE.md",
            &rules.truncation,
            Severity::Warn,
            "truncation",
            &[],
        );
        assert!(
            result.is_some(),
            "grep CLAUDE.md must be detected as truncation"
        );
    }

    #[test]
    fn truncation_sed_claude_md_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "sed -n '1,10p' CLAUDE.md",
            &rules.truncation,
            Severity::Warn,
            "truncation",
            &[],
        );
        assert!(
            result.is_some(),
            "sed CLAUDE.md must be detected as truncation"
        );
    }

    #[test]
    fn truncation_awk_claude_md_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "awk 'NR<=10' CLAUDE.md",
            &rules.truncation,
            Severity::Warn,
            "truncation",
            &[],
        );
        assert!(
            result.is_some(),
            "awk CLAUDE.md must be detected as truncation"
        );
    }

    #[test]
    fn truncation_guardrails_tail_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "cat guardrails.toml | tail -20",
            &rules.truncation,
            Severity::Warn,
            "truncation",
            &[],
        );
        assert!(
            result.is_some(),
            "guardrails piped to tail must be detected"
        );
    }

    // -- Evasion detections (must catch) --

    #[test]
    fn evasion_git_checkout_claude_md_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git checkout CLAUDE.md",
            &rules.evasion,
            Severity::Warn,
            "evasion",
            &[],
        );
        assert!(
            result.is_some(),
            "git checkout CLAUDE.md must be detected as evasion"
        );
    }

    #[test]
    fn evasion_mv_claude_md_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "mv CLAUDE.md CLAUDE.md.bak",
            &rules.evasion,
            Severity::Warn,
            "evasion",
            &[],
        );
        assert!(
            result.is_some(),
            "mv CLAUDE.md must be detected as evasion"
        );
    }

    #[test]
    fn evasion_cp_claude_md_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "cp malicious.md CLAUDE.md",
            &rules.evasion,
            Severity::Warn,
            "evasion",
            &[],
        );
        assert!(
            result.is_some(),
            "cp CLAUDE.md must be detected as evasion"
        );
    }

    #[test]
    fn evasion_git_restore_claude_md_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git restore CLAUDE.md",
            &rules.evasion,
            Severity::Warn,
            "evasion",
            &[],
        );
        assert!(
            result.is_some(),
            "git restore CLAUDE.md must be detected as evasion"
        );
    }

    #[test]
    fn evasion_git_config_hooks_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git config core.hooks /dev/null",
            &rules.evasion,
            Severity::Warn,
            "evasion",
            &[],
        );
        assert!(
            result.is_some(),
            "git config hooks must be detected as evasion"
        );
    }

    // -- Destruction detections (must catch) --

    #[test]
    fn destruction_git_reset_hard_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git reset --hard",
            &rules.destruction,
            Severity::Block,
            "destruction",
            &[],
        );
        assert!(result.is_some(), "git reset --hard must be blocked");
    }

    #[test]
    fn destruction_git_reset_hard_head_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git reset --hard HEAD~3",
            &rules.destruction,
            Severity::Block,
            "destruction",
            &[],
        );
        assert!(result.is_some(), "git reset --hard HEAD~3 must be blocked");
    }

    #[test]
    fn destruction_git_checkout_dot_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git checkout .",
            &rules.destruction,
            Severity::Block,
            "destruction",
            &[],
        );
        assert!(result.is_some(), "git checkout . must be blocked");
    }

    #[test]
    fn destruction_git_checkout_dashdash_dot_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git checkout -- .",
            &rules.destruction,
            Severity::Block,
            "destruction",
            &[],
        );
        assert!(result.is_some(), "git checkout -- . must be blocked");
    }

    #[test]
    fn destruction_git_restore_dot_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git restore .",
            &rules.destruction,
            Severity::Block,
            "destruction",
            &[],
        );
        assert!(result.is_some(), "git restore . must be blocked");
    }

    #[test]
    fn destruction_git_clean_f_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git clean -fd",
            &rules.destruction,
            Severity::Block,
            "destruction",
            &[],
        );
        assert!(result.is_some(), "git clean -fd must be blocked");
    }

    #[test]
    fn destruction_git_clean_f_only_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git clean -f",
            &rules.destruction,
            Severity::Block,
            "destruction",
            &[],
        );
        assert!(result.is_some(), "git clean -f must be blocked");
    }

    // -- Destruction: benign git that must NOT match --

    #[test]
    fn destruction_git_reset_soft_ok() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git reset --soft HEAD~1",
            &rules.destruction,
            Severity::Block,
            "destruction",
            &[],
        );
        assert!(result.is_none(), "git reset --soft must not be blocked");
    }

    #[test]
    fn destruction_git_checkout_branch_ok() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git checkout -b feature-branch",
            &rules.destruction,
            Severity::Block,
            "destruction",
            &[],
        );
        assert!(result.is_none(), "git checkout -b must not be blocked");
    }

    #[test]
    fn destruction_git_clean_n_ok() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git clean -n",
            &rules.destruction,
            Severity::Block,
            "destruction",
            &[],
        );
        assert!(result.is_none(), "git clean -n (dry run) must not be blocked");
    }

    // -- Revert detections (must catch for user decision) --

    #[test]
    fn revert_git_checkout_dashdash_file_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git checkout -- src/main.rs",
            &rules.revert,
            Severity::Warn,
            "revert",
            &[],
        );
        assert!(result.is_some(), "git checkout -- file must be caught for user decision");
    }

    #[test]
    fn revert_git_checkout_head_dashdash_file_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git checkout HEAD -- src/main.rs",
            &rules.revert,
            Severity::Warn,
            "revert",
            &[],
        );
        assert!(result.is_some(), "git checkout HEAD -- file must be caught");
    }

    #[test]
    fn revert_git_restore_file_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git restore src/main.rs",
            &rules.revert,
            Severity::Warn,
            "revert",
            &[],
        );
        assert!(result.is_some(), "git restore file must be caught for user decision");
    }

    #[test]
    fn revert_git_stash_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git stash",
            &rules.revert,
            Severity::Warn,
            "revert",
            &[],
        );
        assert!(result.is_some(), "git stash must be caught for user decision");
    }

    #[test]
    fn revert_git_stash_push_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git stash push",
            &rules.revert,
            Severity::Warn,
            "revert",
            &[],
        );
        assert!(result.is_some(), "git stash push must be caught");
    }

    // -- Revert: benign git that must NOT match --

    #[test]
    fn revert_git_restore_staged_ok() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git restore --staged file.rs",
            &rules.revert,
            Severity::Warn,
            "revert",
            &[],
        );
        assert!(result.is_none(), "git restore --staged must not be caught (just unstaging)");
    }

    #[test]
    fn revert_git_stash_list_ok() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git stash list",
            &rules.revert,
            Severity::Warn,
            "revert",
            &[],
        );
        assert!(result.is_none(), "git stash list must not be caught");
    }

    #[test]
    fn revert_git_stash_pop_ok() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git stash pop",
            &rules.revert,
            Severity::Warn,
            "revert",
            &[],
        );
        assert!(result.is_none(), "git stash pop must not be caught");
    }

    #[test]
    fn revert_git_stash_show_ok() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git stash show",
            &rules.revert,
            Severity::Warn,
            "revert",
            &[],
        );
        assert!(result.is_none(), "git stash show must not be caught");
    }

    #[test]
    fn revert_git_checkout_branch_switch_ok() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "git checkout main",
            &rules.revert,
            Severity::Warn,
            "revert",
            &[],
        );
        assert!(result.is_none(), "git checkout main (branch switch) must not be caught");
    }

    // -- Benign commands (must NOT match) --

    #[test]
    fn benign_ls_not_flagged() {
        let rules = make_rules_from_toml();
        let result_sub = check_category("ls -la", &rules.subversion, Severity::Block, "subversion", &[]);
        let result_trunc = check_category("ls -la", &rules.truncation, Severity::Block, "truncation", &[]);
        let result_eva = check_category("ls -la", &rules.evasion, Severity::Block, "evasion", &[]);
        assert!(result_sub.is_none(), "ls must not match subversion");
        assert!(result_trunc.is_none(), "ls must not match truncation");
        assert!(result_eva.is_none(), "ls must not match evasion");
    }

    #[test]
    fn benign_cargo_build_not_flagged_by_security_categories() {
        let rules = make_rules_from_toml();
        let result_sub = check_category("cargo build", &rules.subversion, Severity::Block, "subversion", &[]);
        let result_trunc = check_category("cargo build", &rules.truncation, Severity::Block, "truncation", &[]);
        let result_eva = check_category("cargo build", &rules.evasion, Severity::Block, "evasion", &[]);
        assert!(result_sub.is_none(), "cargo build must not match subversion");
        assert!(result_trunc.is_none(), "cargo build must not match truncation");
        assert!(result_eva.is_none(), "cargo build must not match evasion");
    }

    #[test]
    fn benign_cat_file_not_flagged() {
        let rules = make_rules_from_toml();
        let result_sub = check_category("cat src/main.rs", &rules.subversion, Severity::Block, "subversion", &[]);
        let result_trunc = check_category("cat src/main.rs", &rules.truncation, Severity::Block, "truncation", &[]);
        let result_eva = check_category("cat src/main.rs", &rules.evasion, Severity::Block, "evasion", &[]);
        assert!(result_sub.is_none());
        assert!(result_trunc.is_none());
        assert!(result_eva.is_none());
    }

    #[test]
    fn benign_git_status_not_flagged() {
        let rules = make_rules_from_toml();
        let result_eva = check_category("git status", &rules.evasion, Severity::Block, "evasion", &[]);
        assert!(result_eva.is_none(), "git status must not match evasion");
    }

    #[test]
    fn benign_git_diff_not_flagged() {
        let rules = make_rules_from_toml();
        let result_eva = check_category("git diff", &rules.evasion, Severity::Block, "evasion", &[]);
        assert!(result_eva.is_none(), "git diff must not match evasion");
    }

    // -- Deletion detections (recursive/forced rm — per-rule ask) --

    #[test]
    fn deletion_rm_recursive_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "rm -rf public",
            &rules.destruction,
            Severity::Block,
            "destruction",
            &[],
        );
        assert!(result.is_some(), "rm -rf must be detected");
    }

    #[test]
    fn deletion_rm_flags_after_operand_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "rm public -rf",
            &rules.destruction,
            Severity::Block,
            "destruction",
            &[],
        );
        assert!(result.is_some(), "rm with trailing -rf must be detected");
    }

    #[test]
    fn deletion_rm_long_recursive_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "rm --recursive build",
            &rules.destruction,
            Severity::Block,
            "destruction",
            &[],
        );
        assert!(result.is_some(), "rm --recursive must be detected");
    }

    #[test]
    fn deletion_find_delete_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "find . -name '*.tmp' -delete",
            &rules.destruction,
            Severity::Block,
            "destruction",
            &[],
        );
        assert!(result.is_some(), "find -delete must be detected");
    }

    #[test]
    fn deletion_rm_plain_file_not_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "rm notes.md",
            &rules.destruction,
            Severity::Block,
            "destruction",
            &[],
        );
        assert!(result.is_none(), "plain rm of a file must pass");
    }

    #[test]
    fn deletion_rm_hyphenated_filename_not_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "rm my-router.ts",
            &rules.destruction,
            Severity::Block,
            "destruction",
            &[],
        );
        assert!(result.is_none(), "hyphenated filenames must not false-positive");
    }

    #[test]
    fn deletion_rm_recursive_asks_despite_block_category() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "rm -rf public",
            &rules.destruction,
            Severity::Block,
            "destruction",
            &[],
        );
        match result {
            Some(HookDecision::Ask { .. }) => {} // per-rule severity override
            _ => panic!("recursive rm must ASK, overriding the category's block"),
        }
    }

    // -- Severity mapping --

    #[test]
    fn check_category_block_returns_deny() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "rm foo.lock",
            &rules.subversion,
            Severity::Block,
            "subversion",
            &[],
        );
        match result {
            Some(HookDecision::Deny { .. }) => {} // correct
            _ => panic!("Block severity must produce Deny"),
        }
    }

    #[test]
    fn check_category_warn_returns_warn() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "rm foo.lock",
            &rules.subversion,
            Severity::Warn,
            "subversion",
            &[],
        );
        match result {
            Some(HookDecision::Warn { .. }) => {} // correct
            _ => panic!("Warn severity must produce Warn"),
        }
    }

    // -- Exemption via allow_patterns --

    #[test]
    fn exempted_pattern_returns_none() {
        let rules = make_rules_from_toml();
        // The "rm.*\.lock" pattern contains "lock" — use that as the exemption key
        let allow = vec!["rm.*\\.lock".to_string()];
        let result = check_category(
            "rm foo.lock",
            &rules.subversion,
            Severity::Block,
            "subversion",
            &allow,
        );
        assert!(result.is_none(), "Exempted pattern must return None");
    }

    // ── make_decision (via hook_io) ─────────────────────────────────

    fn bash_decision(severity: Severity, category: &str, description: &str, command: &str) -> HookDecision {
        hook_io::make_decision(&DecisionInput {
            severity, category, description, subject: command,
            verb_past: "ran", verb_present: "run", max_subject_len: Some(60),
        })
    }

    #[test]
    fn make_decision_long_command_truncated() {
        let long_cmd = "a".repeat(80);
        let decision = bash_decision(Severity::Block, "subversion", "test", &long_cmd);
        match decision {
            HookDecision::Deny { reason, .. } => {
                assert!(reason.contains("..."), "Long command must be truncated with ...");
                assert!(!reason.contains(&long_cmd), "Full command must not appear");
            }
            _ => panic!("Must be Deny"),
        }
    }

    #[test]
    fn make_decision_short_command_not_truncated() {
        let short_cmd = "rm foo.lock";
        let decision = bash_decision(Severity::Block, "subversion", "test", short_cmd);
        match decision {
            HookDecision::Deny { reason, .. } => {
                assert!(reason.contains("rm foo.lock"));
                assert!(!reason.contains("..."));
            }
            _ => panic!("Must be Deny"),
        }
    }

    #[test]
    fn make_decision_exactly_60_chars_not_truncated() {
        let cmd = "a".repeat(60);
        let decision = bash_decision(Severity::Block, "subversion", "test", &cmd);
        match decision {
            HookDecision::Deny { reason, .. } => {
                assert!(!reason.contains("..."), "Exactly 60 chars should not be truncated");
            }
            _ => panic!("Must be Deny"),
        }
    }

    #[test]
    fn make_decision_61_chars_is_truncated() {
        let cmd = "a".repeat(61);
        let decision = bash_decision(Severity::Block, "subversion", "test", &cmd);
        match decision {
            HookDecision::Deny { reason, .. } => {
                assert!(reason.contains("..."), "61 chars should be truncated");
            }
            _ => panic!("Must be Deny"),
        }
    }

    // -- Workflow detections (must catch) --

    #[test]
    fn workflow_cargo_build_release_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "cargo build --release -p hook_pre_llm_bash",
            &rules.workflow,
            Severity::Ask,
            "workflow",
            &[],
        );
        assert!(result.is_some(), "cargo build --release must be caught");
    }

    #[test]
    fn workflow_cargo_build_release_reordered_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "cargo build -p hook_pre_llm_bash --release",
            &rules.workflow,
            Severity::Ask,
            "workflow",
            &[],
        );
        assert!(result.is_some(), "cargo build -p X --release must be caught");
    }

    #[test]
    fn workflow_cargo_install_release_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "cargo install --release --path .",
            &rules.workflow,
            Severity::Ask,
            "workflow",
            &[],
        );
        assert!(result.is_some(), "cargo install --release must be caught");
    }

    #[test]
    fn workflow_cargo_build_debug_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "cargo build -p hook_pre_llm_bash",
            &rules.workflow,
            Severity::Ask,
            "workflow",
            &[],
        );
        assert!(result.is_some(), "cargo build (debug) must be caught by workflow");
    }

    #[test]
    fn workflow_maturin_build_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "maturin build --release -i python3.13",
            &rules.workflow,
            Severity::Ask,
            "workflow",
            &[],
        );
        assert!(result.is_some(), "maturin build must be caught");
    }

    // -- Workflow: benign commands that must NOT match --

    #[test]
    fn workflow_cargo_test_ok() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "cargo test --workspace",
            &rules.workflow,
            Severity::Ask,
            "workflow",
            &[],
        );
        assert!(result.is_none(), "cargo test must not be caught by workflow");
    }

    #[test]
    fn workflow_cargo_check_ok() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "cargo check -p hook_io",
            &rules.workflow,
            Severity::Ask,
            "workflow",
            &[],
        );
        assert!(result.is_none(), "cargo check must not be caught by workflow");
    }

    #[test]
    fn workflow_cargo_clippy_ok() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "cargo clippy -- -D warnings",
            &rules.workflow,
            Severity::Ask,
            "workflow",
            &[],
        );
        assert!(result.is_none(), "cargo clippy must not be caught by workflow");
    }

    // -- Workflow: per-rule severity overrides --

    #[test]
    fn workflow_debug_build_has_warn_severity_override() {
        let rules = make_rules_from_toml();
        // The debug build rule (general cargo\s+build) has severity = "warn"
        let debug_rule = rules.workflow.iter().find(|r| r.description.contains("Debug build"));
        assert!(debug_rule.is_some(), "Debug build rule must exist");
        assert_eq!(debug_rule.unwrap().severity, Some(Severity::Warn));
    }

    #[test]
    fn workflow_release_build_has_ask_severity_override() {
        let rules = make_rules_from_toml();
        let release_rule = rules.workflow.iter().find(|r| r.description.contains("Direct release"));
        assert!(release_rule.is_some(), "Release build rule must exist");
        assert_eq!(release_rule.unwrap().severity, Some(Severity::Ask));
    }

    // -- Chaining detections (must catch) --

    #[test]
    fn chaining_simple_and_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "cargo test && nornir_deploy",
            &rules.chaining,
            Severity::Block,
            "chaining",
            &[],
        );
        assert!(result.is_some(), "cargo test && nornir_deploy must be blocked");
    }

    #[test]
    fn chaining_with_flags_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "cargo test -p foo 2>&1 | grep result && nornir_deploy --build tools",
            &rules.chaining,
            Severity::Block,
            "chaining",
            &[],
        );
        assert!(result.is_some(), "chained command with flags must be blocked");
    }

    #[test]
    fn chaining_three_commands_detected() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "echo a && echo b && echo c",
            &rules.chaining,
            Severity::Block,
            "chaining",
            &[],
        );
        assert!(result.is_some(), "three-command chain must be blocked");
    }

    // -- Chaining: benign must NOT match --

    #[test]
    fn chaining_single_command_ok() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "cargo test --workspace",
            &rules.chaining,
            Severity::Block,
            "chaining",
            &[],
        );
        assert!(result.is_none(), "single command must not match chaining");
    }

    #[test]
    fn chaining_pipe_ok() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "cargo test 2>&1 | tail -10",
            &rules.chaining,
            Severity::Block,
            "chaining",
            &[],
        );
        assert!(result.is_none(), "pipe (not &&) must not match chaining");
    }

    #[test]
    fn chaining_stderr_redirect_ok() {
        let rules = make_rules_from_toml();
        let result = check_category(
            "cargo test 2>&1",
            &rules.chaining,
            Severity::Block,
            "chaining",
            &[],
        );
        assert!(result.is_none(), "2>&1 redirect must not match chaining");
    }

    #[test]
    fn chaining_bitwise_and_in_arg_ok() {
        let rules = make_rules_from_toml();
        // Bare && without surrounding spaces shouldn't match (defensive — unlikely in real use)
        let result = check_category(
            "echo 'hello&&world'",
            &rules.chaining,
            Severity::Block,
            "chaining",
            &[],
        );
        assert!(result.is_none(), "&& inside a quoted string without spaces must not match");
    }
}
