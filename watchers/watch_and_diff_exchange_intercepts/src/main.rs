use diff_core::{build_datagram, classify_priority, diff_messages, diff_system_blocks, diff_tools, split_exchange, Exchange};
use socket_emit::emit;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// =============================================================================
// Types
// =============================================================================

#[derive(Debug, PartialEq)]
enum Mode {
    Replay,
}

#[derive(Debug)]
struct Config {
    mode: Mode,
    jsonl_path: String,
    workspace: String,
    pace: Option<(u64, u64)>, // min_ms, max_ms
}

// =============================================================================
// Arg parsing
// =============================================================================

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut mode = None;
    let mut jsonl_path = None;
    let mut workspace = None;
    let mut pace = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--replay" => {
                mode = Some(Mode::Replay);
            }
            "--jsonl-path" => {
                i += 1;
                jsonl_path = Some(
                    args.get(i)
                        .ok_or("--jsonl-path requires a value")?
                        .clone(),
                );
            }
            "--workspace" => {
                i += 1;
                workspace = Some(
                    args.get(i)
                        .ok_or("--workspace requires a value")?
                        .clone(),
                );
            }
            "--pace" => {
                i += 1;
                let val = args.get(i).ok_or("--pace requires a value (e.g. 800:3000)")?;
                pace = Some(parse_pace(val)?);
            }
            other => {
                return Err(format!("Unknown flag: {other}"));
            }
        }
        i += 1;
    }

    let mode = mode.ok_or("--replay is required (only mode currently supported)")?;
    let jsonl_path = jsonl_path.ok_or("--jsonl-path is required")?;

    let workspace = workspace.unwrap_or_else(|| workspace_from_path(&jsonl_path));

    Ok(Config {
        mode,
        jsonl_path,
        workspace,
        pace,
    })
}

/// Parse "min:max" pace string into (min_ms, max_ms).
fn parse_pace(val: &str) -> Result<(u64, u64), String> {
    let parts: Vec<&str> = val.split(':').collect();
    if parts.len() != 2 {
        return Err(format!("--pace expects min:max (e.g. 800:3000), got: {val}"));
    }
    let min: u64 = parts[0].parse().map_err(|_| format!("Invalid pace min: {}", parts[0]))?;
    let max: u64 = parts[1].parse().map_err(|_| format!("Invalid pace max: {}", parts[1]))?;
    if min > max {
        return Err(format!("--pace min ({min}) must be <= max ({max})"));
    }
    Ok((min, max))
}

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

