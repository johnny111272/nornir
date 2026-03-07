//! Exceptions config loading from .gleipnir/exceptions.toml.
//!
//! Only needed for FileKind::Outside files when violations are found
//! in excusable checks (no_any_types, no_unsafe_imports, no_model_dump,
//! no_suppression_comments).

use std::path::Path;

use crate::structures::ExceptionsConfig;

/// Walk up from file looking for .gleipnir/exceptions.toml.
/// Returns None if not found.
pub fn load_exceptions(file_path: &Path) -> Option<ExceptionsConfig> {
    let mut dir = file_path.parent()?;

    loop {
        let candidate = dir.join(".gleipnir").join("exceptions.toml");
        if candidate.exists() {
            let content = std::fs::read_to_string(&candidate).ok()?;
            let config: ExceptionsConfig = toml::from_str(&content).ok()?;
            return Some(config);
        }

        dir = match dir.parent() {
            Some(parent) if parent != dir => parent,
            _ => return None,
        };
    }
}
