#![allow(clippy::useless_conversion)]
//! Gate: paths_verified_input
//! TOML-to-JSON input gate with path verification.
//! Validates against paths-resolved schema and verifies all paths exist.

use pyo3::prelude::*;
use pyo3::types::PyDict;

use error_core::NornirError;
use format_core::toml_to_json;
use schemas_embedded::PATHS_RESOLVED;

fn gate_validate(input: &str) -> Result<String, NornirError> {
    let json = toml_to_json(input)?;
    let result = PATHS_RESOLVED.validate(&json)?;
    if !result.valid {
        return Err(error_core::SchemaError::ValidationFailed(result.message).into());
    }
    path_verify::verify_paths(PATHS_RESOLVED.schema_json(), &json)?;
    Ok(json)
}

#[pyfunction]
fn validate(py: Python<'_>, data: &str) -> PyResult<PyObject> {
    match gate_validate(data) {
        Ok(output) => {
            let dict = PyDict::new_bound(py);
            dict.set_item("ok", true)?;
            dict.set_item("data", &output)?;
            dict.set_item("error", py.None())?;
            Ok(dict.into())
        }
        Err(e) => {
            let dict = PyDict::new_bound(py);
            dict.set_item("ok", false)?;
            dict.set_item("data", py.None())?;
            let err = PyDict::new_bound(py);
            err.set_item("type", "validation_error")?;
            err.set_item("message", e.to_string())?;
            dict.set_item("error", err)?;
            Ok(dict.into())
        }
    }
}

#[pyfunction]
fn is_valid(_py: Python<'_>, data: &str) -> PyResult<bool> {
    Ok(gate_validate(data).is_ok())
}

#[pyfunction]
fn schema_name() -> &'static str {
    PATHS_RESOLVED.schema_name()
}

#[pymodule]
fn gate_paths_verified_input(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(validate, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid, m)?)?;
    m.add_function(wrap_pyfunction!(schema_name, m)?)?;
    Ok(())
}
