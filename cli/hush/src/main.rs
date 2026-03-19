//! hush — workspace-scoped announce playback killer.
//!
//! Dual-mode: works as a standalone CLI tool and as a Claude Code hook
//! (UserPromptSubmit). When stdin is piped, reads JSON to extract `cwd`.
//! When run from a terminal, uses `--project-dir` or kills all playback.
//!
//! Usage:
//!     hush                          # kill all announce playback
//!     hush --project-dir /path      # kill only this workspace's playback
//!     echo '{"cwd":"/path"}' | hush # hook mode (reads JSON from stdin)

use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use clap::Parser;
use serde::Deserialize;

#[derive(Parser)]
#[command(name = "hush", about = "Kill announce playback processes")]
struct Cli {
    #[arg(long)]
    project_dir: Option<PathBuf>,
}

#[derive(Deserialize)]
struct HookInput {
    #[serde(default)]
    cwd: String,
}

fn main() -> ExitCode {
    let args = Cli::parse();

    // Determine project_dir: stdin JSON > CLI arg > None
    let project_dir = if !io::stdin().is_terminal() {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf).ok()
            .and_then(|_| serde_json::from_str::<HookInput>(&buf).ok())
            .and_then(|input| if input.cwd.is_empty() { None } else { Some(input.cwd) })
            .or_else(|| args.project_dir.as_ref().map(|p| p.display().to_string()))
    } else {
        args.project_dir.as_ref().map(|p| p.display().to_string())
    };

    // Build pkill pattern
    let pattern = match &project_dir {
        Some(dir) => format!("announce.*--play.*--project-dir {dir}"),
        None => "announce.*--play".to_string(),
    };

    let _ = Command::new("pkill")
        .arg("-f")
        .arg(&pattern)
        .output();

    ExitCode::SUCCESS
}
