use std::fs;
use std::path::{Path, PathBuf};

use crate::model::AppState;
use crate::permissions::KNOWN_PERMISSIONS;

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
    Ok(output_path)
}

fn assemble_prompt(state: &AppState) -> Result<String, String> {
    let mut parts: Vec<String> = Vec::new();

    // 1. Selected persona
    if let Some(index) = state.selected_persona {
        if let Some(persona) = state.library.personas.get(index) {
            parts.push(read_fragment(&persona.path)?);
        }
    }

    // 2-6. Always-loaded fragments (already sorted by category order)
    for fragment in &state.library.always {
        parts.push(read_fragment(&fragment.path)?);
    }

    // 7. Coding fragments (if enabled)
    if state.coding_enabled {
        for fragment in &state.library.coding {
            parts.push(read_fragment(&fragment.path)?);
        }
    }

    // 8. Language context (always include selected languages)
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

    // 9. Selected expertise (each is a directory with multiple .md files)
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

pub fn build_env_vars(state: &AppState) -> Vec<(&'static str, &'static str)> {
    let mut vars = Vec::new();
    for (index, selected) in state.selected_permissions.iter().enumerate() {
        if *selected {
            if let Some(permission) = KNOWN_PERMISSIONS.get(index) {
                vars.push((permission.env_var, "1"));
            }
        }
    }
    vars
}
