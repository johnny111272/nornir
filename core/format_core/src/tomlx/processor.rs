//! Main processing pipeline for .tomlx files.
//!
//! Orchestrates: parse -> validate -> convert -> output.

use std::collections::HashMap;
use std::path::Path;

use error_core::FormatError;
use serde_json::{Map, Value};

use super::diagnostics::{IncompatibleUnitFamily, TomlxIssues, UndefinedPathReference, UnknownUnit};
use super::paths::{expand_path, expand_registry_bases};
use super::types::{
    FieldAnnotationType, PathRegistry, SectionPathInfo, TargetFamily, TomlxOutput, TypeAnnotation,
};
use super::units::{convert_unit, lookup_unit, suggest_units, target_family, target_unit_name, UnitFamily};
use super::validation::{validate_tomlx, ValidatedSection, ValidationResult};

/// Identity of a field being processed (name + source line).
struct FieldRef<'a> {
    name: &'a str,
    line: usize,
}

/// Context for path resolution operations.
struct PathContext<'a> {
    registry: &'a PathRegistry,
    config_dir: Option<&'a Path>,
}

/// Parse a .tomlx string and return the converted output.
pub fn parse_tomlx(
    source: &str,
    file: Option<&str>,
    config_dir: Option<&Path>,
) -> Result<TomlxOutput, FormatError> {
    // Step 1: Parse as TOML first (fast path)
    let toml_value: toml::Value = toml::from_str(source)
        .map_err(|e| FormatError::TomlxParse(format!("Invalid TOML: {}", e)))?;

    let json_value = toml_to_json_value(&toml_value);

    // Step 2: Validate annotations
    let validation = validate_tomlx(source, file);

    if validation.issues.has_issues() {
        return Err(FormatError::Educational {
            message: validation.issues.format(),
        });
    }

    // Step 3: Process sections
    process_sections(json_value, validation, config_dir)
}

