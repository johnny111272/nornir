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
pub mod rules;

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

impl HookInput {
    /// Extract target file path from tool_input.
    /// Read/Write/Edit use "file_path", Grep/Glob use "path".
    pub fn target_path(&self) -> Option<&str> {
        self.tool_input
            .get("file_path")
            .and_then(|v| v.as_str())
            .or_else(|| self.tool_input.get("path").and_then(|v| v.as_str()))
    }

    /// Extract command string from tool_input (Bash tool).
    pub fn command(&self) -> &str {
        self.tool_input
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
    }
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
    /// Pause and ask the user — permissionDecision: "ask".
    /// User sees the reason and decides allow/deny interactively.
    Ask {
        category: String,
        event: String,
        reason: String,
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
        print_allow();
        return ExitCode::SUCCESS;
    }

    let hook_input: HookInput = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(_) => {
            print_allow();
            return ExitCode::SUCCESS;
        }
    };

    let tool = hook_input.tool_name.as_deref().unwrap_or("unknown");
    emit_decision(decide_fn(&hook_input), tool);
    ExitCode::SUCCESS
}

/// Dispatch a decision: output JSON, notify user, emit to watchtower.
fn emit_decision(decision: HookDecision, tool: &str) {
    match decision {
        HookDecision::Allow => print_allow(),
        HookDecision::Warn { category, event, user_reason, llm_context } => {
            print_banner_warn(&category, &event, &user_reason);
            print_warn(&user_reason, &llm_context);
            emit_to_watchtower(&WatchtowerEvent {
                decision: "warn", category: &category, event: &event,
                tool, detail: &user_reason, context: &llm_context,
            });
        }
        HookDecision::Ask { category, event, reason, llm_context } => {
            print_banner_ask(&category, &event, &reason);
            print_ask(&reason, &llm_context);
            emit_to_watchtower(&WatchtowerEvent {
                decision: "ask", category: &category, event: &event,
                tool, detail: &reason, context: &llm_context,
            });
        }
        HookDecision::Deny { category, event, reason } => {
            print_banner_deny(&category, &event, &reason);
            print_deny(&reason);
            emit_to_watchtower(&WatchtowerEvent {
                decision: "deny", category: &category, event: &event,
                tool, detail: &reason, context: "",
            });
        }
    }
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

fn print_ask(reason: &str, llm_context: &str) {
    use response::{HookOutput, PreToolUseResponse};
    let resp = PreToolUseResponse::ask(reason).with_context(llm_context);
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

fn print_banner_ask(category: &str, event: &str, explanation: &str) {
    notify_and_log('\u{2753}', category, event, explanation);
}

fn print_banner_deny(category: &str, event: &str, explanation: &str) {
    notify_and_log('\u{2716}', category, event, explanation);
}


fn notify_and_log(icon: char, category: &str, event: &str, explanation: &str) {
    let short = explanation.lines().next().unwrap_or(explanation);
    let workspace = std::env::var("CLAUDE_PROJECT_DIR").unwrap_or_default();
    let workspace_name = std::path::Path::new(&workspace)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or(workspace);

    // macOS notification (persists in notification center)
    let title = format!("{} INTERCEPT [{}]", icon, category);
    let _ = std::process::Command::new("terminal-notifier")
        .args(["-title", &title, "-message", short])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    // Voice alerts handled by Hlidskjalf (receives events via datagram)

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
        "destruction" => "destroy uncommitted work",
        "revert" => "revert file changes",
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
                "floor" | "subversion" | "chaining" | "destruction" => "DANGER",
                _ => "WARNING",
            };
            format!("{}: Claude is trying to {} -- BLOCKED.", prefix, phrase)
        }
        "ask" => format!("ATTENTION: Claude is trying to {} -- Awaiting your decision.", phrase),
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

/// Fields needed to emit a watchtower alert datagram.
struct WatchtowerEvent<'a> {
    decision: &'a str,
    category: &'a str,
    event: &'a str,
    tool: &'a str,
    detail: &'a str,
    context: &'a str,
}

