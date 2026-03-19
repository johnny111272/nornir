//! Unified traffic interceptor for the bifrost pipeline — PyO3 module.
//!
//! Receives a Claude API request as bytes, classifies it, captures raw bytes,
//! routes to the appropriate JSONL file, and — for compactions — captures the
//! pre-compaction snapshot, injects summary instructions, and returns the
//! rewritten request bytes to the caller.
//!
//! All output goes to `{intercept_dir}/sessions/{session_id}/`. The interceptor
//! never touches `{intercept_dir}/traffic/` (workspace symlinks are bifrost's job).
//!
//! Python API:
//!     import traffic_interceptor_rewriter
//!     result = traffic_interceptor_rewriter.process(json_bytes, session_id, workspace, intercept_dir)
//!     # result: {"ok": bool, "rewritten": bytes | None, "error": str | None}

use compaction_inject_core::inject_compaction_system_block;
use intercept_core::{classify_exchange, ExchangeKind};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use schema_core::EmbeddedValidator;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

static WIRE_SCHEMA: EmbeddedValidator =
    EmbeddedValidator::new(include_str!("../cc_wire_schema.json"), "cc-wire-format");

// =============================================================================
// File I/O — live only
// =============================================================================

/// Append raw bytes + newline to raw_session_log.jsonl. Unconditional.
/// Must be the first step — called before validation so schema failures leave
/// a recoverable artifact.
fn append_raw(session_dir: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::create_dir_all(session_dir)
        .map_err(|e| format!("mkdir {}: {e}", session_dir.display()))?;
    let path = session_dir.join("raw_session_log.jsonl");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    file.write_all(bytes)
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    file.write_all(b"\n")
        .map_err(|e| format!("write newline {}: {e}", path.display()))?;
    file.flush()
        .map_err(|e| format!("flush {}: {e}", path.display()))?;
    Ok(())
}

// =============================================================================
// Orchestration
// =============================================================================

fn handle_compaction(
    session_dir: &Path,
    value: &mut serde_json::Value,
    workspace: &str,
) -> Result<Option<Vec<u8>>, String> {
    // Steps 1–4: shared compaction flow (capture + write + truncate + datagram)
    session_io::record_compaction(session_dir, value, workspace)?;

    // Step 5: inject compaction system block (live only — replay skips this)
    inject_compaction_system_block(value)?;

    let output = serde_json::to_vec(value)
        .map_err(|e| format!("serialize rewritten JSON: {e}"))?;
    Ok(Some(output))
}

fn run_inner(
    session_dir: &Path,
    input_bytes: &[u8],
    workspace: &str,
) -> Result<Option<Vec<u8>>, String> {
    if input_bytes.is_empty() {
        return Err("input is empty".to_string());
    }

    // Raw capture is unconditional — happens before validation so that schema
    // failures leave a recoverable artifact in raw_session_log.jsonl.
    append_raw(session_dir, input_bytes)?;

    let json_str = std::str::from_utf8(input_bytes)
        .map_err(|e| format!("invalid UTF-8: {e}"))?;

    let validation = WIRE_SCHEMA
        .validate(json_str)
        .map_err(|e| format!("schema error: {e}"))?;

    if !validation.valid {
        return Err(format!(
            "CC wire format changed — bifrost is blind until schema is updated:\n{}",
            validation.message
        ));
    }

    let mut value = validation
        .data
        .ok_or("schema validated but data was missing")?;

    let kind = match classify_exchange(&value) {
        None => return Ok(None),
        Some(k) => k,
    };

    match kind {
        ExchangeKind::Main => {
            session_io::append_exchange(session_dir, &value)?;
            Ok(None)
        }
        ExchangeKind::Compaction => handle_compaction(session_dir, &mut value, workspace),
        ExchangeKind::Subagent => {
            session_io::append_subagent(session_dir, &value)?;
            Ok(None)
        }
    }
}

// =============================================================================
// PyO3 entry point
// =============================================================================

/// Process a Claude API request.
///
/// Returns {"ok": bool, "rewritten": bytes | None, "error": str | None}.
/// rewritten is non-None only for compaction requests (caller must replace request body).
#[pyfunction]
fn process(
    py: Python<'_>,
    json_bytes: &[u8],
    session_id: &str,
    workspace: &str,
    intercept_dir: &str,
) -> PyResult<PyObject> {
    let session_dir = PathBuf::from(intercept_dir).join("sessions").join(session_id);
    let dict = PyDict::new_bound(py);
    match run_inner(&session_dir, json_bytes, workspace) {
        Ok(rewritten) => {
            dict.set_item("ok", true)?;
            match rewritten {
                Some(bytes) => dict.set_item("rewritten", bytes.as_slice())?,
                None => dict.set_item("rewritten", py.None())?,
            }
            dict.set_item("error", py.None())?;
        }
        Err(e) => {
            dict.set_item("ok", false)?;
            dict.set_item("rewritten", py.None())?;
            dict.set_item("error", e)?;
        }
    }
    Ok(dict.into())
}

#[pymodule]
fn traffic_interceptor_rewriter(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(process, m)?)?;
    Ok(())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tir_{}_{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let session = dir.join("sessions").join("sess1");
        fs::create_dir_all(&session).unwrap();
        dir
    }

    #[test]
    fn append_raw_creates_file_and_appends() {
        let dir = make_test_dir("raw");
        let session = dir.join("sessions/sess1");

        append_raw(&session, b"{\"test\": 1}").unwrap();
        append_raw(&session, b"{\"test\": 2}").unwrap();

        let content = fs::read_to_string(session.join("raw_session_log.jsonl")).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "{\"test\": 1}");
        assert_eq!(lines[1], "{\"test\": 2}");

        let _ = fs::remove_dir_all(&dir);
    }
}
