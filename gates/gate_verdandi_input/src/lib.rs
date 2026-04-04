//! Gate: gate_verdandi_input
//!
//! Generic input gate for Verdandi YAML files. Validates YAML content against
//! a JSON Schema loaded from disk at runtime. Unlike other gates, this does NOT
//! embed schemas at compile time — it loads them by name from a schema directory.
//!
//! Flow: YAML content → JSON (via format_core) → validate against schema → return JSON.

use std::collections::HashMap;
use std::fs;
use std::sync::Mutex;

use jsonschema::Validator;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use serde_json::Value;

use error_core::{format_educational_message, FormatError, ValidationIssue};
use format_core::convert::yaml_to_json;

/// Cached compiled validators keyed by schema file path.
static VALIDATOR_CACHE: Mutex<Option<HashMap<String, Validator>>> = Mutex::new(None);

/// Load a schema from disk, compile it, and insert into cache.
fn compile_and_cache_schema(
    cache: &mut HashMap<String, Validator>,
    schema_path: &str,
) -> Result<(), String> {
    let schema_str = fs::read_to_string(schema_path)
        .map_err(|io_err| format!("Cannot read schema '{}': {}", schema_path, io_err))?;

    let schema_value: Value = serde_json::from_str(&schema_str)
        .map_err(|json_err| format!("Schema '{}' is not valid JSON: {}", schema_path, json_err))?;

    let validator = Validator::new(&schema_value).map_err(|schema_err| {
        format!(
            "Schema '{}' is not valid JSON Schema: {}",
            schema_path, schema_err
        )
    })?;

    cache.insert(schema_path.to_string(), validator);
    Ok(())
}

/// Build the full schema file path from name and directory.
fn schema_file_path(schema_name: &str, schema_dir: &str) -> String {
    format!("{}/{}.schema.json", schema_dir, schema_name)
}

/// Core validation: read YAML file → JSON → validate against named schema.
///
/// The gate IS the I/O boundary. It reads the file, converts YAML to JSON,
/// validates against schema, and returns the validated JSON string.
///
/// Holds the validator cache lock for the validation phase. Compiles and
/// caches the schema on first use per schema path.
fn validate_file(
    file_path: &str,
    schema_name: &str,
    schema_dir: &str,
) -> Result<String, String> {
    let schema_path = schema_file_path(schema_name, schema_dir);

    // Read YAML file from disk
    let yaml_content = fs::read_to_string(file_path)
        .map_err(|io_err| format!("Cannot read '{}': {}", file_path, io_err))?;

    // YAML → JSON (before acquiring lock — no cache needed)
    let json_str = yaml_to_json(&yaml_content)
        .map_err(|format_err: FormatError| format!("YAML parse error in '{}': {}", file_path, format_err))?;

    let data: Value = serde_json::from_str(&json_str)
        .map_err(|json_err| format!("Invalid JSON from YAML conversion: {}", json_err))?;

    // Acquire lock, compile schema if needed, validate
    let mut cache_guard = VALIDATOR_CACHE
        .lock()
        .map_err(|poison_err| format!("Validator cache poisoned: {}", poison_err))?;
    let cache = cache_guard.get_or_insert_with(HashMap::new);

    if !cache.contains_key(&schema_path) {
        compile_and_cache_schema(cache, &schema_path)?;
    }

    let validator = cache
        .get(&schema_path)
        .ok_or_else(|| format!("Schema '{}' not in cache after compilation", schema_path))?;

    let errors: Vec<_> = validator.iter_errors(&data).collect();
    if errors.is_empty() {
        return Ok(json_str);
    }

    let issues: Vec<_> = errors
        .iter()
        .map(ValidationIssue::from_schema_error)
        .collect();
    let message = format_educational_message(schema_name, &issues);
    Err(message)
}

/// Validate a YAML file against a named schema from a schema directory.
///
/// Reads the file, converts YAML to JSON, validates against schema.
/// Returns the validated JSON string on success. Raises ValueError on failure.
#[pyfunction]
fn validate(file_path: &str, schema_name: &str, schema_dir: &str) -> PyResult<String> {
    validate_file(file_path, schema_name, schema_dir)
        .map_err(|message| PyValueError::new_err(message))
}

/// Quick validity check — no error details.
#[pyfunction]
fn is_valid(file_path: &str, schema_name: &str, schema_dir: &str) -> PyResult<bool> {
    Ok(validate_file(file_path, schema_name, schema_dir).is_ok())
}

#[pymodule]
fn gate_verdandi_input(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(validate, module)?)?;
    module.add_function(wrap_pyfunction!(is_valid, module)?)?;
    Ok(())
}
