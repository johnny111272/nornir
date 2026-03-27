//! SessionStart hook: inject files into LLM context on resume, clear, or compact.
//!
//! Reads inject.toml (embedded at compile time) which maps session start events
//! to file lists. Files are resolved relative to the workspace directory (cwd).
//! Content is concatenated in listing order and emitted as additionalContext.
//!
//! This is the recovery mechanism for context lost during compaction — cc_launch
//! writes .SYSTEM_PROMPT.md at launch, and this hook re-injects it when needed.

use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

use serde::Deserialize;

const INJECT_CONFIG: &str = include_str!("../inject.toml");

#[derive(Deserialize)]
struct SessionEvent {
    #[serde(default)]
    source: String,
    #[serde(default)]
    cwd: String,
}

#[derive(Deserialize)]
struct EventConfig {
    #[serde(default)]
    files: Vec<String>,
}

fn announce(workspace: &str, source: &str, project_dir: &str) {
    let control_voice = write_engine::ai_home().join("control/voice");
    if control_voice.join("SILENT.lock").exists() {
        return;
    }
    if workspace_registry::workspace_control_dir(workspace).join("SILENT.lock").exists() {
        return;
    }

    let announce_bin = write_engine::ai_home().join("tools/bin/announce");
    let mut cmd = Command::new(&announce_bin);
    cmd.args(["--severity", "info"]);
    cmd.args(["--source", project_dir]);
    cmd.arg(format!("{workspace}: {source} context injection"));
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _ = cmd.spawn();
}

fn workspace_name(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn run() -> Option<(String, String, String)> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).ok()?;

    let event: SessionEvent = serde_json::from_str(&input).ok()?;

    if event.source.is_empty() || event.cwd.is_empty() {
        return None;
    }

    let config: std::collections::HashMap<String, EventConfig> =
        toml::from_str(INJECT_CONFIG).ok()?;

    let event_config = config.get(&event.source)?;

    if event_config.files.is_empty() {
        return None;
    }

    let workspace = Path::new(&event.cwd);
    let mut parts: Vec<String> = Vec::new();

    for file in &event_config.files {
        let path = workspace.join(file);
        if let Ok(content) = std::fs::read_to_string(&path) {
            if !content.is_empty() {
                parts.push(content);
            }
        }
    }

    if parts.is_empty() {
        return None;
    }

    Some((parts.join("\n\n"), event.source, event.cwd))
}

fn main() -> ExitCode {
    if let Some((context, source, cwd)) = run() {
        let ws = workspace_name(&cwd);
        announce(&ws, &source, &cwd);

        let output = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": context
            }
        });
        println!("{}", output);
    }

    ExitCode::SUCCESS
}
