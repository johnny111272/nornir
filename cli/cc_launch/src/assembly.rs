use std::fs;
use std::path::{Path, PathBuf};

use crate::model::AppState;

pub fn estimate_tokens(state: &AppState) -> usize {
    let mut total_bytes: usize = 0;

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
    let prompt = assemble_prompt(state)?;
    let output_path = std::env::temp_dir().join("cc_launch_prompt.xml");
    fs::write(&output_path, &prompt)
        .map_err(|e| format!("write prompt file: {e}"))?;

    // Persist assembled prompt into workspace for hook-based re-injection on compact
    if let Some(workspace) = state.workspace_path() {
        let workspace_copy = workspace.join(".SYSTEM_PROMPT.md");
        let _ = fs::write(&workspace_copy, &prompt);
    }

    Ok(output_path)
}

fn assemble_prompt(state: &AppState) -> Result<String, String> {
    let mut parts: Vec<String> = Vec::new();

    // === IDENTITY LAYER (who you are, how you think) ===

    // 1. Always-loaded: cognitive-mode → behavior → communication → safety → anthropic
    //    Collaboration paradigm is first (00-prefix in cognitive-mode).
    //    This is the FOUNDATION — how to think, how to behave, how to collaborate.
    for fragment in &state.library.always {
        parts.push(read_fragment(&fragment.path)?);
    }

    // 2. Persona — who you are within the collaboration
    if let Some(index) = state.selected_persona {
        if let Some(persona) = state.library.personas.get(index) {
            parts.push(read_fragment(&persona.path)?);
        }
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

    // 6. Language context
    let languages = state.selected_language_names();
    if !languages.is_empty() {
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
                parts.push(read_expertise_dir(&fragment.path)?);
            }
        }
    }

    Ok(parts.join("\n\n"))
}

fn read_fragment(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))
}

fn read_expertise_dir(directory: &Path) -> Result<String, String> {
    let mut file_contents = Vec::new();

    let entries = fs::read_dir(directory)
        .map_err(|e| format!("read expertise dir {}: {e}", directory.display()))?;

    let mut paths: Vec<_> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("md"))
        .collect();

    paths.sort();

    for path in paths {
        file_contents.push(read_fragment(&path)?);
    }

    Ok(file_contents.join("\n\n"))
}

pub fn build_allow_paths(state: &AppState) -> Option<String> {
    crate::permissions::build_allow_paths(&state.selected_permissions)
}
