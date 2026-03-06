//! Shared IO contracts for Claude Code hook binaries.
//!
//! Two layers:
//!   response module — type-safe response builders for all hook events
//!   run_pre_hook / run_post_hook — entry points that handle stdin/stdout IO
//!
//! Always exits 0 — Claude Code requires this.
//!
//! PreToolUse warn/deny decisions are emitted to Hlidskjalf (watchtower)
//! via Unix stream socket — fire-and-forget, never blocks.
//! PostToolUse assessment emission is handled by syn (not hook_io).

pub mod response;

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

/// Run a PreToolUse hook: read stdin, parse, decide, output.
///
/// The `decide_fn` receives parsed hook input and returns a decision.
/// This function handles all IO and JSON serialization.
pub fn run_pre_hook<F>(decide_fn: F) -> ExitCode
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
    use response::{HookOutput, PreToolUseResponse};
    print!("{}", PreToolUseResponse::allow().to_json());
}

fn print_warn(user_reason: &str, llm_context: &str) {
    use response::{HookOutput, PreToolUseResponse};
    let resp = PreToolUseResponse::allow()
        .with_reason(user_reason)
        .with_context(llm_context);
    print!("{}", resp.to_json());
}

fn print_deny(reason: &str) {
    use response::{HookOutput, PreToolUseResponse};
    print!("{}", PreToolUseResponse::deny(reason).to_json());
}

// ── User notification ───────────────────────────────────────────────
//
// Claude Code captures stdout (JSON) and stderr via pipes.
// macOS notifications + log file are the reliable user-facing channels.

fn print_banner_warn(category: &str, event: &str, explanation: &str) {
    notify_and_log('\u{26A0}', category, event, explanation);
}

fn print_banner_deny(category: &str, event: &str, explanation: &str) {
    notify_and_log('\u{2716}', category, event, explanation);
}


fn notify_and_log(icon: char, category: &str, event: &str, explanation: &str) {
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

    // Voice alerts handled by Hlidskjalf (receives events via socket_emit)

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

// ── Speech text ──────────────────────────────────────────────────

/// One clear spoken phrase per category.
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

fn build_speech(decision: &str, category: &str) -> String {
    let phrase = category_phrase(category);
    match decision {
        "deny" => {
            let prefix = match category {
                "floor" | "subversion" | "chaining" => "DANGER",
                _ => "WARNING",
            };
            format!("{}: Claude is trying to {} -- BLOCKED.", prefix, phrase)
        }
        "warn" => format!("WARNING: Claude is {} -- Correction provided.", phrase),
        _ => String::new(),
    }
}

// ── Watchtower emission ───────────────────────────────────────────

/// Backward-compatible alias.
pub fn run_hook<F>(decide_fn: F) -> ExitCode
where
    F: FnOnce(&HookInput) -> HookDecision,
{
    run_pre_hook(decide_fn)
}

fn emit_to_watchtower(
    decision: &str,
    category: &str,
    event: &str,
    tool: &str,
    detail: &str,
    context: &str,
) {
    let speech = build_speech(decision, category);

    let source = std::env::args()
        .next()
        .and_then(|p| {
            std::path::Path::new(&p)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "hook".to_string());

    let datagram = socket_emit::Datagram {
        timestamp: socket_emit::now(),
        source,
        datagram_type: "alert".to_string(),
        priority: match decision {
            "deny" => "high",
            "warn" => "normal",
            _ => "low",
        }.to_string(),
        workspace: socket_emit::workspace_name(),
        detail: Some(detail.to_string()),
        speech: if speech.is_empty() { None } else { Some(speech) },
        payload: Some(serde_json::json!({
            "category": category,
            "decision": decision,
            "tool": tool,
            "event": event,
            "context_injected": context,
        })),
    };
    socket_emit::emit_datagram(&datagram);
}

// ── PostToolUse contract ─────────────────────────────────────────

/// Hook input from Claude Code PostToolUse payload.
#[derive(Debug, Deserialize)]
pub struct PostHookInput {
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    #[serde(default)]
    pub tool_result: Option<serde_json::Value>,
}

/// Run a PostToolUse hook: read stdin, parse, assess, output.
///
/// The `assess_fn` receives parsed hook input and returns an optional
/// context string. Some(msg) injects into LLM context via additionalContext.
/// None produces no injection (silence).
pub fn run_post_hook<F>(assess_fn: F) -> ExitCode
where
    F: FnOnce(&PostHookInput) -> Option<String>,
{
    use response::{HookOutput, PostToolUseResponse};

    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return ExitCode::SUCCESS;
    }

    let hook_input: PostHookInput = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(_) => return ExitCode::SUCCESS,
    };

    if let Some(msg) = assess_fn(&hook_input) {
        let resp = PostToolUseResponse::allow().with_context(msg);
        let json = resp.to_json();
        if !json.is_empty() {
            print!("{}", json);
        }
    }

    ExitCode::SUCCESS
}
