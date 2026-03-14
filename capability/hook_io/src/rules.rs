//! Shared rule-parsing and severity types for hook binaries.
//!
//! Hook binaries that use TOML rule files share common patterns:
//!   - Severity levels (warn/block) parsed from CLI args
//!   - Rule entries (pattern + description) parsed from embedded TOML
//!
//! This module extracts those shared types so each hook crate can
//! focus on its domain-specific decision logic.

/// Severity level for a rule match.
///
/// Four tiers (ascending enforcement):
///   Warn  — allow, notify user + LLM (informational)
///   Ask   — pause, user decides allow/deny (interactive)
///   Block — hard deny, no override
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Severity {
    Warn,
    Ask,
    Block,
}

/// Parse a severity string from a CLI argument.
///
/// Returns `None` for unrecognized values, which hooks treat as "disabled."
pub fn parse_severity(level: &str) -> Option<Severity> {
    match level {
        "warn" => Some(Severity::Warn),
        "ask" => Some(Severity::Ask),
        "block" => Some(Severity::Block),
        _ => None,
    }
}

/// A single rule entry from a TOML rules file.
///
/// Each rule has a pattern (substring or regex, depending on the hook),
/// a human-readable description, and an optional per-rule severity override.
/// When `severity` is `Some`, it takes precedence over the category default.
#[derive(Debug)]
pub struct RawRule {
    pub pattern: String,
    pub description: String,
    pub severity: Option<Severity>,
}

