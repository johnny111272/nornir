//! Unified traffic interceptor for the bifrost pipeline.
//!
//! Receives a Claude API request on stdin, classifies it, captures raw bytes,
//! routes to the appropriate JSONL file, and — for compactions — injects
//! summary instructions and writes the rewritten request to stdout.
//!
//! Usage:
//!     echo $JSON | traffic_interceptor_rewriter \
//!         --session-id <id> --workspace <name> --traffic-dir <path>
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

    /// Workspace name for directory routing
    #[arg(long)]
    workspace: String,

    /// Root directory for traffic JSONL files
    #[arg(long)]
    traffic_dir: PathBuf,
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

/// Append raw bytes + newline to rawdata_{session_id}.jsonl. Unconditional.
fn append_raw(traffic_dir: &Path, session_id: &str, bytes: &[u8]) -> Result<(), String> {
    fs::create_dir_all(traffic_dir)
        .map_err(|e| format!("mkdir {}: {e}", traffic_dir.display()))?;
    let path = traffic_dir.join(format!("rawdata_{session_id}.jsonl"));
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

/// Append compact JSON + newline to {workspace}/mainexch_{session_id}.jsonl with fsync.
fn append_exchange(
    traffic_dir: &Path,
    workspace: &str,
    session_id: &str,
    value: &serde_json::Value,
) -> Result<(), String> {
    let dir = traffic_dir.join(workspace);
    fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    let path = dir.join(format!("mainexch_{session_id}.jsonl"));
    let compact =
        serde_json::to_string(value).map_err(|e| format!("serialize exchange: {e}"))?;
    write_engine::append_line_fsync(&path, &compact)
}

/// Append compact JSON + newline to {workspace}/subagent_{session_id}.jsonl with fsync.
fn append_subagent(
    traffic_dir: &Path,
    workspace: &str,
    session_id: &str,
    value: &serde_json::Value,
) -> Result<(), String> {
    let dir = traffic_dir.join(workspace);
    fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    let path = dir.join(format!("subagent_{session_id}.jsonl"));
    let compact =
        serde_json::to_string(value).map_err(|e| format!("serialize subagent: {e}"))?;
    write_engine::append_line_fsync(&path, &compact)
}

/// Append compact JSON + newline to {workspace}/precomp_{session_id}.jsonl with fsync.
/// Returns the line number of the appended entry (1-based).
fn append_precompact(
    traffic_dir: &Path,
    workspace: &str,
    session_id: &str,
    value: &serde_json::Value,
) -> Result<u64, String> {
    let dir = traffic_dir.join(workspace);
    fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    let path = dir.join(format!("precomp_{session_id}.jsonl"));
    let compact =
        serde_json::to_string(value).map_err(|e| format!("serialize precompact: {e}"))?;
    write_engine::append_line_fsync(&path, &compact)?;
    count_lines(&path)
}

/// Count lines in a file.
fn count_lines(path: &Path) -> Result<u64, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(content.lines().count() as u64)
}

// =============================================================================
// Orchestration
// =============================================================================

fn handle_compaction(config: &Args, value: &mut serde_json::Value) -> Result<(), String> {
    let line = append_precompact(
        &config.traffic_dir,
        &config.workspace,
        &config.session_id,
        value,
    )?;

    let precompact_path = config.traffic_dir
        .join(&config.workspace)
        .join(format!("precomp_{}.jsonl", config.session_id));
    let precompact_ref = format!("{}:{}", datagram_io::compact_path(&precompact_path.to_string_lossy()), line);
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

    inject_compaction_system_block(value)?;

    let output = serde_json::to_vec(&value)
        .map_err(|e| format!("serialize rewritten JSON: {e}"))?;
    io::stdout()
        .write_all(&output)
        .map_err(|e| format!("write stdout: {e}"))?;
    Ok(())
}

