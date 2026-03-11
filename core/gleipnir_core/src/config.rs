//! User config loading from .gleipnir/user.toml.
//!
//! Only needed for FileKind::Outside files when violations are found
//! in excusable checks (no_any_types, no_unsafe_imports, no_model_dump,
//! no_suppression_comments).

use std::path::Path;

use crate::structures::UserConfig;

/// Walk up from file looking for .gleipnir/user.toml.
/// Returns None if not found.
pub fn load_user_config(file_path: &Path) -> Option<UserConfig> {
    let mut dir = file_path.parent()?;

    loop {
        let candidate = dir.join(".gleipnir").join("user.toml");
        if candidate.exists() {
            let content = std::fs::read_to_string(&candidate).ok()?;
            let config: UserConfig = toml::from_str(&content).ok()?;
            return Some(config);
        }

        dir = match dir.parent() {
            Some(parent) if parent != dir => parent,
            _ => return None,
        };
    }
}
