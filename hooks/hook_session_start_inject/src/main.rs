//! SessionStart hook: re-inject system prompt on compact, resume, or clear.
//!
//! On compaction the assembled system prompt drops out of context. This hook
//! reads it back from the session-specific path where hook_session_start_orient
//! placed it and emits it as additionalContext.
//!
//! Path: ~/.ai/control/workspaces/{workspace}/{session_id}/SYSTEM_PROMPT.xml

use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

use serde::Deserialize;

#[derive(Deserialize)]
struct SessionEvent {
    #[serde(default)]
    source: String,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    cwd: String,
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

fn workspace_name(workspace_path: &str) -> String {
    Path::new(workspace_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn run() -> Option<(String, String, String)> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).ok()?;

    let event: SessionEvent = serde_json::from_str(&input).ok()?;

    // Only re-inject on compact, resume, or clear — not startup
    match event.source.as_str() {
        "compact" | "resume" | "clear" => {}
        _ => return None,
    }

    if event.session_id.is_empty() || event.cwd.is_empty() {
        return None;
    }

    let workspace = workspace_name(&event.cwd);
    let prompt_path = workspace_registry::workspace_control_dir(&workspace)
        .join(&event.session_id)
        .join("SYSTEM_PROMPT.xml");

    let content = std::fs::read_to_string(&prompt_path).ok()?;
    if content.is_empty() {
        return None;
    }

    Some((content, event.source, event.cwd))
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
