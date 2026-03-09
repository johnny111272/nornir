//! PreToolUse hook: validate subagent Bash commands.
//!
//! Replaces validate_bash.py with new invocation patterns for nornir writers.
//!
//! Usage (in agent frontmatter):
//!     hook_pre_subagent_bash --writer tool1 tool2 --inspect cmd1=/path/ cmd2=/path/
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── has_chain_chars ───────────────────────────────────────────

    #[test]
    fn has_chain_chars_semicolon() {
        assert!(has_chain_chars("ls; rm -rf /"));
    }

    #[test]
    fn has_chain_chars_and_and() {
        assert!(has_chain_chars("cmd && cmd2"));
    }

    #[test]
    fn has_chain_chars_or_or() {
        assert!(has_chain_chars("cmd || cmd2"));
    }

    #[test]
    fn has_chain_chars_backtick() {
        assert!(has_chain_chars("echo `date`"));
    }

    #[test]
    fn has_chain_chars_simple_path_false() {
        assert!(!has_chain_chars("ls -la /path"));
    }

    #[test]
    fn has_chain_chars_single_pipe_allowed() {
        // Single pipe is NOT a chaining character in this implementation
        assert!(!has_chain_chars("cat <<'EOF' | writer"));
    }

    #[test]
    fn has_chain_chars_empty_string() {
        assert!(!has_chain_chars(""));
    }

    #[test]
    fn has_chain_chars_normal_command() {
        assert!(!has_chain_chars("cargo build --release"));
    }

    #[test]
    fn has_chain_chars_single_ampersand_not_detected() {
        // Single & (background) is NOT in the check — only && is
        assert!(!has_chain_chars("sleep 5 &"));
    }

    // ── is_bare_name ──────────────────────────────────────────────

    #[test]
    fn is_bare_name_valid_stem() {
        assert!(is_bare_name("entry-123"));
    }

    #[test]
    fn is_bare_name_with_underscore() {
        assert!(is_bare_name("my_file"));
    }

    #[test]
    fn is_bare_name_alphanumeric() {
        assert!(is_bare_name("report2024"));
    }

    #[test]
    fn is_bare_name_path_separator_rejected() {
        assert!(!is_bare_name("foo/bar"));
    }

    #[test]
    fn is_bare_name_hidden_file_rejected() {
        assert!(!is_bare_name(".hidden"));
    }

    #[test]
    fn is_bare_name_flag_rejected() {
        assert!(!is_bare_name("-flag"));
    }

    #[test]
    fn is_bare_name_empty_rejected() {
        assert!(!is_bare_name(""));
    }

    #[test]
    fn is_bare_name_backslash_rejected() {
        assert!(!is_bare_name("foo\\bar"));
    }

    #[test]
    fn is_bare_name_dotdot_rejected() {
        assert!(!is_bare_name("foo..bar"));
    }

    #[test]
    fn is_bare_name_null_byte_rejected() {
        assert!(!is_bare_name("foo\0bar"));
    }

    // ── parse_heredoc_header ──────────────────────────────────────

    #[test]
    fn parse_heredoc_header_basic() {
        let result = parse_heredoc_header("cat <<'RECORD' | append_truth_qc_report_record");
        assert!(result.is_some());
        let (delim, writer, name_arg) = result.unwrap();
        assert_eq!(delim, "RECORD");
        assert_eq!(writer, "append_truth_qc_report_record");
        assert!(name_arg.is_none());
    }

    #[test]
    fn parse_heredoc_header_with_name_arg() {
        let result = parse_heredoc_header("cat <<'EOF' | writer_name arg1");
        assert!(result.is_some());
        let (delim, writer, name_arg) = result.unwrap();
        assert_eq!(delim, "EOF");
        assert_eq!(writer, "writer_name");
        assert_eq!(name_arg, Some("arg1".to_string()));
    }

    #[test]
    fn parse_heredoc_header_not_heredoc() {
        let result = parse_heredoc_header("echo hello");
        assert!(result.is_none());
    }

    #[test]
    fn parse_heredoc_header_unquoted_delimiter_rejected() {
        // No quotes around delimiter — must be rejected
        let result = parse_heredoc_header("cat <<RECORD | writer");
        assert!(result.is_none());
    }

    #[test]
    fn parse_heredoc_header_double_quoted_rejected() {
        // Double quotes — must be rejected (only single quotes allowed)
        let result = parse_heredoc_header("cat <<\"RECORD\" | writer");
        assert!(result.is_none());
    }

    #[test]
    fn parse_heredoc_header_no_pipe() {
        let result = parse_heredoc_header("cat <<'RECORD'");
        assert!(result.is_none());
    }

    #[test]
    fn parse_heredoc_header_lowercase_delimiter_rejected() {
        // Delimiter regex requires [A-Z_]+ — lowercase rejected
        let result = parse_heredoc_header("cat <<'record' | writer");
        assert!(result.is_none());
    }

    // ── validate_writer ───────────────────────────────────────────

    #[test]
    fn validate_writer_valid_heredoc() {
        let cmd = "cat <<'RECORD' | my_writer\n{\"key\":\"value\"}\nRECORD";
        let writers = vec!["my_writer".to_string()];
        let decision = validate_writer(cmd, &writers);
        match decision {
            HookDecision::Allow => {} // correct
            _ => panic!("Valid heredoc with allowed writer must Allow"),
        }
    }

    #[test]
    fn validate_writer_unknown_writer_denied() {
        let cmd = "cat <<'RECORD' | evil_writer\n{\"key\":\"value\"}\nRECORD";
        let writers = vec!["my_writer".to_string()];
        let decision = validate_writer(cmd, &writers);
        match decision {
            HookDecision::Deny { category, reason, .. } => {
                assert_eq!(category, "bash");
                assert!(reason.contains("evil_writer"));
                assert!(reason.contains("not an allowed writer"));
            }
            _ => panic!("Unknown writer must Deny"),
        }
    }

    #[test]
    fn validate_writer_bad_name_arg_denied() {
        let cmd = "cat <<'RECORD' | my_writer ../escape\n{\"key\":\"value\"}\nRECORD";
        let writers = vec!["my_writer".to_string()];
        let decision = validate_writer(cmd, &writers);
        match decision {
            HookDecision::Deny { reason, .. } => {
                assert!(
                    reason.contains("plain filename stem"),
                    "Bad name arg must explain the requirement"
                );
            }
            _ => panic!("Path traversal in name arg must Deny"),
        }
    }

    #[test]
    fn validate_writer_not_heredoc_pattern() {
        let cmd = "rm -rf /";
        let writers = vec!["my_writer".to_string()];
        let decision = validate_writer(cmd, &writers);
        match decision {
            HookDecision::Deny { reason, .. } => {
                assert!(reason.contains("heredoc pipe pattern"));
            }
            _ => panic!("Non-heredoc pattern must Deny"),
        }
    }

    #[test]
    fn validate_writer_echo_pipe_valid() {
        let cmd = "echo 'hello' | my_writer";
        let writers = vec!["my_writer".to_string()];
        let decision = validate_writer(cmd, &writers);
        match decision {
            HookDecision::Allow => {} // correct
            _ => panic!("Valid echo pipe with allowed writer must Allow"),
        }
    }

    #[test]
    fn validate_writer_echo_pipe_unknown_writer() {
        let cmd = "echo 'hello' | bad_writer";
        let writers = vec!["my_writer".to_string()];
        let decision = validate_writer(cmd, &writers);
        match decision {
            HookDecision::Deny { .. } => {} // correct
            _ => panic!("Echo pipe with unknown writer must Deny"),
        }
    }

    #[test]
    fn validate_writer_heredoc_wrong_closing_delimiter() {
        let cmd = "cat <<'RECORD' | my_writer\n{\"key\":\"value\"}\nWRONG";
        let writers = vec!["my_writer".to_string()];
        let decision = validate_writer(cmd, &writers);
        match decision {
            HookDecision::Deny { .. } => {} // correct — mismatched delimiter
            _ => panic!("Mismatched delimiter must Deny"),
        }
    }

    #[test]
    fn validate_writer_heredoc_with_valid_name_arg() {
        let cmd = "cat <<'RECORD' | my_writer entry-42\n{\"key\":\"value\"}\nRECORD";
        let writers = vec!["my_writer".to_string()];
        let decision = validate_writer(cmd, &writers);
        match decision {
            HookDecision::Allow => {} // correct
            _ => panic!("Valid heredoc with bare name arg must Allow"),
        }
    }

    #[test]
    fn validate_writer_name_arg_leading_dot_denied() {
        let cmd = "cat <<'RECORD' | my_writer .secret\n{\"key\":\"value\"}\nRECORD";
        let writers = vec!["my_writer".to_string()];
        let decision = validate_writer(cmd, &writers);
        match decision {
            HookDecision::Deny { .. } => {} // correct
            _ => panic!("Name arg with leading dot must Deny"),
        }
    }

    #[test]
    fn validate_writer_name_arg_leading_dash_denied() {
        let cmd = "cat <<'RECORD' | my_writer -flag\n{\"key\":\"value\"}\nRECORD";
        let writers = vec!["my_writer".to_string()];
        let decision = validate_writer(cmd, &writers);
        match decision {
            HookDecision::Deny { .. } => {} // correct
            _ => panic!("Name arg with leading dash must Deny"),
        }
    }

    // ── validate_inspect ──────────────────────────────────────────

    #[test]
    fn validate_inspect_path_under_prefix_allows() {
        let decision = validate_inspect("ls /allowed/path/file.txt", "ls", "/allowed/path/");
        match decision {
            HookDecision::Allow => {} // correct
            _ => panic!("Path under allowed prefix must Allow"),
        }
    }

    #[test]
    fn validate_inspect_path_outside_prefix_denied() {
        let decision = validate_inspect("ls /etc/passwd", "ls", "/allowed/path/");
        match decision {
            HookDecision::Deny { category, reason, .. } => {
                assert_eq!(category, "bash");
                assert!(reason.contains("/etc/passwd"));
                assert!(reason.contains("/allowed/path/"));
            }
            _ => panic!("Path outside prefix must Deny"),
        }
    }

    #[test]
    fn validate_inspect_with_chaining_denied() {
        let decision = validate_inspect("ls /allowed/path/ && rm -rf /", "ls", "/allowed/path/");
        match decision {
            HookDecision::Deny { category, .. } => {
                assert_eq!(category, "chaining");
            }
            _ => panic!("Chaining must Deny"),
        }
    }

    #[test]
    fn validate_inspect_no_paths_at_all_allows() {
        // Command with no path-like arguments at all
        let decision = validate_inspect("ls", "ls", "/allowed/");
        match decision {
            HookDecision::Allow => {} // correct — no paths to check
            _ => panic!("No paths at all means nothing to block"),
        }
    }

    #[test]
    fn validate_inspect_relative_path_with_slash_caught() {
        // The regex (/\S+) catches "/path" inside "relative/path"
        // This is intentional — slashes in arguments are suspicious
        let decision = validate_inspect("ls relative/path", "ls", "/allowed/");
        match decision {
            HookDecision::Deny { .. } => {} // correct — /path is extracted and denied
            _ => panic!("Embedded slash caught by path regex"),
        }
    }

    #[test]
    fn validate_inspect_multiple_paths_all_valid() {
        let decision = validate_inspect(
            "find /allowed/a /allowed/b -name '*.rs'",
            "find",
            "/allowed/",
        );
        match decision {
            HookDecision::Allow => {} // correct
            _ => panic!("Multiple paths all under prefix must Allow"),
        }
    }

    #[test]
    fn validate_inspect_one_bad_path_among_many() {
        let decision = validate_inspect(
            "find /allowed/a /etc/shadow -name '*.rs'",
            "find",
            "/allowed/",
        );
        match decision {
            HookDecision::Deny { .. } => {} // correct
            _ => panic!("One bad path must Deny the whole command"),
        }
    }

    // ── strip_heredoc_body ────────────────────────────────────────

    #[test]
    fn strip_heredoc_body_multiline() {
        let cmd = "cat <<'RECORD' | writer\n{\"key\":\"value\"}\nRECORD";
        let result = strip_heredoc_body(cmd);
        assert_eq!(result, "cat <<'RECORD' | writer");
    }

    #[test]
    fn strip_heredoc_body_single_line() {
        let cmd = "echo hello";
        let result = strip_heredoc_body(cmd);
        assert_eq!(result, "echo hello");
    }

    #[test]
    fn strip_heredoc_body_empty() {
        let cmd = "";
        let result = strip_heredoc_body(cmd);
        assert_eq!(result, "");
    }

    // ── warn_chaining ─────────────────────────────────────────────

    #[test]
    fn warn_chaining_produces_deny() {
        let decision = warn_chaining("ls; rm -rf /");
        match decision {
            HookDecision::Deny { category, reason, .. } => {
                assert_eq!(category, "chaining");
                assert!(reason.contains("Shell chaining"));
            }
            _ => panic!("warn_chaining must produce Deny"),
        }
    }

    // ── check_writer_and_name ─────────────────────────────────────

    #[test]
    fn check_writer_and_name_valid_writer_no_arg() {
        let writers = vec!["my_writer".to_string()];
        let decision = check_writer_and_name("my_writer", None, &writers);
        match decision {
            HookDecision::Allow => {} // correct
            _ => panic!("Valid writer with no name arg must Allow"),
        }
    }

    #[test]
    fn check_writer_and_name_invalid_writer() {
        let writers = vec!["my_writer".to_string()];
        let decision = check_writer_and_name("evil_writer", None, &writers);
        match decision {
            HookDecision::Deny { .. } => {} // correct
            _ => panic!("Unknown writer must Deny"),
        }
    }

    #[test]
    fn check_writer_and_name_valid_writer_valid_arg() {
        let writers = vec!["my_writer".to_string()];
        let decision = check_writer_and_name("my_writer", Some("entry-42"), &writers);
        match decision {
            HookDecision::Allow => {} // correct
            _ => panic!("Valid writer + valid bare name must Allow"),
        }
    }

    #[test]
    fn check_writer_and_name_valid_writer_bad_arg() {
        let writers = vec!["my_writer".to_string()];
        let decision = check_writer_and_name("my_writer", Some("/etc/passwd"), &writers);
        match decision {
            HookDecision::Deny { .. } => {} // correct
            _ => panic!("Valid writer + path-containing name must Deny"),
        }
    }

    // ── Integration: decide function patterns ─────────────────────

    fn make_bash_input(command: &str) -> HookInput {
        HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: serde_json::json!({ "command": command }),
        }
    }

    #[test]
    fn decide_empty_command_denied() {
        // decide() denies empty commands for subagent bash
        let input = make_bash_input("");
        let command = input.tool_input.get("command").and_then(|v| v.as_str()).unwrap_or("");
        assert!(command.trim().is_empty());
    }

    #[test]
    fn decide_command_extraction_works() {
        let input = make_bash_input("cat <<'RECORD' | my_writer\n{}\nRECORD");
        let command = input.tool_input.get("command").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(command, "cat <<'RECORD' | my_writer\n{}\nRECORD");
    }
}
