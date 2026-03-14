//! Conditional default application for JSON Schema if/then/else blocks.
//!
//! Pure logic — no IO. Walks a JSON Schema's `allOf` entries, evaluates
//! `if` clauses against data, applies defaults from `then` branches for
//! required fields, and strips forbidden fields from `else` branches.
//!
//! Handles both group-level (sibling) and root-level (cross-section)
//! conditionals. Cross-section conditionals use nested property paths
//! to reference fields across schema sections.

use serde_json::Value;

// ─── Public types ────────────────────────────────────────────────────

/// What the gate did to the data.
#[derive(Debug, Default)]
pub struct ApplyReport {
    pub defaults_applied: Vec<DefaultApplied>,
    pub fields_stripped: Vec<FieldStripped>,
}

/// A default value that was injected into the data.
#[derive(Debug)]
pub struct DefaultApplied {
    pub path: String,
    pub value_summary: String,
}

/// A field that was removed because its conditional forbids it.
#[derive(Debug)]
pub struct FieldStripped {
    pub path: String,
    pub reason: String,
}

// ─── Entry point ─────────────────────────────────────────────────────

/// Walk the schema, evaluate conditionals, apply defaults, strip forbidden fields.
///
/// Processes unconditional defaults first (fields with `default` that are `required`
/// and not governed by any conditional), then processes all `allOf` conditional
/// entries at every nesting level.
pub fn apply_conditional_defaults(
    schema: &Value,
    data: &mut Value,
) -> ApplyReport {
    let mut report = ApplyReport::default();
    apply_unconditional_defaults(schema, data, "", &mut report);
    process_all_of(schema, schema, data, "", &mut report);
    walk_properties_for_all_of(schema, schema, data, "", &mut report);
    report
}

// ─── Unconditional defaults ──────────────────────────────────────────

/// Apply defaults for required fields that have no conditional governing them.
/// These are fields listed in `required` with a `default` in their property definition.
fn apply_unconditional_defaults(
    schema: &Value,
    data: &mut Value,
    pointer: &str,
    report: &mut ApplyReport,
) {
    let schema_obj = match schema.as_object() {
        Some(o) => o,
        None => return,
    };
    let required = match schema_obj.get("required").and_then(|v| v.as_array()) {
        Some(r) => r,
        None => return,
    };
    let properties = match schema_obj.get("properties").and_then(|v| v.as_object()) {
        Some(p) => p,
        None => return,
    };
    let data_obj = match data.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    for req_val in required {
        let field_name = match req_val.as_str() {
            Some(s) => s,
            None => continue,
        };
        if data_obj.contains_key(field_name) {
            continue;
        }
        if let Some(default_val) = resolve_default(field_name, properties) {
            let path = format!("{}/{}", pointer, field_name);
            report.defaults_applied.push(DefaultApplied {
                path: path.clone(),
                value_summary: summarize_value(&default_val),
            });
            data_obj.insert(field_name.to_string(), default_val);
        }
    }
    // Recurse into nested objects that exist in both schema properties and data
    for (key, prop_schema) in properties {
        if prop_schema.as_object().map_or(true, |o| o.get("type").and_then(|t| t.as_str()) != Some("object")) {
            continue;
        }
        if let Some(child_data) = data_obj.get_mut(key) {
            let child_pointer = format!("{}/{}", pointer, key);
            apply_unconditional_defaults(prop_schema, child_data, &child_pointer, report);
        }
    }
}

// ─── Conditional processing ──────────────────────────────────────────

/// Process `allOf` entries at the current schema level.
fn process_all_of(
    root_schema: &Value,
    local_schema: &Value,
    data: &mut Value,
    pointer: &str,
    report: &mut ApplyReport,
) {
    let all_of = match local_schema
        .as_object()
        .and_then(|o| o.get("allOf"))
        .and_then(|v| v.as_array())
    {
        Some(a) => a.clone(),
        None => return,
    };
    for entry in &all_of {
        let if_clause = match entry.get("if") {
            Some(c) => c,
            None => continue,
        };
        let matched = matches_if_clause(if_clause, data, pointer);
        if matched {
            if let Some(then_clause) = entry.get("then") {
                apply_flat_then(then_clause, local_schema, data, pointer, report);
                apply_cross_section_then(then_clause, root_schema, data, report);
            }
        } else if let Some(else_clause) = entry.get("else") {
            apply_else_strip(else_clause, data, pointer, report);
        }
    }
}

