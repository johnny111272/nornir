/// A permission is a set of paths exempt from hook probing/gaming checks.
/// These are superpowers — only for sessions that work ON the security system.
/// Most sessions need zero permissions.
///
/// The actual mechanism is HOOK_LLM_ALLOW_PATHS (colon-separated paths).
pub struct Permission {
    pub name: &'static str,
    pub paths: &'static [&'static str],
}

pub static KNOWN_PERMISSIONS: &[Permission] = &[Permission {
    name: "Security Infrastructure",
    paths: &[
        "/Users/johnny/.ai/smidja/nornir/",
        "/Users/johnny/.ai/tools/",
        "/Users/johnny/.claude/",
    ],
}];

/// Build HOOK_LLM_ALLOW_PATHS from selected permissions.
/// Returns None if no permissions selected.
pub fn build_allow_paths(selected: &[bool]) -> Option<String> {
    let mut paths: Vec<&str> = Vec::new();

    for (index, is_selected) in selected.iter().enumerate() {
        if !*is_selected {
            continue;
        }
        if let Some(permission) = KNOWN_PERMISSIONS.get(index) {
            for path in permission.paths {
                if !paths.contains(path) {
                    paths.push(path);
                }
            }
        }
    }

    if paths.is_empty() {
        None
    } else {
        Some(paths.join(":"))
    }
}
