//! Rewrite compaction requests by injecting summary instructions.
//!
//! Reads JSON from stdin, appends a system block with embedded compaction
//! instructions, writes modified JSON to stdout.
//!
//! Usage:
//!     echo $JSON | rewrite_compaction_summary
//!     echo $JSON | rewrite_compaction_summary --debug --output-dir /path/to/dir/
//!
//! Exit codes: 0=success, 1=stdin/parse error, 2=arg parse error

use clap::Parser;
use compaction_inject_core::{inject_compaction_system_block, COMPACTION_INSTRUCTIONS};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process;

// =============================================================================
// Types
// =============================================================================

/// Inject compaction summary instructions into a JSON request on stdin.
#[derive(Debug, Parser)]
#[command(name = "rewrite_compaction_summary")]
struct Args {
    /// Write pre/post debug snapshots to disk
    #[arg(long)]
    debug: bool,

    /// Directory for debug snapshot files
    #[arg(long)]
    output_dir: Option<String>,
}

// =============================================================================
// Helpers (all return Result)
// =============================================================================

fn read_stdin() -> Result<String, String> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("reading stdin: {e}"))?;
    if input.trim().is_empty() {
        return Err("stdin is empty".to_string());
    }
    Ok(input)
}

fn parse_json(input: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(input).map_err(|e| format!("invalid JSON: {e}"))
}

fn serialize_json(value: &serde_json::Value) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| format!("JSON serialization: {e}"))
}

fn write_debug_snapshot(directory: &str, prefix: &str, content: &str) {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let path = Path::new(directory).join(format!("{prefix}_{timestamp}.json"));

    if let Err(e) = std::fs::write(&path, content) {
        eprintln!("warning: debug write failed for {}: {e}", path.display());
    }
}

// =============================================================================
// Core logic
// =============================================================================

fn run(args: &Args) -> Result<(), String> {
    let input = read_stdin()?;
    let mut value = parse_json(&input)?;

    if let Some(ref dir) = args.output_dir {
        if args.debug {
            write_debug_snapshot(dir, "pre", &input);
        }
    }

    inject_compaction_system_block(&mut value)?;

    let output = serialize_json(&value)?;

    if let Some(ref dir) = args.output_dir {
        if args.debug {
            write_debug_snapshot(dir, "post", &output);
        }
    }

    io::stdout()
        .write_all(output.as_bytes())
        .map_err(|e| format!("write stdout: {e}"))?;

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
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // arg parsing (clap)
    // =========================================================================

    #[test]
    fn parse_args_no_flags() {
        let parsed = Args::try_parse_from(["rewrite_compaction_summary"]).unwrap();
        assert!(!parsed.debug);
        assert!(parsed.output_dir.is_none());
    }

    #[test]
    fn parse_args_debug_flag() {
        let parsed = Args::try_parse_from(["rewrite_compaction_summary", "--debug"]).unwrap();
        assert!(parsed.debug);
    }

    #[test]
    fn parse_args_output_dir() {
        let parsed = Args::try_parse_from(["rewrite_compaction_summary", "--output-dir", "/tmp/out"]).unwrap();
        assert_eq!(parsed.output_dir.as_deref(), Some("/tmp/out"));
    }

    #[test]
    fn parse_args_output_dir_missing_value() {
        let result = Args::try_parse_from(["rewrite_compaction_summary", "--output-dir"]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("output-dir"), "error should mention flag: {err}");
    }

    #[test]
    fn parse_args_unknown_flag() {
        let result = Args::try_parse_from(["rewrite_compaction_summary", "--banana"]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--banana"), "error should mention unknown flag: {err}");
    }

    // =========================================================================
    // inject_system_block — valid JSON with system array
    // =========================================================================

    #[test]
    fn inject_system_block_appends_to_system_array() {
        let mut value = serde_json::json!({
            "model": "test-model",
            "system": [
                { "type": "text", "text": "existing instruction" }
            ],
            "messages": []
        });

        inject_compaction_system_block(&mut value).unwrap();

        let system = value["system"].as_array().unwrap();
        assert_eq!(system.len(), 2, "system array should have 2 entries after injection");

        let injected = &system[1];
        assert_eq!(injected["type"], "text");
        assert_eq!(injected["text"], COMPACTION_INSTRUCTIONS);
    }

    #[test]
    fn inject_system_block_preserves_other_fields() {
        let mut value = serde_json::json!({
            "model": "claude-3",
            "max_tokens": 4096,
            "system": [
                { "type": "text", "text": "original" }
            ],
            "messages": [
                { "role": "user", "content": "hello" }
            ]
        });

        inject_compaction_system_block(&mut value).unwrap();

        // Other fields must be untouched
        assert_eq!(value["model"], "claude-3");
        assert_eq!(value["max_tokens"], 4096);
        let messages = value["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["content"], "hello");
    }

    #[test]
    fn inject_system_block_empty_system_array() {
        let mut value = serde_json::json!({
            "system": [],
            "messages": []
        });

        inject_compaction_system_block(&mut value).unwrap();

        let system = value["system"].as_array().unwrap();
        assert_eq!(system.len(), 1);
        assert_eq!(system[0]["type"], "text");
        assert_eq!(system[0]["text"], COMPACTION_INSTRUCTIONS);
    }

    #[test]
    fn inject_system_block_no_system_key() {
        let mut value = serde_json::json!({
            "messages": []
        });

        let err = inject_compaction_system_block(&mut value).unwrap_err();
        assert!(err.contains("system"), "error should mention missing system: {err}");
    }

    #[test]
    fn inject_system_block_system_not_array() {
        let mut value = serde_json::json!({
            "system": "just a string"
        });

        let err = inject_compaction_system_block(&mut value).unwrap_err();
        assert!(err.contains("system"), "error should mention system: {err}");
    }

    // =========================================================================
    // inject content matches embedded instructions
    // =========================================================================

    #[test]
    fn injected_content_matches_embedded_instructions() {
        let mut value = serde_json::json!({ "system": [] });
        inject_compaction_system_block(&mut value).unwrap();

        let injected_text = value["system"][0]["text"].as_str().unwrap();
        assert_eq!(injected_text, COMPACTION_INSTRUCTIONS);
        // Sanity: instructions are non-empty
        assert!(!COMPACTION_INSTRUCTIONS.is_empty());
    }

    // =========================================================================
    // parse_json / serialize_json roundtrip
    // =========================================================================

    #[test]
    fn parse_json_valid() {
        let value = parse_json(r#"{"key": "value"}"#).unwrap();
        assert_eq!(value["key"], "value");
    }

    #[test]
    fn parse_json_invalid() {
        let err = parse_json("not json at all").unwrap_err();
        assert!(err.contains("invalid JSON"), "error should mention invalid JSON: {err}");
    }

    #[test]
    fn serialize_json_roundtrip() {
        let original = serde_json::json!({"a": 1, "b": [2, 3]});
        let serialized = serialize_json(&original).unwrap();
        let recovered = parse_json(&serialized).unwrap();
        assert_eq!(original, recovered);
    }
}
