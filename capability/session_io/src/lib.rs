//! Session file I/O shared between the live interceptor and replay binary.
//!
//! All functions write to `{session_dir}/{filename}` and create the session
//! directory on first write. Used identically by traffic_interceptor_rewriter
//! (live) and intercept_replay — ensuring derived files are always produced
//! by the same code path.

use datagram_io::{Datagram, DatagramKind, Priority};
use std::fs;
use std::io;
use std::path::Path;

// =============================================================================
// Exchange log writers
// =============================================================================

/// Append compact JSON + newline to main_exchange_log.jsonl with fsync.
pub fn append_exchange(session_dir: &Path, value: &serde_json::Value) -> Result<(), String> {
    fs::create_dir_all(session_dir)
        .map_err(|e| format!("mkdir {}: {e}", session_dir.display()))?;
    let path = session_dir.join("main_exchange_log.jsonl");
    let compact =
        serde_json::to_string(value).map_err(|e| format!("serialize exchange: {e}"))?;
    write_engine::append_line_fsync(&path, &compact)
}

/// Append compact JSON + newline to subagent_log.jsonl with fsync.
pub fn append_subagent(session_dir: &Path, value: &serde_json::Value) -> Result<(), String> {
    fs::create_dir_all(session_dir)
        .map_err(|e| format!("mkdir {}: {e}", session_dir.display()))?;
    let path = session_dir.join("subagent_log.jsonl");
    let compact =
        serde_json::to_string(value).map_err(|e| format!("serialize subagent: {e}"))?;
    write_engine::append_line_fsync(&path, &compact)
}

/// Append compact JSON + newline to compaction_instructions.jsonl with fsync.
pub fn append_compaction(session_dir: &Path, value: &serde_json::Value) -> Result<(), String> {
    fs::create_dir_all(session_dir)
        .map_err(|e| format!("mkdir {}: {e}", session_dir.display()))?;
    let path = session_dir.join("compaction_instructions.jsonl");
    let compact =
        serde_json::to_string(value).map_err(|e| format!("serialize compaction: {e}"))?;
    write_engine::append_line_fsync(&path, &compact)
}

// =============================================================================
// Compaction state management
// =============================================================================

/// Read the last non-empty line from a file. Returns None if file is empty or missing.
pub fn read_last_line(path: &Path) -> Result<Option<String>, String> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("read {}: {e}", path.display())),
    };
    Ok(content.lines().rev().find(|l| !l.trim().is_empty()).map(String::from))
}

/// Capture pre-compaction snapshot: read last main exchange line, append to pre_compact_state.
/// Returns the pre_compact_state line number (1-based) or None if main exchange was empty.
pub fn capture_precompaction(session_dir: &Path) -> Result<Option<u64>, String> {
    let mainexch_path = session_dir.join("main_exchange_log.jsonl");
    let last_line = match read_last_line(&mainexch_path)? {
        Some(line) => line,
        None => return Ok(None),
    };

    let precomp_path = session_dir.join("pre_compact_state.jsonl");
    write_engine::append_line_fsync(&precomp_path, &last_line)?;

    let content = fs::read_to_string(&precomp_path)
        .map_err(|e| format!("read {}: {e}", precomp_path.display()))?;
    Ok(Some(content.lines().count() as u64))
}

/// Truncate main_exchange_log to just its last line (the pre-compaction exchange).
pub fn truncate_mainexch(session_dir: &Path) -> Result<(), String> {
    let path = session_dir.join("main_exchange_log.jsonl");
    let last_line = match read_last_line(&path)? {
        Some(line) => line,
        None => return Ok(()),
    };
    write_engine::write_truncate_fsync(&path, &format!("{last_line}\n"))
}

