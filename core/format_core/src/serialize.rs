//! Format serializers — convert serde_json::Value to format strings.
//!
//! These are raw serializers without educational diagnostics.
//! For conversions with diagnostics, use the convert module.

use error_core::FormatError;
use serde_json::Value;

/// Serialize Value to pretty-printed JSON.
pub fn to_json(value: &Value) -> Result<String, FormatError> {
    serde_json::to_string_pretty(value)
        .map_err(|e| FormatError::Conversion(format!("JSON serialization failed: {}", e)))
}

/// Serialize Value to YAML.
pub fn to_yaml(value: &Value) -> Result<String, FormatError> {
    serde_yaml::to_string(value)
        .map_err(|e| FormatError::Conversion(format!("YAML serialization failed: {}", e)))
}

/// Serialize Value to TOML (raw — no educational diagnostics).
///
/// For conversion with educational errors on failure, use `convert::json_to_toml`.
pub fn to_toml(value: &Value) -> Result<String, FormatError> {
    let toml_value = json_value_to_toml_value(value.clone())
        .map_err(|e| FormatError::Conversion(format!("TOML conversion failed: {}", e)))?;
    toml::to_string_pretty(&toml_value)
        .map_err(|e| FormatError::Conversion(format!("TOML serialization failed: {}", e)))
}

/// Serialize Value to TOON.
pub fn to_toon(value: &Value) -> Result<String, FormatError> {
    toon_format::encode_default(value)
        .map_err(|e| FormatError::Conversion(format!("TOON serialization failed: {}", e)))
}

// =============================================================================
// JSON → TOML value conversion
// =============================================================================

pub(crate) fn json_value_to_toml_value(
    value: serde_json::Value,
) -> Result<toml::Value, String> {
    match value {
        Value::Null => Err("TOML does not support null values".to_string()),
        Value::Bool(b) => Ok(toml::Value::Boolean(b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(toml::Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(toml::Value::Float(f))
            } else {
                Err(format!("Cannot convert number {} to TOML", n))
            }
        }
        Value::String(s) => Ok(toml::Value::String(s)),
        Value::Array(arr) => {
            let toml_arr: Result<Vec<toml::Value>, String> =
                arr.into_iter().map(json_value_to_toml_value).collect();
            Ok(toml::Value::Array(toml_arr?))
        }
        Value::Object(map) => {
            let mut table = toml::map::Map::new();
            for (k, v) in map {
                table.insert(k, json_value_to_toml_value(v)?);
            }
            Ok(toml::Value::Table(table))
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_to_json() {
        let value = json!({"key": "value"});
        let result = to_json(&value).unwrap();
        assert!(result.contains("\"key\""));
        assert!(result.contains("\"value\""));
    }

    #[test]
    fn test_to_yaml() {
        let value = json!({"key": "value"});
        let result = to_yaml(&value).unwrap();
        assert!(result.contains("key:"));
    }

    #[test]
    fn test_to_toml() {
        let value = json!({"key": "value"});
        let result = to_toml(&value).unwrap();
        assert!(result.contains("key = "));
    }

    #[test]
    fn test_to_toml_null_fails() {
        let value = json!({"key": null});
        let err = to_toml(&value).unwrap_err();
        assert!(matches!(err, FormatError::Conversion(_)));
    }

    #[test]
    fn test_to_toon() {
        let value = json!({"key": "value"});
        let result = to_toon(&value).unwrap();
        assert!(result.contains("key:"));
    }

    #[test]
    fn test_to_toml_preserves_key_order() {
        // Keys deliberately not alphabetical — z before a before m
        let json_str = r#"{"zebra": 1, "alpha": 2, "mango": 3}"#;
        let value: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let toml_str = to_toml(&value).unwrap();
        let z_pos = toml_str.find("zebra").unwrap();
        let a_pos = toml_str.find("alpha").unwrap();
        let m_pos = toml_str.find("mango").unwrap();
        assert!(
            z_pos < a_pos && a_pos < m_pos,
            "key order not preserved: z={z_pos} a={a_pos} m={m_pos}\n{toml_str}"
        );
    }

    #[test]
    fn test_to_toml_preserves_nested_key_order() {
        let json_str = r#"{"outer": {"charlie": 1, "bravo": 2, "alpha": 3}}"#;
        let value: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let toml_str = to_toml(&value).unwrap();
        let c_pos = toml_str.find("charlie").unwrap();
        let b_pos = toml_str.find("bravo").unwrap();
        let a_pos = toml_str.find("alpha").unwrap();
        assert!(
            c_pos < b_pos && b_pos < a_pos,
            "nested key order not preserved: c={c_pos} b={b_pos} a={a_pos}\n{toml_str}"
        );
    }

    #[test]
    fn test_to_toon_with_array() {
        let value = json!({
            "items": [
                {"name": "a", "count": 1},
                {"name": "b", "count": 2}
            ]
        });
        let result = to_toon(&value).unwrap();
        // TOON should encode this more compactly than JSON
        assert!(result.len() < serde_json::to_string_pretty(&value).unwrap().len());
    }
}
