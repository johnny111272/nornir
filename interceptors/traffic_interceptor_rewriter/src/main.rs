//! Unified traffic interceptor for the bifrost pipeline.
//!
//! Receives a Claude API request on stdin, classifies it, captures raw bytes,
//! routes to the appropriate JSONL file, and — for compactions — captures the
//! pre-compaction snapshot, injects summary instructions, and writes the
//! rewritten request to stdout.
//!
//! All output goes to `{intercept_dir}/sessions/{session_id}/`. The interceptor
//! never touches `{intercept_dir}/traffic/` (workspace symlinks are bifrost's job).
//!
//! Usage:
//!     echo $JSON | traffic_interceptor_rewriter \
//!         --session-id <id> --workspace <name> --intercept-dir <path>
//!
//! Exit codes: 0=success, 1=runtime error, 2=arg parse error
//!
//! Stdout: rewritten JSON bytes for compactions, empty for everything else.
//! The caller (bifrost addon) checks stdout: if non-empty, set flow.request.content.

use clap::Parser;
use compaction_inject_core::inject_compaction_system_block;
use datagram_io::{Datagram, DatagramKind, Priority};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

// =============================================================================
// Types
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExchangeKind {
    Main,
    Compaction,
    Subagent,
}

/// Classify, capture, and optionally rewrite Claude API traffic.
#[derive(Debug, Parser)]
#[command(name = "traffic_interceptor_rewriter")]
struct Args {
    /// Session identifier for file naming
    #[arg(long)]
    session_id: String,

    /// Workspace name (used for datagram metadata, not file paths)
    #[arg(long)]
    workspace: String,

    /// Root intercept directory (~/.ai/intercept). Files go to {intercept_dir}/sessions/{session_id}/.
    #[arg(long)]
    intercept_dir: PathBuf,
}

impl Args {
    fn session_dir(&self) -> PathBuf {
        self.intercept_dir.join("sessions").join(&self.session_id)
    }
}

// =============================================================================
// Classification — pure
// =============================================================================

/// Check if the tools array contains a tool with the given name.
fn has_tool(value: &serde_json::Value, name: &str) -> bool {
    value
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .any(|tool| tool.get("name").and_then(|n| n.as_str()) == Some(name))
        })
        .unwrap_or(false)
}

/// Count tools in the request.
fn tool_count(value: &serde_json::Value) -> usize {
    value
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|arr| arr.len())
        .unwrap_or(0)
}

/// Classify an exchange by tool composition.
///
/// 1. No tools → None (skip)
/// 2. Exactly one tool = Read → Compaction (verified at rewrite step)
/// 3. Multiple tools including Task → Main (architectural: only main agent has Task)
/// 4. Everything else → Subagent (captured for analysis)
fn classify_exchange(value: &serde_json::Value) -> Option<ExchangeKind> {
    let tools = tool_count(value);
    if tools == 0 {
        return None;
    }
    if tools == 1 && has_tool(value, "Read") {
        return Some(ExchangeKind::Compaction);
    }
    if tools > 1 && has_tool(value, "Task") {
        return Some(ExchangeKind::Main);
    }
    Some(ExchangeKind::Subagent)
}

// =============================================================================
// File I/O — impure
// =============================================================================

/// Append raw bytes + newline to raw_session_log.jsonl. Unconditional.
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

/// Append compact JSON + newline to main_exchange_log.jsonl with fsync.
fn append_exchange(session_dir: &Path, value: &serde_json::Value) -> Result<(), String> {
    fs::create_dir_all(session_dir)
        .map_err(|e| format!("mkdir {}: {e}", session_dir.display()))?;
    let path = session_dir.join("main_exchange_log.jsonl");
    let compact =
        serde_json::to_string(value).map_err(|e| format!("serialize exchange: {e}"))?;
    write_engine::append_line_fsync(&path, &compact)
}

/// Append compact JSON + newline to subagent_log.jsonl with fsync.
fn append_subagent(session_dir: &Path, value: &serde_json::Value) -> Result<(), String> {
    fs::create_dir_all(session_dir)
        .map_err(|e| format!("mkdir {}: {e}", session_dir.display()))?;
    let path = session_dir.join("subagent_log.jsonl");
    let compact =
        serde_json::to_string(value).map_err(|e| format!("serialize subagent: {e}"))?;
    write_engine::append_line_fsync(&path, &compact)
}

/// Append compact JSON + newline to compaction_instructions.jsonl with fsync.
fn append_compaction(session_dir: &Path, value: &serde_json::Value) -> Result<(), String> {
    fs::create_dir_all(session_dir)
        .map_err(|e| format!("mkdir {}: {e}", session_dir.display()))?;
    let path = session_dir.join("compaction_instructions.jsonl");
    let compact =
        serde_json::to_string(value).map_err(|e| format!("serialize compaction: {e}"))?;
    write_engine::append_line_fsync(&path, &compact)
}

