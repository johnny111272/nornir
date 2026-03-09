//! Shared rule-parsing and severity types for hook binaries.
//!
//! Hook binaries that use TOML rule files share common patterns:
//!   - Severity levels (warn/block) parsed from CLI args
//!   - Rule entries (pattern + description) parsed from embedded TOML
//!
//! This module extracts those shared types so each hook crate can
//! focus on its domain-specific decision logic.

/// Severity level for a rule match — controls whether the hook warns or blocks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Severity {
    Warn,
    Block,
}

/// Parse a severity string from a CLI argument.
///
/// Returns `None` for unrecognized values, which hooks treat as "disabled."
pub fn parse_severity(s: &str) -> Option<Severity> {
    match s {
        "warn" => Some(Severity::Warn),
        "block" => Some(Severity::Block),
        _ => None,
    }
}

/// A single rule entry from a TOML rules file.
///
/// Each rule has a pattern (substring or regex, depending on the hook)
/// and a human-readable description used in log messages.
#[derive(Debug)]
pub struct RawRule {
    pub pattern: String,
    pub description: String,
}

/// Parse one named array of rules from a TOML table.
///
/// Reads `[[key]]` entries, each expected to have `pattern` and `description`
/// string fields. Entries missing either field are silently skipped.
pub fn parse_rule_array(table: &toml::Table, key: &str) -> Vec<RawRule> {
    table
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let t = item.as_table()?;
                    Some(RawRule {
                        pattern: t.get("pattern")?.as_str()?.to_string(),
                        description: t.get("description")?.as_str()?.to_string(),
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
