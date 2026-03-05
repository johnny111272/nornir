//! Path expansion logic for .tomlx.
//!
//! Handles path reference resolution and expansion (~ for home, $VAR for env).

use std::env;
use std::path::Path;

use super::types::{ExpandMode, PathRegistry};

/// Expand a path value using the registry and reference.
pub fn expand_path(
    value: &str,
    reference: &str,
    registry: &PathRegistry,
    config_dir: Option<&Path>,
) -> Result<String, String> {
    let base = resolve_base(reference, registry, config_dir)?;

    let full_path = if base.is_empty() {
        value.to_string()
    } else {
        let base = base.trim_end_matches('/');
        let value = value.trim_start_matches('/');
        format!("{}/{}", base, value)
    };

    let expanded = apply_expansion(&full_path, registry.expand_mode)?;
    let normalized = normalize_path(&expanded);

    Ok(normalized)
}

fn resolve_base(
    reference: &str,
    registry: &PathRegistry,
    config_dir: Option<&Path>,
) -> Result<String, String> {
    match reference.to_lowercase().as_str() {
        "home" => home_dir().ok_or_else(|| "Could not determine home directory".to_string()),
        "absolute" => Ok(String::new()),
        "config" => config_dir
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .ok_or_else(|| "Config directory not available for 'config' reference".to_string()),
        _ => registry
            .get_base(reference)
            .map(|s| s.to_string())
            .ok_or_else(|| format!("Undefined path reference '{}'", reference)),
    }
}

fn apply_expansion(path: &str, mode: ExpandMode) -> Result<String, String> {
    match mode {
        ExpandMode::User => Ok(expand_user(path)),
        ExpandMode::Env => expand_env(path),
        ExpandMode::All => {
            let user_expanded = expand_user(path);
            expand_env(&user_expanded)
        }
        ExpandMode::None => Ok(path.to_string()),
    }
}

fn expand_user(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(home) = home_dir() {
            return format!("{}/{}", home.trim_end_matches('/'), &path[2..]);
        }
    } else if path == "~" {
        if let Some(home) = home_dir() {
            return home;
        }
    }
    path.to_string()
}

fn expand_env(path: &str) -> Result<String, String> {
    let mut result = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' {
            if chars.peek() == Some(&'{') {
                chars.next();
                let var_name: String = chars.by_ref().take_while(|&c| c != '}').collect();
                if var_name.is_empty() {
                    return Err("Empty variable name in ${}".to_string());
                }
                match env::var(&var_name) {
                    Ok(value) => result.push_str(&value),
                    Err(_) => return Err(format!("Environment variable '{}' not set", var_name)),
                }
            } else {
                let mut var_name = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        var_name.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if var_name.is_empty() {
                    result.push('$');
                    continue;
                }
                match env::var(&var_name) {
                    Ok(value) => result.push_str(&value),
                    Err(_) => return Err(format!("Environment variable '{}' not set", var_name)),
                }
            }
        } else {
            result.push(c);
        }
    }

    Ok(result)
}

fn home_dir() -> Option<String> {
    env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .ok()
        .map(|s| s.replace('\\', "/"))
}

fn normalize_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    let mut components: Vec<&str> = Vec::new();
    let is_absolute = path.starts_with('/');

    for part in path.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                if components.last().map(|s| *s != "..").unwrap_or(false) {
                    components.pop();
                } else if !is_absolute {
                    components.push("..");
                }
            }
            _ => components.push(part),
        }
    }

    let result = components.join("/");
    if is_absolute {
        format!("/{}", result)
    } else if result.is_empty() {
        ".".to_string()
    } else {
        result
    }
}

/// Expand all bases in a registry to absolute paths.
pub fn expand_registry_bases(registry: &mut PathRegistry, _config_dir: Option<&Path>) {
    let expand_mode = registry.expand_mode;
    let expanded: Vec<(String, String)> = registry
        .user_defined
        .iter()
        .filter_map(|(name, value)| {
            let expanded = apply_expansion(value, expand_mode).ok()?;
            let normalized = normalize_path(&expanded);
            Some((name.clone(), normalized))
        })
        .collect();

    for (name, value) in expanded {
        registry.user_defined.insert(name, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_user_tilde() {
        env::set_var("HOME", "/Users/test");
        assert_eq!(expand_user("~/Documents"), "/Users/test/Documents");
    }

    #[test]
    fn test_expand_env() {
        env::set_var("TEST_TOMLX_VAR", "/test/path");
        assert_eq!(expand_env("$TEST_TOMLX_VAR/sub").unwrap(), "/test/path/sub");
    }

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("/foo/./bar"), "/foo/bar");
        assert_eq!(normalize_path("/foo/../bar"), "/bar");
        assert_eq!(normalize_path("/foo//bar"), "/foo/bar");
    }

    #[test]
    fn test_resolve_base_absolute() {
        let registry = PathRegistry::new();
        assert_eq!(resolve_base("absolute", &registry, None).unwrap(), "");
    }

    #[test]
    fn test_expand_path_full() {
        env::set_var("HOME", "/Users/test");
        let mut registry = PathRegistry::new();
        registry.user_defined.insert("base".to_string(), "~/.ai/phoenix/".to_string());
        registry.expand_mode = ExpandMode::User;

        let result = expand_path("cli/", "base", &registry, None).unwrap();
        assert_eq!(result, "/Users/test/.ai/phoenix/cli");
    }
}
