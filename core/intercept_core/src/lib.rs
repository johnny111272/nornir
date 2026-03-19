//! Pure classification logic for Claude API traffic intercept.
//!
//! No I/O, no side effects. Determines what kind of exchange a request
//! represents so that callers can route it to the appropriate log file.

use serde_json::Value;

// =============================================================================
// Types
// =============================================================================

/// Classification of a Claude API exchange by its tool composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeKind {
    /// Main agent exchange — has Task tool + multiple tools.
    Main,
    /// Compaction trigger — exactly one tool (Read).
    Compaction,
    /// Subagent exchange — has tools but not the Main pattern.
    Subagent,
}

// =============================================================================
// Classification
// =============================================================================

/// Check if the tools array contains a tool with the given name.
pub fn has_tool(value: &Value, name: &str) -> bool {
    value
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .any(|tool| tool.get("name").and_then(|n| n.as_str()) == Some(name))
        })
        .unwrap_or(false)
}

/// Count tools in the request.
pub fn tool_count(value: &Value) -> usize {
    value
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|arr| arr.len())
        .unwrap_or(0)
}

/// Classify an exchange by tool composition.
///
/// 1. No tools → None (skip — utility calls, title generation, etc.)
/// 2. Exactly one tool = Read → Compaction
/// 3. Has any main-agent-exclusive tool → Main
///    - Task: old CC format (claude-opus-4-6 with 28-tool set)
///    - ToolSearch: new CC format (deferred tool loading, main agent only)
///    - EnterPlanMode / ExitPlanMode: plan mode tools, main agent only
/// 4. Everything else → Subagent
pub fn classify_exchange(value: &Value) -> Option<ExchangeKind> {
    let tools = tool_count(value);
    if tools == 0 {
        return None;
    }
    if tools == 1 && has_tool(value, "Read") {
        return Some(ExchangeKind::Compaction);
    }
    if has_tool(value, "Task")
        || has_tool(value, "ToolSearch")
        || has_tool(value, "EnterPlanMode")
        || has_tool(value, "ExitPlanMode")
    {
        return Some(ExchangeKind::Main);
    }
    Some(ExchangeKind::Subagent)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn has_tool_found() {
        let value = json!({"tools": [{"name": "Bash"}, {"name": "Read"}, {"name": "Task"}]});
        assert!(has_tool(&value, "Task"));
        assert!(has_tool(&value, "Read"));
    }

    #[test]
    fn has_tool_not_found() {
        let value = json!({"tools": [{"name": "Bash"}, {"name": "Read"}]});
        assert!(!has_tool(&value, "Task"));
    }

    #[test]
    fn has_tool_no_tools_key() {
        let value = json!({"system": []});
        assert!(!has_tool(&value, "Read"));
    }

    #[test]
    fn has_tool_empty_array() {
        let value = json!({"tools": []});
        assert!(!has_tool(&value, "Read"));
    }

    #[test]
    fn tool_count_with_tools() {
        let value = json!({"tools": [{"name": "A"}, {"name": "B"}, {"name": "C"}]});
        assert_eq!(tool_count(&value), 3);
    }

    #[test]
    fn tool_count_no_tools_key() {
        let value = json!({"system": []});
        assert_eq!(tool_count(&value), 0);
    }

    #[test]
    fn tool_count_empty_array() {
        let value = json!({"tools": []});
        assert_eq!(tool_count(&value), 0);
    }

    #[test]
    fn classify_main_has_task_and_multiple_tools() {
        let value = json!({
            "tools": [{"name": "Bash"}, {"name": "Read"}, {"name": "Write"}, {"name": "Task"}]
        });
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Main));
    }

    #[test]
    fn classify_compaction_single_read() {
        let value = json!({"tools": [{"name": "Read"}]});
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Compaction));
    }

    #[test]
    fn classify_no_tools_skipped() {
        let value = json!({"tools": []});
        assert_eq!(classify_exchange(&value), None);
    }

    #[test]
    fn classify_subagent_many_tools_no_task() {
        let value = json!({
            "tools": [{"name": "Bash"}, {"name": "Read"}, {"name": "Write"}, {"name": "Grep"}]
        });
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Subagent));
    }

    #[test]
    fn classify_web_search_only_becomes_subagent() {
        let value = json!({"tools": [{"name": "web_search"}]});
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Subagent));
    }

    #[test]
    fn classify_single_non_read_tool_subagent() {
        let value = json!({"tools": [{"name": "Bash"}]});
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Subagent));
    }

    #[test]
    fn classify_task_only_becomes_main() {
        let value = json!({"tools": [{"name": "Task"}]});
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Main));
    }

    #[test]
    fn classify_tool_search_only_becomes_main() {
        let value = json!({"tools": [{"name": "ToolSearch"}]});
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Main));
    }

    #[test]
    fn classify_tool_search_with_bash_becomes_main() {
        let value = json!({"tools": [{"name": "Bash"}, {"name": "ToolSearch"}]});
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Main));
    }

    #[test]
    fn classify_enter_plan_mode_becomes_main() {
        let value = json!({"tools": [{"name": "Bash"}, {"name": "Read"}, {"name": "EnterPlanMode"}]});
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Main));
    }

    #[test]
    fn classify_exit_plan_mode_becomes_main() {
        let value = json!({"tools": [{"name": "Bash"}, {"name": "ExitPlanMode"}, {"name": "ToolSearch"}]});
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Main));
    }

    #[test]
    fn classify_mcp_tools_only_becomes_subagent() {
        let value = json!({"tools": [
            {"name": "Read"},
            {"name": "mcp__context7__resolve-library-id"},
            {"name": "mcp__context7__query-docs"}
        ]});
        assert_eq!(classify_exchange(&value), Some(ExchangeKind::Subagent));
    }

    #[test]
    fn classify_no_tools_key_skipped() {
        let value = json!({"system": []});
        assert_eq!(classify_exchange(&value), None);
    }
}