/// Parse one named array of rules from a TOML table.
///
/// Reads `[[key]]` entries, each expected to have `pattern` and `description`
/// string fields. Optional `severity` field overrides the category default.
/// Entries missing pattern or description are silently skipped.
pub fn parse_rule_array(table: &toml::Table, key: &str) -> Vec<RawRule> {
    table
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let entry = item.as_table()?;
                    Some(RawRule {
                        pattern: entry.get("pattern")?.as_str()?.to_string(),
                        description: entry.get("description")?.as_str()?.to_string(),
                        severity: entry
                            .get("severity")
                            .and_then(|v| v.as_str())
                            .and_then(parse_severity),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a TOML string into a table, suitable for passing to `parse_rule_array`.
///
/// Returns a descriptive error if the TOML is malformed.
pub fn parse_toml_table(toml_str: &str) -> Result<toml::Table, String> {
    toml_str
        .parse()
        .map_err(|e| format!("embedded rules.toml is invalid \u{2014} {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_severity ────────────────────────────────────────────

    #[test]
    fn parse_severity_warn() {
        assert_eq!(parse_severity("warn"), Some(Severity::Warn));
    }

    #[test]
    fn parse_severity_block() {
        assert_eq!(parse_severity("block"), Some(Severity::Block));
    }

    #[test]
    fn parse_severity_uppercase_rejected() {
        assert_eq!(parse_severity("WARN"), None);
    }

    #[test]
    fn parse_severity_empty_string() {
        assert_eq!(parse_severity(""), None);
    }

    #[test]
    fn parse_severity_ask() {
        assert_eq!(parse_severity("ask"), Some(Severity::Ask));
    }

    #[test]
    fn parse_severity_unknown_value() {
        assert_eq!(parse_severity("deny"), None);
    }

    #[test]
    fn parse_severity_block_uppercase_rejected() {
        assert_eq!(parse_severity("BLOCK"), None);
    }

    #[test]
    fn parse_severity_mixed_case_rejected() {
        assert_eq!(parse_severity("Warn"), None);
    }

    // ── parse_toml_table ──────────────────────────────────────────

    #[test]
    fn parse_toml_table_valid() {
        let result = parse_toml_table("[section]\nkey = \"value\"");
        assert!(result.is_ok());
        let table = result.unwrap();
        assert!(table.contains_key("section"));
    }

    #[test]
    fn parse_toml_table_invalid() {
        let result = parse_toml_table("[[[broken");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("embedded rules.toml is invalid"));
    }

    #[test]
    fn parse_toml_table_empty_string() {
        let result = parse_toml_table("");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn parse_toml_table_preserves_values() {
        let toml_str = r#"
[[rules]]
pattern = "test"
description = "a test rule"
"#;
        let result = parse_toml_table(toml_str);
        assert!(result.is_ok());
        let table = result.unwrap();
        assert!(table.contains_key("rules"));
    }

    // ── parse_rule_array ──────────────────────────────────────────

    #[test]
    fn parse_rule_array_valid() {
        let toml_str = r#"
[[floor]]
pattern = "/.ssh/"
description = "SSH keys"

[[floor]]
pattern = "/.aws/"
description = "AWS credentials"
"#;
        let table = parse_toml_table(toml_str).unwrap();
        let rules = parse_rule_array(&table, "floor");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].pattern, "/.ssh/");
        assert_eq!(rules[0].description, "SSH keys");
        assert_eq!(rules[1].pattern, "/.aws/");
        assert_eq!(rules[1].description, "AWS credentials");
    }

    #[test]
    fn parse_rule_array_missing_key_returns_empty() {
        let table = parse_toml_table("").unwrap();
        let rules = parse_rule_array(&table, "nonexistent");
        assert!(rules.is_empty());
    }

    #[test]
    fn parse_rule_array_entry_missing_pattern_skipped() {
        let toml_str = r#"
[[rules]]
description = "no pattern here"
"#;
        let table = parse_toml_table(toml_str).unwrap();
        let rules = parse_rule_array(&table, "rules");
        assert!(rules.is_empty());
    }

    #[test]
    fn parse_rule_array_entry_missing_description_skipped() {
        let toml_str = r#"
[[rules]]
pattern = "some_pattern"
"#;
        let table = parse_toml_table(toml_str).unwrap();
        let rules = parse_rule_array(&table, "rules");
        assert!(rules.is_empty());
    }

    #[test]
    fn parse_rule_array_mixed_valid_invalid() {
        let toml_str = r#"
[[rules]]
pattern = "valid"
description = "good rule"

[[rules]]
description = "missing pattern"

[[rules]]
pattern = "also_valid"
description = "another good rule"

[[rules]]
pattern = "no_desc"
"#;
        let table = parse_toml_table(toml_str).unwrap();
        let rules = parse_rule_array(&table, "rules");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].pattern, "valid");
        assert_eq!(rules[1].pattern, "also_valid");
    }

    #[test]
    fn parse_rule_array_key_is_not_array() {
        let toml_str = r#"
[rules]
pattern = "not_an_array"
"#;
        let table = parse_toml_table(toml_str).unwrap();
        let rules = parse_rule_array(&table, "rules");
        assert!(rules.is_empty());
    }

    #[test]
    fn parse_rule_array_per_rule_severity_block() {
        let toml_str = r#"
[[rules]]
pattern = "test"
description = "test rule"
severity = "block"
"#;
        let table = parse_toml_table(toml_str).unwrap();
        let rules = parse_rule_array(&table, "rules");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].severity, Some(Severity::Block));
    }

    #[test]
    fn parse_rule_array_per_rule_severity_ask() {
        let toml_str = r#"
[[rules]]
pattern = "test"
description = "test rule"
severity = "ask"
"#;
        let table = parse_toml_table(toml_str).unwrap();
        let rules = parse_rule_array(&table, "rules");
        assert_eq!(rules[0].severity, Some(Severity::Ask));
    }

    #[test]
    fn parse_rule_array_no_severity_is_none() {
        let toml_str = r#"
[[rules]]
pattern = "test"
description = "test rule"
"#;
        let table = parse_toml_table(toml_str).unwrap();
        let rules = parse_rule_array(&table, "rules");
        assert_eq!(rules[0].severity, None);
    }

    #[test]
    fn parse_rule_array_invalid_severity_is_none() {
        let toml_str = r#"
[[rules]]
pattern = "test"
description = "test rule"
severity = "nuke"
"#;
        let table = parse_toml_table(toml_str).unwrap();
        let rules = parse_rule_array(&table, "rules");
        assert_eq!(rules[0].severity, None);
    }

    #[test]
    fn parse_rule_array_extra_fields_still_parses() {
        let toml_str = r#"
[[rules]]
pattern = "test"
description = "test rule"
extra_field = 42
"#;
        let table = parse_toml_table(toml_str).unwrap();
        let rules = parse_rule_array(&table, "rules");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern, "test");
    }
}
