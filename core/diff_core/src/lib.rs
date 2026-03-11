//! JSON-level exchange diffing for bifrost.
//!
//! Compares consecutive Claude API exchanges and finds additions:
//! new messages, new system blocks, new tool definitions.
//!
//! Pure functions, no I/O.

use serde_json::Value;
use datagram::{Datagram, DatagramKind, Priority};

/// The three components extracted from a Claude API exchange.
#[derive(Debug, Clone)]
pub struct Exchange {
    pub messages: Vec<Value>,
    pub system: Vec<Value>,
    pub tools: Vec<Value>,
}

/// Extract messages, system, and tools from a raw exchange JSON.
/// Strips transient fields (cache_control, signature) that cause false diffs.
/// All content blocks are preserved — restructuring happens at datagram emission.
pub fn split_exchange(value: &Value) -> Exchange {
    let raw_messages = value
        .get("messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let messages: Vec<Value> = raw_messages.iter().map(strip_transient).collect();

    let system = match value.get("system") {
        Some(Value::Array(arr)) => arr.iter().map(strip_transient).collect(),
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

// =============================================================================
// Diff — find additions between consecutive exchanges
// =============================================================================

/// Find new messages: the tail of curr after the common prefix with prev.
pub fn diff_messages(prev: &[Value], curr: &[Value]) -> Vec<Value> {
    let common = prev
        .iter()
        .zip(curr.iter())
        .take_while(|(a, b)| a == b)
        .count();

    curr[common..].to_vec()
}

/// Find system blocks in curr whose text content wasn't in prev.
pub fn diff_system_blocks(prev: &[Value], curr: &[Value]) -> Vec<Value> {
    let prev_texts: Vec<&str> = prev.iter().filter_map(system_block_text).collect();

    curr.iter()
        .filter(|block| {
            match system_block_text(block) {
                Some(text) => !prev_texts.contains(&text),
                None => false,
            }
        })
        .cloned()
        .collect()
}

/// Find tools in curr whose name wasn't in prev.
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

// =============================================================================
// Transient field stripping — diff correctness only
// =============================================================================

/// Strip `cache_control` and `signature` from an object and its content blocks.
/// These fields shift between exchanges without semantic change.
fn strip_transient(value: &Value) -> Value {
    let mut value = value.clone();
    remove_transient_fields(&mut value);

    if let Some(content) = value.get_mut("content").and_then(|c| c.as_array_mut()) {
        for block in content.iter_mut() {
            remove_transient_fields(block);
        }
    }

    value
}

fn remove_transient_fields(value: &mut Value) {
    if let Some(obj) = value.as_object_mut() {
        obj.remove("signature");
        obj.remove("cache_control");
    }
}

// =============================================================================
// Restructure — flatten API format to semantic labels for datagram payload
// =============================================================================

/// Restructure raw API messages into a single semantic object.
///
/// Flattens the nested role/content-block structure into labeled fields.
/// Multiple values for the same label get concatenated (strings with "\n\n",
/// objects/arrays collected into an array).
///
/// Labels: user, assistant, thinking, system, tool_return,
/// file_read, file_write, file_edit, file_search, text_search,
/// web_search, web_fetch, shell, agent_dispatch, tool_use,
/// ask_question, start_plan, finish_plan.
pub fn restructure_messages(messages: &[Value]) -> Value {
    let mut map = serde_json::Map::new();

    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        match msg.get("content") {
            Some(Value::Array(blocks)) => {
                for block in blocks {
                    if let Some((label, value)) = restructure_block(block, role) {
                        merge_into(&mut map, &label, value);
                    }
                }
            }
            Some(Value::String(s)) => {
                let label = if role == "user" { "user" } else { "assistant" };
                merge_into(&mut map, label, Value::String(s.clone()));
            }
            _ => {}
        }
    }

    Value::Object(map)
}

/// Merge a value into the map under the given label.
/// Strings: concatenate with "\n\n". Others: collect into array.
fn merge_into(map: &mut serde_json::Map<String, Value>, label: &str, value: Value) {
    match map.get_mut(label) {
        Some(existing) => match (existing, &value) {
            (Value::String(ref mut s), Value::String(new)) => {
                s.push_str("\n\n");
                s.push_str(new);
            }
            (Value::Array(ref mut arr), _) => {
                arr.push(value);
            }
            (existing_val, _) => {
                let prev = existing_val.clone();
                *existing_val = Value::Array(vec![prev, value]);
            }
        },
        None => {
            map.insert(label.to_string(), value);
        }
    }
}

/// Map a tool name to its semantic category.
fn tool_label(name: &str) -> &str {
    match name {
        "Read" => "file_read",
        "Write" => "file_write",
        "Edit" | "NotebookEdit" => "file_edit",
        "Glob" => "file_search",
        "Grep" => "text_search",
        "WebSearch" => "web_search",
        "WebFetch" => "web_fetch",
        "Bash" => "shell",
        "Task" => "agent_dispatch",
        "AskUserQuestion" => "ask_question",
        "EnterPlanMode" => "start_plan",
        "ExitPlanMode" => "finish_plan",
        _ => "tool_use",
    }
}

/// Map a single content block to a (label, value) pair.
fn restructure_block(block: &Value, role: &str) -> Option<(String, Value)> {
    let block_type = block.get("type").and_then(|t| t.as_str())?;

    match block_type {
        "text" => {
            let text = block.get("text").and_then(|t| t.as_str()).unwrap_or("");
            let label = if text.contains("<system-reminder>") {
                "system"
            } else if role == "user" {
                "user"
            } else {
                "assistant"
            };
            Some((label.into(), Value::String(text.to_string())))
        }
        "thinking" => {
            let text = block.get("thinking").and_then(|t| t.as_str()).unwrap_or("");
            Some(("thinking".into(), Value::String(text.to_string())))
        }
        "tool_use" => {
            let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
            let label = tool_label(name).to_string();
            let input = block.get("input").cloned().unwrap_or(Value::Null);
            Some((label, input))
        }
        "tool_result" => {
            let content = block.get("content").cloned().unwrap_or(Value::Null);
            Some(("tool_return".into(), content))
        }
        _ => None,
    }
}

// =============================================================================
// Exchange kind classification
// =============================================================================

/// Classify the exchange based on which semantic labels are present.
pub fn classify_exchange_kind(payload: &serde_json::Map<String, Value>, is_startup: bool) -> &'static str {
    if is_startup {
        return "startup";
    }
    if payload.contains_key("start_plan") || payload.contains_key("finish_plan") {
        "planning"
    } else if payload.contains_key("agent_dispatch") {
        "subagent"
    } else if payload.contains_key("user") {
        "conversation"
    } else {
        "tool"
    }
}

// =============================================================================
// System injection detection
// =============================================================================

/// True if a system-reminder was generated by user hooks (expected, not alertable).
fn is_user_generated_reminder(text: &str) -> bool {
    text.contains("type: syn_report")
        || text.contains("hook additional context:")
        || text.contains("<user-prompt-submit-hook>")
}

/// True if the payload contains platform-injected system-reminders
/// (not user-generated hooks). These are Anthropic's arbitrary injections
/// that can affect Claude behavior.
fn has_platform_injection(payload: &serde_json::Map<String, Value>) -> bool {
    let system_text = match payload.get("system") {
        Some(Value::String(s)) => s.as_str(),
        _ => return false,
    };

    // Each system-reminder block starts with <system-reminder>
    // They were concatenated with \n\n between them
    for chunk in system_text.split("<system-reminder>") {
        let trimmed = chunk.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !is_user_generated_reminder(trimmed) {
            return true;
        }
    }

    false
}

// =============================================================================
// Datagram construction
// =============================================================================

/// Construct a datagram from diff results.
/// Messages are restructured from API format to semantic labels.
/// System blocks become `instructions`, tool diffs become `tools_added`.
pub fn build_datagram(
    new_messages: &[Value],
    new_system: &[Value],
    new_tools: &[Value],
    workspace: &str,
    priority: Priority,
    source_ref: &str,
    is_startup: bool,
) -> Datagram {
    let mut payload = if !new_messages.is_empty() {
        match restructure_messages(new_messages) {
            Value::Object(map) => map,
            _ => serde_json::Map::new(),
        }
    } else {
        serde_json::Map::new()
    };

    // System prompt block changes → concatenated text under "instructions"
    if !new_system.is_empty() {
        let texts: Vec<&str> = new_system
            .iter()
            .filter_map(system_block_text)
            .collect();
        if !texts.is_empty() {
            payload.insert("instructions".into(), Value::String(texts.join("\n\n")));
        }
    }

    // Tool definition changes → just the names
    if !new_tools.is_empty() {
        let names: Vec<Value> = new_tools
            .iter()
            .filter_map(tool_name)
            .map(|n| Value::String(n.to_string()))
            .collect();
        if !names.is_empty() {
            payload.insert("tools_added".into(), Value::Array(names));
        }
    }

    // Derived metadata
    let system_injection = has_platform_injection(&payload);
    let exchange_kind = classify_exchange_kind(&payload, is_startup);

    payload.insert("traffic_kind".into(), Value::String(exchange_kind.into()));
    payload.insert("system_injection".into(), Value::Bool(system_injection));

    payload.insert("source".into(), Value::String(source_ref.into()));

    Datagram {
        timestamp: datagram::now(),
        source: "bifrost".into(),
        kind: DatagramKind::Traffic,
        classifier: Some(exchange_kind.into()),
        priority,
        workspace: workspace.into(),
        detail: None,
        speech: None,
        payload: Some(Value::Object(payload)),
    }
}

// =============================================================================
// Internal helpers
// =============================================================================

fn system_block_text(block: &Value) -> Option<&str> {
    block.get("text").and_then(|v| v.as_str())
}

fn tool_name(tool: &Value) -> Option<&str> {
    tool.get("name").and_then(|v| v.as_str())
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
        assert_eq!(split.system[0]["text"], "You are a helpful assistant.");
    }

    #[test]
    fn split_handles_missing_fields() {
        let exchange = json!({"model": "claude-opus-4-6"});
        let split = split_exchange(&exchange);
        assert!(split.messages.is_empty());
        assert!(split.system.is_empty());
        assert!(split.tools.is_empty());
    }

    #[test]
    fn split_strips_cache_control_and_signature() {
        let exchange = json!({
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "hi", "cache_control": {"type": "ephemeral"}}
                ]},
                {"role": "assistant", "signature": "abc123", "content": [
                    {"type": "text", "text": "hello"}
                ]}
            ],
            "system": [
                {"type": "text", "text": "sys", "cache_control": {"type": "ephemeral"}}
            ]
        });

        let split = split_exchange(&exchange);
        // Message-level signature stripped
        assert!(split.messages[1].get("signature").is_none());
        // Block-level cache_control stripped
        assert!(split.messages[0]["content"][0].get("cache_control").is_none());
        // System block cache_control stripped
        assert!(split.system[0].get("cache_control").is_none());
        // Content preserved
        assert_eq!(split.messages[0]["content"][0]["text"], "hi");
    }

    #[test]
    fn split_preserves_all_block_types() {
        let exchange = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "hmm"},
                    {"type": "text", "text": "let me check"},
                    {"type": "tool_use", "id": "t1", "name": "Read", "input": {"file_path": "/foo"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": "file data"}
                ]}
            ]
        });

        let split = split_exchange(&exchange);
        assert_eq!(split.messages.len(), 2);
        let content0 = split.messages[0]["content"].as_array().unwrap();
        assert_eq!(content0.len(), 3); // thinking + text + tool_use all kept
        let content1 = split.messages[1]["content"].as_array().unwrap();
        assert_eq!(content1.len(), 1); // tool_result kept
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
    }

    #[test]
    fn messages_diff_ignores_cache_control() {
        // cache_control and signature are stripped by split_exchange before diff.
        let prev_raw = json!({
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "hello", "cache_control": {"type": "ephemeral"}}]},
                {"role": "assistant", "content": [{"type": "text", "text": "hi"}], "signature": "abc123"},
            ]
        });
        let curr_raw = json!({
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "hello"}]},
                {"role": "assistant", "content": [{"type": "text", "text": "hi"}]},
                {"role": "user", "content": [{"type": "text", "text": "new question"}]},
            ]
        });
        let prev = split_exchange(&prev_raw);
        let curr = split_exchange(&curr_raw);
        let new_msgs = diff_messages(&prev.messages, &curr.messages);
        assert_eq!(new_msgs.len(), 1);
        assert_eq!(new_msgs[0]["content"][0]["text"], "new question");
    }

    #[test]
    fn messages_identical() {
        let msgs = vec![json!({"role": "user", "content": "hello"})];
        let new_msgs = diff_messages(&msgs, &msgs);
        assert!(new_msgs.is_empty());
    }

    // --- diff_system_blocks ---

    #[test]
    fn system_new_block_added() {
        let prev = vec![json!({"type": "text", "text": "You are helpful."})];
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
        let blocks = vec![json!({"type": "text", "text": "You are helpful."})];
        let new_blocks = diff_system_blocks(&blocks, &blocks);
        assert!(new_blocks.is_empty());
    }

    #[test]
    fn system_cache_control_ignored() {
        let prev = vec![json!({"type": "text", "text": "Same content."})];
        let curr = vec![json!({"type": "text", "text": "Same content.", "cache_control": {"type": "ephemeral"}})];

        // split_exchange strips cache_control, so these compare equal
        let prev_split = split_exchange(&json!({"messages": [], "system": prev}));
        let curr_split = split_exchange(&json!({"messages": [], "system": curr}));
        let new_blocks = diff_system_blocks(&prev_split.system, &curr_split.system);
        assert!(new_blocks.is_empty());
    }

    #[test]
    fn system_empty_to_populated() {
        let prev: Vec<Value> = vec![];
        let curr = vec![json!({"type": "text", "text": "System prompt appeared."})];

        let new_blocks = diff_system_blocks(&prev, &curr);
        assert_eq!(new_blocks.len(), 1);
    }

    // --- diff_tools ---

    #[test]
    fn tools_new_tool_appeared() {
        let prev = vec![json!({"name": "Bash", "description": "Run commands"})];
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
        let tools = vec![json!({"name": "Bash", "description": "Run commands"})];
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

    // --- restructure_messages ---

    #[test]
    fn restructure_conversation_turn() {
        let messages = vec![
            json!({"role": "user", "content": [{"type": "text", "text": "hello"}]}),
            json!({"role": "assistant", "content": [
                {"type": "thinking", "thinking": "user said hello"},
                {"type": "text", "text": "hi there"}
            ]}),
        ];

        let result = restructure_messages(&messages);
        assert_eq!(result["user"], "hello");
        assert_eq!(result["thinking"], "user said hello");
        assert_eq!(result["assistant"], "hi there");
    }

    #[test]
    fn restructure_tool_use_turn() {
        let messages = vec![
            json!({"role": "assistant", "content": [
                {"type": "thinking", "thinking": "need to read file"},
                {"type": "tool_use", "id": "t1", "name": "Read", "input": {"file_path": "/foo"}}
            ]}),
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": "file contents here"}
            ]}),
        ];

        let result = restructure_messages(&messages);
        assert_eq!(result["thinking"], "need to read file");
        assert_eq!(result["file_read"]["file_path"], "/foo");
        assert_eq!(result["tool_return"], "file contents here");
    }

    #[test]
    fn restructure_semantic_tool_labels() {
        let messages = vec![
            json!({"role": "assistant", "content": [
                {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}},
                {"type": "tool_use", "id": "t2", "name": "Glob", "input": {"pattern": "*.rs"}},
                {"type": "tool_use", "id": "t3", "name": "Grep", "input": {"pattern": "fn main"}},
                {"type": "tool_use", "id": "t4", "name": "Task", "input": {"prompt": "explore"}},
                {"type": "tool_use", "id": "t5", "name": "AskUserQuestion", "input": {"questions": []}},
                {"type": "tool_use", "id": "t6", "name": "EnterPlanMode", "input": {}},
                {"type": "tool_use", "id": "t7", "name": "TaskUpdate", "input": {"taskId": "1"}}
            ]}),
        ];

        let result = restructure_messages(&messages);
        assert!(result.get("shell").is_some());
        assert!(result.get("file_search").is_some());
        assert!(result.get("text_search").is_some());
        assert!(result.get("agent_dispatch").is_some());
        assert!(result.get("ask_question").is_some());
        assert!(result.get("start_plan").is_some());
        assert!(result.get("tool_use").is_some()); // unmapped → generic
    }

    #[test]
    fn restructure_all_returns_are_tool_return() {
        let messages = vec![
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": "bash output"},
                {"type": "tool_result", "tool_use_id": "task_1", "content": "agent findings"},
            ]}),
        ];

        let result = restructure_messages(&messages);
        // Multiple string tool_returns concatenated
        let returns = result["tool_return"].as_str().unwrap();
        assert!(returns.contains("bash output"));
        assert!(returns.contains("agent findings"));
    }

    #[test]
    fn restructure_system_concatenated() {
        let messages = vec![
            json!({"role": "user", "content": [
                {"type": "text", "text": "<system-reminder>\nFirst reminder\n</system-reminder>"},
                {"type": "text", "text": "<system-reminder>\nSecond reminder\n</system-reminder>"},
                {"type": "text", "text": "my actual question"}
            ]}),
        ];

        let result = restructure_messages(&messages);
        let system = result["system"].as_str().unwrap();
        assert!(system.contains("First reminder"));
        assert!(system.contains("Second reminder"));
        assert!(system.contains("\n\n")); // concatenated with blank line
        assert_eq!(result["user"], "my actual question");
    }

    #[test]
    fn restructure_string_content() {
        let messages = vec![
            json!({"role": "user", "content": "plain string"}),
        ];

        let result = restructure_messages(&messages);
        assert_eq!(result["user"], "plain string");
    }

    #[test]
    fn restructure_multiple_user_texts_concatenated() {
        let messages = vec![
            json!({"role": "user", "content": [{"type": "text", "text": "first"}]}),
            json!({"role": "user", "content": [{"type": "text", "text": "second"}]}),
        ];

        let result = restructure_messages(&messages);
        let user = result["user"].as_str().unwrap();
        assert!(user.contains("first"));
        assert!(user.contains("second"));
        assert!(user.contains("\n\n"));
    }

    // --- build_datagram ---

    #[test]
    fn datagram_has_correct_structure() {
        let msgs = vec![json!({"role": "user", "content": [{"type": "text", "text": "hi"}]})];

        let dg = build_datagram(&msgs, &[], &[], "odinn", Priority::Low, "test.jsonl:1", false);

        assert_eq!(dg.source, "bifrost");
        assert_eq!(dg.kind, DatagramKind::Traffic);
        assert_eq!(dg.priority, Priority::Low);
        assert_eq!(dg.workspace, "odinn");

        let payload = dg.payload.unwrap();
        assert_eq!(payload["user"], "hi");
        assert_eq!(payload["traffic_kind"], "conversation");
        assert_eq!(payload["system_injection"], false);
        assert!(payload.get("tools_added").is_none());
        assert!(payload.get("instructions").is_none());
    }

    #[test]
    fn datagram_instructions_from_system_blocks() {
        let msgs = vec![json!({"role": "user", "content": [{"type": "text", "text": "hi"}]})];
        let system = vec![json!({"type": "text", "text": "New CLAUDE.md instruction"})];

        let dg = build_datagram(&msgs, &system, &[], "odinn", Priority::Normal, "test.jsonl:1", false);

        let payload = dg.payload.unwrap();
        assert_eq!(payload["instructions"], "New CLAUDE.md instruction");
        assert!(payload.get("system").is_none()); // no system-reminders in messages
    }

    #[test]
    fn datagram_tools_added_names_only() {
        let msgs = vec![json!({"role": "user", "content": [{"type": "text", "text": "hi"}]})];
        let tools = vec![
            json!({"name": "Bash", "description": "Run commands", "input_schema": {"type": "object"}}),
            json!({"name": "Read", "description": "Read files", "input_schema": {"type": "object"}}),
        ];

        let dg = build_datagram(&msgs, &[], &tools, "odinn", Priority::Normal, "test.jsonl:1", false);

        let payload = dg.payload.unwrap();
        let added = payload["tools_added"].as_array().unwrap();
        assert_eq!(added, &vec![json!("Bash"), json!("Read")]);
    }

    #[test]
    fn datagram_source_ref() {
        let msgs = vec![json!({"role": "user", "content": [{"type": "text", "text": "hi"}]})];

        let dg = build_datagram(&msgs, &[], &[], "odinn", Priority::Low, "mainexch_abc.jsonl:42", false);

        let payload = dg.payload.unwrap();
        assert_eq!(payload["source"], "mainexch_abc.jsonl:42");
    }

    #[test]
    fn datagram_startup_kind() {
        let msgs = vec![json!({"role": "user", "content": [{"type": "text", "text": "hi"}]})];

        let dg = build_datagram(&msgs, &[], &[], "odinn", Priority::Low, "test.jsonl:1", true);

        let payload = dg.payload.unwrap();
        assert_eq!(payload["traffic_kind"], "startup");
    }

    // --- traffic_kind classification ---

    #[test]
    fn kind_conversation_with_user() {
        let msgs = vec![
            json!({"role": "user", "content": [{"type": "text", "text": "hello"}]}),
            json!({"role": "assistant", "content": [{"type": "text", "text": "hi"}]}),
        ];
        let dg = build_datagram(&msgs, &[], &[], "test", Priority::Low, "test.jsonl:1", false);
        assert_eq!(dg.payload.unwrap()["traffic_kind"], "conversation");
    }

    #[test]
    fn kind_tool_without_user() {
        let msgs = vec![
            json!({"role": "assistant", "content": [
                {"type": "tool_use", "id": "t1", "name": "Read", "input": {"file_path": "/x"}}
            ]}),
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": "data"}
            ]}),
        ];
        let dg = build_datagram(&msgs, &[], &[], "test", Priority::Low, "test.jsonl:1", false);
        assert_eq!(dg.payload.unwrap()["traffic_kind"], "tool");
    }

    #[test]
    fn kind_subagent() {
        let msgs = vec![
            json!({"role": "assistant", "content": [
                {"type": "tool_use", "id": "t1", "name": "Task", "input": {"prompt": "explore"}}
            ]}),
        ];
        let dg = build_datagram(&msgs, &[], &[], "test", Priority::Low, "test.jsonl:1", false);
        assert_eq!(dg.payload.unwrap()["traffic_kind"], "subagent");
    }

    #[test]
    fn kind_planning() {
        let msgs = vec![
            json!({"role": "assistant", "content": [
                {"type": "tool_use", "id": "t1", "name": "EnterPlanMode", "input": {}}
            ]}),
        ];
        let dg = build_datagram(&msgs, &[], &[], "test", Priority::Low, "test.jsonl:1", false);
        assert_eq!(dg.payload.unwrap()["traffic_kind"], "planning");
    }

    // --- system_injection detection ---

    #[test]
    fn injection_false_for_user_hooks() {
        let msgs = vec![
            json!({"role": "user", "content": [
                {"type": "text", "text": "<system-reminder>\nPostToolUse:Edit hook additional context: type: syn_report\n</system-reminder>"},
                {"type": "text", "text": "my message"}
            ]}),
        ];
        let dg = build_datagram(&msgs, &[], &[], "test", Priority::Low, "test.jsonl:1", false);
        assert_eq!(dg.payload.unwrap()["system_injection"], false);
    }

    #[test]
    fn injection_true_for_platform_reminders() {
        let msgs = vec![
            json!({"role": "user", "content": [
                {"type": "text", "text": "<system-reminder>\nThe task tools haven't been used recently.\n</system-reminder>"},
                {"type": "text", "text": "my message"}
            ]}),
        ];
        let dg = build_datagram(&msgs, &[], &[], "test", Priority::Low, "test.jsonl:1", false);
        assert_eq!(dg.payload.unwrap()["system_injection"], true);
    }

    #[test]
    fn injection_false_when_no_reminders() {
        let msgs = vec![
            json!({"role": "user", "content": [{"type": "text", "text": "hello"}]}),
        ];
        let dg = build_datagram(&msgs, &[], &[], "test", Priority::Low, "test.jsonl:1", false);
        assert_eq!(dg.payload.unwrap()["system_injection"], false);
    }
}
