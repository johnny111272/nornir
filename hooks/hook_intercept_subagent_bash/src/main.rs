//! PreToolUse hook: validate subagent Bash commands.
//!
//! Replaces validate_bash.py with new invocation patterns for nornir writers.
//!
//! Usage (in agent frontmatter):
//!     hook_intercept_subagent_bash --writer tool1 tool2 --inspect cmd1=/path/ cmd2=/path/
//!
//! Categories:
//!     --writer: nornir writer binaries. Validates heredoc-pipe pattern,
//!              writer name in allowed list, optional name arg is bare filename.
//!     --inspect: unix commands (ls, find, tree, etc.). First-word match,
//!               all paths under allowed prefix, no shell chaining.

use std::collections::HashMap;
use std::process::ExitCode;

use hook_io::{HookDecision, HookInput};
use regex::Regex;

fn main() -> ExitCode {
    hook_io::run_hook(decide)
}

// ── Arg parsing ────────────────────────────────────────────────────

struct Config {
    writers: Vec<String>,
    inspect: HashMap<String, String>, // cmd_name → allowed_path_prefix
}

fn parse_args() -> Config {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut writers = Vec::new();
    let mut inspect = HashMap::new();
    let mut current: Option<&str> = None;

    for arg in &args {
        match arg.as_str() {
            "--writer" => current = Some("writer"),
            "--inspect" => current = Some("inspect"),
            _ => match current {
                Some("writer") => writers.push(arg.clone()),
                Some("inspect") => {
                    if let Some((name, path)) = arg.split_once('=') {
                        inspect.insert(name.to_string(), path.to_string());
                    }
                }
                _ => {}
            },
        }
    }

    Config { writers, inspect }
}

// ── Shell safety ───────────────────────────────────────────────────

/// Characters that indicate shell chaining.
fn has_chain_chars(s: &str) -> bool {
    // Check for ;  |  &  ` outside of normal pipe usage
    // We explicitly allow a single | for the heredoc-pipe pattern
    s.contains(';') || s.contains('`') || s.contains("&&") || s.contains("||")
}

/// Validate that a name arg is a bare filename (no path parts).
fn is_bare_name(s: &str) -> bool {
    !s.is_empty()
        && !s.contains('/')
        && !s.contains('\\')
        && !s.contains("..")
        && !s.contains('\0')
        && !s.starts_with('.')
        && !s.starts_with('-')
}

// ── Writer validation ──────────────────────────────────────────────

/// Parse the header line of a heredoc-pipe command.
/// Returns (delimiter, writer_name, optional_name_arg) or None.
fn parse_heredoc_header(first_line: &str) -> Option<(String, String, Option<String>)> {
    // Match: cat <<'DELIM' | writer_name [name_arg]
    let re = Regex::new(r"^cat\s+<<'([A-Z_]+)'\s*\|\s*(\S+)(?:\s+(\S+))?\s*$").unwrap();
    let caps = re.captures(first_line)?;
    Some((
        caps.get(1).unwrap().as_str().to_string(),
        caps.get(2).unwrap().as_str().to_string(),
        caps.get(3).map(|m| m.as_str().to_string()),
    ))
}

fn validate_writer(command: &str, writers: &[String]) -> HookDecision {
    // Try heredoc pattern: first line is header, last line is delimiter
    let lines: Vec<&str> = command.lines().collect();
    if lines.len() >= 3 {
        if let Some((delim, writer_name, name_arg)) = parse_heredoc_header(lines[0]) {
            // Verify command ends with the delimiter on its own line
            if lines.last().map(|l| l.trim()) != Some(delim.as_str()) {
                return deny_bad_writer_pattern();
            }
            return check_writer_and_name(&writer_name, name_arg.as_deref(), writers);
        }
    }

    // Also allow: echo '...' | writer_name [name_arg]
    let echo_re = Regex::new(r"^echo\s+'[^']*'\s*\|\s*(\S+)(?:\s+(\S+))?$").unwrap();
    if let Some(caps) = echo_re.captures(command) {
        let writer_name = caps.get(1).unwrap().as_str();
        let name_arg = caps.get(2).map(|m| m.as_str());
        return check_writer_and_name(writer_name, name_arg, writers);
    }

    deny_bad_writer_pattern()
}

