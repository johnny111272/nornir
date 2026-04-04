use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::model::AppState;

pub fn estimate_tokens(state: &AppState) -> usize {
    let mut total_bytes: usize = 0;

    // Foundation
    if let Some(ref foundation) = state.library.foundation {
        total_bytes += foundation.byte_size;
    }

    // Always-loaded fragments
    for fragment in &state.library.always {
        total_bytes += fragment.byte_size;
    }

    // Selected persona
    if let Some(index) = state.selected_persona {
        if let Some(persona) = state.library.personas.get(index) {
            total_bytes += persona.byte_size;
        }
    }

    // Auto-matched space descriptor
    if let Some(space_idx) = state.auto_space_index() {
        if let Some(descriptor) = state.library.descriptors.get(space_idx) {
            total_bytes += descriptor.byte_size;
        }
    }

    // Selected system descriptors
    for index in state.ordered_descriptor_indices() {
        if let Some(descriptor) = state.library.descriptors.get(index) {
            total_bytes += descriptor.byte_size;
        }
    }

    // Coding fragments (if enabled)
    if state.coding_enabled {
        for fragment in &state.library.coding {
            total_bytes += fragment.byte_size;
        }
    }

    // Selected expertise
    for (index, selected) in state.selected_expertise.iter().enumerate() {
        if *selected {
            if let Some(fragment) = state.library.expertise.get(index) {
                total_bytes += fragment.byte_size;
            }
        }
    }

    total_bytes / 4
}

pub fn write_prompt_file(state: &AppState) -> Result<PathBuf, String> {
    if state.selected_persona.is_none() {
        return Err("No persona selected. Select a persona before launching.".to_string());
    }

    let prompt = assemble_prompt(state)?;
    let output_path = std::env::temp_dir().join("cc_launch_prompt.xml");
    fs::write(&output_path, &prompt)
        .map_err(|e| format!("write prompt file: {e}"))?;

    // Persist assembled prompt into workspace for hook-based re-injection on compact.
    // Use profile workspace path if selected, otherwise fall back to actual CWD
    // (Auto mode — user launched from their current directory without selecting a profile).
    let prompt_dir = state
        .workspace_path()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let workspace_copy = prompt_dir.join(".SYSTEM_PROMPT.xml");
    let _ = fs::write(&workspace_copy, &prompt);

    Ok(output_path)
}

fn assemble_prompt(state: &AppState) -> Result<String, String> {
    let mut parts: Vec<String> = Vec::new();

    // === IDENTITY LAYER (who you are, how you think) ===

    // 1. Foundation — the collaboration paradigm. Position 1, always.
    if let Some(ref foundation) = state.library.foundation {
        parts.push(read_fragment(&foundation.path)?);
    }

    // 2. Persona — who you are within the collaboration. Position 2.
    if let Some(index) = state.selected_persona {
        if let Some(persona) = state.library.personas.get(index) {
            parts.push(read_fragment(&persona.path)?);
        }
    }

    // 3. Always-loaded: cognitive-mode → behavior → communication → safety → anthropic
    for fragment in &state.library.always {
        parts.push(read_fragment(&fragment.path)?);
    }

    // === CONTEXT LAYER (where you are, what you're working on) ===

    // 3. Auto-matched space descriptor (your workspace environment)
    if let Some(space_idx) = state.auto_space_index() {
        if let Some(descriptor) = state.library.descriptors.get(space_idx) {
            parts.push(read_fragment(&descriptor.path)?);
        }
    }

    // 4. System descriptors (primary first, siblings, overview last)
    for index in state.ordered_descriptor_indices() {
        if let Some(descriptor) = state.library.descriptors.get(index) {
            parts.push(read_fragment(&descriptor.path)?);
        }
    }

    // === MODE LAYER (how you work this session) ===

    // 5. Coding fragments (if enabled)
    if state.coding_enabled {
        for fragment in &state.library.coding {
            parts.push(read_fragment(&fragment.path)?);
        }
    }

    // 6. Language context (only when coding enabled)
    let languages = state.selected_language_names();
    if state.coding_enabled && !languages.is_empty() {
        let lang_tags: Vec<String> = languages
            .iter()
            .map(|language| format!("  <language>{language}</language>"))
            .collect();
        parts.push(format!(
            "<context>\n{}\n</context>",
            lang_tags.join("\n")
        ));
    }

    // 7. Selected expertise
    for (index, selected) in state.selected_expertise.iter().enumerate() {
        if *selected {
            if let Some(fragment) = state.library.expertise.get(index) {
                parts.push(read_fragment(&fragment.path)?);
            }
        }
    }

    let persona_id = state
        .selected_persona
        .and_then(|i| state.library.personas.get(i))
        .map(|p| p.id.as_str())
        .unwrap_or("none");

    let workspace_name = state
        .workspace_path()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "auto".to_string());

    let body = parts.join("\n\n");

    // Check for unsubstituted template variables (e.g. {MEMORY_DIR})
    let template_re = Regex::new(r"\{[A-Z][A-Z_]+\}").map_err(|e| format!("regex: {e}"))?;
    if let Some(m) = template_re.find(&body) {
        return Err(format!("Unsubstituted template variable in assembled prompt: {}", m.as_str()));
    }

    Ok(format!(
        "<prompt version=\"1\" persona=\"{persona_id}\" workspace=\"{workspace_name}\" coding=\"{}\">\n\n{body}\n\n</prompt>",
        state.coding_enabled,
    ))
}

fn read_fragment(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))
}

/// Write assembled prompt directly into a session's control directory.
/// Used by --update to hot-swap the system prompt for an active session.
pub fn write_update_file(state: &AppState, session_id: &str, workspace_name: &str) -> Result<PathBuf, String> {
    if state.selected_persona.is_none() {
        return Err("No persona selected. Select a persona before updating.".to_string());
    }

    let prompt = assemble_prompt(state)?;
    let session_dir = workspace_registry::workspace_control_dir(workspace_name).join(session_id);

    if !session_dir.exists() {
        return Err(format!(
            "Session directory does not exist: {}\nIs the session ID correct?",
            session_dir.display()
        ));
    }

    let target = session_dir.join("SYSTEM_PROMPT.xml");
    fs::write(&target, &prompt)
        .map_err(|e| format!("write update file: {e}"))?;

    Ok(target)
}

pub fn build_allow_paths(state: &AppState) -> Option<String> {
    crate::permissions::build_allow_paths(&state.selected_permissions)
}
