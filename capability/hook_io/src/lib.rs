//! Shared IO contract for Claude Code PreToolUse hook binaries.
//!
//! Reads hook JSON from stdin, calls a decision function, outputs the
//! appropriate JSON response to stdout.
//! Always exits 0 — Claude Code requires this.
//!
//! Warn and Deny decisions are also emitted to Hlidskjalf (watchtower)
//! via Unix stream socket — fire-and-forget, never blocks.

use std::io::{Read, Write};
use std::process::ExitCode;

use serde::Deserialize;

/// Hook input from Claude Code (subset of PreToolUse payload).
#[derive(Debug, Deserialize)]
pub struct HookInput {
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_input: serde_json::Value,
}

/// Decision from a hook's logic.
pub enum HookDecision {
    /// Allow silently — no output except permissionDecision: "allow".
    Allow,
    /// Allow but warn both user (stderr banner) and LLM (context injection).
    Warn {
        category: String,
        event: String,
        user_reason: String,
        llm_context: String,
    },
    /// Deny with reason shown to LLM.
    Deny {
        category: String,
        event: String,
        reason: String,
    },
}

/// Run a hook: read stdin, parse, decide, output.
///
/// The `decide_fn` receives parsed hook input and returns a decision.
/// This function handles all IO and JSON serialization.
pub fn run_hook<F>(decide_fn: F) -> ExitCode
where
    F: FnOnce(&HookInput) -> HookDecision,
{
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        // Can't read stdin — allow (fail open for hooks, fail closed would block everything)
        print_allow();
        return ExitCode::SUCCESS;
    }

    let hook_input: HookInput = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(_) => {
            // Malformed JSON — allow (don't block on parse errors)
            print_allow();
            return ExitCode::SUCCESS;
        }
    };

    let tool = hook_input
        .tool_name
        .as_deref()
        .unwrap_or("unknown");

    match decide_fn(&hook_input) {
        HookDecision::Allow => print_allow(),
        HookDecision::Warn {
            category,
            event,
            user_reason,
            llm_context,
        } => {
            print_banner_warn(&category, &event, &user_reason);
            print_warn(&user_reason, &llm_context);
            emit_to_watchtower("warn", &category, &event, tool, &user_reason, &llm_context);
        }
        HookDecision::Deny {
            category,
            event,
            reason,
        } => {
            print_banner_deny(&category, &event, &reason);
            print_deny(&reason);
            emit_to_watchtower("deny", &category, &event, tool, &reason, "");
        }
    }

    ExitCode::SUCCESS
}

// ── JSON output to stdout ──────────────────────────────────────────

fn print_allow() {
    let out = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow"
        }
    });
    print!("{}", out);
}

fn print_warn(user_reason: &str, llm_context: &str) {
    let out = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "permissionDecisionReason": user_reason,
            "additionalContext": llm_context
        }
    });
    print!("{}", out);
}

fn print_deny(reason: &str) {
    let out = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason
        }
    });
    print!("{}", out);
}

// ── User notification ───────────────────────────────────────────────
//
// Claude Code captures stdout (JSON) and stderr via pipes.
// macOS notifications + log file are the reliable user-facing channels.

fn print_banner_warn(category: &str, event: &str, explanation: &str) {
    let phrase = category_phrase(category);
    let speech = format!("WARNING: Claude is {} -- Correction provided.", phrase);
    notify_and_log('\u{26A0}', category, event, explanation, &speech);
}

fn print_banner_deny(category: &str, event: &str, explanation: &str) {
    let phrase = category_phrase(category);
    let prefix = if category == "floor" || category == "subversion" || category == "chaining" {
        "DANGER"
    } else {
        "WARNING"
    };
    let speech = format!("{}: Claude is trying to {} -- BLOCKED.", prefix, phrase);
    notify_and_log('\u{2716}', category, event, explanation, &speech);
}

/// One clear spoken phrase per category. Details go to log and notification.
fn category_phrase(category: &str) -> &'static str {
    match category {
        "floor" => "access sensitive credentials",
        "probing" => "probing the guardrail settings",
        "gaming" => "gaming the guardrails",
        "subversion" => "subvert the safety controls",
        "truncation" => "reading constraint files selectively",
        "evasion" => "evade the safety controls",
        "chaining" => "chain shell commands to escape the sandbox",
        "path" => "access a restricted path",
        "bash" => "run a restricted command",
        _ => "do something unexpected",
    }
}

fn notify_and_log(icon: char, category: &str, event: &str, explanation: &str, speech: &str) {
    let short = explanation.lines().next().unwrap_or(explanation);
    let workspace = std::env::var("CLAUDE_PROJECT_DIR").unwrap_or_default();
    let workspace_name = std::path::Path::new(&workspace)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| workspace.clone());

    // macOS notification (persists in notification center)
    let title = format!("{} INTERCEPT [{}]", icon, category);
    let _ = std::process::Command::new("terminal-notifier")
        .args(["-title", &title, "-message", short])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    // Audible callout
    let _ = std::process::Command::new("say")
        .args(["-v", "Fiona", speech])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    // Append to log file
    if let Ok(mut log) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(
            log,
            "[{}] [{}] [{}] {} — {}",
            timestamp, workspace_name, category, event, short
        );
    }
}

fn log_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home).join(".claude").join("intercept.log")
}

// ── Watchtower emission ───────────────────────────────────────────

fn emit_to_watchtower(
    decision: &str,
    category: &str,
    event: &str,
    tool: &str,
    detail: &str,
    context: &str,
) {
    let watchtower_event = socket_emit::WatchtowerEvent {
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0),
        category: category.to_string(),
        decision: decision.to_string(),
        event_name: format!("{}:{}", tool, event),
        workspace: socket_emit::workspace_name(),
        detail: detail.to_string(),
        context_injected: context.to_string(),
        payload: None,
    };
    socket_emit::emit(&watchtower_event);
}