/// Walk into schema properties to find nested `allOf` at group/section level.
fn walk_properties_for_all_of(
    root_schema: &Value,
    local_schema: &Value,
    data: &mut Value,
    pointer: &str,
    report: &mut ApplyReport,
) {
    let properties = match local_schema
        .as_object()
        .and_then(|o| o.get("properties"))
        .and_then(|v| v.as_object())
    {
        Some(p) => p.clone(),
        None => return,
    };
    for (key, prop_schema) in &properties {
        let is_object = prop_schema
            .as_object()
            .map_or(false, |o| o.get("type").and_then(|t| t.as_str()) == Some("object"));
        if !is_object {
            continue;
        }
        let child_pointer = format!("{}/{}", pointer, key);
        process_all_of(root_schema, prop_schema, data, &child_pointer, report);
        walk_properties_for_all_of(root_schema, prop_schema, data, &child_pointer, report);
    }
}

// ─── If-clause evaluation ────────────────────────────────────────────

/// Check if an `if` clause matches the data.
///
/// Handles two forms:
/// 1. Flat (sibling): `{ "required": ["field"], "properties": { "field": { "const": ... } } }`
/// 2. Nested (cross-section): `{ "properties": { "section": { "properties": { "group": { ... } } } } }`
fn matches_if_clause(if_clause: &Value, data: &Value, pointer: &str) -> bool {
    let if_obj = match if_clause.as_object() {
        Some(o) => o,
        None => return false,
    };

    // Check for required + properties at this level (flat/sibling conditional)
    if let Some(required) = if_obj.get("required").and_then(|v| v.as_array()) {
        let local_data = resolve_pointer(data, pointer);
        return matches_flat_condition(required, if_obj.get("properties"), local_data);
    }

    // No required at this level — must be a nested cross-section conditional.
    // Walk the nested properties path to find the leaf with required.
    if let Some(props) = if_obj.get("properties").and_then(|v| v.as_object()) {
        return matches_nested_condition(props, data);
    }

    false
}

/// Evaluate a flat condition: required fields present + property constraints match.
fn matches_flat_condition(
    required: &[Value],
    properties: Option<&Value>,
    data: Option<&Value>,
) -> bool {
    let data_obj = match data.and_then(|v| v.as_object()) {
        Some(o) => o,
        None => return false,
    };

    for req_val in required {
        let field_name = match req_val.as_str() {
            Some(s) => s,
            None => return false,
        };
        if !data_obj.contains_key(field_name) {
            return false;
        }
        // If there are property constraints, check them
        if let Some(constraint) = properties
            .and_then(|p| p.as_object())
            .and_then(|p| p.get(field_name))
        {
            let field_value = &data_obj[field_name];
            if !matches_constraint(field_value, constraint) {
                return false;
            }
        }
    }
    true
}

/// Walk nested property paths for cross-section conditionals.
/// Each level has `properties: { segment: { ... } }` until we reach a leaf
/// with `required` + `properties` (the actual condition).
fn matches_nested_condition(
    props: &serde_json::Map<String, Value>,
    data: &Value,
) -> bool {
    for (segment, nested) in props {
        let nested_obj = match nested.as_object() {
            Some(o) => o,
            None => return false,
        };
        let data_child = match data.as_object().and_then(|o| o.get(segment)) {
            Some(v) => v,
            None => return false,
        };

        // If this level has `required`, it's the leaf — evaluate here
        if let Some(required) = nested_obj.get("required").and_then(|v| v.as_array()) {
            return matches_flat_condition(required, nested_obj.get("properties"), Some(data_child));
        }

        // Otherwise recurse deeper
        if let Some(inner_props) = nested_obj.get("properties").and_then(|v| v.as_object()) {
            return matches_nested_condition(inner_props, data_child);
        }
    }
    false
}

/// Check if a data value matches a const or enum constraint.
fn matches_constraint(value: &Value, constraint: &Value) -> bool {
    let constraint_obj = match constraint.as_object() {
        Some(o) => o,
        None => return false,
    };
    if let Some(const_val) = constraint_obj.get("const") {
        return value == const_val;
    }
    if let Some(enum_vals) = constraint_obj.get("enum").and_then(|v| v.as_array()) {
        return enum_vals.contains(value);
    }
    false
}

