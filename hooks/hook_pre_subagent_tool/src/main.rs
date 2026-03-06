//! PreToolUse hook: validate file tool paths against per-tool allowed prefixes.
//!
//! Usage (in agent frontmatter):
//!     hook_pre_subagent_tool Read=/schemas/,/docs/ Grep=/schemas/ Write=/output/
//!
//! Each CLI arg is Tool=path1,path2 — the tool name and its allowed path prefixes.
//! Reads tool_name from stdin JSON, looks up its allowed prefixes, validates
//! the target path from tool_input.file_path or tool_input.path.

use std::collections::HashMap;
use std::process::ExitCode;

use hook_io::{HookDecision, HookInput};

fn main() -> ExitCode {
    hook_io::run_hook(decide)
}

fn parse_tool_paths() -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for arg in std::env::args().skip(1) {
        if let Some((tool, paths_str)) = arg.split_once('=') {
            let paths: Vec<String> = paths_str
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            map.entry(tool.to_string()).or_default().extend(paths);
        }
    }
    map
}

fn decide(input: &HookInput) -> HookDecision {
    let tool_map = parse_tool_paths();

    if tool_map.is_empty() {
        return HookDecision::Deny {
            category: "config".into(),
            event: "no tool path mappings configured".into(),
            reason: "No tool path mappings configured.".into(),
        };
    }

    let tool_name = match &input.tool_name {
        Some(name) => name.as_str(),
        None => return HookDecision::Allow,
    };

    let allowed = match tool_map.get(tool_name) {
        Some(paths) => paths,
        None => {
            return HookDecision::Deny {
                category: "path".into(),
                event: format!("tool '{}' has no path grants", tool_name),
                reason: format!("Tool '{}' is not in the allowed tool list.", tool_name),
            };
        }
    };

    let tool_input = &input.tool_input;

    // Read/Write/Edit use file_path, Grep/Glob use path
    let target = tool_input
        .get("file_path")
        .and_then(|v| v.as_str())
        .or_else(|| tool_input.get("path").and_then(|v| v.as_str()));

    let target = match target {
        Some(t) => t,
        None => return HookDecision::Allow, // Grep/Glob with no path = cwd
    };

    for prefix in allowed {
        if target.starts_with(prefix.as_str()) {
            return HookDecision::Allow;
        }
    }

    let paths_list: Vec<String> = allowed.iter().map(|p| format!("  {}", p)).collect();
    HookDecision::Deny {
        category: "path".into(),
        event: format!("blocked {} access to '{}'", tool_name, target),
        reason: format!(
            "'{}' is outside {}'s allowed paths.\nYou may only access:\n{}\n\
             Adjust your path to use one of these locations.",
            target,
            tool_name,
            paths_list.join("\n")
        ),
    }
}
