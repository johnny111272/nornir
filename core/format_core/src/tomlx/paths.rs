//! Path expansion logic for .tomlx.
//!
//! Pure path resolution and expansion (~ for home, $VAR for env).
//! All environment access is injected via a resolver closure — this module
//! performs no I/O.

use std::path::Path;

use super::types::{ExpandMode, PathRegistry};

/// Expand a path value using the registry and reference.
///
/// `resolve_env` resolves environment variable names to values. Pass
/// `|name| std::env::var(name).ok()` for live resolution.
pub fn expand_path(
    value: &str,
    reference: &str,
    registry: &PathRegistry,
    config_dir: Option<&Path>,
    resolve_env: &dyn Fn(&str) -> Option<String>,
) -> Result<String, String> {
    let base = resolve_base(reference, registry, config_dir, resolve_env)?;

    let full_path = if base.is_empty() {
        value.to_string()
    } else {
        let base = base.trim_end_matches('/');
        let value = value.trim_start_matches('/');
        format!("{}/{}", base, value)
    };

    let expanded = apply_expansion(&full_path, registry.expand_mode, resolve_env)?;
    let normalized = normalize_path(&expanded);

    Ok(normalized)
}

fn resolve_base(
    reference: &str,
    registry: &PathRegistry,
    config_dir: Option<&Path>,
    resolve_env: &dyn Fn(&str) -> Option<String>,
) -> Result<String, String> {
    match reference.to_lowercase().as_str() {
        "home" => home_dir(resolve_env)
            .ok_or_else(|| "Could not determine home directory".to_string()),
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

fn apply_expansion(
    path: &str,
    mode: ExpandMode,
    resolve_env: &dyn Fn(&str) -> Option<String>,
) -> Result<String, String> {
    match mode {
        ExpandMode::User => Ok(expand_user(path, resolve_env)),
        ExpandMode::Env => expand_env_vars(path, resolve_env),
        ExpandMode::All => {
            let user_expanded = expand_user(path, resolve_env);
            expand_env_vars(&user_expanded, resolve_env)
        }
        ExpandMode::None => Ok(path.to_string()),
    }
}

fn expand_user(path: &str, resolve_env: &dyn Fn(&str) -> Option<String>) -> String {
    if path.starts_with("~/") {
        if let Some(home) = home_dir(resolve_env) {
            return format!("{}/{}", home.trim_end_matches('/'), &path[2..]);
        }
    } else if path == "~" {
        if let Some(home) = home_dir(resolve_env) {
            return home;
        }
    }
    path.to_string()
}

fn resolve_var(
    var_name: &str,
    resolve_env: &dyn Fn(&str) -> Option<String>,
) -> Result<String, String> {
    resolve_env(var_name)
        .ok_or_else(|| format!("Environment variable '{}' not set", var_name))
}

fn expand_braced_var(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    resolve_env: &dyn Fn(&str) -> Option<String>,
) -> Result<String, String> {
    let var_name: String = chars.by_ref().take_while(|&c| c != '}').collect();
    if var_name.is_empty() {
        return Err("Empty variable name in ${}".to_string());
    }
    resolve_var(&var_name, resolve_env)
}

fn expand_bare_var(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    resolve_env: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<String>, String> {
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
        return Ok(None);
    }
    resolve_var(&var_name, resolve_env).map(Some)
}

fn expand_env_vars(
    path: &str,
    resolve_env: &dyn Fn(&str) -> Option<String>,
) -> Result<String, String> {
    let mut result = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '$' {
            result.push(c);
            continue;
        }
        if chars.peek() == Some(&'{') {
            chars.next();
            result.push_str(&expand_braced_var(&mut chars, resolve_env)?);
        } else {
            match expand_bare_var(&mut chars, resolve_env)? {
                Some(value) => result.push_str(&value),
                None => result.push('$'),
            }
        }
    }

    Ok(result)
}

fn home_dir(resolve_env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    resolve_env("HOME")
        .or_else(|| resolve_env("USERPROFILE"))
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
///
/// `resolve_env` resolves environment variable names to values.
pub fn expand_registry_bases(
    registry: &mut PathRegistry,
    _config_dir: Option<&Path>,
    resolve_env: &dyn Fn(&str) -> Option<String>,
) {
    let expand_mode = registry.expand_mode;
    let expanded: Vec<(String, String)> = registry
        .user_defined
        .iter()
        .filter_map(|(name, value)| {
            let expanded = apply_expansion(value, expand_mode, resolve_env).ok()?;
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

    fn test_env(name: &str) -> Option<String> {
        match name {
            "HOME" => Some("/Users/test".to_string()),
            "TEST_TOMLX_VAR" => Some("/test/path".to_string()),
            _ => None,
        }
    }

    #[test]
    fn test_expand_user_tilde() {
        assert_eq!(expand_user("~/Documents", &test_env), "/Users/test/Documents");
    }

    #[test]
    fn test_expand_env_vars() {
        assert_eq!(
            expand_env_vars("$TEST_TOMLX_VAR/sub", &test_env).unwrap(),
            "/test/path/sub",
        );
    }

    #[test]
    fn test_expand_env_braced() {
        assert_eq!(
            expand_env_vars("${TEST_TOMLX_VAR}/sub", &test_env).unwrap(),
            "/test/path/sub",
        );
    }

    #[test]
    fn test_expand_env_missing_var() {
        assert!(expand_env_vars("$NONEXISTENT/sub", &test_env).is_err());
    }

    #[test]
    fn test_expand_env_bare_dollar() {
        assert_eq!(expand_env_vars("$ literal", &test_env).unwrap(), "$ literal");
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
        assert_eq!(resolve_base("absolute", &registry, None, &test_env).unwrap(), "");
    }

    #[test]
    fn test_resolve_base_home() {
        let registry = PathRegistry::new();
        assert_eq!(
            resolve_base("home", &registry, None, &test_env).unwrap(),
            "/Users/test",
        );
    }

    #[test]
    fn test_expand_path_full() {
        let mut registry = PathRegistry::new();
        registry.user_defined.insert("base".to_string(), "~/.ai/phoenix/".to_string());
        registry.expand_mode = ExpandMode::User;

        let result = expand_path("cli/", "base", &registry, None, &test_env).unwrap();
        assert_eq!(result, "/Users/test/.ai/phoenix/cli");
    }
}
