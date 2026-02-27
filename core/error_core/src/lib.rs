//! Nornir error types and educational formatting for pipeline validation.
//!
//! Adopts the educational error philosophy from `validator_errors`:
//! - **What** is wrong (rule violated)
//! - **Where** the problem is (JSON path)
//! - **What was found** (the invalid value)
//! - **What was expected** (the schema constraint)
//! - **How to fix it** (actionable suggestion)

use jsonschema::ValidationError;
use serde::Serialize;
use thiserror::Error;

// =============================================================================
// Top-level error enum
// =============================================================================

/// Top-level error type for all Nornir operations.
#[derive(Debug, Error)]
pub enum NornirError {
    #[error("Format error: {0}")]
    Format(#[from] FormatError),

    #[error("Schema error: {0}")]
    Schema(#[from] SchemaError),

    #[error("Path error: {0}")]
    Path(#[from] PathError),

    #[error("IO error: {0}")]
    Io(#[from] IoError),
}

// =============================================================================
// Specific error types
// =============================================================================

/// TOML parse, JSON parse, or conversion errors.
#[derive(Debug, Error)]
pub enum FormatError {
    #[error("TOML parse error: {0}")]
    TomlParse(String),

    #[error("JSON parse error: {0}")]
    JsonParse(String),

    #[error("Conversion error: {0}")]
    Conversion(String),
}

/// Schema validation errors — invalid input or schema compilation failures.
#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("Invalid JSON input: {0}")]
    InvalidInput(String),

    #[error("Schema compilation error: {0}")]
    SchemaCompilation(String),

    #[error("Validation failed: {0}")]
    ValidationFailed(String),
}

/// Missing filesystem paths.
#[derive(Debug, Error)]
#[error("Missing paths: {}", format_missing_paths(&self.missing))]
pub struct PathError {
    pub missing: Vec<PathField>,
}

/// A path field extracted from data via schema annotation.
#[derive(Debug, Clone, Serialize)]
pub struct PathField {
    /// JSON pointer to the field (e.g., "/security/schemas/security_input_schema")
    pub json_pointer: String,
    /// The path value from the data
    pub value: String,
}

/// IO read/write failures.
#[derive(Debug, Error)]
pub enum IoError {
    #[error("Cannot read '{path}': {message}")]
    ReadFailed { path: String, message: String },

    #[error("Cannot write '{path}': {message}")]
    WriteFailed { path: String, message: String },

    #[error("Cannot read stdin: {0}")]
    StdinFailed(String),
}

// =============================================================================
// ValidationIssue — educational error context
// =============================================================================

/// A single validation issue with educational context.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationIssue {
    /// JSON path to the invalid value (e.g., "/users/0/email")
    pub path: String,
    /// What rule was violated
    pub rule: String,
    /// The invalid value (truncated if too long)
    pub found: String,
    /// What was expected
    pub expected: String,
    /// Actionable fix suggestion
    pub fix: String,
}

impl ValidationIssue {
    /// Create a `ValidationIssue` from a `jsonschema::ValidationError`.
    pub fn from_schema_error(error: &ValidationError) -> Self {
        let path = error.instance_path.to_string();
        let path = if path.is_empty() {
            "/".to_string()
        } else {
            path
        };

        let (rule, expected, fix) = categorize_error(error);
        let found = truncate_value(&error.instance.to_string(), 50);

        Self {
            path,
            rule,
            found,
            expected,
            fix,
        }
    }
}

// =============================================================================
// Error categorization (adopted from validator_errors)
// =============================================================================

/// Categorize a schema validation error and provide educational context.
fn categorize_error(error: &ValidationError) -> (String, String, String) {
    let msg = error.to_string();

    // Type mismatch
    if msg.contains("is not of type") {
        let rule = "type_mismatch".to_string();
        let expected = extract_expected_type(&msg);
        let fix = format!(
            "Change the value to match the expected type: {}",
            expected
        );
        return (rule, expected, fix);
    }

    // Required property missing
    if msg.contains("is a required property") {
        let rule = "missing_required".to_string();
        let prop = extract_property_name(&msg);
        let expected = format!("Property '{}' must be present", prop);
        let fix = format!("Add the required property '{}'", prop);
        return (rule, expected, fix);
    }

    // Additional property not allowed
    if msg.contains("Additional properties are not allowed") {
        let rule = "additional_property".to_string();
        let expected = "No additional properties allowed".to_string();
        let fix =
            "Remove the unexpected property, or update the schema to allow it".to_string();
        return (rule, expected, fix);
    }

    // Enum value not allowed
    if msg.contains("is not one of") {
        let rule = "invalid_enum".to_string();
        let expected = extract_enum_values(&msg);
        let fix = format!("Use one of the allowed values: {}", expected);
        return (rule, expected, fix);
    }

    // Pattern mismatch
    if msg.contains("does not match") {
        let rule = "pattern_mismatch".to_string();
        let expected = "Value must match the pattern".to_string();
        let fix = "Adjust the value to match the required pattern".to_string();
        return (rule, expected, fix);
    }

    // Minimum/maximum violations
    if msg.contains("is less than") || msg.contains("is greater than") {
        let rule = "range_violation".to_string();
        let expected = extract_range(&msg);
        let fix = format!(
            "Adjust the value to be within the allowed range: {}",
            expected
        );
        return (rule, expected, fix);
    }

    // MinLength/MaxLength
    if msg.contains("is shorter than") || msg.contains("is longer than") {
        let rule = "length_violation".to_string();
        let expected = "String length out of bounds".to_string();
        let fix = "Adjust the string length to meet requirements".to_string();
        return (rule, expected, fix);
    }

    // Array length
    if msg.contains("has less than") || msg.contains("has more than") {
        let rule = "array_length".to_string();
        let expected = "Array length out of bounds".to_string();
        let fix = "Adjust the number of array items".to_string();
        return (rule, expected, fix);
    }

    // Format validation
    if msg.contains("is not a")
        && (msg.contains("email") || msg.contains("uri") || msg.contains("date"))
    {
        let rule = "format_invalid".to_string();
        let expected = "Value must match the specified format".to_string();
        let fix = "Ensure the value matches the required format".to_string();
        return (rule, expected, fix);
    }

    // Fallback
    let rule = "validation_failed".to_string();
    let expected = msg.clone();
    let fix = "Check the value against the schema requirements".to_string();
    (rule, expected, fix)
}

// =============================================================================
// Formatting helpers
// =============================================================================

fn extract_expected_type(msg: &str) -> String {
    if let Some(start) = msg.find("type \"") {
        let rest = &msg[start + 6..];
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }
    if let Some(start) = msg.find("type '") {
        let rest = &msg[start + 6..];
        if let Some(end) = rest.find('\'') {
            return rest[..end].to_string();
        }
    }
    "unknown".to_string()
}

fn extract_property_name(msg: &str) -> String {
    if let Some(start) = msg.find('"') {
        let rest = &msg[start + 1..];
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }
    if let Some(start) = msg.find('\'') {
        let rest = &msg[start + 1..];
        if let Some(end) = rest.find('\'') {
            return rest[..end].to_string();
        }
    }
    "unknown".to_string()
}

fn extract_enum_values(msg: &str) -> String {
    if let Some(start) = msg.find('[') {
        if let Some(end) = msg.find(']') {
            return msg[start..=end].to_string();
        }
    }
    "allowed values".to_string()
}

fn extract_range(msg: &str) -> String {
    msg.to_string()
}

/// Truncate a value string for display.
pub fn truncate_value(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

/// Format all issues into a human-readable educational message.
pub fn format_educational_message(schema_name: &str, issues: &[ValidationIssue]) -> String {
    if issues.is_empty() {
        return format!("{} validation passed", schema_name);
    }

    let mut lines = vec![
        format!(
            "{} validation failed — {} issue(s) found:",
            schema_name,
            issues.len()
        ),
        String::new(),
    ];

    for (i, issue) in issues.iter().enumerate() {
        lines.push(format!("  {}. Path: {}", i + 1, issue.path));
        lines.push(format!("     Rule: {}", issue.rule));
        lines.push(format!("     Found: {}", issue.found));
        lines.push(format!("     Expected: {}", issue.expected));
        lines.push(format!("     Fix: {}", issue.fix));
        lines.push(String::new());
    }

    lines.join("\n")
}

/// Format path verification errors into a human-readable message.
pub fn format_path_errors(missing: &[PathField]) -> String {
    if missing.is_empty() {
        return "All referenced paths exist".to_string();
    }

    let mut lines = vec![format!(
        "Path verification failed — {} missing path(s):",
        missing.len()
    )];
    lines.push(String::new());

    for (i, field) in missing.iter().enumerate() {
        lines.push(format!("  {}. Field: {}", i + 1, field.json_pointer));
        lines.push(format!("     Path: {}", field.value));
        lines.push("     Fix: Create the file or correct the path".to_string());
        lines.push(String::new());
    }

    lines.join("\n")
}

fn format_missing_paths(missing: &[PathField]) -> String {
    missing
        .iter()
        .map(|p| format!("{} ({})", p.value, p.json_pointer))
        .collect::<Vec<_>>()
        .join(", ")
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_value_short() {
        assert_eq!(truncate_value("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_value_long() {
        let result = truncate_value("a very long string that exceeds the limit", 10);
        assert_eq!(result, "a very lon...");
    }

    #[test]
    fn test_format_educational_message_empty() {
        let msg = format_educational_message("agent-definition", &[]);
        assert_eq!(msg, "agent-definition validation passed");
    }

    #[test]
    fn test_format_educational_message_with_issues() {
        let issues = vec![ValidationIssue {
            path: "/name".to_string(),
            rule: "type_mismatch".to_string(),
            found: "123".to_string(),
            expected: "string".to_string(),
            fix: "Change the value to match the expected type: string".to_string(),
        }];
        let msg = format_educational_message("agent-definition", &issues);
        assert!(msg.contains("agent-definition validation failed"));
        assert!(msg.contains("1 issue(s) found"));
        assert!(msg.contains("Path: /name"));
        assert!(msg.contains("Rule: type_mismatch"));
        assert!(msg.contains("Fix:"));
    }

    #[test]
    fn test_validation_issue_from_schema_error() {
        let schema: serde_json::Value =
            serde_json::from_str(r#"{"type": "string"}"#).unwrap();
        let validator = jsonschema::Validator::new(&schema).unwrap();
        let data: serde_json::Value = serde_json::from_str("123").unwrap();

        let errors: Vec<_> = validator.iter_errors(&data).collect();
        assert!(!errors.is_empty());

        let issue = ValidationIssue::from_schema_error(&errors[0]);
        assert_eq!(issue.rule, "type_mismatch");
        assert_eq!(issue.path, "/");
    }

    #[test]
    fn test_required_property_error() {
        let schema: serde_json::Value = serde_json::from_str(
            r#"{"type": "object", "required": ["name", "age"]}"#,
        )
        .unwrap();
        let validator = jsonschema::Validator::new(&schema).unwrap();
        let data: serde_json::Value = serde_json::from_str("{}").unwrap();

        let errors: Vec<_> = validator.iter_errors(&data).collect();
        assert!(!errors.is_empty());

        let issue = ValidationIssue::from_schema_error(&errors[0]);
        assert_eq!(issue.rule, "missing_required");
        assert_ne!(
            issue.expected,
            "Property 'unknown' must be present",
            "Property name extraction failed"
        );
    }

    #[test]
    fn test_format_path_errors_empty() {
        assert_eq!(format_path_errors(&[]), "All referenced paths exist");
    }

    #[test]
    fn test_format_path_errors_with_missing() {
        let missing = vec![PathField {
            json_pointer: "/security/schemas/input".to_string(),
            value: "/path/to/missing.json".to_string(),
        }];
        let msg = format_path_errors(&missing);
        assert!(msg.contains("1 missing path(s)"));
        assert!(msg.contains("/security/schemas/input"));
        assert!(msg.contains("/path/to/missing.json"));
    }

    #[test]
    fn test_nornir_error_display() {
        let err = NornirError::Format(FormatError::TomlParse("bad toml".into()));
        assert!(err.to_string().contains("bad toml"));

        let err = NornirError::Path(PathError {
            missing: vec![PathField {
                json_pointer: "/foo".into(),
                value: "/no/such/file".into(),
            }],
        });
        assert!(err.to_string().contains("/no/such/file"));
    }
}
