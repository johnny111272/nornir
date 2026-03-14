//! Read TOML, apply conditional defaults, validate, write TOML.
//!
//! Composes `default_apply_core` (pure logic) with `format_core` (TOML↔JSON)
//! and `schema_core` (validation). This is the capability layer — it performs IO.

use std::fs;
use std::path::Path;

use error_core::{IoError, NornirError, SchemaError};
use schema_core::EmbeddedValidator;

/// Read raw TOML, apply conditional defaults, validate, write clean TOML.
/// Returns the modified JSON string for downstream use.
pub fn read_apply_validate_write(
    schema: &EmbeddedValidator,
    input_path: &str,
    output_path: &str,
) -> Result<String, NornirError> {
    let toml_str = read_file(input_path)?;
    let json_str = format_core::toml_to_json(&toml_str)?;

    let mut data: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| SchemaError::InvalidInput(format!("Invalid JSON: {}", e)))?;
    let schema_value: serde_json::Value = serde_json::from_str(schema.schema_json())
        .map_err(|e| SchemaError::InvalidInput(format!("Invalid schema JSON: {}", e)))?;

    default_apply_core::apply_conditional_defaults(&schema_value, &mut data);

    let modified_json = serde_json::to_string(&data)
        .map_err(|e| SchemaError::InvalidInput(format!("Serialization failed: {}", e)))?;

    let result = schema.validate(&modified_json)?;
    if !result.valid {
        return Err(SchemaError::ValidationFailed(result.message).into());
    }

    let output_toml = format_core::json_to_toml(&modified_json)?;
    write_file(output_path, &output_toml)?;

    Ok(modified_json)
}

fn read_file(path: &str) -> Result<String, NornirError> {
    fs::read_to_string(path).map_err(|e| {
        IoError::ReadFailed {
            path: path.to_string(),
            message: e.to_string(),
        }
        .into()
    })
}

fn write_file(path: &str, content: &str) -> Result<(), NornirError> {
    let target = Path::new(path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| IoError::WriteFailed {
            path: path.to_string(),
            message: format!("Cannot create parent directory: {}", e),
        })?;
    }
    let with_newline = if content.ends_with('\n') {
        content.to_string()
    } else {
        format!("{}\n", content)
    };
    fs::write(path, &with_newline).map_err(|e| {
        IoError::WriteFailed {
            path: path.to_string(),
            message: e.to_string(),
        }
        .into()
    })
}