fn emit_to_watchtower(alert: &WatchtowerEvent) {
    let speech = build_speech(alert.decision, alert.category);

    let source = std::env::args()
        .next()
        .and_then(|p| {
            std::path::Path::new(&p)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "hook".to_string());

    let datagram = datagram_io::Datagram {
        timestamp: datagram_io::now(),
        source,
        kind: datagram_io::DatagramKind::Alert,
        classifier: None,
        priority: match alert.decision {
            "deny" => datagram_io::Priority::High,
            "warn" => datagram_io::Priority::Normal,
            _ => datagram_io::Priority::Low,
        },
        workspace: datagram_io::workspace_name(),
        detail: Some(alert.detail.to_string()),
        speech: if speech.is_empty() { None } else { Some(speech) },
        payload: Some(serde_json::json!({
            "category": alert.category,
            "decision": alert.decision,
            "tool": alert.tool,
            "event": alert.event,
            "context_injected": alert.context,
        })),
    };
    datagram_io::emit(&datagram);
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── HookInput::target_path ───────────────────────────────────

    #[test]
    fn target_path_from_file_path() {
        let input = HookInput {
            tool_name: Some("Read".into()),
            tool_input: serde_json::json!({ "file_path": "/src/main.rs" }),
        };
        assert_eq!(input.target_path(), Some("/src/main.rs"));
    }

    #[test]
    fn target_path_from_path() {
        let input = HookInput {
            tool_name: Some("Grep".into()),
            tool_input: serde_json::json!({ "path": "/src/" }),
        };
        assert_eq!(input.target_path(), Some("/src/"));
    }

    #[test]
    fn target_path_file_path_wins_over_path() {
        let input = HookInput {
            tool_name: Some("Read".into()),
            tool_input: serde_json::json!({ "file_path": "/a.rs", "path": "/b.rs" }),
        };
        assert_eq!(input.target_path(), Some("/a.rs"));
    }

    #[test]
    fn target_path_neither_field() {
        let input = HookInput {
            tool_name: Some("Read".into()),
            tool_input: serde_json::json!({}),
        };
        assert!(input.target_path().is_none());
    }

    #[test]
    fn target_path_null_value() {
        let input = HookInput {
            tool_name: Some("Read".into()),
            tool_input: serde_json::json!({ "file_path": null }),
        };
        assert!(input.target_path().is_none());
    }

    // ── HookInput::command ───────────────────────────────────────

    #[test]
    fn command_present() {
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: serde_json::json!({ "command": "ls -la" }),
        };
        assert_eq!(input.command(), "ls -la");
    }

    #[test]
    fn command_missing() {
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: serde_json::json!({}),
        };
        assert_eq!(input.command(), "");
    }

    #[test]
    fn command_null_value() {
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: serde_json::json!({ "command": null }),
        };
        assert_eq!(input.command(), "");
    }

    // ── PostHookInput::target_path ───────────────────────────────

    #[test]
    fn post_target_path_from_file_path() {
        let input = PostHookInput {
            tool_name: Some("Write".into()),
            tool_input: serde_json::json!({ "file_path": "/out/file.py" }),
            tool_result: None,
        };
        assert_eq!(input.target_path(), Some("/out/file.py"));
    }

    #[test]
    fn post_target_path_from_path() {
        let input = PostHookInput {
            tool_name: Some("Glob".into()),
            tool_input: serde_json::json!({ "path": "/search/" }),
            tool_result: None,
        };
        assert_eq!(input.target_path(), Some("/search/"));
    }

    #[test]
    fn post_target_path_neither_field() {
        let input = PostHookInput {
            tool_name: Some("Write".into()),
            tool_input: serde_json::json!({}),
            tool_result: None,
        };
        assert!(input.target_path().is_none());
    }
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

impl PostHookInput {
    /// Extract target file path from tool_input.
    /// Read/Write/Edit use "file_path", Grep/Glob use "path".
    pub fn target_path(&self) -> Option<&str> {
        self.tool_input
            .get("file_path")
            .and_then(|v| v.as_str())
            .or_else(|| self.tool_input.get("path").and_then(|v| v.as_str()))
    }
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
