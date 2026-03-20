use clap::Parser;
use diff_core::{accumulate_line, build_datagram, classify_priority, diff_messages, diff_system_blocks, diff_tools, parse_pace, split_exchange, workspace_from_parent_dir, DatagramContext, Exchange};
use datagram_io::emit_validated_or_alert;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// =============================================================================
// Types
// =============================================================================

#[derive(Debug, PartialEq)]
enum Mode {
    Replay,
    Watch,
}

/// Watch or replay bifrost exchange intercepts, diffing consecutive exchanges.
#[derive(Debug, Parser)]
#[command(name = "watch_and_diff_exchange_intercepts")]
struct Args {
    /// Replay mode — process entire file from start
    #[arg(long, group = "mode")]
    replay: bool,

    /// Watch mode — tail file for new exchanges
    #[arg(long, group = "mode")]
    watch: bool,

    /// Path to mainexch JSONL file
    #[arg(long)]
    jsonl_path: String,

    /// Workspace name (auto-derived from path if omitted)
    #[arg(long)]
    workspace: Option<String>,

    /// Pacing as min:max milliseconds (e.g. 800:3000)
    #[arg(long, value_parser = parse_pace_arg)]
    pace: Option<(u64, u64)>,
}

fn resolve_mode(args: &Args) -> Result<Mode, String> {
    match (args.replay, args.watch) {
        (true, false) => Ok(Mode::Replay),
        (false, true) => Ok(Mode::Watch),
        _ => Err("--replay or --watch is required".into()),
    }
}

fn parse_pace_arg(input: &str) -> Result<(u64, u64), String> {
    parse_pace(input)
}

const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Simple PRNG — just needs natural variation, not cryptographic quality.
/// Uses xorshift64 seeded from system time.
fn jitter_sleep(min_ms: u64, max_ms: u64, seed: &mut u64) {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    let range = max_ms - min_ms + 1;
    let delay = min_ms + (*seed % range);
    std::thread::sleep(Duration::from_millis(delay));
}

// =============================================================================
// Diff engine — shared between replay and watch
// =============================================================================

/// Diff a new exchange against the previous one. Emit datagram if changed.
/// Returns the datagram payload if one was emitted, None otherwise.
fn diff_and_emit(
    previous: &Option<Exchange>,
    current: &Exchange,
    workspace: &str,
    source_ref: &str,
) -> Option<serde_json::Value> {
    match previous {
        None => {
            if current.messages.is_empty() {
                return None;
            }
            let dg = build_datagram(
                &current.messages,
                &[],
                &[],
                &DatagramContext {
                    workspace,
                    priority: datagram_io::Priority::Low,
                    source_ref,
                    is_startup: true,
                    timestamp: datagram_io::now(),
                },
            );
            if !emit_validated_or_alert(&dg, "bifrost_watcher") {
                return None;
            }
            dg.payload
        }
        Some(prev) => {
            let new_messages = diff_messages(&prev.messages, &current.messages);
            let new_system = diff_system_blocks(&prev.system, &current.system);
            let new_tools = diff_tools(&prev.tools, &current.tools);

            if new_messages.is_empty() && new_system.is_empty() && new_tools.is_empty() {
                return None;
            }

            let priority = classify_priority(&new_system, &new_tools);
            let dg = build_datagram(
                &new_messages,
                &new_system,
                &new_tools,
                &DatagramContext {
                    workspace,
                    priority,
                    source_ref,
                    is_startup: false,
                    timestamp: datagram_io::now(),
                },
            );
            if !emit_validated_or_alert(&dg, "bifrost_watcher") {
                return None;
            }
            dg.payload
        }
    }
}

// =============================================================================
// Replay
// =============================================================================

/// Process a JSONL file from start to end, diffing consecutive exchanges.
fn run_replay(path: &Path, workspace: &str, pace: Option<(u64, u64)>) -> Result<String, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;

    let compact = datagram_io::compact_path(&path.to_string_lossy());

    let transcript_path = transcript_path_for(path);
    let mut transcript = open_transcript(&transcript_path)?;

    let mut previous: Option<Exchange> = None;
    let mut line_number = 0u64;
    let mut datagrams_emitted = 0u64;

    let mut rng_seed: u64 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(12345);

    for line in content.lines() {
        line_number += 1;

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let value: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|e| format!("Line {line_number}: invalid JSON: {e}"))?;

        let current = split_exchange(&value);
        let source_ref = format!("{compact}:{line_number}");

        if let Some(payload) = diff_and_emit(&previous, &current, workspace, &source_ref) {
            append_transcript(&mut transcript, &payload);
            datagrams_emitted += 1;
        }

        previous = Some(current);

        if let Some((min_ms, max_ms)) = pace {
            jitter_sleep(min_ms, max_ms, &mut rng_seed);
        }
    }

    Ok(format!(
        "Replay complete: {line_number} exchanges processed, {datagrams_emitted} datagrams emitted"
    ))
}

