//! JSON-to-TOML converter. Reads JSON from stdin, strips nulls, writes TOML.
//!
//! Usage:
//!     echo '{"key": "value"}' | json_to_toml output.toml
//!     echo '{"key": "value"}' | json_to_toml              # stdout

use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process;

// =============================================================================
// Helpers
// =============================================================================

fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    let parent = path.parent().unwrap_or(Path::new("."));
    if !parent.exists() {
        return Err(format!(
            "directory does not exist: {}",
            parent.display()
        ));
    }

    let tmp = parent.join(format!(
        ".{}.tmp",
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "output".into())
    ));

    fs::write(&tmp, content)
        .map_err(|e| format!("write failed: {e}"))?;

    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("rename failed: {e}"));
    }

    Ok(())
}

// =============================================================================
// Core logic
// =============================================================================

fn run(args: &[String]) -> Result<(), String> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("reading stdin: {e}"))?;
    let input = input.trim();

    if input.is_empty() {
        return Err("stdin is empty — pipe JSON into this command".to_string());
    }

    let value: serde_json::Value = serde_json::from_str(input)
        .map_err(|e| format!("invalid JSON — {e}"))?;

    let cleaned = format_core::convert::strip_nulls(value);

    let toml_str = format_core::serialize::to_toml(&cleaned)
        .map_err(|e| format!("TOML conversion failed — {e}"))?;

    match args.first() {
        Some(path) => write_atomic(Path::new(path), &toml_str)?,
        None => {
            io::stdout()
                .write_all(toml_str.as_bytes())
                .map_err(|e| format!("write stdout: {e}"))?;
        }
    }

    Ok(())
}

// =============================================================================
// Entry point
// =============================================================================

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match run(&args) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}
