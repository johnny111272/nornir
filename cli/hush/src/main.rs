//! hush — workspace-scoped announce playback killer.
//!
//! Dual-mode: works as a standalone CLI tool and as a Claude Code hook
//! (UserPromptSubmit). CLI `--source` takes precedence; if not provided
//! and stdin is piped, reads JSON to extract `cwd` as fallback.
//!
//! Usage:
//!     hush                          # kill all announce playback
//!     hush --source /path           # kill only this workspace's playback
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
    source: Option<PathBuf>,
}

#[derive(Deserialize)]
struct HookInput {
    #[serde(default)]
    cwd: String,
}

fn main() -> ExitCode {
    let args = Cli::parse();

    // Priority: CLI --source > stdin JSON cwd > None (kill all)
    let source = if let Some(ref dir) = args.source {
        Some(dir.display().to_string())
    } else if !io::stdin().is_terminal() {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf).ok()
            .and_then(|_| serde_json::from_str::<HookInput>(&buf).ok())
            .and_then(|input| if input.cwd.is_empty() { None } else { Some(input.cwd) })
    } else {
        None
    };

    // Build pkill pattern
    let pattern = match &source {
        Some(dir) => format!("announce.*--play.*--source {dir}"),
        None => "announce.*--play".to_string(),
    };

    let _ = Command::new("pkill")
        .arg("-f")
        .arg(&pattern)
        .output();

    ExitCode::SUCCESS
}
