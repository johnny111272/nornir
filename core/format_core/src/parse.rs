//! Format parsers — convert source format strings to serde_json::Value.
//!
//! All parsers produce serde_json::Value as the universal intermediate representation.

use error_core::FormatError;
use serde_json::Value;

/// Parse JSON string to Value.
pub fn json(content: &str) -> Result<Value, FormatError> {
    serde_json::from_str(content).map_err(|e| FormatError::JsonParse(e.to_string()))
}

/// Parse YAML string to Value.
pub fn yaml(content: &str) -> Result<Value, FormatError> {
    serde_yaml::from_str(content).map_err(|e| FormatError::YamlParse(e.to_string()))
}

/// Parse TOML string to Value (via toml::Value intermediate).
pub fn toml(content: &str) -> Result<Value, FormatError> {
    let toml_value: toml::Value =
        toml::from_str(content).map_err(|e| FormatError::TomlParse(e.to_string()))?;
    toml_value_to_json_value(toml_value)
}

/// Parse TOON string to Value.
pub fn toon(content: &str) -> Result<Value, FormatError> {
    toon_format::decode_default(content).map_err(|e| FormatError::ToonParse(e.to_string()))
}

// =============================================================================
// TOML → JSON value conversion
// =============================================================================

fn toml_value_to_json_value(value: toml::Value) -> Result<Value, FormatError> {
    Ok(match value {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::Number(i.into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(dt) => Value::String(dt.to_string()),
        toml::Value::Array(arr) => {
            let items: Result<Vec<Value>, FormatError> =
                arr.into_iter().map(toml_value_to_json_value).collect();
            Value::Array(items?)
        }
        toml::Value::Table(table) => {
            let mut map = serde_json::Map::new();
            for (k, v) in table {
                map.insert(k, toml_value_to_json_value(v)?);
            }
            Value::Object(map)
        }
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json() {
        let result = json(r#"{"key": "value"}"#).unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn test_parse_yaml() {
        let result = yaml("key: value").unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn test_parse_toml() {
        let result = toml("key = \"value\"").unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn test_parse_toon() {
        // TOON with simple key-value
        let result = toon("key: value").unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn test_parse_json_error() {
        let err = json("not valid json").unwrap_err();
        assert!(matches!(err, FormatError::JsonParse(_)));
    }

    #[test]
    fn test_parse_yaml_error() {
        let err = yaml("invalid: yaml: [unterminated").unwrap_err();
        assert!(matches!(err, FormatError::YamlParse(_)));
    }

    #[test]
    fn test_parse_toml_error() {
        let err = toml("this is not [valid toml").unwrap_err();
        assert!(matches!(err, FormatError::TomlParse(_)));
    }

    #[test]
    fn test_parse_toml_nested() {
        let input = r#"
[metadata]
name = "test"
version = "1.0"

[[items]]
id = 1
label = "first"
"#;
        let result = toml(input).unwrap();
        assert_eq!(result["metadata"]["name"], "test");
        assert_eq!(result["items"][0]["id"], 1);
    }
}
