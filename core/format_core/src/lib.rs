//! TOML<->JSON format conversion.
//!
//! Pure functions for converting between TOML and JSON string representations.
//! No IO, no side effects.

use error_core::FormatError;

/// Convert a TOML string to a pretty-printed JSON string.
pub fn toml_to_json(toml_str: &str) -> Result<String, FormatError> {
    let toml_value: toml::Value =
        toml::from_str(toml_str).map_err(|e| FormatError::TomlParse(e.to_string()))?;

    let json_value = toml_value_to_json_value(toml_value);

    serde_json::to_string_pretty(&json_value)
        .map_err(|e| FormatError::Conversion(format!("TOML->JSON serialization failed: {}", e)))
}

/// Convert a JSON string to a TOML string.
pub fn json_to_toml(json_str: &str) -> Result<String, FormatError> {
    let json_value: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| FormatError::JsonParse(e.to_string()))?;

    let toml_value = json_value_to_toml_value(json_value)
        .map_err(|e| FormatError::Conversion(format!("JSON->TOML conversion failed: {}", e)))?;

    toml::to_string_pretty(&toml_value)
        .map_err(|e| FormatError::Conversion(format!("TOML serialization failed: {}", e)))
}

/// Convert a `toml::Value` to a `serde_json::Value`.
fn toml_value_to_json_value(value: toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(i) => serde_json::Value::Number(i.into()),
        toml::Value::Float(f) => {
            serde_json::Number::from_f64(f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null)
        }
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(toml_value_to_json_value).collect())
        }
        toml::Value::Table(table) => {
            let map: serde_json::Map<String, serde_json::Value> = table
                .into_iter()
                .map(|(k, v)| (k, toml_value_to_json_value(v)))
                .collect();
            serde_json::Value::Object(map)
        }
    }
}

/// Convert a `serde_json::Value` to a `toml::Value`.
fn json_value_to_toml_value(value: serde_json::Value) -> Result<toml::Value, String> {
    match value {
        serde_json::Value::Null => Err("TOML does not support null values".to_string()),
        serde_json::Value::Bool(b) => Ok(toml::Value::Boolean(b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(toml::Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(toml::Value::Float(f))
            } else {
                Err(format!("Cannot convert number {} to TOML", n))
            }
        }
        serde_json::Value::String(s) => Ok(toml::Value::String(s)),
        serde_json::Value::Array(arr) => {
            let toml_arr: Result<Vec<toml::Value>, String> =
                arr.into_iter().map(json_value_to_toml_value).collect();
            Ok(toml::Value::Array(toml_arr?))
        }
        serde_json::Value::Object(map) => {
            let mut table = toml::map::Map::new();
            for (k, v) in map {
                table.insert(k, json_value_to_toml_value(v)?);
            }
            Ok(toml::Value::Table(table))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toml_to_json_simple() {
        let toml = r#"
name = "test"
count = 42
active = true
"#;
        let json = toml_to_json(toml).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["name"], "test");
        assert_eq!(parsed["count"], 42);
        assert_eq!(parsed["active"], true);
    }

    #[test]
    fn test_json_to_toml_simple() {
        let json = r#"{"name": "test", "count": 42, "active": true}"#;
        let toml_str = json_to_toml(json).unwrap();
        assert!(toml_str.contains("name = \"test\""));
        assert!(toml_str.contains("count = 42"));
        assert!(toml_str.contains("active = true"));
    }

    #[test]
    fn test_roundtrip_toml_json_toml() {
        let original = r#"
[metadata]
name = "test-agent"
version = "1.0"

[settings]
enabled = true
count = 5

[[items]]
id = 1
label = "first"

[[items]]
id = 2
label = "second"
"#;
        let json = toml_to_json(original).unwrap();
        let back = json_to_toml(&json).unwrap();

        // Parse both to verify structural equivalence
        let orig_val: toml::Value = toml::from_str(original).unwrap();
        let back_val: toml::Value = toml::from_str(&back).unwrap();
        assert_eq!(orig_val, back_val);
    }

    #[test]
    fn test_roundtrip_json_toml_json() {
        let original =
            r#"{"name": "test", "nested": {"key": "value"}, "list": [1, 2, 3]}"#;
        let toml_str = json_to_toml(original).unwrap();
        let back = toml_to_json(&toml_str).unwrap();

        let orig_val: serde_json::Value = serde_json::from_str(original).unwrap();
        let back_val: serde_json::Value = serde_json::from_str(&back).unwrap();
        assert_eq!(orig_val, back_val);
    }

    #[test]
    fn test_malformed_toml() {
        let bad = "this is not [valid toml";
        let result = toml_to_json(bad);
        assert!(result.is_err());
        match result.unwrap_err() {
            FormatError::TomlParse(_) => {}
            other => panic!("Expected TomlParse, got: {:?}", other),
        }
    }

    #[test]
    fn test_malformed_json() {
        let bad = "not json {{{";
        let result = json_to_toml(bad);
        assert!(result.is_err());
        match result.unwrap_err() {
            FormatError::JsonParse(_) => {}
            other => panic!("Expected JsonParse, got: {:?}", other),
        }
    }

    #[test]
    fn test_json_null_fails() {
        let json = r#"{"key": null}"#;
        let result = json_to_toml(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_table() {
        let toml = "[empty]\n";
        let json = toml_to_json(toml).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["empty"].is_object());
    }

    #[test]
    fn test_nested_arrays_of_tables() {
        let toml = r#"
[[servers]]
name = "alpha"
port = 8080

[[servers]]
name = "beta"
port = 9090
"#;
        let json = toml_to_json(toml).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["servers"][0]["name"], "alpha");
        assert_eq!(parsed["servers"][1]["port"], 9090);
    }
}
