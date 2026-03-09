//! Pure compaction injection logic.
//!
//! Appends a system block with compaction summary instructions to a Claude API
//! request. The instructions teach the model to write honest, structured
//! compaction summaries.
//!
//! No I/O. Single function, single constant.

/// The compaction summary instructions, embedded at compile time.
pub const COMPACTION_INSTRUCTIONS: &str = include_str!("../instructions/compaction_summary.md");

/// Append a compaction instructions system block to the request's system array.
///
/// Mutates `value` in place. Returns error if `system` is missing or not an array.
pub fn inject_compaction_system_block(value: &mut serde_json::Value) -> Result<(), String> {
    let system = value
        .get_mut("system")
        .and_then(|s| s.as_array_mut())
        .ok_or_else(|| "no 'system' array in request JSON".to_string())?;

    let block = serde_json::json!({
        "type": "text",
        "text": COMPACTION_INSTRUCTIONS,
    });

    system.push(block);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn instructions_are_non_empty() {
        assert!(!COMPACTION_INSTRUCTIONS.is_empty());
    }

    #[test]
    fn inject_appends_to_existing_system_array() {
        let mut value = json!({
            "system": [{"type": "text", "text": "existing"}],
            "messages": []
        });

        inject_compaction_system_block(&mut value).unwrap();

        let system = value["system"].as_array().unwrap();
        assert_eq!(system.len(), 2);
        assert_eq!(system[1]["type"], "text");
        assert_eq!(system[1]["text"], COMPACTION_INSTRUCTIONS);
    }

    #[test]
    fn inject_works_on_empty_system_array() {
        let mut value = json!({"system": [], "messages": []});
        inject_compaction_system_block(&mut value).unwrap();

        let system = value["system"].as_array().unwrap();
        assert_eq!(system.len(), 1);
        assert_eq!(system[0]["text"], COMPACTION_INSTRUCTIONS);
    }

    #[test]
    fn inject_preserves_other_fields() {
        let mut value = json!({
            "model": "claude-opus-4-6",
            "max_tokens": 4096,
            "system": [{"type": "text", "text": "original"}],
            "messages": [{"role": "user", "content": "hello"}]
        });

        inject_compaction_system_block(&mut value).unwrap();

        assert_eq!(value["model"], "claude-opus-4-6");
        assert_eq!(value["max_tokens"], 4096);
        assert_eq!(value["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn inject_fails_without_system_key() {
        let mut value = json!({"messages": []});
        let err = inject_compaction_system_block(&mut value).unwrap_err();
        assert!(err.contains("system"), "error should mention system: {err}");
    }

    #[test]
    fn inject_fails_when_system_is_not_array() {
        let mut value = json!({"system": "just a string"});
        let err = inject_compaction_system_block(&mut value).unwrap_err();
        assert!(err.contains("system"), "error should mention system: {err}");
    }
}
