//! PostCompact hook: persist compact_summary, emit datagram, announce via TTS.
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_compact_session_log --project-dir $CLAUDE_PROJECT_DIR

use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use datagram_core::{Datagram, DatagramKind, Priority};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "hook_compact_session_log", about = "PostCompact hook: log summary + TTS")]
struct Cli {
    /// Session project directory (from $CLAUDE_PROJECT_DIR)
    #[arg(long)]
    project_dir: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// PostCompact event input (stdin JSON from Claude Code)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CompactEvent {
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    compact_summary: String,
    #[serde(default)]
    cwd: String,
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

fn persist_summary(session_id: &str, compact_summary: &str) {
    if session_id.is_empty() || compact_summary.is_empty() {
        return;
    }

    let epoch_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let timestamp = time_core::iso_zulu(epoch_secs);

    let record = serde_json::json!({
        "timestamp": timestamp,
        "compact_summary": compact_summary,
    });

    let sessions_dir = write_engine::ai_home()
        .join("intercept/sessions")
        .join(session_id);

    if std::fs::create_dir_all(&sessions_dir).is_ok() {
        let jsonl_path = sessions_dir.join("compact_summary.jsonl");
        let _ = write_engine::append_line_fsync(&jsonl_path, &record.to_string());
    }
}

fn emit_datagram(workspace: &str) {
    let datagram = Datagram {
        timestamp: datagram_io::now(),
        source: "hook_compact_session_log".into(),
        kind: DatagramKind::Notify,
        classifier: None,
        priority: Priority::Low,
        workspace: workspace.into(),
        detail: Some(format!("{workspace} compacted")),
        speech: None,
        payload: None,
    };
    datagram_io::emit(&datagram);
}

fn announce_compaction(workspace: &str, project_dir: Option<&str>) {
    let control_voice = write_engine::ai_home().join("control/voice");
    if control_voice.join("SILENT.lock").exists() {
        return;
    }
    if workspace_registry::workspace_control_dir(workspace).join("SILENT.lock").exists() {
        return;
    }

    let announce = write_engine::ai_home().join("tools/bin/announce");
    let mut cmd = Command::new(&announce);
    cmd.args(["--severity", "info"]);
    if let Some(dir) = project_dir {
        cmd.args(["--source", dir]);
    }
    cmd.arg(format!("{workspace} compacted"));
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _ = cmd.spawn();
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

    let event: CompactEvent = match serde_json::from_str(&input) {
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

    persist_summary(&event.session_id, &event.compact_summary);
    emit_datagram(&workspace);
    announce_compaction(&workspace, project_dir.as_deref());

    ExitCode::SUCCESS
}