fn toml_to_json_value(toml: &toml::Value) -> Value {
    match toml {
        toml::Value::String(s) => Value::String(s.clone()),
        toml::Value::Integer(i) => Value::Number((*i).into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        toml::Value::Boolean(b) => Value::Bool(*b),
        toml::Value::Datetime(dt) => Value::String(dt.to_string()),
        toml::Value::Array(arr) => Value::Array(arr.iter().map(toml_to_json_value).collect()),
        toml::Value::Table(table) => {
            let map: Map<String, Value> = table
                .iter()
                .map(|(k, v)| (k.clone(), toml_to_json_value(v)))
                .collect();
            Value::Object(map)
        }
    }
}

fn process_sections(
    mut data: Value,
    validation: ValidationResult,
    config_dir: Option<&Path>,
) -> Result<TomlxOutput, FormatError> {
    let mut section_units: HashMap<String, String> = HashMap::new();
    let mut section_paths: HashMap<String, SectionPathInfo> = HashMap::new();
    let mut section_types: HashMap<String, String> = HashMap::new();
    let mut typed_sections: HashMap<String, (TypeAnnotation, Vec<String>)> = HashMap::new();
    let mut issues = validation.issues;
    let mut path_registry = validation.path_registry;

    expand_registry_bases(&mut path_registry, config_dir);

    for (section_path, section) in &validation.sections {
        if section_path.is_empty() {
            if let Some(ann) = &section.annotation {
                let paths = PathContext { registry: &path_registry, config_dir };
                process_root_fields(&mut data, section, ann, &paths, &mut issues)?;
            }
            continue;
        }

        let section_data = match get_section_mut(&mut data, section_path) {
            Some(data) => data,
            None => continue,
        };

        match &section.annotation {
            None => {}
            Some(ann) => {
                if let Some(type_ann) = &ann.type_annotation {
                    section_types.insert(section_path.clone(), type_ann.as_str());
                    let field_names: Vec<String> = if let Value::Object(map) = section_data {
                        map.keys().cloned().collect()
                    } else {
                        vec![]
                    };
                    typed_sections.insert(section_path.clone(), (type_ann.clone(), field_names));
                }

                match &ann.target {
                    Some(TargetFamily::Time(_)) | Some(TargetFamily::Size) => {
                        process_unit_section(section_data, section, ann, &mut issues)?;
                        if let Some(target) = &ann.target {
                            section_units.insert(
                                section_path.clone(),
                                target_unit_name(target).to_string(),
                            );
                        }
                    }
                    Some(TargetFamily::Path) => {
                        let paths = PathContext { registry: &path_registry, config_dir };
                        process_path_section(
                            section_data, section, ann, &paths, &mut issues,
                        )?;

                        let bases: HashMap<String, String> = path_registry
                            .user_defined
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        section_paths.insert(
                            section_path.clone(),
                            SectionPathInfo { bases, expand: path_registry.expand_mode },
                        );
                    }
                    None => {}
                }
            }
        }
    }

    if issues.has_issues() {
        return Err(FormatError::Educational { message: issues.format() });
    }

    let schema = if typed_sections.is_empty() {
        None
    } else {
        Some(build_document_schema(&typed_sections))
    };

    Ok(TomlxOutput { data, schema, section_types, section_units, section_paths })
}

fn build_document_schema(typed_sections: &HashMap<String, (TypeAnnotation, Vec<String>)>) -> Value {
    let mut properties = Map::new();

    for (section_path, (type_ann, field_names)) in typed_sections {
        let field_schema = type_annotation_to_json_schema(type_ann);
        let section_schema = build_section_schema(&field_schema, field_names);
        insert_nested_schema(&mut properties, section_path, section_schema);
    }

    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("properties".to_string(), Value::Object(properties));
    Value::Object(schema)
}

fn type_annotation_to_json_schema(type_ann: &TypeAnnotation) -> Value {
    let mut schema = Map::new();
    match type_ann {
        TypeAnnotation::Int => { schema.insert("type".to_string(), Value::String("integer".to_string())); }
        TypeAnnotation::Float => { schema.insert("type".to_string(), Value::String("number".to_string())); }
        TypeAnnotation::Str => { schema.insert("type".to_string(), Value::String("string".to_string())); }
        TypeAnnotation::Bool => { schema.insert("type".to_string(), Value::String("boolean".to_string())); }
        TypeAnnotation::List(inner) => {
            schema.insert("type".to_string(), Value::String("array".to_string()));
            schema.insert("items".to_string(), type_annotation_to_json_schema(inner));
        }
    }
    Value::Object(schema)
}

fn build_section_schema(field_schema: &Value, field_names: &[String]) -> Value {
    let mut props = Map::new();
    for name in field_names {
        props.insert(name.clone(), field_schema.clone());
    }

    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("properties".to_string(), Value::Object(props));
    Value::Object(schema)
}

fn insert_nested_schema(properties: &mut Map<String, Value>, path: &str, schema: Value) {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() { return; }

    if parts.len() == 1 {
        properties.insert(parts[0].to_string(), schema);
        return;
    }

    let first = parts[0];
    let rest = parts[1..].join(".");

    let entry = properties.entry(first.to_string()).or_insert_with(|| {
        let mut obj = Map::new();
        obj.insert("type".to_string(), Value::String("object".to_string()));
        obj.insert("properties".to_string(), Value::Object(Map::new()));
        Value::Object(obj)
    });

    if let Value::Object(ref mut obj) = entry {
        if let Some(Value::Object(ref mut nested_props)) = obj.get_mut("properties") {
            insert_nested_schema(nested_props, &rest, schema);
        }
    }
}

fn get_section_mut<'a>(data: &'a mut Value, path: &str) -> Option<&'a mut Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = data;
    for part in parts {
        current = current.get_mut(part)?;
    }
    Some(current)
}

