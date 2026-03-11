//! Gleipnir: guardrail checks for Python source files.
//!
//! Pure computation library. No I/O — caller provides source bytes,
//! library returns violations. Used by saga_core as a direct dependency.

pub mod checks;
pub mod checks_rust;
pub mod classify;
pub mod config;
pub mod matrix;
pub mod parsing;
pub mod structures;

use std::collections::HashMap;
use std::sync::LazyLock;

pub use structures::{
    CheckConfig, CheckMessages, FileKind, ParsedSource, Severity, UserConfig, Violation,
};

// Embedded messages, parsed once on first access.
static MESSAGES_TOML: &str = include_str!("../gleipnir_messages.toml");

static MESSAGES: LazyLock<HashMap<String, CheckMessages>> = LazyLock::new(|| {
    toml::from_str(MESSAGES_TOML).expect("gleipnir_messages.toml parse error")
});

/// Get the static message fields for a check by name.
pub fn messages(check_name: &str) -> CheckMessages {
    MESSAGES
        .get(check_name)
        .cloned()
        .unwrap_or_default()
}

/// Run all applicable gleipnir checks on a Python source file.
///
/// Classifies the file, selects checks from the matrix, parses with
/// tree-sitter, runs checks, returns violations.
pub fn run_checks(
    file_path: &str,
    source: &[u8],
    user_config: Option<&UserConfig>,
) -> Vec<Violation> {
    let first_line = source
        .split(|&b| b == b'\n')
        .next()
        .and_then(|l| std::str::from_utf8(l).ok())
        .unwrap_or("");

    let kind = classify::classify_file(file_path, first_line);
    let config = CheckConfig::for_kind(kind, user_config);
    let entries = matrix::checks_for_kind(kind);

    let parsed = parsing::build_parsed_source(file_path, source);

    let mut violations = Vec::new();
    for entry in &entries {
        let mut check_violations = (entry.check_fn)(&parsed, &config);
        // Stamp each violation with the check name, severity, and messages
        let msgs = messages(entry.name);
        for viol in &mut check_violations {
            if viol.check_name.is_empty() {
                viol.check_name = entry.name.to_string();
            }
            viol.severity = entry.severity;
            if viol.detail.is_empty() {
                viol.detail = msgs.detail.clone();
            }
            if viol.signal.is_empty() {
                viol.signal = msgs.signal.clone();
            }
            if viol.direction.is_empty() {
                viol.direction = msgs.direction.clone();
            }
            if viol.canary.is_empty() {
                viol.canary = msgs.canary.clone();
            }
        }
        violations.extend(check_violations);
    }
    violations
}

/// Run all applicable gleipnir checks on a Rust source file.
///
/// Parses with tree-sitter-rust, runs Rust-specific checks, returns violations.
/// Skips test modules and #[test] functions for panic-on-failure checks.
pub fn run_checks_rust(file_path: &str, source: &[u8]) -> Vec<Violation> {
    let parsed = parsing::build_parsed_source_rust(file_path, source);
    let config = CheckConfig::for_kind(structures::FileKind::Outside, None);

    let rust_checks: &[(&str, Severity, fn(&structures::ParsedSource, &CheckConfig) -> Vec<Violation>)] = &[
        ("no_unwrap", Severity::Error, checks_rust::prohibited::check_no_unwrap),
    ];

    let mut violations = Vec::new();
    for &(name, severity, check_fn) in rust_checks {
        let mut check_violations = check_fn(&parsed, &config);
        let msgs = messages(name);
        for viol in &mut check_violations {
            if viol.check_name.is_empty() {
                viol.check_name = name.to_string();
            }
            viol.severity = severity;
            if viol.detail.is_empty() {
                viol.detail = msgs.detail.clone();
            }
            if viol.signal.is_empty() {
                viol.signal = msgs.signal.clone();
            }
            if viol.direction.is_empty() {
                viol.direction = msgs.direction.clone();
            }
            if viol.canary.is_empty() {
                viol.canary = msgs.canary.clone();
            }
        }
        violations.extend(check_violations);
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_load_successfully() {
        let msgs = messages("no_any_types");
        assert!(!msgs.detail.is_empty());
        assert!(!msgs.signal.is_empty());
        assert!(!msgs.direction.is_empty());
        assert!(!msgs.canary.is_empty());
    }

    #[test]
    fn missing_check_returns_default() {
        let msgs = messages("nonexistent_check");
        assert!(msgs.detail.is_empty());
    }

    #[test]
    fn run_checks_on_empty_file() {
        let violations = run_checks("/test/empty.py", b"", None);
        assert!(violations.is_empty());
    }

    #[test]
    fn run_checks_classifies_test_file() {
        let source = b"def test_foo():\n    assert True\n";
        let violations = run_checks("/project/tests/test_foo.py", source, None);
        // Test files have a minimal check set — should run without panic
        let _ = violations;
    }
}
