//! SessionStart hook: register workspace and session in the workspace registry.
//!
//! Runs on every session start (startup, resume, clear, compact).
//! Registers the workspace name + path and session_id → workspace mapping.
//! Creates the control directory for the workspace if it doesn't exist.
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_start_session_orient --project-dir $CLAUDE_PROJECT_DIR

use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "hook_start_session_orient", about = "SessionStart hook: register workspace + session")]
struct Cli {
    /// Session project directory (from $CLAUDE_PROJECT_DIR)
    #[arg(long)]
    project_dir: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// SessionStart event input (stdin JSON from Claude Code)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SessionEvent {
    #[serde(default)]
    session_id: String,
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

    let event: SessionEvent = match serde_json::from_str(&input) {
        Ok(e) => e,
        Err(_) => return ExitCode::SUCCESS,
    };

    // Resolve project dir: CLI --project-dir > stdin cwd
    let project_dir = args.project_dir
        .map(|p| p.display().to_string())
        .or_else(|| if event.cwd.is_empty() { None } else { Some(event.cwd.clone()) });

    let project_dir = match project_dir {
        Some(d) => d,
        None => return ExitCode::SUCCESS,
    };

    // Extract workspace name from last path component
    let workspace = std::path::Path::new(&project_dir)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Register workspace (upsert — safe to call on every session start)
    let _ = workspace_registry::register_workspace(&workspace, &project_dir);

    // Register session → workspace mapping
    if !event.session_id.is_empty() {
        let _ = workspace_registry::register_session(&event.session_id, &workspace);
    }

    // Ensure control directory exists
    let control_dir = workspace_registry::workspace_control_dir(&workspace);
    let _ = std::fs::create_dir_all(&control_dir);

    ExitCode::SUCCESS
}