fn check_writer_and_name(
    writer_name: &str,
    name_arg: Option<&str>,
    writers: &[String],
) -> HookDecision {
    if !writers.iter().any(|w| w == writer_name) {
        return HookDecision::Deny {
            category: "bash".into(),
            event: format!("'{}' not in allowed writers", writer_name),
            reason: format!(
                "'{}' is not an allowed writer.\nAllowed writers:\n{}",
                writer_name,
                writers.iter().map(|w| format!("  {}", w)).collect::<Vec<_>>().join("\n")
            ),
        };
    }
    if let Some(name) = name_arg {
        if !is_bare_name(name) {
            return HookDecision::Deny {
                category: "bash".into(),
                event: format!("invalid name arg '{}'", name),
                reason: format!(
                    "Name argument '{}' must be a plain filename stem (e.g., 'entry-123').\n\
                     No path separators, no leading dots, no leading dashes.",
                    name
                ),
            };
        }
    }
    HookDecision::Allow
}

fn deny_bad_writer_pattern() -> HookDecision {
    HookDecision::Deny {
        category: "bash".into(),
        event: "unrecognized writer invocation pattern".into(),
        reason: "Writer commands must use heredoc pipe pattern:\n  \
                 cat <<'RECORD' | writer_name [name]\n  \
                 {...json...}\n  \
                 RECORD".into(),
    }
}

// ── Inspect validation ─────────────────────────────────────────────

fn validate_inspect(command: &str, _cmd_name: &str, allowed_path: &str) -> HookDecision {
    if has_chain_chars(command) {
        return warn_chaining(command);
    }

    // Extract all absolute paths from the command
    let path_re = Regex::new(r"(/\S+)").unwrap();
    for m in path_re.find_iter(command) {
        let path = m.as_str();
        if !path.starts_with(allowed_path) {
            return HookDecision::Deny {
                category: "bash".into(),
                event: format!("path '{}' outside allowed prefix", path),
                reason: format!(
                    "Path '{}' is not under '{}'.",
                    path, allowed_path
                ),
            };
        }
    }

    HookDecision::Allow
}

// ── Shell chaining warning ─────────────────────────────────────────

/// Shell chaining in a sandboxed subagent is an early adversarial signal.
/// Deny the command AND produce a loud banner so the user knows.
fn warn_chaining(command: &str) -> HookDecision {
    let first_word = command.split_whitespace().next().unwrap_or("?");
    HookDecision::Deny {
        category: "chaining".into(),
        event: format!(
            "shell chaining detected in '{}' command — possible sandbox escape",
            first_word
        ),
        reason: format!(
            "Shell chaining characters (;  &&  ||  `) detected.\n\
             This subagent is sandboxed. Each command must be a single operation.\n\
             Chaining attempts are logged and flagged to the user."
        ),
    }
}

// ── Decision logic ─────────────────────────────────────────────────

fn decide(input: &HookInput) -> HookDecision {
    let config = parse_args();

    let command = input
        .tool_input
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if command.trim().is_empty() {
        return HookDecision::Deny {
            category: "bash".into(),
            event: "empty command".into(),
            reason: "Empty command.".into(),
        };
    }

    let first_word = command.split_whitespace().next().unwrap_or("");

    // Check if it's a writer invocation (starts with cat or echo piped to a writer)
    if (first_word == "cat" || first_word == "echo") && command.contains('|') {
        // Extract the pipe target to see if it's a known writer
        let pipe_target = command
            .splitn(2, '|')
            .nth(1)
            .map(|s| s.trim().split_whitespace().next().unwrap_or(""))
            .unwrap_or("");

        if config.writers.iter().any(|w| w == pipe_target) {
            // Check for chaining outside the heredoc body
            let outside = strip_heredoc_body(command);
            if has_chain_chars(&outside) {
                return warn_chaining(command);
            }
            return validate_writer(command, &config.writers);
        }
    }

    // Check if it's a known inspect command
    if let Some(allowed_path) = config.inspect.get(first_word) {
        return validate_inspect(command, first_word, allowed_path);
    }

    // Not allowed — build help text
    let mut allowed_list = Vec::new();
    for w in &config.writers {
        allowed_list.push(format!("  cat <<'RECORD' | {}\n  {{...json...}}\n  RECORD", w));
    }
    for (cmd, _) in &config.inspect {
        allowed_list.push(format!("  {} <args>", cmd));
    }
    let help = if allowed_list.is_empty() {
        "  (none)".to_string()
    } else {
        allowed_list.join("\n")
    };

    HookDecision::Deny {
        category: "bash".into(),
        event: format!("'{}' is not allowed", first_word),
        reason: format!(
            "'{}' is not an allowed command.\nAllowed commands:\n{}\n\
             All other Bash commands are blocked by the security boundary.",
            first_word, help
        ),
    }
}

/// Strip heredoc body from command, returning only the header line.
fn strip_heredoc_body(command: &str) -> String {
    // For chaining detection, we only need the first line (the pipe header)
    // and the delimiter line. The heredoc body is safe to ignore.
    command.lines().next().unwrap_or(command).to_string()
}
