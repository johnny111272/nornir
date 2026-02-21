//! JSON Schema validation engine.
//!
//! Provides `EmbeddedValidator` — a compiled schema validator initialized
//! once via `OnceLock` and cached for the process lifetime. Schemas are
//! baked into the binary at compile time via `include_str!()`.

use std::sync::OnceLock;

use jsonschema::Validator;
use serde_json::Value;

use error_core::{
    format_educational_message, SchemaError, ValidationIssue,
};

/// Compiled validator from an embedded schema string.
///
/// The schema is baked into the binary at compile time. The `Validator` is
/// compiled on first use and cached for the process lifetime.
pub struct EmbeddedValidator {
    validator: OnceLock<Validator>,
    schema_json: &'static str,
    schema_name: &'static str,
}

impl EmbeddedValidator {
    /// Create a new embedded validator. Call this in a `static` with
    /// `include_str!()` for the schema.
    pub const fn new(schema_json: &'static str, schema_name: &'static str) -> Self {
        Self {
            validator: OnceLock::new(),
            schema_json,
            schema_name,
        }
    }

    /// Get the schema name.
    pub fn schema_name(&self) -> &'static str {
        self.schema_name
    }

    /// Get the raw schema JSON string.
    pub fn schema_json(&self) -> &'static str {
        self.schema_json
    }

    /// Get or compile the validator.
    fn get_validator(&self) -> &Validator {
        self.validator.get_or_init(|| {
            let schema: Value = serde_json::from_str(self.schema_json)
                .expect("embedded schema must be valid JSON");
            Validator::new(&schema)
                .expect("embedded schema must be valid JSON Schema")
        })
    }

    /// Validate JSON data and return a full result with educational errors.
    pub fn validate(&self, data_json: &str) -> Result<ValidateResult, SchemaError> {
        let data: Value = serde_json::from_str(data_json)
            .map_err(|e| SchemaError::InvalidInput(format!("Invalid JSON: {}", e)))?;

        let validator = self.get_validator();
        let errors: Vec<_> = validator.iter_errors(&data).collect();

        if errors.is_empty() {
            Ok(ValidateResult {
                valid: true,
                data: Some(data),
                issues: vec![],
                message: format!("{} validation passed", self.schema_name),
                schema_name: self.schema_name.to_string(),
            })
        } else {
            let issues: Vec<_> = errors
                .iter()
                .map(ValidationIssue::from_schema_error)
                .collect();
            let message = format_educational_message(self.schema_name, &issues);
            Ok(ValidateResult {
                valid: false,
                data: None,
                issues,
                message,
                schema_name: self.schema_name.to_string(),
            })
        }
    }

    /// Quick validity check — no error details.
    pub fn is_valid(&self, data_json: &str) -> Result<bool, SchemaError> {
        let data: Value = serde_json::from_str(data_json)
            .map_err(|e| SchemaError::InvalidInput(format!("Invalid JSON: {}", e)))?;
        Ok(self.get_validator().is_valid(&data))
    }
}

/// Result from validation.
#[derive(Debug)]
pub struct ValidateResult {
    /// Whether the data passed validation.
    pub valid: bool,
    /// The parsed data (only present when valid).
    pub data: Option<Value>,
    /// Validation issues with educational context.
    pub issues: Vec<ValidationIssue>,
    /// Human-readable educational message.
    pub message: String,
    /// Which schema was validated against.
    pub schema_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_SCHEMA: &str = r#"{
        "type": "object",
        "required": ["name"],
        "properties": {
            "name": {"type": "string"},
            "age": {"type": "integer"}
        }
    }"#;

    static VALIDATOR: EmbeddedValidator = EmbeddedValidator::new(TEST_SCHEMA, "test-schema");

    #[test]
    fn test_validate_valid_data() {
        let result = VALIDATOR.validate(r#"{"name": "test"}"#).unwrap();
        assert!(result.valid);
        assert!(result.issues.is_empty());
        assert!(result.message.contains("passed"));
    }

    #[test]
    fn test_validate_missing_required() {
        let result = VALIDATOR.validate(r#"{}"#).unwrap();
        assert!(!result.valid);
        assert!(!result.issues.is_empty());
        assert!(result.message.contains("failed"));
        assert!(result.issues.iter().any(|i| i.rule == "missing_required"));
    }

    #[test]
    fn test_validate_wrong_type() {
        let result = VALIDATOR.validate(r#"{"name": 123}"#).unwrap();
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.rule == "type_mismatch"));
    }

    #[test]
    fn test_validate_invalid_json() {
        let result = VALIDATOR.validate("not json");
        assert!(result.is_err());
        match result.unwrap_err() {
            SchemaError::InvalidInput(msg) => assert!(msg.contains("Invalid JSON")),
            other => panic!("Expected InvalidInput, got: {:?}", other),
        }
    }

    #[test]
    fn test_is_valid_true() {
        assert!(VALIDATOR.is_valid(r#"{"name": "test"}"#).unwrap());
    }

    #[test]
    fn test_is_valid_false() {
        assert!(!VALIDATOR.is_valid(r#"{}"#).unwrap());
    }

    #[test]
    fn test_is_valid_bad_json() {
        assert!(VALIDATOR.is_valid("garbage").is_err());
    }

    #[test]
    fn test_schema_name() {
        assert_eq!(VALIDATOR.schema_name(), "test-schema");
    }

    #[test]
    fn test_schema_json_accessor() {
        let json = VALIDATOR.schema_json();
        assert!(json.contains("\"name\""));
        assert!(json.contains("\"type\""));
    }
}