/// Read the last non-empty line from a file. Returns None if file is empty or missing.
fn read_last_line(path: &Path) -> Result<Option<String>, String> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("read {}: {e}", path.display())),
    };
    Ok(content.lines().rev().find(|l| !l.trim().is_empty()).map(String::from))
}

/// Capture pre-compaction snapshot: read last main exchange line, append to pre_compact_state.
/// Returns the pre_compact_state line number (1-based) or None if main exchange was empty.
fn capture_precompaction(session_dir: &Path) -> Result<Option<u64>, String> {
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
fn truncate_mainexch(session_dir: &Path) -> Result<(), String> {
    let path = session_dir.join("main_exchange_log.jsonl");
    let last_line = match read_last_line(&path)? {
        Some(line) => line,
        None => return Ok(()),
    };
    write_engine::write_truncate_fsync(&path, &format!("{last_line}\n"))
}

// =============================================================================
// Orchestration
// =============================================================================

fn handle_compaction(config: &Args, value: &mut serde_json::Value) -> Result<(), String> {
    let session_dir = config.session_dir();

    // 1. Capture pre-compaction snapshot (last mainexch line → pre_compact_state)
    let precomp_line = capture_precompaction(&session_dir)?;

    // 2. Write compaction instruction to compaction_instructions
    append_compaction(&session_dir, value)?;

    // 3. Truncate main_exchange_log to just the pre-compaction exchange
    if precomp_line.is_some() {
        truncate_mainexch(&session_dir)?;
    }

    // 4. Emit datagram
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
        workspace: config.workspace.clone(),
        detail: Some(precompact_ref.clone()),
        speech: Some(format!("Compaction detected in {}", config.workspace)),
        payload: Some(serde_json::json!({
            "precompact": precompact_ref,
            "line": line,
        })),
    };
    datagram_io::emit(&dg);

    // 5. Inject compaction system block + write rewritten JSON to stdout
    inject_compaction_system_block(value)?;
    let output = serde_json::to_vec(&value)
        .map_err(|e| format!("serialize rewritten JSON: {e}"))?;
    io::stdout()
        .write_all(&output)
        .map_err(|e| format!("write stdout: {e}"))?;
    Ok(())
}

fn run(config: &Args) -> Result<(), String> {
    let session_dir = config.session_dir();

    let mut input_bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut input_bytes)
        .map_err(|e| format!("reading stdin: {e}"))?;
    if input_bytes.is_empty() {
        return Err("stdin is empty".to_string());
    }

    append_raw(&session_dir, &input_bytes)?;

    let mut value: serde_json::Value = serde_json::from_slice(&input_bytes)
        .map_err(|e| format!("invalid JSON: {e}"))?;

    let kind = match classify_exchange(&value) {
        None => return Ok(()),
        Some(k) => k,
    };

    match kind {
        ExchangeKind::Main => {
            append_exchange(&session_dir, &value)?;
        }
        ExchangeKind::Compaction => {
            handle_compaction(config, &mut value)?;
        }
        ExchangeKind::Subagent => {
            append_subagent(&session_dir, &value)?;
        }
    }

    Ok(())
}

// =============================================================================
// Entry point
// =============================================================================