// ─── Then-branch: apply defaults ─────────────────────────────────────

/// Inject defaults for required fields at a specific data location.
/// The functional primitive: given required field names, schema properties
/// (for default lookup), and a data object — fill in missing defaults.
fn inject_required_defaults(
    required: &[Value],
    schema_props: Option<&serde_json::Map<String, Value>>,
    data_obj: &mut serde_json::Map<String, Value>,
    pointer: &str,
    report: &mut ApplyReport,
) {
    let props = match schema_props {
        Some(p) => p,
        None => return,
    };
    for req_val in required {
        let field_name = match req_val.as_str() {
            Some(s) => s,
            None => continue,
        };
        if data_obj.contains_key(field_name) {
            continue;
        }
        if let Some(default_val) = resolve_default(field_name, props) {
            let path = format!("{}/{}", pointer, field_name);
            report.defaults_applied.push(DefaultApplied {
                path,
                value_summary: summarize_value(&default_val),
            });
            data_obj.insert(field_name.to_string(), default_val);
        }
    }
}

/// Flat then: inject defaults for required fields at the local pointer.
fn apply_flat_then(
    then_clause: &Value,
    local_schema: &Value,
    data: &mut Value,
    pointer: &str,
    report: &mut ApplyReport,
) {
    let required = match then_clause.as_object().and_then(|o| o.get("required")).and_then(|v| v.as_array()) {
        Some(r) => r,
        None => return,
    };
    let schema_props = local_schema
        .as_object()
        .and_then(|o| o.get("properties"))
        .and_then(|v| v.as_object());
    if let Some(data_obj) = resolve_pointer_mut(data, pointer).and_then(|v| v.as_object_mut()) {
        inject_required_defaults(required, schema_props, data_obj, pointer, report);
    }
}

/// Cross-section then: walk nested property paths to find required fields.
fn apply_cross_section_then(
    then_clause: &Value,
    root_schema: &Value,
    data: &mut Value,
    report: &mut ApplyReport,
) {
    let props = match then_clause.as_object().and_then(|o| o.get("properties")).and_then(|v| v.as_object()) {
        Some(p) => p,
        None => return,
    };
    apply_nested_then(props, root_schema, data, "", report);
}

/// Walk nested property paths in a cross-section `then` clause.
fn apply_nested_then(
    props: &serde_json::Map<String, Value>,
    root_schema: &Value,
    data: &mut Value,
    pointer: &str,
    report: &mut ApplyReport,
) {
    for (segment, nested) in props {
        let nested_obj = match nested.as_object() {
            Some(o) => o,
            None => continue,
        };
        let child_pointer = format!("{}/{}", pointer, segment);

        if let Some(required) = nested_obj.get("required").and_then(|v| v.as_array()) {
            let schema_props = resolve_schema_properties(root_schema, &child_pointer);
            if let Some(data_obj) = resolve_pointer_mut(data, &child_pointer)
                .and_then(|v| v.as_object_mut())
            {
                inject_required_defaults(required, schema_props, data_obj, &child_pointer, report);
            }
            continue;
        }

        if let Some(inner_props) = nested_obj.get("properties").and_then(|v| v.as_object()) {
            apply_nested_then(inner_props, root_schema, data, &child_pointer, report);
        }
    }
}

// ─── Else-branch: strip forbidden fields ─────────────────────────────

/// Process an `else` branch: strip fields marked as `false`.
fn apply_else_strip(
    else_clause: &Value,
    data: &mut Value,
    pointer: &str,
    report: &mut ApplyReport,
) {
    let else_obj = match else_clause.as_object() {
        Some(o) => o,
        None => return,
    };

    // Flat else: { "properties": { "field": false } }
    if let Some(props) = else_obj.get("properties").and_then(|v| v.as_object()) {
        let is_flat = props.values().any(|v| v.is_boolean());
        if is_flat {
            let data_obj = match resolve_pointer_mut(data, pointer).and_then(|v| v.as_object_mut()) {
                Some(o) => o,
                None => return,
            };
            for (field_name, prop_val) in props {
                if *prop_val == Value::Bool(false) && data_obj.remove(field_name).is_some() {
                    let path = format!("{}/{}", pointer, field_name);
                    report.fields_stripped.push(FieldStripped {
                        path,
                        reason: "forbidden by conditional".to_string(),
                    });
                }
            }
            return;
        }

        // Nested else (cross-section)
        strip_nested_else(props, data, "", report);
    }
}