fn run(config: &Args) -> Result<(), String> {
    let mut input_bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut input_bytes)
        .map_err(|e| format!("reading stdin: {e}"))?;
    if input_bytes.is_empty() {
        return Err("stdin is empty".to_string());
    }

    append_raw(&config.traffic_dir, &config.session_id, &input_bytes)?;

    let mut value: serde_json::Value = serde_json::from_slice(&input_bytes)
        .map_err(|e| format!("invalid JSON: {e}"))?;

    let kind = match classify_exchange(&value) {
        None => return Ok(()),
        Some(k) => k,
    };

    match kind {
        ExchangeKind::Main => {
            append_exchange(&config.traffic_dir, &config.workspace, &config.session_id, &value)?;
        }
        ExchangeKind::Compaction => {
            handle_compaction(config, &mut value)?;
        }
        ExchangeKind::Subagent => {
            append_subagent(&config.traffic_dir, &config.workspace, &config.session_id, &value)?;
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
        let value = json!({
            "tools": [{"name": "Read"}]
        });
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
        let value = json!({
            "tools": [{"name": "web_search"}]
        });
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Subagent));
    }

    #[test]
    fn classify_single_non_read_tool_subagent() {
        let value = json!({
            "tools": [{"name": "Bash"}]
        });
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Subagent));
    }

    #[test]
    fn classify_task_only_becomes_subagent() {
        // Single Task tool: tools > 1 check fails, so not Main
        let value = json!({
            "tools": [{"name": "Task"}]
        });
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
            "--traffic-dir", "/tmp/traffic",
        ]).unwrap();
        assert_eq!(args.session_id, "abc123");
        assert_eq!(args.workspace, "odinn");
        assert_eq!(args.traffic_dir, PathBuf::from("/tmp/traffic"));
    }

    #[test]
    fn parse_args_missing_session_id() {
        let result = Args::try_parse_from(["traffic_interceptor_rewriter", "--workspace", "odinn", "--traffic-dir", "/tmp"]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--session-id"), "should mention --session-id: {err}");
    }

    #[test]
    fn parse_args_missing_workspace() {
        let result = Args::try_parse_from(["traffic_interceptor_rewriter", "--session-id", "abc", "--traffic-dir", "/tmp"]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--workspace"), "should mention --workspace: {err}");
    }

    #[test]
    fn parse_args_missing_traffic_dir() {
        let result = Args::try_parse_from(["traffic_interceptor_rewriter", "--session-id", "abc", "--workspace", "odinn"]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--traffic-dir"), "should mention --traffic-dir: {err}");
    }

    #[test]
    fn parse_args_unknown_flag() {
        let result = Args::try_parse_from([
            "traffic_interceptor_rewriter",
            "--session-id", "abc",
            "--workspace", "odinn",
            "--traffic-dir", "/tmp",
            "--banana",
        ]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--banana"), "should mention unknown flag: {err}");
    }

    #[test]
    fn parse_args_session_id_missing_value() {
        let result = Args::try_parse_from(["traffic_interceptor_rewriter", "--session-id"]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("session-id"), "should mention --session-id: {err}");
    }

    // =========================================================================
    // File I/O — integration tests with temp dirs
    // =========================================================================

    #[test]
    fn append_raw_creates_file_and_appends() {
        let dir = std::env::temp_dir().join("tir_test_raw");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        append_raw(&dir, "sess1", b"{\"test\": 1}").unwrap();
        append_raw(&dir, "sess1", b"{\"test\": 2}").unwrap();

        let content = fs::read_to_string(dir.join("rawdata_sess1.jsonl")).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "{\"test\": 1}");
        assert_eq!(lines[1], "{\"test\": 2}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_exchange_creates_workspace_dir() {
        let dir = std::env::temp_dir().join("tir_test_exchange");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let value = json!({"model": "test", "messages": []});
        append_exchange(&dir, "myworkspace", "sess1", &value).unwrap();

        let path = dir.join("myworkspace/mainexch_sess1.jsonl");
        assert!(path.exists());

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content.lines().count(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_subagent_creates_file() {
        let dir = std::env::temp_dir().join("tir_test_subagent");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let value = json!({"model": "claude-haiku-4-5-20251001", "tools": [{"name": "Read"}], "messages": []});
        append_subagent(&dir, "bragi", "sess1", &value).unwrap();
        append_subagent(&dir, "bragi", "sess1", &value).unwrap();

        let path = dir.join("bragi/subagent_sess1.jsonl");
        assert!(path.exists());

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content.lines().count(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_precompact_returns_line_count() {
        let dir = std::env::temp_dir().join("tir_test_precompact");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let value = json!({"system": [], "messages": []});
        let line1 = append_precompact(&dir, "ws", "sess1", &value).unwrap();
        let line2 = append_precompact(&dir, "ws", "sess1", &value).unwrap();
        let line3 = append_precompact(&dir, "ws", "sess1", &value).unwrap();

        assert_eq!(line1, 1);
        assert_eq!(line2, 2);
        assert_eq!(line3, 3);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn count_lines_empty_file() {
        let dir = std::env::temp_dir().join("tir_test_count");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let path = dir.join("empty.jsonl");
        fs::write(&path, "").unwrap();

        assert_eq!(count_lines(&path).unwrap(), 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn count_lines_three_lines() {
        let dir = std::env::temp_dir().join("tir_test_count3");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let path = dir.join("three.jsonl");
        fs::write(&path, "line1\nline2\nline3\n").unwrap();

        assert_eq!(count_lines(&path).unwrap(), 3);

        let _ = fs::remove_dir_all(&dir);
    }
}