fn main() {
    let args = Args::parse();

    match run(&args) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // =========================================================================
    // Classification — has_tool
    // =========================================================================

    #[test]
    fn has_tool_found() {
        let value = json!({"tools": [{"name": "Bash"}, {"name": "Read"}, {"name": "Task"}]});
        assert!(has_tool(&value, "Task"));
        assert!(has_tool(&value, "Read"));
    }

    #[test]
    fn has_tool_not_found() {
        let value = json!({"tools": [{"name": "Bash"}, {"name": "Read"}]});
        assert!(!has_tool(&value, "Task"));
    }

    #[test]
    fn has_tool_no_tools_key() {
        let value = json!({"system": []});
        assert!(!has_tool(&value, "Read"));
    }

    #[test]
    fn has_tool_empty_array() {
        let value = json!({"tools": []});
        assert!(!has_tool(&value, "Read"));
    }

    // =========================================================================
    // Classification — tool_count
    // =========================================================================

    #[test]
    fn tool_count_with_tools() {
        let value = json!({"tools": [{"name": "A"}, {"name": "B"}, {"name": "C"}]});
        assert_eq!(tool_count(&value), 3);
    }

    #[test]
    fn tool_count_no_tools_key() {
        let value = json!({"system": []});
        assert_eq!(tool_count(&value), 0);
    }

    #[test]
    fn tool_count_empty_array() {
        let value = json!({"tools": []});
        assert_eq!(tool_count(&value), 0);
    }

    // =========================================================================
    // Classification — classify_exchange
    // =========================================================================

    #[test]
    fn classify_main_has_task_and_multiple_tools() {
        let value = json!({
            "tools": [{"name": "Bash"}, {"name": "Read"}, {"name": "Write"}, {"name": "Task"}]
        });
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Main));
    }

    #[test]
    fn classify_compaction_single_read() {
        let value = json!({"tools": [{"name": "Read"}]});
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Compaction));
    }

    #[test]
    fn classify_no_tools_skipped() {
        let value = json!({"tools": []});
        assert_eq!(classify_exchange(&value), None);
    }

    #[test]
    fn classify_subagent_many_tools_no_task() {
        let value = json!({
            "tools": [{"name": "Bash"}, {"name": "Read"}, {"name": "Write"}, {"name": "Grep"}]
        });
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Subagent));
    }

    #[test]
    fn classify_web_search_only_becomes_subagent() {
        let value = json!({"tools": [{"name": "web_search"}]});
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Subagent));
    }

    #[test]
    fn classify_single_non_read_tool_subagent() {
        let value = json!({"tools": [{"name": "Bash"}]});
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Subagent));
    }

    #[test]
    fn classify_task_only_becomes_subagent() {
        let value = json!({"tools": [{"name": "Task"}]});
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Subagent));
    }

    #[test]
    fn classify_no_tools_key_skipped() {
        let value = json!({"system": []});
        assert_eq!(classify_exchange(&value), None);
    }

    // =========================================================================
    // Arg parsing (clap)
    // =========================================================================

    #[test]
    fn parse_args_valid() {
        let args = Args::try_parse_from([
            "traffic_interceptor_rewriter",
            "--session-id", "abc123",
            "--workspace", "odinn",
            "--intercept-dir", "/tmp/intercept",
        ]).unwrap();
        assert_eq!(args.session_id, "abc123");
        assert_eq!(args.workspace, "odinn");
        assert_eq!(args.intercept_dir, PathBuf::from("/tmp/intercept"));
        assert_eq!(args.session_dir(), PathBuf::from("/tmp/intercept/sessions/abc123"));
    }

    #[test]
    fn parse_args_missing_session_id() {
        let result = Args::try_parse_from([
            "traffic_interceptor_rewriter", "--workspace", "odinn", "--intercept-dir", "/tmp",
        ]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--session-id"), "should mention --session-id: {err}");
    }

    #[test]
    fn parse_args_missing_workspace() {
        let result = Args::try_parse_from([
            "traffic_interceptor_rewriter", "--session-id", "abc", "--intercept-dir", "/tmp",
        ]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--workspace"), "should mention --workspace: {err}");
    }

    #[test]
    fn parse_args_missing_intercept_dir() {
        let result = Args::try_parse_from([
            "traffic_interceptor_rewriter", "--session-id", "abc", "--workspace", "odinn",
        ]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--intercept-dir"), "should mention --intercept-dir: {err}");
    }

    #[test]
    fn parse_args_unknown_flag() {
        let result = Args::try_parse_from([
            "traffic_interceptor_rewriter",
            "--session-id", "abc",
            "--workspace", "odinn",
            "--intercept-dir", "/tmp",
            "--banana",
        ]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--banana"), "should mention unknown flag: {err}");
    }

    // =========================================================================
    // File I/O — integration tests with temp dirs
    // =========================================================================

    /// Create test dir with per-session directory: temp/sessions/sess1/
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

    // =========================================================================
    // read_last_line
    // =========================================================================

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

    // =========================================================================
    // capture_precompaction + truncate_mainexch
    // =========================================================================

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

        // No mainexch file exists
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

        // Simulate 3 main exchanges
        let mainexch = session.join("main_exchange_log.jsonl");
        fs::write(&mainexch, "{\"n\":1}\n{\"n\":2}\n{\"n\":3}\n").unwrap();

        // Capture precompaction
        let line = capture_precompaction(&session).unwrap();
        assert_eq!(line, Some(1));

        // Write compaction instruction
        let compaction_value = json!({"tools": [{"name": "Read"}], "messages": [{"role": "user", "content": "compact"}]});
        append_compaction(&session, &compaction_value).unwrap();

        // Truncate main exchange log
        truncate_mainexch(&session).unwrap();

        // Verify: pre_compact_state has the last exchange
        let precomp = fs::read_to_string(session.join("pre_compact_state.jsonl")).unwrap();
        assert_eq!(precomp.trim(), "{\"n\":3}");

        // Verify: compaction_instructions has the instruction
        let compaction = fs::read_to_string(session.join("compaction_instructions.jsonl")).unwrap();
        assert_eq!(compaction.lines().count(), 1);

        // Verify: main_exchange_log truncated to last line
        let mainexch_content = fs::read_to_string(&mainexch).unwrap();
        assert_eq!(mainexch_content, "{\"n\":3}\n");

        let _ = fs::remove_dir_all(&dir);
    }
}
