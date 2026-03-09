//! JSON-level exchange diffing for bifrost.
//!
//! Compares consecutive Claude API exchanges and finds additions:
//! new messages, new system blocks, new tool definitions.
//!
//! Pure functions, no I/O.

use serde_json::Value;
use socket_emit::{Datagram, DatagramKind, Priority};

/// The three components extracted from a Claude API exchange.
#[derive(Debug, Clone)]
pub struct Exchange {
    pub messages: Vec<Value>,
    pub system: Vec<Value>,
    pub tools: Vec<Value>,
}

/// Extract messages, system, and tools from a raw exchange JSON.
/// Everything else is discarded.
pub fn split_exchange(value: &Value) -> Exchange {
    let messages = value
        .get("messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let system = match value.get("system") {
        Some(Value::Array(arr)) => arr.clone(),
        Some(Value::String(s)) => {
            vec![serde_json::json!({"type": "text", "text": s})]
        }
        _ => Vec::new(),
    };

    let tools = value
        .get("tools")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    Exchange {
        messages,
        system,
        tools,
    }
}

/// Find new messages: the tail of curr after the common prefix with prev.
/// If no common prefix exists, all of curr is returned as "new."
pub fn diff_messages(prev: &[Value], curr: &[Value]) -> Vec<Value> {
    let common = prev
        .iter()
        .zip(curr.iter())
        .take_while(|(a, b)| a == b)
        .count();

    curr[common..].to_vec()
}

/// Find system blocks in curr whose text content wasn't in prev.
fn system_block_text(block: &Value) -> Option<&str> {
    block.get("text").and_then(|v| v.as_str())
}

pub fn diff_system_blocks(prev: &[Value], curr: &[Value]) -> Vec<Value> {
    let prev_texts: Vec<&str> = prev.iter().filter_map(system_block_text).collect();

    curr.iter()
        .filter(|block| {
            match system_block_text(block) {
                Some(text) => !prev_texts.contains(&text),
                None => false, // blocks without text are not interesting
            }
        })
        .cloned()
        .collect()
}

/// Find tools in curr whose name wasn't in prev.
fn tool_name(tool: &Value) -> Option<&str> {
    tool.get("name").and_then(|v| v.as_str())
}

pub fn diff_tools(prev: &[Value], curr: &[Value]) -> Vec<Value> {
    let prev_names: Vec<&str> = prev.iter().filter_map(tool_name).collect();

    curr.iter()
        .filter(|tool| match tool_name(tool) {
            Some(name) => !prev_names.contains(&name),
            None => false,
        })
        .cloned()
        .collect()
}

/// Determine datagram priority from diff results.
/// Normal if any system or tool additions, Low otherwise.
pub fn classify_priority(new_system: &[Value], new_tools: &[Value]) -> Priority {
    if new_system.is_empty() && new_tools.is_empty() {
        Priority::Low
    } else {
        Priority::Normal
    }
}

