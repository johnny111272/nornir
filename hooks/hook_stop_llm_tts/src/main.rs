//! Stop hook: TTS playback of assistant responses.
//!
//! Receives the Claude Code Stop event, filters the response text
//! to remove code blocks and markdown noise, checks QUIET.lock files,
//! then calls `announce` in the background.
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_stop_llm_tts --project-dir $CLAUDE_PROJECT_DIR

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use clap::Parser;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "hook_stop_llm_tts", about = "Stop hook: TTS playback")]
struct Cli {
    /// Session project directory (from $CLAUDE_PROJECT_DIR)
    #[arg(long)]
    project_dir: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Stop event input (stdin JSON from Claude Code)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct StopEvent {
    #[serde(default)]
    last_assistant_message: String,
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

    let event: StopEvent = match serde_json::from_str(&input) {
        Ok(e) => e,
        Err(_) => return ExitCode::SUCCESS,
    };

    let message = event.last_assistant_message.trim().to_string();
    if message.is_empty() {
        return ExitCode::SUCCESS;
    }

    // Resolve project dir: CLI --project-dir > stdin cwd > None
    let project_dir = args.project_dir
        .map(|p| p.display().to_string())
        .or_else(|| if event.cwd.is_empty() { None } else { Some(event.cwd.clone()) });

    // Resolve workspace from project dir
    let workspace = project_dir.as_deref().and_then(|p| {
        workspace_registry::resolve_workspace_from_path(p).ok().flatten()
    });

    // QUIET.lock: global first, then per-workspace
    let control_voice = write_engine::ai_home().join("control/voice");
    if control_voice.join("QUIET.lock").exists() {
        return ExitCode::SUCCESS;
    }
    if let Some(ref ws) = workspace {
        if workspace_registry::workspace_control_dir(ws).join("QUIET.lock").exists() {
            return ExitCode::SUCCESS;
        }
    }

    // Filter markdown noise, then make paths speakable
    let filtered = text_core::strip_markdown(&message);
    let filtered = text_core::tts_clean(&filtered);
    if filtered.trim().is_empty() {
        return ExitCode::SUCCESS;
    }

    // Spawn announce in background — do not wait
    let mut cmd = Command::new(announce_bin());
    cmd.args(["--profile", "cc"]);
    if let Some(ref dir) = project_dir {
        cmd.args(["--source", dir]);
    }
    cmd.arg("--stdin");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return ExitCode::SUCCESS,
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(filtered.as_bytes());
        // stdin drops here, closing the pipe
    }

    // Don't wait — announce manages its own background playback
    ExitCode::SUCCESS
}

fn announce_bin() -> PathBuf {
    write_engine::ai_home().join("tools/bin/announce")
}

