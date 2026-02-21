//! Extract `path_exists_absolute` fields from data using schema as a map.
//!
//! Pure function — no filesystem access, extraction only. Walks the schema
//! recursively through `properties`, `allOf`, `oneOf`, `anyOf`, `if/then/else`,
//! and `items` to find all fields annotated with `format: path_exists_absolute`,
//! then extracts the corresponding values from the data using JSON pointers.

use error_core::{PathField, SchemaError};
use serde_json::Value;

/// Extract all path fields annotated with `format: path_exists_absolute` from
/// the data, using the schema as a map to locate them.
///
/// Returns a `Vec<PathField>` containing each field's JSON pointer and value.
/// Fields that exist in the schema but are missing from the data are silently
/// skipped (the schema validator catches missing required fields separately).
pub fn extract_path_fields(
    schema_json: &str,
    data_json: &str,
) -> Result<Vec<PathField>, SchemaError> {
    let schema: Value = serde_json::from_str(schema_json)
        .map_err(|e| SchemaError::InvalidInput(format!("Invalid schema JSON: {}", e)))?;
    let data: Value = serde_json::from_str(data_json)
        .map_err(|e| SchemaError::InvalidInput(format!("Invalid data JSON: {}", e)))?;

    let mut fields = Vec::new();
    walk_schema(&schema, &data, String::new(), &mut fields);
    Ok(fields)
}

