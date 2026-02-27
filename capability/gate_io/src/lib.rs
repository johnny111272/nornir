//! Shared gate IO operations.
//!
//! Provides the three gate patterns used by all Nornir pipeline gates:
//! - Input gates: read TOML file → validate → return JSON
//! - Output gates: validate JSON → write TOML file
//! - Passthrough gates: read TOML → validate → verify paths → write TOML

use std::fs;
use std::path::Path;

use error_core::{IoError, NornirError, SchemaError};
use format_core::{json_to_toml, toml_to_json};
use schema_core::EmbeddedValidator;

/// Input gate: read TOML from disk, validate against schema, return JSON.
pub fn read_and_validate(
    schema: &EmbeddedValidator,
    path: &str,
    verify_paths: bool,
) -> Result<String, NornirError> {
    let toml_str = read_file(path)?;
    let json = toml_to_json(&toml_str)?;
    let result = schema.validate(&json)?;
    if !result.valid {
        return Err(SchemaError::ValidationFailed(result.message).into());
    }
    if verify_paths {
        path_verify::verify_paths(schema.schema_json(), &json)?;
    }
    Ok(json)
}

/// Output gate: validate JSON against schema, convert to TOML, write to disk.
pub fn validate_and_write(
    schema: &EmbeddedValidator,
    data: &str,
    path: &str,
    verify_paths: bool,
) -> Result<(), NornirError> {
    let result = schema.validate(data)?;
    if !result.valid {
        return Err(SchemaError::ValidationFailed(result.message).into());
    }
    if verify_paths {
        path_verify::verify_paths(schema.schema_json(), data)?;
    }
    let toml_str = json_to_toml(data)?;
    write_file(path, &toml_str)?;
    Ok(())
}

/// Passthrough gate: read TOML, validate, verify paths, write TOML.
pub fn read_validate_write(
    schema: &EmbeddedValidator,
    input_path: &str,
    output_path: &str,
) -> Result<(), NornirError> {
    let toml_str = read_file(input_path)?;
    let json = toml_to_json(&toml_str)?;
    let result = schema.validate(&json)?;
    if !result.valid {
        return Err(SchemaError::ValidationFailed(result.message).into());
    }
    path_verify::verify_paths(schema.schema_json(), &json)?;
    let output_toml = json_to_toml(&json)?;
    write_file(output_path, &output_toml)?;
    Ok(())
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
    let p = Path::new(path);
    if let Some(parent) = p.parent() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_file_not_found() {
        let result = read_file("/absolutely/does/not/exist.toml");
        assert!(result.is_err());
        match result.unwrap_err() {
            NornirError::Io(IoError::ReadFailed { path, .. }) => {
                assert_eq!(path, "/absolutely/does/not/exist.toml");
            }
            other => panic!("Expected ReadFailed, got: {:?}", other),
        }
    }

    #[test]
    fn test_write_file_creates_parent() {
        let dir = std::env::temp_dir().join("nornir_gate_io_test");
        let file = dir.join("sub").join("output.toml");
        let _ = fs::remove_dir_all(&dir);

        write_file(file.to_str().unwrap(), "name = \"test\"\n").unwrap();
        assert!(file.exists());

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "name = \"test\"\n");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_write_file_ensures_trailing_newline() {
        let dir = std::env::temp_dir().join("nornir_gate_io_newline");
        let file = dir.join("output.toml");
        let _ = fs::remove_dir_all(&dir);

        write_file(file.to_str().unwrap(), "name = \"test\"").unwrap();
        let content = fs::read_to_string(&file).unwrap();
        assert!(content.ends_with('\n'));

        fs::remove_dir_all(&dir).ok();
    }
}
