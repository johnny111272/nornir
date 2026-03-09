//! PyO3 module exposing intercept pipeline I/O operations to Python.
//!
//! Two functions:
//! - `json_to_toml(json_str)` — strip nulls, convert JSON to TOML string
//! - `append_jsonl_line(path, json_str)` — validate JSON object, compact append with fsync

use pyo3::prelude::*;
use std::fs::OpenOptions;
use std::io::Write;

/// Convert a JSON string to TOML, stripping null values.
///
/// Delegates to format_core: strip_nulls then serialize to TOML.
/// Returns the TOML string on success, raises ValueError on parse/conversion failure.
#[pyfunction]
fn json_to_toml(json_str: &str) -> PyResult<String> {
    let value: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("invalid JSON: {e}")))?;

    let cleaned = format_core::convert::strip_nulls(value);

    format_core::serialize::to_toml(&cleaned)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("TOML conversion failed: {e}")))
}

/// Append one compact JSON line to a JSONL file with fsync.
///
/// Validates that input parses as a JSON object (not array or scalar).
/// Serializes compact (no pretty-print), appends line + newline, calls fsync.
#[pyfunction]
fn append_jsonl_line(path: &str, json_str: &str) -> PyResult<()> {
    let value: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("invalid JSON: {e}")))?;

    if !value.is_object() {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "expected JSON object, got array or scalar",
        ));
    }

    let compact = serde_json::to_string(&value)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("serialize failed: {e}")))?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| pyo3::exceptions::PyOSError::new_err(format!("open {path}: {e}")))?;

    file.write_all(compact.as_bytes())
        .map_err(|e| pyo3::exceptions::PyOSError::new_err(format!("write failed: {e}")))?;
    file.write_all(b"\n")
        .map_err(|e| pyo3::exceptions::PyOSError::new_err(format!("write newline failed: {e}")))?;
    file.sync_all()
        .map_err(|e| pyo3::exceptions::PyOSError::new_err(format!("fsync failed: {e}")))?;

    Ok(())
}

#[pymodule]
fn intercept_io(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(json_to_toml, m)?)?;
    m.add_function(wrap_pyfunction!(append_jsonl_line, m)?)?;
    Ok(())
}