// =============================================================================
// Watch
// =============================================================================

struct WatchState {
    filename: String,
    previous: Option<Exchange>,
    datagrams_emitted: u64,
    exchanges_processed: u64,
    line_number: u64,
    partial_line: String,
}

fn process_exchange(
    state: &mut WatchState,
    value: &serde_json::Value,
    workspace: &str,
    transcript: &mut File,
) {
    state.line_number += 1;
    state.exchanges_processed += 1;

    let current = split_exchange(value);
    let source_ref = format!("{}:{}", state.filename, state.line_number);

    if let Some(payload) = diff_and_emit(&state.previous, &current, workspace, &source_ref) {
        append_transcript(transcript, &payload);
        state.datagrams_emitted += 1;
    }

    state.previous = Some(current);

    if state.exchanges_processed % 50 == 0 {
        eprintln!(
            "Watch: {} exchanges, {} datagrams",
            state.exchanges_processed, state.datagrams_emitted
        );
    }
}

/// Detect if the watched file was truncated (e.g., by compaction).
/// If the file size is smaller than the reader's stream position, seek to 0 and reset state.
fn detect_truncation(path: &Path, reader: &mut BufReader<File>, state: &mut WatchState) {
    let current_pos = match reader.seek(SeekFrom::Current(0)) {
        Ok(pos) => pos,
        Err(_) => return,
    };
    let file_len = match path.metadata() {
        Ok(m) => m.len(),
        Err(_) => return,
    };
    if file_len < current_pos {
        eprintln!("Detected truncation ({file_len} < {current_pos}), resetting");
        let _ = reader.seek(SeekFrom::Start(0));
        state.line_number = 0;
        state.previous = None;
        state.partial_line.clear();
    }
}

/// Tail a JSONL file, diffing new exchanges as they appear.
/// Seeks to end of file (or start if file doesn't exist yet), polls for new lines.
fn run_watch(path: &Path, workspace: &str) -> Result<String, String> {
    let transcript_path = transcript_path_for(path);
    let mut transcript = open_transcript(&transcript_path)?;

    while !path.exists() {
        eprintln!("Waiting for {} ...", path.display());
        std::thread::sleep(Duration::from_secs(2));
    }

    let mut file = File::open(path)
        .map_err(|e| format!("Failed to open {}: {e}", path.display()))?;

    // Read existing lines, keeping the last valid exchange as `previous`
    // so resumed sessions produce a normal diff instead of dumping
    // the entire conversation history as a "startup" datagram.
    let mut line_number = 0u64;
    let mut last_exchange: Option<Exchange> = None;
    {
        let reader = BufReader::new(&mut file);
        for line in reader.lines() {
            line_number += 1;
            if let Ok(ref text) = line {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
                        last_exchange = Some(split_exchange(&value));
                    }
                }
            }
        }
    }

    file.seek(SeekFrom::End(0))
        .map_err(|e| format!("Failed to seek {}: {e}", path.display()))?;

    let mut state = WatchState {
        filename: datagram_io::compact_path(&path.to_string_lossy()),
        previous: last_exchange,
        datagrams_emitted: 0,
        exchanges_processed: 0,
        line_number,
        partial_line: String::new(),
    };

    let mut reader = BufReader::new(file);

    eprintln!(
        "Watching {} (workspace: {workspace}, starting at line {}, previous: {})",
        path.display(), state.line_number,
        if state.previous.is_some() { "loaded" } else { "none" }
    );

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => {
                detect_truncation(path, &mut reader, &mut state);
                std::thread::sleep(WATCH_POLL_INTERVAL);
            }
            Ok(_) => {
                match accumulate_line(&mut state.partial_line, &line) {
                    Ok(Some(value)) => {
                        process_exchange(&mut state, &value, workspace, &mut transcript);
                    }
                    Ok(None) => {}
                    Err(e) => eprintln!("Skipping malformed line: {e}"),
                }
            }
            Err(e) => {
                eprintln!("Read error: {e}");
                std::thread::sleep(WATCH_POLL_INTERVAL);
            }
        }
    }
}

// =============================================================================
// Transcript — per-session structured log
// =============================================================================

/// Derive transcript path: sibling running_transcript.jsonl in the same session directory.
fn transcript_path_for(mainexch_path: &Path) -> PathBuf {
    mainexch_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("running_transcript.jsonl")
}