/// Construct a datagram from diff results.
pub fn build_datagram(
    new_messages: &[Value],
    new_system: &[Value],
    new_tools: &[Value],
    workspace: &str,
    priority: Priority,
) -> Datagram {
    let mut payload = serde_json::Map::new();

    if !new_messages.is_empty() {
        payload.insert("messages".into(), Value::Array(new_messages.to_vec()));
    }
    if !new_system.is_empty() {
        payload.insert("system".into(), Value::Array(new_system.to_vec()));
    }
    if !new_tools.is_empty() {
        payload.insert("tools".into(), Value::Array(new_tools.to_vec()));
    }

    Datagram {
        timestamp: socket_emit::now(),
        source: "intercept".into(),
        kind: DatagramKind::Exchange,
        priority,
        workspace: workspace.into(),
        detail: None,
        speech: None,
        payload: Some(Value::Object(payload)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- split_exchange ---

    #[test]
    fn split_extracts_three_arrays() {
        let exchange = json!({
            "model": "claude-opus-4-6",
            "max_tokens": 16000,
            "messages": [{"role": "user", "content": "hello"}],
            "system": [{"type": "text", "text": "You are helpful."}],
            "tools": [{"name": "Bash", "input_schema": {"type": "object"}}],
            "metadata": {"user_id": "session_abc123"}
        });

        let split = split_exchange(&exchange);
        assert_eq!(split.messages.len(), 1);
        assert_eq!(split.system.len(), 1);
        assert_eq!(split.tools.len(), 1);
    }

    #[test]
    fn split_handles_system_as_string() {
        let exchange = json!({
            "messages": [],
            "system": "You are a helpful assistant.",
            "tools": []
        });

        let split = split_exchange(&exchange);
        assert_eq!(split.system.len(), 1);
        assert_eq!(
            split.system[0]["text"],
            "You are a helpful assistant."
        );
    }

    #[test]
    fn split_handles_missing_fields() {
        let exchange = json!({"model": "claude-opus-4-6"});
        let split = split_exchange(&exchange);
        assert!(split.messages.is_empty());
        assert!(split.system.is_empty());
        assert!(split.tools.is_empty());
    }

    // --- diff_messages ---

    #[test]
    fn messages_normal_progression() {
        let prev = vec![
            json!({"role": "user", "content": "hello"}),
            json!({"role": "assistant", "content": "hi"}),
        ];
        let curr = vec![
            json!({"role": "user", "content": "hello"}),
            json!({"role": "assistant", "content": "hi"}),
            json!({"role": "user", "content": "how are you?"}),
        ];

        let new_msgs = diff_messages(&prev, &curr);
        assert_eq!(new_msgs.len(), 1);
        assert_eq!(new_msgs[0]["content"], "how are you?");
    }

    #[test]
    fn messages_no_common_prefix() {
        let prev = vec![
            json!({"role": "user", "content": "old conversation"}),
            json!({"role": "assistant", "content": "old response"}),
        ];
        let curr = vec![
            json!({"role": "user", "content": "compacted summary"}),
        ];

        let new_msgs = diff_messages(&prev, &curr);
        assert_eq!(new_msgs.len(), 1);
        assert_eq!(new_msgs[0]["content"], "compacted summary");
    }

    #[test]
    fn messages_empty_previous() {
        let prev: Vec<Value> = vec![];
        let curr = vec![
            json!({"role": "user", "content": "first message"}),
        ];

        let new_msgs = diff_messages(&prev, &curr);
        assert_eq!(new_msgs.len(), 1);
        assert_eq!(new_msgs[0]["content"], "first message");
    }

    #[test]
    fn messages_identical() {
        let msgs = vec![
            json!({"role": "user", "content": "hello"}),
        ];

        let new_msgs = diff_messages(&msgs, &msgs);
        assert!(new_msgs.is_empty());
    }

    // --- diff_system_blocks ---

    #[test]
    fn system_new_block_added() {
        let prev = vec![
            json!({"type": "text", "text": "You are helpful."}),
        ];
        let curr = vec![
            json!({"type": "text", "text": "You are helpful."}),
            json!({"type": "text", "text": "New instruction injected."}),
        ];

        let new_blocks = diff_system_blocks(&prev, &curr);
        assert_eq!(new_blocks.len(), 1);
        assert_eq!(new_blocks[0]["text"], "New instruction injected.");
    }

    #[test]
    fn system_no_changes() {
        let blocks = vec![
            json!({"type": "text", "text": "You are helpful."}),
        ];

        let new_blocks = diff_system_blocks(&blocks, &blocks);
        assert!(new_blocks.is_empty());
    }

    #[test]
    fn system_cache_control_ignored() {
        let prev = vec![
            json!({"type": "text", "text": "Same content."}),
        ];
        let curr = vec![
            json!({"type": "text", "text": "Same content.", "cache_control": {"type": "ephemeral"}}),
        ];

        let new_blocks = diff_system_blocks(&prev, &curr);
        assert!(new_blocks.is_empty());
    }

    #[test]
    fn system_empty_to_populated() {
        let prev: Vec<Value> = vec![];
        let curr = vec![
            json!({"type": "text", "text": "System prompt appeared."}),
        ];

        let new_blocks = diff_system_blocks(&prev, &curr);
        assert_eq!(new_blocks.len(), 1);
    }

    // --- diff_tools ---

    #[test]
    fn tools_new_tool_appeared() {
        let prev = vec![
            json!({"name": "Bash", "description": "Run commands"}),
        ];
        let curr = vec![
            json!({"name": "Bash", "description": "Run commands"}),
            json!({"name": "Read", "description": "Read files"}),
        ];

        let new_tools = diff_tools(&prev, &curr);
        assert_eq!(new_tools.len(), 1);
        assert_eq!(new_tools[0]["name"], "Read");
    }

    #[test]
    fn tools_no_changes() {
        let tools = vec![
            json!({"name": "Bash", "description": "Run commands"}),
        ];

        let new_tools = diff_tools(&tools, &tools);
        assert!(new_tools.is_empty());
    }

    #[test]
    fn tools_empty_to_populated() {
        let prev: Vec<Value> = vec![];
        let curr = vec![
            json!({"name": "Bash", "description": "Run commands"}),
            json!({"name": "Read", "description": "Read files"}),
        ];

        let new_tools = diff_tools(&prev, &curr);
        assert_eq!(new_tools.len(), 2);
    }

    // --- classify_priority ---

    #[test]
    fn priority_low_when_no_additions() {
        assert_eq!(classify_priority(&[], &[]), Priority::Low);
    }

    #[test]
    fn priority_normal_when_system_additions() {
        let system = vec![json!({"type": "text", "text": "new"})];
        assert_eq!(classify_priority(&system, &[]), Priority::Normal);
    }

    #[test]
    fn priority_normal_when_tool_additions() {
        let tools = vec![json!({"name": "NewTool"})];
        assert_eq!(classify_priority(&[], &tools), Priority::Normal);
    }

    // --- build_datagram ---

    #[test]
    fn datagram_has_correct_structure() {
        let msgs = vec![json!({"role": "user", "content": "hi"})];
        let system: Vec<Value> = vec![];
        let tools: Vec<Value> = vec![];

        let dg = build_datagram(&msgs, &system, &tools, "odinn", Priority::Low);

        assert_eq!(dg.source, "intercept");
        assert_eq!(dg.kind, DatagramKind::Exchange);
        assert_eq!(dg.priority, Priority::Low);
        assert_eq!(dg.workspace, "odinn");

        let payload = dg.payload.unwrap();
        assert!(payload.get("messages").is_some());
        assert!(payload.get("system").is_none()); // empty, not included
        assert!(payload.get("tools").is_none());
    }

    #[test]
    fn datagram_includes_system_when_present() {
        let msgs = vec![json!({"role": "user", "content": "hi"})];
        let system = vec![json!({"type": "text", "text": "injected"})];
        let tools: Vec<Value> = vec![];

        let dg = build_datagram(&msgs, &system, &tools, "odinn", Priority::Normal);

        let payload = dg.payload.unwrap();
        assert!(payload.get("system").is_some());
        assert_eq!(payload["system"].as_array().unwrap().len(), 1);
    }
}
