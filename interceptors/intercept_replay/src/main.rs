//! Replay raw_session_log.jsonl to reconstruct derived session files.
//!
//! Archives existing derived files, spawns a watcher subprocess, then processes
//! each raw log line: classifying and routing through the same session_io functions
//! used by the live interceptor. Silently skips unparseable or unclassifiable lines
//! so that mixed-format logs (spanning a CC update) replay cleanly.
//!
//! Usage:
//!     intercept_replay --session-dir <path> [--workspace <name>]

use clap::Parser;
use intercept_core::{classify_exchange, ExchangeKind};
use schema_core::EmbeddedValidator;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

static WIRE_SCHEMA: EmbeddedValidator =
    EmbeddedValidator::new(include_str!("../cc_wire_schema.json"), "cc-wire-format");

const DERIVED_FILES: &[&str] = &[
    "main_exchange_log.jsonl",
    "subagent_log.jsonl",
    "compaction_instructions.jsonl",
    "pre_compact_state.jsonl",
    "running_transcript.jsonl",
];

// =============================================================================
// CLI
// =============================================================================

#[derive(Parser)]
#[command(about = "Replay raw_session_log.jsonl to reconstruct derived session files")]
struct Args {
    /// Path to the session directory (sessions/{session_id}/)
    #[arg(long)]
    session_dir: PathBuf,

    /// Workspace name for datagram metadata
    #[arg(long, default_value = "replay")]
    workspace: String,
}

// =============================================================================
// Steps
// =============================================================================

fn archive_derived_files(session_dir: &Path) -> Result<(), String> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("system time: {e}"))?
        .as_secs();
    let archive_dir = session_dir.join("archive").join(secs.to_string());
    for name in DERIVED_FILES {
        let src = session_dir.join(name);
        if !src.exists() {
            continue;
        }
        fs::create_dir_all(&archive_dir)
            .map_err(|e| format!("mkdir {}: {e}", archive_dir.display()))?;
        fs::rename(&src, archive_dir.join(name))
            .map_err(|e| format!("archive {name}: {e}"))?;
    }
    Ok(())
}

fn spawn_watcher(session_dir: &Path, workspace: &str) -> Result<Child, String> {
    let jsonl_path = session_dir.join("main_exchange_log.jsonl");
    let jsonl_str = jsonl_path
        .to_str()
        .ok_or("session dir path is not valid UTF-8")?;
    Command::new("watch_and_diff_exchange_intercepts")
        .args(["--watch", "--jsonl-path", jsonl_str, "--workspace", workspace])
        .spawn()
        .map_err(|e| format!("spawn watcher: {e}"))
}

/// Process a single raw log line. Returns Ok(true) if an exchange was written,
/// Ok(false) if the line was skipped (empty, parse failure, schema failure,
/// or unclassifiable), Err if a classified exchange failed to write.
fn process_line(session_dir: &Path, workspace: &str, line: &str) -> Result<bool, String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }

    let validation = match WIRE_SCHEMA.validate(trimmed) {
        Ok(v) => v,
        Err(_) => return Ok(false),
    };
    if !validation.valid {
        return Ok(false);
    }

    let value = match validation.data {
        Some(v) => v,
        None => return Ok(false),
    };

    let kind = match classify_exchange(&value) {
        None => return Ok(false),
        Some(k) => k,
    };

    match kind {
        ExchangeKind::Main => session_io::append_exchange(session_dir, &value)?,
        ExchangeKind::Subagent => session_io::append_subagent(session_dir, &value)?,
        ExchangeKind::Compaction => session_io::record_compaction(session_dir, &value, workspace)?,
    }
    Ok(true)
}

fn process_raw_log(
    session_dir: &Path,
    workspace: &str,
    raw_path: &Path,
) -> Result<(u64, u64), String> {
    let file = fs::File::open(raw_path)
        .map_err(|e| format!("open {}: {e}", raw_path.display()))?;
    let mut processed: u64 = 0;
    let mut skipped: u64 = 0;
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|e| format!("read error: {e}"))?;
        match process_line(session_dir, workspace, &line) {
            Ok(true) => {
                processed += 1;
                thread::sleep(Duration::from_millis(10));
            }
            Ok(false) => skipped += 1,
            Err(e) => {
                eprintln!("warning: {e}");
                skipped += 1;
            }
        }
    }
    Ok((processed, skipped))
}

// =============================================================================
// Entry point
// =============================================================================

fn main() {
    let args = Args::parse();

    if !args.session_dir.exists() {
        eprintln!("error: session dir not found: {}", args.session_dir.display());
        std::process::exit(1);
    }

    if let Err(e) = archive_derived_files(&args.session_dir) {
        eprintln!("error: archive failed: {e}");
        std::process::exit(1);
    }

    let mut watcher = match spawn_watcher(&args.session_dir, &args.workspace) {
        Ok(child) => child,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    let raw_path = args.session_dir.join("raw_session_log.jsonl");
    let result = process_raw_log(&args.session_dir, &args.workspace, &raw_path);

    thread::sleep(Duration::from_secs(1));
    let _ = watcher.kill();

    match result {
        Ok((processed, skipped)) => {
            println!("Replay complete: {processed} exchanges processed, {skipped} lines skipped");
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
