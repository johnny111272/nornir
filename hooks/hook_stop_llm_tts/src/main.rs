//! Stop hook: TTS playback of assistant responses.
//!
//! Receives the Claude Code Stop event, filters the response text
//! to remove code blocks and markdown noise, checks QUIET.lock files,
//! then calls `announce` in the background.
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_stop_llm_tts --project-dir $CLAUDE_PROJECT_DIR

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use clap::Parser;
use regex::Regex;
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

    // QUIET.lock: silence all CC sessions
    let voice_dir = voice_dir();
    if Path::new(&voice_dir).join("QUIET.lock").exists() {
        return ExitCode::SUCCESS;
    }

    // Per-workspace QUIET.lock
    if let Some(ref dir) = project_dir {
        if Path::new(dir).join("QUIET.lock").exists() {
            return ExitCode::SUCCESS;
        }
    }

    // Filter markdown noise
    let filtered = filter_text(&message);
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

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

fn voice_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    format!("{home}/.ai/voice")
}

fn announce_bin() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".ai/tools/bin/announce")
}

// ---------------------------------------------------------------------------
// Text filtering
// ---------------------------------------------------------------------------

fn filter_text(text: &str) -> String {
    // Strip fenced code blocks (```...```) — (?s) enables dot-matches-newline
    let Ok(fenced) = Regex::new(r"(?s)```[^\n]*\n.*?```") else { return text.to_string() };
    let text = fenced.replace_all(text, "");

    // Strip markdown tables (lines starting with |)
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim_start().starts_with('|'))
        .collect();
    let text = lines.join("\n");

    // Strip inline code backticks — keep inner text
    let Ok(inline) = Regex::new(r"`([^`\n]+)`") else { return text };
    let text = inline.replace_all(&text, "$1");

    // Strip bold (**text**) — keep inner text
    let Ok(bold) = Regex::new(r"\*\*([^*\n]+)\*\*") else { return text.to_string() };
    let text = bold.replace_all(&text, "$1");

    // Strip italic (*text*) — keep inner text (after bold stripped)
    let Ok(italic) = Regex::new(r"\*([^*\n]+)\*") else { return text.to_string() };
    let text = italic.replace_all(&text, "$1");

    // Strip markdown header prefixes (# ## etc.) — keep heading text
    let Ok(headers) = Regex::new(r"(?m)^#{1,6}\s+(.+)$") else { return text.to_string() };
    let text = headers.replace_all(&text, "$1");

    // Collapse 3+ consecutive blank lines → one blank line
    let Ok(blank_lines) = Regex::new(r"\n{3,}") else { return text.to_string() };
    let text = blank_lines.replace_all(&text, "\n\n");

    text.trim().to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_fenced_code_block() {
        let input = "Here is some code:\n```rust\nfn main() {}\n```\nAnd some text.";
        let result = filter_text(input);
        assert!(!result.contains("fn main"));
        assert!(result.contains("Here is some code"));
        assert!(result.contains("And some text"));
    }

    #[test]
    fn strips_markdown_table() {
        let input = "Before\n| col1 | col2 |\n| ---- | ---- |\n| a    | b    |\nAfter";
        let result = filter_text(input);
        assert!(!result.contains('|'));
        assert!(result.contains("Before"));
        assert!(result.contains("After"));
    }

    #[test]
    fn strips_inline_code_keeps_text() {
        let result = filter_text("Use the `announce` binary.");
        assert!(!result.contains('`'));
        assert!(result.contains("announce"));
    }

    #[test]
    fn strips_bold_keeps_text() {
        let result = filter_text("This is **important** text.");
        assert!(!result.contains("**"));
        assert!(result.contains("important"));
    }

    #[test]
    fn strips_italic_keeps_text() {
        let result = filter_text("This is *emphasized* text.");
        assert!(!result.contains('*'));
        assert!(result.contains("emphasized"));
    }

    #[test]
    fn strips_header_prefix_keeps_text() {
        let result = filter_text("## My Section\nSome content.");
        assert!(!result.contains('#'));
        assert!(result.contains("My Section"));
        assert!(result.contains("Some content"));
    }

    #[test]
    fn collapses_excess_blank_lines() {
        let result = filter_text("First\n\n\n\nSecond");
        assert!(!result.contains("\n\n\n"));
    }

    #[test]
    fn plain_text_passes_through() {
        let input = "Hello, this is a plain sentence.";
        assert_eq!(filter_text(input), input);
    }

    #[test]
    fn empty_after_filtering_returns_empty() {
        let input = "```rust\nfn main() {}\n```";
        assert!(filter_text(input).trim().is_empty());
    }
}
