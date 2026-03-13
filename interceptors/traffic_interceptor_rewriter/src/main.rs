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
// Constants
// =============================================================================

const MAIN_AGENT_IDENTITY: &str = "You are Claude Code, Anthropic's official CLI for Claude.";

// =============================================================================
// Types
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExchangeKind {
    Main,
    Compaction,
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

/// Check if system[1].text contains the main agent identity string.
fn is_main_agent(value: &serde_json::Value) -> bool {
    value
        .get("system")
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.get(1))
        .and_then(|block| block.get("text"))
        .and_then(|t| t.as_str())
        .map(|text| text.contains(MAIN_AGENT_IDENTITY))
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

/// Check if the request has exactly one tool named "web_search".
fn is_web_search_only(value: &serde_json::Value) -> bool {
    let tools = match value.get("tools").and_then(|t| t.as_array()) {
        Some(arr) if arr.len() == 1 => arr,
        _ => return false,
    };
    tools[0]
        .get("name")
        .and_then(|n| n.as_str())
        .map(|name| name == "web_search")
        .unwrap_or(false)
}

/// Classify an exchange into Main, Compaction, or None (ignored).
///
/// Four checks in order:
/// 1. Not main agent → None (subagents, unknown identity)
/// 2. No tools → None (haiku internal utility calls)
/// 3. Only web_search → None (opus web_search-only calls)
/// 4. Exactly one tool → Compaction, otherwise → Main
fn classify_exchange(value: &serde_json::Value) -> Option<ExchangeKind> {
    if !is_main_agent(value) {
        return None;
    }
    let tools = tool_count(value);
    if tools == 0 {
        return None;
    }
    if tools == 1 && is_web_search_only(value) {
        return None;
    }
    if tools == 1 {
        return Some(ExchangeKind::Compaction);
    }
    Some(ExchangeKind::Main)
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
    // Classification — is_main_agent
    // =========================================================================

    #[test]
    fn main_agent_detected_at_system_index_1() {
        let value = json!({
            "system": [
                {"type": "text", "text": "cache control block"},
                {"type": "text", "text": "You are Claude Code, Anthropic's official CLI for Claude.\nMore instructions here."}
            ],
            "tools": [{"name": "Bash"}, {"name": "Read"}]
        });
        assert!(is_main_agent(&value));
    }

    #[test]
    fn subagent_not_detected_as_main() {
        let value = json!({
            "system": [
                {"type": "text", "text": "cache control block"},
                {"type": "text", "text": "You are a Claude agent, built on Anthropic's Claude Agent SDK."}
            ],
            "tools": [{"name": "Bash"}, {"name": "Read"}]
        });
        assert!(!is_main_agent(&value));
    }

    #[test]
    fn missing_system_not_main() {
        let value = json!({"tools": [{"name": "Bash"}]});
        assert!(!is_main_agent(&value));
    }

    #[test]
    fn empty_system_array_not_main() {
        let value = json!({"system": [], "tools": []});
        assert!(!is_main_agent(&value));
    }

    #[test]
    fn single_system_block_not_main() {
        // Only system[0], no system[1]
        let value = json!({
            "system": [{"type": "text", "text": "You are Claude Code, Anthropic's official CLI for Claude."}],
            "tools": [{"name": "Bash"}]
        });
        assert!(!is_main_agent(&value));
    }

    // =========================================================================
    // Classification — tool helpers
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

    #[test]
    fn web_search_only_true() {
        let value = json!({"tools": [{"name": "web_search"}]});
        assert!(is_web_search_only(&value));
    }

    #[test]
    fn web_search_only_false_with_other_tool() {
        let value = json!({"tools": [{"name": "Read"}]});
        assert!(!is_web_search_only(&value));
    }

    #[test]
    fn web_search_only_false_with_multiple_tools() {
        let value = json!({"tools": [{"name": "web_search"}, {"name": "Read"}]});
        assert!(!is_web_search_only(&value));
    }

    // =========================================================================
    // Classification — classify_exchange
    // =========================================================================

    fn main_agent_system() -> serde_json::Value {
        json!([
            {"type": "text", "text": "cache block"},
            {"type": "text", "text": "You are Claude Code, Anthropic's official CLI for Claude.\nFull system prompt here."}
        ])
    }

    fn subagent_system() -> serde_json::Value {
        json!([
            {"type": "text", "text": "cache block"},
            {"type": "text", "text": "You are a Claude agent, built on Anthropic's Claude Agent SDK."}
        ])
    }

    #[test]
    fn classify_main_conversation() {
        let value = json!({
            "system": main_agent_system(),
            "tools": [{"name": "Bash"}, {"name": "Read"}, {"name": "Write"}, {"name": "Edit"}]
        });
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Main));
    }

    #[test]
    fn classify_compaction() {
        let value = json!({
            "system": main_agent_system(),
            "tools": [{"name": "Read"}]
        });
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Compaction));
    }

    #[test]
    fn classify_subagent_ignored() {
        let value = json!({
            "system": subagent_system(),
            "tools": [{"name": "Bash"}, {"name": "Read"}]
        });
        assert_eq!(classify_exchange(&value), None);
    }

    #[test]
    fn classify_no_tools_ignored() {
        let value = json!({
            "system": main_agent_system(),
            "tools": []
        });
        assert_eq!(classify_exchange(&value), None);
    }

    #[test]
    fn classify_web_search_only_ignored() {
        let value = json!({
            "system": main_agent_system(),
            "tools": [{"name": "web_search"}]
        });
        assert_eq!(classify_exchange(&value), None);
    }

    #[test]
    fn classify_missing_system_ignored() {
        let value = json!({
            "tools": [{"name": "Bash"}, {"name": "Read"}]
        });
        assert_eq!(classify_exchange(&value), None);
    }

    #[test]
    fn classify_identity_string_exact() {
        // Verify we use the exact identity string, not a substring
        let wrong_identity = json!({
            "system": [
                {"type": "text", "text": "cache"},
                {"type": "text", "text": "You are Claude Code."}
            ],
            "tools": [{"name": "Bash"}, {"name": "Read"}]
        });
        assert_eq!(classify_exchange(&wrong_identity), None);
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