fn process_root_fields(
    data: &mut Value,
    section: &ValidatedSection,
    annotation: &super::types::SectionAnnotation,
    paths: &PathContext,
    issues: &mut TomlxIssues,
) -> Result<(), FormatError> {
    if let Value::Object(map) = data {
        for field in &section.fields {
            let field_ref = FieldRef {
                name: field.field_path.rsplit('.').next().unwrap_or(&field.field_path),
                line: field.line,
            };
            if let Some(value) = map.get_mut(field_ref.name) {
                match (&annotation.target, &field.annotation_type) {
                    (Some(target @ (TargetFamily::Time(_) | TargetFamily::Size)), FieldAnnotationType::Unit(unit)) => {
                        convert_field_unit(value, unit, target, &field_ref, issues)?;
                    }
                    (Some(TargetFamily::Path), FieldAnnotationType::Path(reference)) => {
                        convert_field_path(value, reference, paths, &field_ref, issues)?;
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn process_unit_section(
    section_data: &mut Value,
    section: &ValidatedSection,
    annotation: &super::types::SectionAnnotation,
    issues: &mut TomlxIssues,
) -> Result<(), FormatError> {
    let target = match &annotation.target {
        Some(t) => t,
        None => return Ok(()),
    };

    if let Value::Object(map) = section_data {
        for field in &section.fields {
            let field_ref = FieldRef {
                name: field.field_path.rsplit('.').next().unwrap_or(&field.field_path),
                line: field.line,
            };
            if let Some(value) = map.get_mut(field_ref.name) {
                if let FieldAnnotationType::Unit(unit) = &field.annotation_type {
                    convert_field_unit(value, unit, target, &field_ref, issues)?;
                }
            }
        }
    }
    Ok(())
}

fn convert_field_unit(
    value: &mut Value,
    unit: &str,
    target: &TargetFamily,
    field: &FieldRef,
    issues: &mut TomlxIssues,
) -> Result<(), FormatError> {
    let num = match value {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        _ => return Ok(()),
    };

    let unit_info = match lookup_unit(unit) {
        Some(info) => info,
        None => {
            issues.unknown_units.push(UnknownUnit {
                location: issues.loc(field.line),
                field_name: field.name.to_string(),
                unit: unit.to_string(),
                suggestions: suggest_units(unit),
            });
            return Ok(());
        }
    };

    let expected_family = target_family(target);
    if let Some(expected) = expected_family {
        if unit_info.family != expected {
            let (unit_fam, target_fam) = match (unit_info.family, expected) {
                (UnitFamily::Time, UnitFamily::Size) => ("time", "size"),
                (UnitFamily::Size, UnitFamily::Time) => ("size", "time"),
                _ => ("unknown", "unknown"),
            };
            issues.incompatible_unit_families.push(IncompatibleUnitFamily {
                location: issues.loc(field.line),
                field_name: field.name.to_string(),
                unit: unit.to_string(),
                unit_family: unit_fam.to_string(),
                target: target_unit_name(target).to_string(),
                target_family: target_fam.to_string(),
            });
            return Ok(());
        }
    }

    match convert_unit(num, unit, target) {
        Ok(converted) => {
            if converted.fract() == 0.0 && converted.abs() < i64::MAX as f64 {
                *value = Value::Number((converted as i64).into());
            } else if let Some(n) = serde_json::Number::from_f64(converted) {
                *value = Value::Number(n);
            }
        }
        Err(_) => {}
    }

    Ok(())
}

fn process_path_section(
    section_data: &mut Value,
    section: &ValidatedSection,
    _ann: &super::types::SectionAnnotation,
    paths: &PathContext,
    issues: &mut TomlxIssues,
) -> Result<(), FormatError> {
    if let Value::Object(map) = section_data {
        for field in &section.fields {
            let field_ref = FieldRef {
                name: field.field_path.rsplit('.').next().unwrap_or(&field.field_path),
                line: field.line,
            };
            if let Some(value) = map.get_mut(field_ref.name) {
                if let FieldAnnotationType::Path(reference) = &field.annotation_type {
                    convert_field_path(value, reference, paths, &field_ref, issues)?;
                }
            }
        }
    }
    Ok(())
}

fn convert_field_path(
    value: &mut Value,
    reference: &str,
    paths: &PathContext,
    field: &FieldRef,
    issues: &mut TomlxIssues,
) -> Result<(), FormatError> {
    let path_str = match value {
        Value::String(s) => s.clone(),
        _ => return Ok(()),
    };

    if !PathRegistry::is_builtin(reference) && !paths.registry.user_defined.contains_key(reference) {
        issues.undefined_path_references.push(UndefinedPathReference {
            location: issues.loc(field.line),
            field_name: field.name.to_string(),
            reference: reference.to_string(),
            defined_refs: paths.registry.user_defined.keys().cloned().collect(),
        });
        return Ok(());
    }

    match expand_path(&path_str, reference, paths.registry, paths.config_dir) {
        Ok(expanded) => { *value = Value::String(expanded); }
        Err(_) => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_parse_unit_conversion() {
        let source = "\n[ttl]  # target=seconds\nsession = 15  # unit=minutes\ncache = 4     # unit=hours\n";
        let result = parse_tomlx(source, None, None).unwrap();

        let ttl = result.data.get("ttl").unwrap();
        assert_eq!(ttl.get("session").unwrap(), 900);
        assert_eq!(ttl.get("cache").unwrap(), 14400);
        assert_eq!(result.section_units.get("ttl"), Some(&"seconds".to_string()));
    }

    #[test]
    fn test_parse_path_expansion() {
        env::set_var("HOME", "/Users/test");
        let source = "\n[paths]  # target=path, base=~/.ai/phoenix/, expand=user\ncli = \"cli/\"    # path=base\n";
        let result = parse_tomlx(source, None, None).unwrap();

        let paths = result.data.get("paths").unwrap();
        assert_eq!(paths.get("cli").unwrap().as_str().unwrap(), "/Users/test/.ai/phoenix/cli");
    }

    #[test]
    fn test_parse_mixed() {
        env::set_var("HOME", "/Users/test");
        let source = "\n[metadata]\nname = \"test\"\n\n[ttl]  # target=seconds\nsession = 15  # unit=minutes\n";
        let result = parse_tomlx(source, None, None).unwrap();

        assert_eq!(result.data.get("metadata").unwrap().get("name").unwrap(), "test");
        assert_eq!(result.data.get("ttl").unwrap().get("session").unwrap(), 900);
    }

    #[test]
    fn test_orphan_error() {
        let source = "\n[settings]\ntimeout = 30  # unit=seconds\n";
        let result = parse_tomlx(source, Some("config.tomlx"), None);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("timeout"));
    }

    #[test]
    fn test_size_units() {
        let source = "\n[limits]  # target=bytes\nmax_size = 500  # unit=mb\nbuffer = 64     # unit=kib\n";
        let result = parse_tomlx(source, None, None).unwrap();

        let limits = result.data.get("limits").unwrap();
        assert_eq!(limits.get("max_size").unwrap(), 500_000_000i64);
        assert_eq!(limits.get("buffer").unwrap(), 65536i64);
    }

    #[test]
    fn test_type_only_section() {
        let source = "\n[features]  # type=bool\ndark_mode = true\nnotifications = false\n";
        let result = parse_tomlx(source, None, None).unwrap();

        assert_eq!(result.data.get("features").unwrap().get("dark_mode").unwrap(), true);
        assert_eq!(result.section_types.get("features"), Some(&"bool".to_string()));
        assert!(result.schema.is_some());
    }

    #[test]
    fn test_type_with_target() {
        let source = "\n[ttl]  # type=int, target=seconds\nsession = 15  # unit=minutes\n";
        let result = parse_tomlx(source, None, None).unwrap();

        assert_eq!(result.data.get("ttl").unwrap().get("session").unwrap(), 900);
        assert_eq!(result.section_types.get("ttl"), Some(&"int".to_string()));
        assert_eq!(result.section_units.get("ttl"), Some(&"seconds".to_string()));
        assert!(result.schema.is_some());
    }

    #[test]
    fn test_list_type_schema() {
        let source = "\n[tags]  # type=list[str]\ncategories = [\"news\", \"tech\"]\n";
        let result = parse_tomlx(source, None, None).unwrap();

        assert_eq!(result.section_types.get("tags"), Some(&"list[str]".to_string()));
        let schema = result.schema.unwrap();
        let props = schema.get("properties").unwrap();
        let tags_schema = props.get("tags").unwrap();
        let tags_props = tags_schema.get("properties").unwrap();
        let cat_schema = tags_props.get("categories").unwrap();
        assert_eq!(cat_schema.get("type").unwrap(), "array");
        assert_eq!(cat_schema.get("items").unwrap().get("type").unwrap(), "string");
    }

    #[test]
    fn test_output_to_json() {
        let source = "\n[ttl]  # target=seconds\nsession = 15  # unit=minutes\n";
        let result = parse_tomlx(source, None, None).unwrap();
        let json = result.to_json();
        assert_eq!(json.get("section_units").unwrap().get("ttl").unwrap(), "seconds");
    }
}