/// Walk nested property paths in a cross-section `else` clause.
fn strip_nested_else(
    props: &serde_json::Map<String, Value>,
    data: &mut Value,
    pointer: &str,
    report: &mut ApplyReport,
) {
    for (segment, nested) in props {
        let nested_obj = match nested.as_object() {
            Some(o) => o,
            None => {
                // This is `field_name: false` — strip at current level
                if *nested == Value::Bool(false) {
                    let data_obj = match resolve_pointer_mut(data, pointer)
                        .and_then(|v| v.as_object_mut())
                    {
                        Some(o) => o,
                        None => continue,
                    };
                    if data_obj.remove(segment).is_some() {
                        let path = format!("{}/{}", pointer, segment);
                        report.fields_stripped.push(FieldStripped {
                            path,
                            reason: "forbidden by conditional".to_string(),
                        });
                    }
                }
                continue;
            }
        };
        let child_pointer = format!("{}/{}", pointer, segment);
        if let Some(inner_props) = nested_obj.get("properties").and_then(|v| v.as_object()) {
            strip_nested_else(inner_props, data, &child_pointer, report);
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────

/// Resolve a field's `default` value from schema properties.
fn resolve_default(
    field_name: &str,
    properties: &serde_json::Map<String, Value>,
) -> Option<Value> {
    properties
        .get(field_name)?
        .as_object()?
        .get("default")
        .cloned()
}

/// Follow a JSON pointer path into a Value, returning a reference.
fn resolve_pointer<'a>(data: &'a Value, pointer: &str) -> Option<&'a Value> {
    if pointer.is_empty() {
        return Some(data);
    }
    data.pointer(pointer)
}

/// Follow a JSON pointer path into a Value, returning a mutable reference.
fn resolve_pointer_mut<'a>(data: &'a mut Value, pointer: &str) -> Option<&'a mut Value> {
    if pointer.is_empty() {
        return Some(data);
    }
    data.pointer_mut(pointer)
}

/// Resolve schema properties at a JSON pointer path.
/// Walks through nested `properties` following each path segment.
fn resolve_schema_properties<'a>(
    schema: &'a Value,
    pointer: &str,
) -> Option<&'a serde_json::Map<String, Value>> {
    let segments: Vec<&str> = pointer
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let mut current = schema;
    for segment in &segments {
        current = current
            .as_object()?
            .get("properties")?
            .as_object()?
            .get(*segment)?;
    }
    current.as_object()?.get("properties")?.as_object()
}

