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

use std::io::{self, Read, Write};
use std::path::Path;
use std::process;

const COMPACTION_INSTRUCTIONS: &str = include_str!("../instructions/compaction_summary.md");

// =============================================================================
// Types
// =============================================================================

struct Args {
    debug: bool,
    output_dir: Option<String>,
}

// =============================================================================
// Arg parsing
// =============================================================================

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut debug = false;
    let mut output_dir = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--debug" => debug = true,
            "--output-dir" => {
                i += 1;
                if i >= args.len() {
                    return Err("--output-dir requires a path".to_string());
                }
                output_dir = Some(args[i].clone());
            }
            other => {
                return Err(format!("unknown argument: {other}"));
            }
        }
        i += 1;
    }

    Ok(Args { debug, output_dir })
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

fn inject_system_block(value: &mut serde_json::Value) -> Result<(), String> {
    let system = value
        .get_mut("system")
        .and_then(|s| s.as_array_mut())
        .ok_or_else(|| "no 'system' array in request JSON".to_string())?;

    let block = serde_json::json!({
        "type": "text",
        "text": COMPACTION_INSTRUCTIONS,
    });

    system.push(block);
    Ok(())
}

fn write_debug_snapshot(dir: &str, prefix: &str, content: &str) {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let path = Path::new(dir).join(format!("{prefix}_{timestamp}.json"));

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

    inject_system_block(&mut value)?;

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
    let args: Vec<String> = std::env::args().skip(1).collect();

    let args = match parse_args(&args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    };

    match run(&args) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}