/// Record a compaction event: snapshot pre-compaction state, write instruction,
/// truncate main exchange log, emit datagram. Used by both live interceptor and replay.
///
/// The live interceptor calls this then additionally injects the compaction system
/// block and rewrites the request. Replay calls this and stops — no rewrite needed.
pub fn record_compaction(
    session_dir: &Path,
    value: &serde_json::Value,
    workspace: &str,
) -> Result<(), String> {
    let precomp_line = capture_precompaction(session_dir)?;
    append_compaction(session_dir, value)?;
    if precomp_line.is_some() {
        truncate_mainexch(session_dir)?;
    }

    let precomp_path = session_dir.join("pre_compact_state.jsonl");
    let line = precomp_line.unwrap_or(0);
    let precompact_ref = format!(
        "{}:{}",
        datagram_io::compact_path(&precomp_path.to_string_lossy()),
        line
    );
    let dg = Datagram {
        timestamp: datagram_io::now(),
        source: "bifrost".into(),
        kind: DatagramKind::Alert,
        classifier: None,
        priority: Priority::High,
        workspace: workspace.to_string(),
        detail: Some(precompact_ref.clone()),
        speech: Some(format!("Compaction detected in {workspace}")),
        payload: Some(serde_json::json!({
            "precompact": precompact_ref,
            "line": line,
        })),
    };
    datagram_io::emit(&dg);
    Ok(())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn make_test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sio_{}_{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let session = dir.join("sessions").join("sess1");
        fs::create_dir_all(&session).unwrap();
        dir
    }

    #[test]
    fn append_exchange_writes_to_session_dir() {
        let dir = make_test_dir("exchange");
        let session = dir.join("sessions/sess1");

        let value = json!({"model": "test", "messages": []});
        append_exchange(&session, &value).unwrap();

        let path = session.join("main_exchange_log.jsonl");
        assert!(path.exists());
        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_subagent_writes_to_session_dir() {
        let dir = make_test_dir("subagent");
        let session = dir.join("sessions/sess1");

        let value = json!({"model": "haiku", "tools": [{"name": "Read"}], "messages": []});
        append_subagent(&session, &value).unwrap();
        append_subagent(&session, &value).unwrap();

        let path = session.join("subagent_log.jsonl");
        assert!(path.exists());
        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_compaction_writes_to_session_dir() {
        let dir = make_test_dir("compaction");
        let session = dir.join("sessions/sess1");

        let value = json!({"system": [], "messages": [], "tools": [{"name": "Read"}]});
        append_compaction(&session, &value).unwrap();

        let path = session.join("compaction_instructions.jsonl");
        assert!(path.exists());
        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_last_line_returns_last() {
        let dir = make_test_dir("lastline");
        let path = dir.join("sessions/sess1/test.jsonl");
        fs::write(&path, "line1\nline2\nline3\n").unwrap();

        assert_eq!(read_last_line(&path).unwrap(), Some("line3".into()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_last_line_skips_empty_trailing() {
        let dir = make_test_dir("lastline_trailing");
        let path = dir.join("sessions/sess1/test.jsonl");
        fs::write(&path, "line1\nline2\n\n\n").unwrap();

        assert_eq!(read_last_line(&path).unwrap(), Some("line2".into()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_last_line_empty_file() {
        let dir = make_test_dir("lastline_empty");
        let path = dir.join("sessions/sess1/test.jsonl");
        fs::write(&path, "").unwrap();

        assert_eq!(read_last_line(&path).unwrap(), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_last_line_missing_file() {
        let dir = make_test_dir("lastline_missing");
        let path = dir.join("sessions/sess1/nonexistent.jsonl");

        assert_eq!(read_last_line(&path).unwrap(), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_last_line_single_line() {
        let dir = make_test_dir("lastline_single");
        let path = dir.join("sessions/sess1/test.jsonl");
        fs::write(&path, "only_line\n").unwrap();

        assert_eq!(read_last_line(&path).unwrap(), Some("only_line".into()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn capture_precompaction_copies_last_mainexch_line() {
        let dir = make_test_dir("precomp");
        let session = dir.join("sessions/sess1");

        let mainexch = session.join("main_exchange_log.jsonl");
        fs::write(&mainexch, "{\"n\":1}\n{\"n\":2}\n{\"n\":3}\n").unwrap();

        let line = capture_precompaction(&session).unwrap();
        assert_eq!(line, Some(1));

        let precomp = fs::read_to_string(session.join("pre_compact_state.jsonl")).unwrap();
        assert_eq!(precomp.trim(), "{\"n\":3}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn capture_precompaction_appends_on_multiple_compactions() {
        let dir = make_test_dir("precomp_multi");
        let session = dir.join("sessions/sess1");

        let mainexch = session.join("main_exchange_log.jsonl");
        fs::write(&mainexch, "{\"n\":1}\n").unwrap();
        capture_precompaction(&session).unwrap();

        fs::write(&mainexch, "{\"n\":2}\n").unwrap();
        let line = capture_precompaction(&session).unwrap();
        assert_eq!(line, Some(2));

        let precomp = fs::read_to_string(session.join("pre_compact_state.jsonl")).unwrap();
        assert_eq!(precomp.lines().count(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn capture_precompaction_empty_mainexch() {
        let dir = make_test_dir("precomp_empty");
        let session = dir.join("sessions/sess1");

        let result = capture_precompaction(&session).unwrap();
        assert_eq!(result, None);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn truncate_mainexch_keeps_last_line() {
        let dir = make_test_dir("truncate");
        let session = dir.join("sessions/sess1");

        let mainexch = session.join("main_exchange_log.jsonl");
        fs::write(&mainexch, "{\"n\":1}\n{\"n\":2}\n{\"n\":3}\n").unwrap();

        truncate_mainexch(&session).unwrap();

        let content = fs::read_to_string(&mainexch).unwrap();
        assert_eq!(content, "{\"n\":3}\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn truncate_mainexch_noop_on_empty() {
        let dir = make_test_dir("truncate_empty");
        let session = dir.join("sessions/sess1");

        let mainexch = session.join("main_exchange_log.jsonl");
        fs::write(&mainexch, "").unwrap();

        truncate_mainexch(&session).unwrap();

        let content = fs::read_to_string(&mainexch).unwrap();
        assert_eq!(content, "");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn full_compaction_flow() {
        let dir = make_test_dir("full_compaction");
        let session = dir.join("sessions/sess1");

        let mainexch = session.join("main_exchange_log.jsonl");
        fs::write(&mainexch, "{\"n\":1}\n{\"n\":2}\n{\"n\":3}\n").unwrap();

        let line = capture_precompaction(&session).unwrap();
        assert_eq!(line, Some(1));

        let compaction_value = json!({"tools": [{"name": "Read"}], "messages": [{"role": "user", "content": "compact"}]});
        append_compaction(&session, &compaction_value).unwrap();

        truncate_mainexch(&session).unwrap();

        let precomp = fs::read_to_string(session.join("pre_compact_state.jsonl")).unwrap();
        assert_eq!(precomp.trim(), "{\"n\":3}");

        let compaction = fs::read_to_string(session.join("compaction_instructions.jsonl")).unwrap();
        assert_eq!(compaction.lines().count(), 1);

        let mainexch_content = fs::read_to_string(&mainexch).unwrap();
        assert_eq!(mainexch_content, "{\"n\":3}\n");

        let _ = fs::remove_dir_all(&dir);
    }
}