/// Summarize a value for logging (truncate long strings).
fn summarize_value(value: &Value) -> String {
    match value {
        Value::String(s) if s.len() > 40 => format!("\"{}...\"", &s[..37]),
        Value::String(s) => format!("\"{}\"", s),
        other => other.to_string(),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_matches_flat_const() {
        let if_clause = json!({
            "required": ["mode"],
            "properties": { "mode": { "const": "batch" } }
        });
        let data = json!({"mode": "batch"});
        assert!(matches_if_clause(&if_clause, &data, ""));
    }

    #[test]
    fn test_flat_const_mismatch() {
        let if_clause = json!({
            "required": ["mode"],
            "properties": { "mode": { "const": "batch" } }
        });
        let data = json!({"mode": "full"});
        assert!(!matches_if_clause(&if_clause, &data, ""));
    }

    #[test]
    fn test_flat_field_absent() {
        let if_clause = json!({
            "required": ["mode"],
            "properties": { "mode": { "const": "batch" } }
        });
        let data = json!({});
        assert!(!matches_if_clause(&if_clause, &data, ""));
    }

    #[test]
    fn test_matches_flat_enum() {
        let if_clause = json!({
            "required": ["return_mode"],
            "properties": { "return_mode": { "enum": ["status", "status-metrics"] } }
        });
        let data = json!({"return_mode": "status"});
        assert!(matches_if_clause(&if_clause, &data, ""));
    }

    #[test]
    fn test_flat_enum_mismatch() {
        let if_clause = json!({
            "required": ["return_mode"],
            "properties": { "return_mode": { "enum": ["status-metrics", "metrics-output"] } }
        });
        let data = json!({"return_mode": "status"});
        assert!(!matches_if_clause(&if_clause, &data, ""));
    }

    #[test]
    fn test_matches_nested_const() {
        let if_clause = json!({
            "properties": {
                "indirect": {
                    "properties": {
                        "dispatch": {
                            "properties": {
                                "dispatch_input_delivery": { "const": "file" }
                            },
                            "required": ["dispatch_input_delivery"]
                        }
                    }
                }
            }
        });
        let data = json!({
            "indirect": { "dispatch": { "dispatch_input_delivery": "file" } }
        });
        assert!(matches_if_clause(&if_clause, &data, ""));
    }

    #[test]
    fn test_nested_const_mismatch() {
        let if_clause = json!({
            "properties": {
                "indirect": {
                    "properties": {
                        "dispatch": {
                            "properties": {
                                "dispatch_input_delivery": { "const": "file" }
                            },
                            "required": ["dispatch_input_delivery"]
                        }
                    }
                }
            }
        });
        let data = json!({
            "indirect": { "dispatch": { "dispatch_input_delivery": "inline" } }
        });
        assert!(!matches_if_clause(&if_clause, &data, ""));
    }

    #[test]
    fn test_apply_then_default_for_required_field() {
        let schema = json!({
            "type": "object",
            "properties": {
                "mode": { "type": "string" },
                "status": { "type": "string", "default": "Return SUCCESS or FAILURE." }
            },
            "allOf": [{
                "if": {
                    "required": ["mode"],
                    "properties": { "mode": { "const": "status" } }
                },
                "then": { "required": ["status"] },
                "else": { "properties": { "status": false } }
            }]
        });
        let mut data = json!({"mode": "status"});
        let report = apply_conditional_defaults(&schema, &mut data);

        assert_eq!(data["status"], "Return SUCCESS or FAILURE.");
        assert_eq!(report.defaults_applied.len(), 1);
        assert!(report.defaults_applied[0].path.contains("status"));
    }

    #[test]
    fn test_strip_forbidden_field() {
        let schema = json!({
            "type": "object",
            "properties": {
                "mode": { "type": "string" },
                "output": { "type": "string", "default": "Return output." }
            },
            "allOf": [{
                "if": {
                    "required": ["mode"],
                    "properties": { "mode": { "enum": ["metrics-output", "output"] } }
                },
                "then": { "required": ["output"] },
                "else": { "properties": { "output": false } }
            }]
        });
        let mut data = json!({"mode": "status", "output": "should be stripped"});
        let report = apply_conditional_defaults(&schema, &mut data);

        assert!(data.get("output").is_none());
        assert_eq!(report.fields_stripped.len(), 1);
    }

    #[test]
    fn test_no_default_when_field_present() {
        let schema = json!({
            "type": "object",
            "properties": {
                "mode": { "type": "string" },
                "status": { "type": "string", "default": "default text" }
            },
            "allOf": [{
                "if": {
                    "required": ["mode"],
                    "properties": { "mode": { "const": "status" } }
                },
                "then": { "required": ["status"] },
                "else": { "properties": { "status": false } }
            }]
        });
        let mut data = json!({"mode": "status", "status": "custom text"});
        let report = apply_conditional_defaults(&schema, &mut data);

        assert_eq!(data["status"], "custom text");
        assert!(report.defaults_applied.is_empty());
    }

    #[test]
    fn test_unconditional_defaults_applied() {
        let schema = json!({
            "type": "object",
            "required": ["name", "mode"],
            "properties": {
                "name": { "type": "string" },
                "mode": { "type": "string", "default": "status" }
            }
        });
        let mut data = json!({"name": "test"});
        let report = apply_conditional_defaults(&schema, &mut data);

        assert_eq!(data["mode"], "status");
        assert_eq!(report.defaults_applied.len(), 1);
    }

    #[test]
    fn test_nested_unconditional_defaults() {
        let schema = json!({
            "type": "object",
            "required": ["task"],
            "properties": {
                "task": {
                    "type": "object",
                    "required": ["return"],
                    "properties": {
                        "return": {
                            "type": "object",
                            "required": ["return_mode"],
                            "properties": {
                                "return_mode": {
                                    "type": "string",
                                    "default": "status"
                                }
                            }
                        }
                    }
                }
            }
        });
        let mut data = json!({"task": {"return": {}}});
        let report = apply_conditional_defaults(&schema, &mut data);

        assert_eq!(data["task"]["return"]["return_mode"], "status");
        assert_eq!(report.defaults_applied.len(), 1);
    }

    #[test]
    fn test_chained_conditionals() {
        // output_file_format = jsonl → requires output_write_frequency
        // output_write_frequency = batch → requires output_batch_size
        let schema = json!({
            "type": "object",
            "required": ["output_file_format"],
            "properties": {
                "output_file_format": { "type": "string" },
                "output_write_frequency": { "type": "string", "default": "record" },
                "output_batch_size": { "type": "integer", "default": 20 }
            },
            "allOf": [
                {
                    "if": {
                        "required": ["output_file_format"],
                        "properties": { "output_file_format": { "const": "jsonl" } }
                    },
                    "then": { "required": ["output_write_frequency"] },
                    "else": { "properties": { "output_write_frequency": false } }
                },
                {
                    "if": {
                        "required": ["output_write_frequency"],
                        "properties": { "output_write_frequency": { "const": "batch" } }
                    },
                    "then": { "required": ["output_batch_size"] },
                    "else": { "properties": { "output_batch_size": false } }
                }
            ]
        });

        // jsonl + no write freq → should default write_freq to "record"
        // output_batch_size was never present, so nothing to strip
        let mut data = json!({"output_file_format": "jsonl"});
        let report = apply_conditional_defaults(&schema, &mut data);

        assert_eq!(data["output_write_frequency"], "record");
        assert!(data.get("output_batch_size").is_none());
        assert_eq!(report.defaults_applied.len(), 1);
        assert_eq!(report.fields_stripped.len(), 0);
    }

    #[test]
    fn test_cross_section_defaults() {
        let schema = json!({
            "type": "object",
            "required": ["task", "security"],
            "properties": {
                "task": {
                    "type": "object",
                    "required": ["processing"],
                    "properties": {
                        "processing": {
                            "type": "object",
                            "required": ["scratch_needed"],
                            "properties": {
                                "scratch_needed": { "type": "boolean", "default": false }
                            }
                        }
                    }
                },
                "security": {
                    "type": "object",
                    "properties": {
                        "security_io": {
                            "type": "object",
                            "properties": {
                                "io_scratch_tempdir": {
                                    "type": "string",
                                    "default": "scratch"
                                }
                            }
                        }
                    }
                }
            },
            "allOf": [{
                "if": {
                    "properties": {
                        "task": {
                            "properties": {
                                "processing": {
                                    "properties": { "scratch_needed": { "const": true } },
                                    "required": ["scratch_needed"]
                                }
                            }
                        }
                    }
                },
                "then": {
                    "properties": {
                        "security": {
                            "properties": {
                                "security_io": {
                                    "required": ["io_scratch_tempdir"]
                                }
                            }
                        }
                    }
                },
                "else": {
                    "properties": {
                        "security": {
                            "properties": {
                                "security_io": {
                                    "properties": { "io_scratch_tempdir": false }
                                }
                            }
                        }
                    }
                }
            }]
        });

        // scratch_needed=false → strip io_scratch_tempdir
        let mut data = json!({
            "task": { "processing": { "scratch_needed": false } },
            "security": { "security_io": { "io_scratch_tempdir": "was here" } }
        });
        let report = apply_conditional_defaults(&schema, &mut data);
        assert!(data["security"]["security_io"].get("io_scratch_tempdir").is_none());
        assert_eq!(report.fields_stripped.len(), 1);

        // scratch_needed=true, no tempdir → should NOT apply default (no default for cross-section required)
        // (cross-section fields typically don't have defaults — they must be provided)
        let mut data2 = json!({
            "task": { "processing": { "scratch_needed": true } },
            "security": { "security_io": {} }
        });
        let report2 = apply_conditional_defaults(&schema, &mut data2);
        // The default "scratch" should be applied since the schema has it
        assert_eq!(data2["security"]["security_io"]["io_scratch_tempdir"], "scratch");
        assert_eq!(report2.defaults_applied.len(), 1);
    }

    #[test]
    fn test_summarize_value_short() {
        assert_eq!(summarize_value(&json!("hello")), "\"hello\"");
    }

    #[test]
    fn test_summarize_value_long() {
        let long = "a".repeat(50);
        let result = summarize_value(&json!(long));
        assert!(result.len() < 50);
        assert!(result.ends_with("...\""));
    }

    #[test]
    fn test_summarize_value_number() {
        assert_eq!(summarize_value(&json!(42)), "42");
    }
}
