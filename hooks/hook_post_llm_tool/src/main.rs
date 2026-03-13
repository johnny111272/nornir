//! PostToolUse hook: quality assessment injection for file writes.
//!
//! When a tool writes a Python, Rust, or Svelte file, runs the quality pipeline:
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
        FileKind::Python | FileKind::Rust | FileKind::Svelte => assess_source(&file_path),
        FileKind::Other => None,
    }
}

// ── File classification ───────────────────────────────────────────

enum FileKind {
    Python,
    Rust,
    Svelte,
    Other,
}

fn classify_file(path: &Path) -> FileKind {
    match path.extension().and_then(|e| e.to_str()) {
        Some("py") => FileKind::Python,
        Some("rs") => FileKind::Rust,
        Some("svelte") => FileKind::Svelte,
        _ => FileKind::Other,
    }
}

fn extract_file_path(input: &PostHookInput) -> Option<PathBuf> {
    let path = Path::new(input.target_path()?);
    if path.exists() {
        Some(path.to_path_buf())
    } else {
        None
    }
}

// ── Source assessment (Python + Rust + Svelte) ───────────────────

fn assess_source(file_path: &Path) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── classify_file ─────────────────────────────────────────────

    #[test]
    fn classify_python_file() {
        match classify_file(Path::new("/tmp/test.py")) {
            FileKind::Python => {} // correct
            _ => panic!(".py must classify as Python"),
        }
    }

    #[test]
    fn classify_rust_file() {
        match classify_file(Path::new("/tmp/test.rs")) {
            FileKind::Rust => {} // correct
            _ => panic!(".rs must classify as Rust"),
        }
    }

    #[test]
    fn classify_svelte_file() {
        match classify_file(Path::new("/tmp/App.svelte")) {
            FileKind::Svelte => {} // correct
            _ => panic!(".svelte must classify as Svelte"),
        }
    }

    #[test]
    fn classify_no_extension_as_other() {
        match classify_file(Path::new("/tmp/Makefile")) {
            FileKind::Other => {} // correct
            _ => panic!("No extension must classify as Other"),
        }
    }

    #[test]
    fn classify_javascript_as_other() {
        match classify_file(Path::new("/tmp/test.js")) {
            FileKind::Other => {} // correct
            _ => panic!(".js must classify as Other"),
        }
    }

    #[test]
    fn classify_pyw_as_other() {
        // .pyw is NOT .py — strict extension match
        match classify_file(Path::new("/tmp/test.pyw")) {
            FileKind::Other => {} // correct
            _ => panic!(".pyw must classify as Other"),
        }
    }

    // ── extract_file_path ─────────────────────────────────────────

    #[test]
    fn extract_file_path_no_field() {
        let input = PostHookInput {
            tool_name: Some("Write".to_string()),
            tool_input: serde_json::json!({}),
            tool_result: None,
        };
        assert!(extract_file_path(&input).is_none());
    }

    #[test]
    fn extract_file_path_null_value() {
        let input = PostHookInput {
            tool_name: Some("Write".to_string()),
            tool_input: serde_json::json!({ "file_path": null }),
            tool_result: None,
        };
        assert!(extract_file_path(&input).is_none());
    }

    #[test]
    fn extract_file_path_numeric_value() {
        let input = PostHookInput {
            tool_name: Some("Write".to_string()),
            tool_input: serde_json::json!({ "file_path": 42 }),
            tool_result: None,
        };
        assert!(extract_file_path(&input).is_none());
    }

    // ── tools_bin / saga_bin / syn_bin resolution ──────────────────

    #[test]
    fn saga_bin_under_tools_bin() {
        let saga = saga_bin();
        let tools = tools_bin();
        assert!(saga.starts_with(&tools));
        assert_eq!(saga.file_name().unwrap().to_str().unwrap(), "saga");
    }

    #[test]
    fn syn_bin_under_tools_bin() {
        let syn = syn_bin();
        let tools = tools_bin();
        assert!(syn.starts_with(&tools));
        assert_eq!(syn.file_name().unwrap().to_str().unwrap(), "syn");
    }
}