/// Recursively walk the schema, accumulating path fields found in the data.
fn walk_schema(schema: &Value, data: &Value, pointer: String, fields: &mut Vec<PathField>) {
    let obj = match schema.as_object() {
        Some(o) => o,
        None => return,
    };

    // Check if THIS schema node has format: path_exists_absolute
    if let Some(Value::String(fmt)) = obj.get("format") {
        if fmt == "path_exists_absolute" {
            // This is a leaf path field — extract value from data
            if let Some(Value::String(path_value)) = resolve_pointer(data, &pointer) {
                fields.push(PathField {
                    json_pointer: if pointer.is_empty() {
                        "/".to_string()
                    } else {
                        pointer.clone()
                    },
                    value: path_value.clone(),
                });
            }
            return;
        }
    }

    // Walk properties
    if let Some(Value::Object(props)) = obj.get("properties") {
        for (key, prop_schema) in props {
            let child_pointer = format!("{}/{}", pointer, key);
            walk_schema(prop_schema, data, child_pointer, fields);
        }
    }

    // Walk allOf
    if let Some(Value::Array(all_of)) = obj.get("allOf") {
        for sub_schema in all_of {
            walk_schema(sub_schema, data, pointer.clone(), fields);
        }
    }

    // Walk oneOf
    if let Some(Value::Array(one_of)) = obj.get("oneOf") {
        for sub_schema in one_of {
            walk_schema(sub_schema, data, pointer.clone(), fields);
        }
    }

    // Walk anyOf
    if let Some(Value::Array(any_of)) = obj.get("anyOf") {
        for sub_schema in any_of {
            walk_schema(sub_schema, data, pointer.clone(), fields);
        }
    }

    // Walk if/then/else
    if let Some(then_schema) = obj.get("then") {
        walk_schema(then_schema, data, pointer.clone(), fields);
    }
    if let Some(else_schema) = obj.get("else") {
        walk_schema(else_schema, data, pointer.clone(), fields);
    }

    // Walk items (array elements)
    if let Some(items_schema) = obj.get("items") {
        if let Some(Value::Array(data_items)) = resolve_pointer(data, &pointer) {
            match items_schema {
                Value::Object(_) => {
                    for (i, _item) in data_items.iter().enumerate() {
                        let item_pointer = format!("{}/{}", pointer, i);
                        walk_schema(items_schema, data, item_pointer, fields);
                    }
                }
                Value::Array(item_schemas) => {
                    for (i, item_schema) in item_schemas.iter().enumerate() {
                        if i < data_items.len() {
                            let item_pointer = format!("{}/{}", pointer, i);
                            walk_schema(item_schema, data, item_pointer, fields);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Resolve a JSON pointer path against a Value.
/// Returns `None` if the path doesn't exist in the data.
fn resolve_pointer<'a>(data: &'a Value, pointer: &str) -> Option<&'a Value> {
    if pointer.is_empty() {
        return Some(data);
    }
    data.pointer(pointer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_flat_path_field() {
        let schema = r#"{
            "type": "object",
            "properties": {
                "config_path": {
                    "type": "string",
                    "format": "path_exists_absolute"
                }
            }
        }"#;
        let data = r#"{"config_path": "/etc/config.json"}"#;
        let fields = extract_path_fields(schema, data).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].json_pointer, "/config_path");
        assert_eq!(fields[0].value, "/etc/config.json");
    }

    #[test]
    fn test_nested_properties() {
        let schema = r#"{
            "type": "object",
            "properties": {
                "security": {
                    "type": "object",
                    "properties": {
                        "schemas": {
                            "type": "object",
                            "properties": {
                                "input_schema": {
                                    "type": "string",
                                    "format": "path_exists_absolute"
                                }
                            }
                        }
                    }
                }
            }
        }"#;
        let data = r#"{"security": {"schemas": {"input_schema": "/path/to/schema.json"}}}"#;
        let fields = extract_path_fields(schema, data).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(
            fields[0].json_pointer,
            "/security/schemas/input_schema"
        );
        assert_eq!(fields[0].value, "/path/to/schema.json");
    }

    #[test]
    fn test_allof_traversal() {
        let schema = r#"{
            "type": "object",
            "allOf": [
                {
                    "properties": {
                        "path_a": {
                            "type": "string",
                            "format": "path_exists_absolute"
                        }
                    }
                },
                {
                    "properties": {
                        "path_b": {
                            "type": "string",
                            "format": "path_exists_absolute"
                        }
                    }
                }
            ]
        }"#;
        let data = r#"{"path_a": "/a/path", "path_b": "/b/path"}"#;
        let fields = extract_path_fields(schema, data).unwrap();
        assert_eq!(fields.len(), 2);
    }

    #[test]
    fn test_oneof_traversal() {
        let schema = r#"{
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "oneOf": [
                            {
                                "type": "object",
                                "properties": {
                                    "include": {
                                        "type": "string",
                                        "format": "path_exists_absolute"
                                    }
                                }
                            },
                            {
                                "type": "object",
                                "properties": {
                                    "inline": {"type": "string"}
                                }
                            }
                        ]
                    }
                }
            }
        }"#;
        let data = r#"{"items": [{"include": "/path/to/file.md"}, {"inline": "text"}]}"#;
        let fields = extract_path_fields(schema, data).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].json_pointer, "/items/0/include");
        assert_eq!(fields[0].value, "/path/to/file.md");
    }

    #[test]
    fn test_if_then_else_traversal() {
        let schema = r#"{
            "type": "object",
            "allOf": [
                {
                    "if": {
                        "properties": {"type": {"const": "file"}}
                    },
                    "then": {
                        "properties": {
                            "source": {
                                "type": "string",
                                "format": "path_exists_absolute"
                            }
                        }
                    },
                    "else": {
                        "properties": {
                            "url": {"type": "string"}
                        }
                    }
                }
            ]
        }"#;
        let data = r#"{"type": "file", "source": "/data/input.csv"}"#;
        let fields = extract_path_fields(schema, data).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].json_pointer, "/source");
    }

    #[test]
    fn test_missing_data_fields_skipped() {
        let schema = r#"{
            "type": "object",
            "properties": {
                "required_path": {
                    "type": "string",
                    "format": "path_exists_absolute"
                },
                "optional_path": {
                    "type": "string",
                    "format": "path_exists_absolute"
                }
            }
        }"#;
        let data = r#"{"required_path": "/exists"}"#;
        let fields = extract_path_fields(schema, data).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].value, "/exists");
    }

    #[test]
    fn test_real_schema_structure() {
        // Mimics the real agent-paths-resolved schema structure:
        // top-level properties -> nested objects -> allOf with if/then/else
        let schema = r#"{
            "type": "object",
            "properties": {
                "security": {
                    "type": "object",
                    "properties": {
                        "schemas": {
                            "type": "object",
                            "properties": {
                                "security_input_schema": {
                                    "type": "string",
                                    "format": "path_exists_absolute"
                                },
                                "security_output_schema": {
                                    "type": "string",
                                    "format": "path_exists_absolute"
                                }
                            }
                        }
                    },
                    "allOf": [
                        {
                            "if": {
                                "properties": { "has_tools": { "const": true } }
                            },
                            "then": {
                                "properties": {
                                    "tool_config": {
                                        "type": "string",
                                        "format": "path_exists_absolute"
                                    }
                                }
                            },
                            "else": {}
                        }
                    ]
                },
                "execution": {
                    "type": "object",
                    "properties": {
                        "instructions": {
                            "type": "array",
                            "items": {
                                "oneOf": [
                                    {
                                        "type": "object",
                                        "properties": {
                                            "include": {
                                                "type": "string",
                                                "format": "path_exists_absolute"
                                            }
                                        }
                                    },
                                    {
                                        "type": "object",
                                        "properties": {
                                            "text": { "type": "string" }
                                        }
                                    }
                                ]
                            }
                        }
                    }
                }
            }
        }"#;
        let data = r#"{
            "security": {
                "has_tools": true,
                "schemas": {
                    "security_input_schema": "/schemas/input.json",
                    "security_output_schema": "/schemas/output.json"
                },
                "tool_config": "/config/tools.json"
            },
            "execution": {
                "instructions": [
                    {"include": "/instructions/step1.md"},
                    {"text": "inline instruction"},
                    {"include": "/instructions/step3.md"}
                ]
            }
        }"#;
        let fields = extract_path_fields(schema, data).unwrap();
        // Should find: input_schema, output_schema, tool_config, step1.md, step3.md
        assert_eq!(fields.len(), 5);
        let values: Vec<&str> = fields.iter().map(|f| f.value.as_str()).collect();
        assert!(values.contains(&"/schemas/input.json"));
        assert!(values.contains(&"/schemas/output.json"));
        assert!(values.contains(&"/config/tools.json"));
        assert!(values.contains(&"/instructions/step1.md"));
        assert!(values.contains(&"/instructions/step3.md"));
    }
}
