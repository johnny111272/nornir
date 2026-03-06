//! PostToolUse hook: quality assessment injection for file writes.
//!
//! When a tool writes a Python file, runs the quality pipeline:
//!   saga <file> --sidecar | syn --stdin
//!
//! If syn reports violations, injects the TOON assessment as a systemMessage.
//! If syn reports zero violations, returns silence (no injection).
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_post_llm_tool

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use hook_io::PostHookInput;

fn main() -> ExitCode {
    hook_io::run_post_hook(assess)
}

// ── Dispatcher ────────────────────────────────────────────────────

fn assess(input: &PostHookInput) -> Option<String> {
    let file_path = extract_file_path(input)?;
    match classify_file(&file_path) {
        FileKind::Python => assess_python(&file_path),
        FileKind::Other => None,
    }
}

// ── File classification ───────────────────────────────────────────

enum FileKind {
    Python,
    Other,
}

fn classify_file(path: &Path) -> FileKind {
    match path.extension().and_then(|e| e.to_str()) {
        Some("py") => FileKind::Python,
        _ => FileKind::Other,
    }
}

fn extract_file_path(input: &PostHookInput) -> Option<PathBuf> {
    let path_str = input
        .tool_input
        .get("file_path")
        .and_then(|v| v.as_str())?;
    let path = Path::new(path_str);
    if path.exists() {
        Some(path.to_path_buf())
    } else {
        None
    }
}

// ── Python assessment ─────────────────────────────────────────────

fn assess_python(file_path: &Path) -> Option<String> {
    let file_str = file_path.to_str()?;

    let saga = Command::new(saga_bin())
        .args([file_str, "--sidecar"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let saga_stdout = saga.stdout?;

    let syn_output = Command::new(syn_bin())
        .arg("--stdin")
        .stdin(saga_stdout)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    let toon = String::from_utf8_lossy(&syn_output.stdout);
    let trimmed = toon.trim();

    if trimmed.is_empty() || trimmed == "All checks passed." {
        None
    } else {
        Some(trimmed.to_string())
    }
}

// ── Binary resolution ─────────────────────────────────────────────

fn tools_bin() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
        .join(".ai/tools/bin")
}

fn saga_bin() -> PathBuf {
    tools_bin().join("saga")
}

fn syn_bin() -> PathBuf {
    tools_bin().join("syn")
}