/// Open (or create) a transcript file for appending.
fn open_transcript(path: &PathBuf) -> Result<File, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create transcript dir: {e}"))?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("Failed to open transcript {}: {e}", path.display()))
}

/// Append a restructured payload to the transcript as a single JSON line.
fn append_transcript(file: &mut File, payload: &serde_json::Value) {
    if let Ok(mut json) = serde_json::to_vec(payload) {
        json.push(b'\n');
        let _ = file.write_all(&json);
    }
}

// =============================================================================
// Entry point
// =============================================================================

fn main() {
    let args = Args::parse();

    let mode = match resolve_mode(&args) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    let workspace = args.workspace.unwrap_or_else(|| workspace_from_parent_dir(&args.jsonl_path));

    let result = match mode {
        Mode::Replay => run_replay(Path::new(&args.jsonl_path), &workspace, args.pace),
        Mode::Watch => run_watch(Path::new(&args.jsonl_path), &workspace),
    };

    match result {
        Ok(msg) => {
            eprintln!("{msg}");
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // arg parsing (clap) — valid configurations
    // =========================================================================

    #[test]
    fn parse_args_replay_with_path() {
        let args = Args::try_parse_from(["watch_and_diff_exchange_intercepts", "--replay", "--jsonl-path", "/tmp/session.jsonl"]).unwrap();
        assert_eq!(resolve_mode(&args).unwrap(), Mode::Replay);
        assert_eq!(args.jsonl_path, "/tmp/session.jsonl");
    }

    #[test]
    fn parse_args_explicit_workspace() {
        let args = Args::try_parse_from([
            "watch_and_diff_exchange_intercepts",
            "--replay", "--jsonl-path", "/tmp/session.jsonl",
            "--workspace", "odinn",
        ]).unwrap();
        assert_eq!(args.workspace.as_deref(), Some("odinn"));
    }

    #[test]
    fn parse_args_workspace_derived_from_path() {
        let args = Args::try_parse_from([
            "watch_and_diff_exchange_intercepts",
            "--replay", "--jsonl-path",
            "/home/user/.ai/intercept/traffic/odinn/abc123.jsonl",
        ]).unwrap();
        let workspace = args.workspace.unwrap_or_else(|| workspace_from_parent_dir(&args.jsonl_path));
        assert_eq!(workspace, "odinn");
    }

    // =========================================================================
    // arg parsing — missing required flags
    // =========================================================================

    #[test]
    fn parse_args_missing_mode() {
        // clap won't error on missing mode (they're optional bools), resolve_mode will
        let args = Args::try_parse_from(["watch_and_diff_exchange_intercepts", "--jsonl-path", "/tmp/session.jsonl"]).unwrap();
        let err = resolve_mode(&args).unwrap_err();
        assert!(
            err.contains("--replay") || err.contains("--watch"),
            "error should mention mode flags: {err}"
        );
    }

    #[test]
    fn parse_args_watch_mode() {
        let args = Args::try_parse_from(["watch_and_diff_exchange_intercepts", "--watch", "--jsonl-path", "/tmp/session.jsonl"]).unwrap();
        assert_eq!(resolve_mode(&args).unwrap(), Mode::Watch);
    }

    #[test]
    fn parse_args_missing_jsonl_path() {
        let result = Args::try_parse_from(["watch_and_diff_exchange_intercepts", "--replay"]);
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("--jsonl-path"),
            "error should mention --jsonl-path: {err}"
        );
    }

    // =========================================================================
    // arg parsing — error cases
    // =========================================================================

    #[test]
    fn parse_args_unknown_flag() {
        let result = Args::try_parse_from(["watch_and_diff_exchange_intercepts", "--replay", "--jsonl-path", "/tmp/x.jsonl", "--banana"]);
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("--banana"),
            "error should mention unknown flag: {err}"
        );
    }

    #[test]
    fn parse_args_jsonl_path_without_value() {
        let result = Args::try_parse_from(["watch_and_diff_exchange_intercepts", "--replay", "--jsonl-path"]);
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("jsonl-path"),
            "error should mention --jsonl-path: {err}"
        );
    }

    // =========================================================================
    // arg parsing — pace flag
    // =========================================================================

    #[test]
    fn parse_args_with_pace() {
        let args = Args::try_parse_from(["watch_and_diff_exchange_intercepts", "--replay", "--jsonl-path", "/tmp/x.jsonl", "--pace", "800:3000"]).unwrap();
        assert_eq!(args.pace, Some((800, 3000)));
    }

    #[test]
    fn parse_args_without_pace() {
        let args = Args::try_parse_from(["watch_and_diff_exchange_intercepts", "--replay", "--jsonl-path", "/tmp/x.jsonl"]).unwrap();
        assert_eq!(args.pace, None);
    }

}
