//! PreToolUse hook: validate Read/Write/Edit/Grep/Glob paths against allowed prefixes.
//!
//! Replaces validate_read.py and validate_write.py.
//!
//! Usage (in agent frontmatter):
//!     hook_intercept_subagent_tool /abs/path1/ /abs/path2/ ...
//!
//! Reads Claude Code hook JSON from stdin. Extracts the target path from
//! tool_input.file_path (Read/Write/Edit) or tool_input.path (Grep/Glob).
//! Grep/Glob with no path defaults to cwd — allowed.

use std::process::ExitCode;

use hook_io::{HookDecision, HookInput};

fn main() -> ExitCode {
    hook_io::run_hook(decide)
}

fn decide(input: &HookInput) -> HookDecision {
    let allowed_prefixes: Vec<String> = std::env::args().skip(1).collect();

    if allowed_prefixes.is_empty() {
        return HookDecision::Deny {
            category: "config".into(),
            event: "no allowed prefixes configured".into(),
            reason: "No allowed prefixes configured.".into(),
        };
    }

    let tool_input = &input.tool_input;

    // Read/Write/Edit use file_path, Grep/Glob use path
    let target = tool_input
        .get("file_path")
        .and_then(|v| v.as_str())
        .or_else(|| tool_input.get("path").and_then(|v| v.as_str()));

    let target = match target {
        Some(t) => t,
        None => {
            // Grep/Glob with no path default to cwd — allow
            return HookDecision::Allow;
        }
    };

    for prefix in &allowed_prefixes {
        if target.starts_with(prefix.as_str()) {
            return HookDecision::Allow;
        }
    }

    let paths_list: Vec<String> = allowed_prefixes.iter().map(|p| format!("  {}", p)).collect();
    let reason = format!(
        "'{}' is outside your allowed paths.\nYou may only access:\n{}\nAdjust your path to use one of these locations.",
        target,
        paths_list.join("\n")
    );

    HookDecision::Deny {
        category: "path".into(),
        event: format!("blocked access to '{}'", target),
        reason,
    }
}
