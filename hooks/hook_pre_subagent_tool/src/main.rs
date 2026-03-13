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

    decide_with_map(input, &tool_map)
}

/// Core decision logic, separated from CLI arg parsing for testability.
fn decide_with_map(
    input: &HookInput,
    tool_map: &HashMap<String, Vec<String>>,
) -> HookDecision {
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

    let target = match input.target_path() {
        Some(t) => t,
        None => return HookDecision::Allow, // Grep/Glob with no path = cwd
    };

    // Reject path traversal before prefix check — ".." can escape any prefix
    if target.contains("..") {
        return HookDecision::Deny {
            category: "path".into(),
            event: format!("blocked {} path traversal attempt", tool_name),
            reason: format!(
                "'{}' contains path traversal (..) which is not allowed.\n\
                 Use absolute paths without '..' components.",
                target
            ),
        };
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool_map() -> HashMap<String, Vec<String>> {
        let mut map = HashMap::new();
        map.insert(
            "Read".to_string(),
            vec!["/schemas/".to_string(), "/docs/".to_string()],
        );
        map.insert(
            "Write".to_string(),
            vec!["/output/".to_string()],
        );
        map.insert(
            "Grep".to_string(),
            vec!["/schemas/".to_string()],
        );
        map
    }

    fn make_input(tool: &str, file_path: &str) -> HookInput {
        HookInput {
            tool_name: Some(tool.to_string()),
            tool_input: serde_json::json!({ "file_path": file_path }),
        }
    }

    fn make_input_with_path(tool: &str, path: &str) -> HookInput {
        HookInput {
            tool_name: Some(tool.to_string()),
            tool_input: serde_json::json!({ "path": path }),
        }
    }

    fn make_input_no_path(tool: &str) -> HookInput {
        HookInput {
            tool_name: Some(tool.to_string()),
            tool_input: serde_json::json!({}),
        }
    }

    fn make_input_no_tool() -> HookInput {
        HookInput {
            tool_name: None,
            tool_input: serde_json::json!({}),
        }
    }

    // ── Path under allowed prefix → Allow ─────────────────────────

    #[test]
    fn read_under_schemas_allowed() {
        let map = make_tool_map();
        let input = make_input("Read", "/schemas/agent.yaml");
        match decide_with_map(&input, &map) {
            HookDecision::Allow => {} // correct
            _ => panic!("Read under /schemas/ must Allow"),
        }
    }

    #[test]
    fn read_under_docs_allowed() {
        let map = make_tool_map();
        let input = make_input("Read", "/docs/readme.md");
        match decide_with_map(&input, &map) {
            HookDecision::Allow => {} // correct
            _ => panic!("Read under /docs/ must Allow"),
        }
    }

    #[test]
    fn write_under_output_allowed() {
        let map = make_tool_map();
        let input = make_input("Write", "/output/result.json");
        match decide_with_map(&input, &map) {
            HookDecision::Allow => {} // correct
            _ => panic!("Write under /output/ must Allow"),
        }
    }

    // ── Path outside allowed prefix → Deny ────────────────────────

    #[test]
    fn read_outside_prefix_denied() {
        let map = make_tool_map();
        let input = make_input("Read", "/etc/passwd");
        match decide_with_map(&input, &map) {
            HookDecision::Deny { category, reason, .. } => {
                assert_eq!(category, "path");
                assert!(reason.contains("/etc/passwd"));
                assert!(reason.contains("Read"));
            }
            _ => panic!("Read outside allowed paths must Deny"),
        }
    }

    #[test]
    fn write_outside_prefix_denied() {
        let map = make_tool_map();
        let input = make_input("Write", "/schemas/hack.yaml");
        match decide_with_map(&input, &map) {
            HookDecision::Deny { category, .. } => {
                assert_eq!(category, "path");
            }
            _ => panic!("Write to /schemas/ when only /output/ allowed must Deny"),
        }
    }

    // ── Tool not in allowed list → Deny ───────────────────────────

    #[test]
    fn unknown_tool_denied() {
        let map = make_tool_map();
        let input = make_input("Edit", "/schemas/file.yaml");
        match decide_with_map(&input, &map) {
            HookDecision::Deny { category, reason, .. } => {
                assert_eq!(category, "path");
                assert!(reason.contains("Edit"));
                assert!(reason.contains("not in the allowed tool list"));
            }
            _ => panic!("Unknown tool must Deny"),
        }
    }

    // ── No tool name → Allow ──────────────────────────────────────

    #[test]
    fn no_tool_name_allowed() {
        let map = make_tool_map();
        let input = make_input_no_tool();
        match decide_with_map(&input, &map) {
            HookDecision::Allow => {} // correct
            _ => panic!("No tool name must Allow"),
        }
    }

    // ── No path in input → Allow ──────────────────────────────────

    #[test]
    fn no_path_in_input_allowed() {
        let map = make_tool_map();
        let input = make_input_no_path("Grep");
        match decide_with_map(&input, &map) {
            HookDecision::Allow => {} // correct — Grep/Glob with no path = cwd
            _ => panic!("No path in input must Allow"),
        }
    }

    // ── "path" field (Grep/Glob) works ────────────────────────────

    #[test]
    fn grep_with_path_field_under_prefix_allowed() {
        let map = make_tool_map();
        let input = make_input_with_path("Grep", "/schemas/nested/file.yaml");
        match decide_with_map(&input, &map) {
            HookDecision::Allow => {} // correct
            _ => panic!("Grep with path under /schemas/ must Allow"),
        }
    }

    #[test]
    fn grep_with_path_field_outside_prefix_denied() {
        let map = make_tool_map();
        let input = make_input_with_path("Grep", "/etc/shadow");
        match decide_with_map(&input, &map) {
            HookDecision::Deny { .. } => {} // correct
            _ => panic!("Grep with path outside prefix must Deny"),
        }
    }

    // ── Empty tool map → Deny ─────────────────────────────────────
    // (This is tested via the `decide()` function path, but we can test
    // the map-based logic with an empty map too)

    #[test]
    fn empty_map_still_denies_unknown_tools() {
        let map = HashMap::new();
        let input = make_input("Read", "/schemas/file.yaml");
        match decide_with_map(&input, &map) {
            HookDecision::Deny { .. } => {} // correct
            _ => panic!("Empty map must Deny tools"),
        }
    }

    // ── Multiple prefixes for same tool ───────────────────────────

    #[test]
    fn multiple_prefixes_second_matches() {
        let map = make_tool_map();
        let input = make_input("Read", "/docs/guide.md");
        match decide_with_map(&input, &map) {
            HookDecision::Allow => {} // correct — /docs/ is second prefix for Read
            _ => panic!("Second prefix for Read must also Allow"),
        }
    }

    // ── Path traversal attack ─────────────────────────────────────

    #[test]
    fn path_traversal_denied() {
        let map = make_tool_map();
        let input = make_input("Read", "/schemas/../../etc/passwd");
        match decide_with_map(&input, &map) {
            HookDecision::Deny { category, .. } => {
                assert_eq!(category, "path");
            }
            _ => panic!("Path traversal with .. must be denied"),
        }
    }

    #[test]
    fn path_traversal_mid_path_denied() {
        let map = make_tool_map();
        let input = make_input("Read", "/schemas/foo/../../../etc/shadow");
        match decide_with_map(&input, &map) {
            HookDecision::Deny { category, .. } => {
                assert_eq!(category, "path");
            }
            _ => panic!("Path traversal with .. must be denied"),
        }
    }

    // ── file_path takes precedence over path ──────────────────────

    #[test]
    fn file_path_field_preferred_over_path() {
        let input = HookInput {
            tool_name: Some("Read".to_string()),
            tool_input: serde_json::json!({
                "file_path": "/schemas/a.yaml",
                "path": "/etc/shadow"
            }),
        };
        let map = make_tool_map();
        // file_path is checked first, so /schemas/a.yaml should match
        match decide_with_map(&input, &map) {
            HookDecision::Allow => {} // correct — file_path matched first
            _ => panic!("file_path must take precedence over path"),
        }
    }
}
