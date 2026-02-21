//! Filesystem path verification.
//!
//! Composes `path_core` extraction with filesystem existence checks.
//! This is the only impure function in core/capability (filesystem access).

use std::path::Path;

use error_core::{PathError, PathField, SchemaError};

/// Verify that all `path_exists_absolute` fields in the data reference
/// files that exist on the filesystem.
///
/// Returns `Ok(())` if all paths exist, `Err(PathError)` if any are missing.
pub fn verify_paths(schema_json: &str, data_json: &str) -> Result<(), SchemaError> {
    let fields = path_core::extract_path_fields(schema_json, data_json)?;

    let missing: Vec<PathField> = fields
        .into_iter()
        .filter(|f| !Path::new(&f.value).exists())
        .collect();

    if missing.is_empty() {
        Ok(())
    } else {
        Err(SchemaError::ValidationFailed(
            PathError { missing }.to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_verify_paths_all_exist() {
        let dir = std::env::temp_dir().join("nornir_test_verify");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("exists.txt");
        let mut f = std::fs::File::create(&file).unwrap();
        f.write_all(b"test").unwrap();

        let schema = r#"{
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "format": "path_exists_absolute"
                }
            }
        }"#;
        let data = format!(r#"{{"path": "{}"}}"#, file.display());
        let result = verify_paths(schema, &data);
        assert!(result.is_ok());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_verify_paths_missing() {
        let schema = r#"{
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "format": "path_exists_absolute"
                }
            }
        }"#;
        let data = r#"{"path": "/absolutely/does/not/exist/file.txt"}"#;
        let result = verify_paths(schema, data);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_paths_no_path_fields() {
        let schema = r#"{
            "type": "object",
            "properties": {
                "name": {"type": "string"}
            }
        }"#;
        let data = r#"{"name": "test"}"#;
        let result = verify_paths(schema, data);
        assert!(result.is_ok());
    }
}