/// Derive workspace name from JSONL path.
/// Path convention: .../traffic/{workspace}/{session_id}.jsonl
fn workspace_from_path(path: &str) -> String {
    let p = Path::new(path);
    p.parent()
        .and_then(|dir| dir.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

// =============================================================================
// Help
// =============================================================================

fn print_usage() {
    eprintln!(
        "Usage: watch_and_diff_exchange_intercepts --replay --jsonl-path <path> [--workspace <name>] [--pace min:max]"
    );
}

// =============================================================================
// Replay
// =============================================================================

/// Process a JSONL file from start to end, diffing consecutive exchanges.
/// For each pair: parse → split → diff → classify → construct → emit.
fn run_replay(path: &Path, workspace: &str, pace: Option<(u64, u64)>) -> Result<String, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;

    let mut previous: Option<Exchange> = None;
    let mut line_number = 0u64;
    let mut datagrams_emitted = 0u64;

    // Seed PRNG from system time nanoseconds
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

        match &previous {
            None => {
                // First exchange: emit messages as baseline, no system/tool diff
                let new_messages = current.messages.clone();
                if !new_messages.is_empty() {
                    let dg = build_datagram(
                        &new_messages,
                        &[],
                        &[],
                        workspace,
                        socket_emit::Priority::Low,
                    );
                    emit(&dg);
                    datagrams_emitted += 1;
                }
            }
            Some(prev) => {
                let new_messages = diff_messages(&prev.messages, &current.messages);
                let new_system = diff_system_blocks(&prev.system, &current.system);
                let new_tools = diff_tools(&prev.tools, &current.tools);

                // Skip if nothing changed
                if new_messages.is_empty() && new_system.is_empty() && new_tools.is_empty() {
                    previous = Some(current);
                    continue;
                }

                let priority = classify_priority(&new_system, &new_tools);
                let dg = build_datagram(&new_messages, &new_system, &new_tools, workspace, priority);
                emit(&dg);
                datagrams_emitted += 1;
            }
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
// Entry point
// =============================================================================

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let config = match parse_args(&args[1..]) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            print_usage();
            std::process::exit(1);
        }
    };

    let result = match config.mode {
        Mode::Replay => run_replay(Path::new(&config.jsonl_path), &config.workspace, config.pace),
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

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // =========================================================================
    // parse_args — valid configurations
    // =========================================================================

    #[test]
    fn parse_args_replay_with_path() {
        let a = args(&["--replay", "--jsonl-path", "/tmp/session.jsonl"]);
        let config = parse_args(&a).unwrap();
        assert_eq!(config.mode, Mode::Replay);
        assert_eq!(config.jsonl_path, "/tmp/session.jsonl");
    }

    #[test]
    fn parse_args_explicit_workspace() {
        let a = args(&[
            "--replay",
            "--jsonl-path",
            "/tmp/session.jsonl",
            "--workspace",
            "odinn",
        ]);
        let config = parse_args(&a).unwrap();
        assert_eq!(config.workspace, "odinn");
    }

    #[test]
    fn parse_args_workspace_derived_from_path() {
        let a = args(&[
            "--replay",
            "--jsonl-path",
            "/home/user/.ai/intercept/traffic/odinn/abc123.jsonl",
        ]);
        let config = parse_args(&a).unwrap();
        assert_eq!(config.workspace, "odinn");
    }

    // =========================================================================
    // parse_args — missing required flags
    // =========================================================================

    #[test]
    fn parse_args_missing_mode() {
        let a = args(&["--jsonl-path", "/tmp/session.jsonl"]);
        let err = parse_args(&a).unwrap_err();
        assert!(
            err.contains("--replay"),
            "error should mention --replay: {err}"
        );
    }

    #[test]
    fn parse_args_missing_jsonl_path() {
        let a = args(&["--replay"]);
        let err = parse_args(&a).unwrap_err();
        assert!(
            err.contains("--jsonl-path"),
            "error should mention --jsonl-path: {err}"
        );
    }

    // =========================================================================
    // parse_args — error cases
    // =========================================================================

    #[test]
    fn parse_args_unknown_flag() {
        let a = args(&["--replay", "--jsonl-path", "/tmp/x.jsonl", "--banana"]);
        let err = parse_args(&a).unwrap_err();
        assert!(
            err.contains("--banana"),
            "error should mention unknown flag: {err}"
        );
    }

    #[test]
    fn parse_args_jsonl_path_without_value() {
        let a = args(&["--replay", "--jsonl-path"]);
        let err = parse_args(&a).unwrap_err();
        assert!(
            err.contains("--jsonl-path"),
            "error should mention --jsonl-path: {err}"
        );
    }

    // =========================================================================
    // parse_args — pace flag
    // =========================================================================

    #[test]
    fn parse_args_with_pace() {
        let a = args(&["--replay", "--jsonl-path", "/tmp/x.jsonl", "--pace", "800:3000"]);
        let config = parse_args(&a).unwrap();
        assert_eq!(config.pace, Some((800, 3000)));
    }

    #[test]
    fn parse_args_without_pace() {
        let a = args(&["--replay", "--jsonl-path", "/tmp/x.jsonl"]);
        let config = parse_args(&a).unwrap();
        assert_eq!(config.pace, None);
    }

    // =========================================================================
    // parse_pace
    // =========================================================================

    #[test]
    fn parse_pace_valid() {
        assert_eq!(parse_pace("800:3000").unwrap(), (800, 3000));
    }

    #[test]
    fn parse_pace_equal_values() {
        assert_eq!(parse_pace("1000:1000").unwrap(), (1000, 1000));
    }

    #[test]
    fn parse_pace_min_greater_than_max() {
        let err = parse_pace("3000:800").unwrap_err();
        assert!(err.contains("min"), "error should mention min: {err}");
    }

    #[test]
    fn parse_pace_bad_format() {
        let err = parse_pace("800").unwrap_err();
        assert!(err.contains("min:max"), "error should mention format: {err}");
    }

    // =========================================================================
    // workspace_from_path
    // =========================================================================

    #[test]
    fn workspace_from_traffic_path() {
        assert_eq!(
            workspace_from_path("/home/user/.ai/intercept/traffic/odinn/session.jsonl"),
            "odinn"
        );
    }

    #[test]
    fn workspace_from_bare_filename() {
        // A bare filename has no parent directory name to use
        assert_eq!(workspace_from_path("session.jsonl"), "unknown");
    }
}
