//! Notification hook: context-aware TTS for Claude Code notifications.
//!
//! Speaks workspace-specific messages based on notification type:
//!   permission_prompt → "{workspace} needs approval"
//!   idle_prompt       → "{workspace} idle"
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_notify_llm_tts --project-dir $CLAUDE_PROJECT_DIR

use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use clap::Parser;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "hook_notify_llm_tts", about = "Notification hook: TTS alerts")]
struct Cli {
    /// Session project directory (from $CLAUDE_PROJECT_DIR)
    #[arg(long)]
    project_dir: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Notification event input (stdin JSON from Claude Code)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct NotificationEvent {
    #[serde(default)]
    notification_type: String,
    #[serde(default)]
    cwd: String,
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    let args = Cli::parse();

    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        return ExitCode::SUCCESS;
    }

    let event: NotificationEvent = match serde_json::from_str(&input) {
        Ok(e) => e,
        Err(_) => return ExitCode::SUCCESS,
    };

    // Resolve project dir: CLI --project-dir > stdin cwd > None
    let project_dir = args.project_dir
        .map(|p| p.display().to_string())
        .or_else(|| if event.cwd.is_empty() { None } else { Some(event.cwd.clone()) });

    // Extract workspace name from project dir path
    let workspace = project_dir.as_deref()
        .and_then(|p| std::path::Path::new(p).file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "workspace".to_string());

    // Map notification_type to severity + message
    let (severity, message) = match event.notification_type.as_str() {
        "permission_prompt" => ("notify", format!("{workspace} needs approval")),
        "idle_prompt" => ("info", format!("{workspace} idle")),
        _ => return ExitCode::SUCCESS,
    };

    // SILENT.lock: global first, then per-workspace
    let control_voice = write_engine::ai_home().join("control/voice");
    if control_voice.join("SILENT.lock").exists() {
        return ExitCode::SUCCESS;
    }
    if let Some(ref ws) = project_dir.as_deref()
        .and_then(|p| workspace_registry::resolve_workspace_from_path(p).ok().flatten())
    {
        if workspace_registry::workspace_control_dir(ws).join("SILENT.lock").exists() {
            return ExitCode::SUCCESS;
        }
    }

    // Spawn announce in background — do not wait
    let announce = write_engine::ai_home().join("tools/bin/announce");
    let mut cmd = Command::new(&announce);
    cmd.args(["--severity", severity]);
    if let Some(ref dir) = project_dir {
        cmd.args(["--source", dir]);
    }
    cmd.arg(&message);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _ = cmd.spawn();

    ExitCode::SUCCESS
}
