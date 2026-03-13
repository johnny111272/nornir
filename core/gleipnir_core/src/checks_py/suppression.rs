//! Suppression comment detection.
//!
//! Check: no_suppression_comments.

use regex::Regex;
use std::sync::LazyLock;

use crate::structures::{CheckConfig, ParsedSource, Severity, Violation};

fn violation(line: usize, message: String) -> Violation {
    Violation {
        line,
        check_name: String::new(),
        severity: Severity::Blocked,
        message,
        detail: String::new(),
        signal: String::new(),
        direction: String::new(),
        canary: String::new(),
    }
}

// Patterns and descriptions constructed to avoid self-triggering.
struct ForbiddenPattern {
    regex: Regex,
    description: String,
}

static FORBIDDEN_PATTERNS: LazyLock<Vec<ForbiddenPattern>> = LazyLock::new(|| {
    let build_desc = |prefix: &str, suffix: &str| format!("# {prefix}: {suffix}");
    vec![
        ForbiddenPattern {
            regex: Regex::new(r"(?i)#\s*type:\s*ignore(?:\s*$|\s*#)").unwrap(),
            description: build_desc("type", "ignore (blanket suppression)"),
        },
        ForbiddenPattern {
            regex: Regex::new(r"(?i)#\s*type:\s*ignore\s*\[").unwrap(),
            description: build_desc("type", "ignore[...] (use pyright syntax instead)"),
        },
        ForbiddenPattern {
            regex: Regex::new(r"(?i)#\s*noqa(?:\s*$|\s*#|\s*:)").unwrap(),
            description: "# noqa (ruff/flake8 suppression)".to_string(),
        },
        ForbiddenPattern {
            regex: Regex::new(r"(?i)#\s*pylint:\s*disable").unwrap(),
            description: build_desc("pylint", "disable (pylint suppression)"),
        },
        ForbiddenPattern {
            regex: Regex::new(r"(?i)#\s*pyright:\s*ignore\s*(?:$|#)").unwrap(),
            description: build_desc("pyright", "ignore (blanket - must specify error code)"),
        },
    ]
});

static PYRIGHT_IGNORE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)#\s*pyright:\s*ignore\s*\[([^\]]+)\]").unwrap());

fn pyright_msg_prefix() -> String {
    format!("# {}: {}", "pyright", "ignore")
}

fn path_matches_patterns(file_path: &str, patterns: &[String]) -> bool {
    let filename = file_path.rsplit('/').next().unwrap_or(file_path);
    for pattern in patterns {
        if pattern.ends_with('/') {
            if file_path.contains(pattern.as_str())
                || file_path.contains(&format!("/{pattern}"))
            {
                return true;
            }
        } else if glob_match(filename, pattern) {
            return true;
        }
    }
    false
}

fn glob_match(name: &str, pattern: &str) -> bool {
    if !pattern.contains('*') && !pattern.contains('?') {
        return name == pattern;
    }
    // Simple glob: only support * as wildcard
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return name == pattern;
    }
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match name[pos..].find(part) {
            Some(idx) => {
                if i == 0 && idx != 0 {
                    return false;
                }
                pos += idx + part.len();
            }
            None => return false,
        }
    }
    if !parts.last().unwrap_or(&"").is_empty() {
        pos == name.len()
    } else {
        true
    }
}

pub fn check_no_suppression_comments(
    source: &ParsedSource,
    config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let prefix = pyright_msg_prefix();

    let in_whitelist = path_matches_patterns(source.file_path, &config.suppression_whitelist);
    let in_blacklist = path_matches_patterns(source.file_path, &config.suppression_blacklist);

    let file_can_suppress = if !config.suppression_whitelist.is_empty() {
        in_whitelist
    } else {
        !in_blacklist
    };

    for (line_num, line) in source.lines.iter().enumerate() {
        let line_num = line_num + 1;

        let mut found_forbidden = false;
        for fp in FORBIDDEN_PATTERNS.iter() {
            if fp.regex.is_match(line) {
                violations.push(violation(line_num, fp.description.clone()));
                found_forbidden = true;
                break;
            }
        }

        if found_forbidden {
            continue;
        }

        if let Some(caps) = PYRIGHT_IGNORE_RE.captures(line) {
            let codes_str = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let codes: Vec<&str> = codes_str.split(',').map(|s| s.trim()).collect();

            if !file_can_suppress {
                let reason = if in_blacklist {
                    "file in blacklisted path"
                } else {
                    "file not in whitelist"
                };
                violations.push(violation(
                    line_num,
                    format!("{prefix}[{codes_str}] - {reason}"),
                ));
                continue;
            }

            for code in &codes {
                if !config.allowed_pyright_ignores.iter().any(|a| a == code) {
                    violations.push(violation(
                        line_num,
                        format!("{prefix}[{code}] - error code not in whitelist"),
                    ));
                }
            }
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::build_parsed_source;
    use crate::structures::FileKind;

    fn parse(code: &str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        build_parsed_source("/test/file.py", source).unwrap()
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(FileKind::Outside)
    }

    #[test]
    fn blanket_type_ignore_caught() {
        let comment = format!("x = 1  # {}: {}", "type", "ignore");
        let parsed = parse(&comment);
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("blanket suppression"));
    }

    #[test]
    fn noqa_caught() {
        let comment = format!("x = 1  # {}", "noqa");
        let parsed = parse(&comment);
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("noqa"));
    }

    #[test]
    fn pylint_disable_caught() {
        let comment = format!("x = 1  # {}: {}", "pylint", "disable=C0301");
        let parsed = parse(&comment);
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("pylint"));
    }

    #[test]
    fn pyright_ignore_with_allowed_code_ok() {
        let comment = format!("x = 1  # {}: {}[reportGeneralIssue]", "pyright", "ignore");
        let parsed = parse(&comment);
        let mut config = default_config();
        config.allowed_pyright_ignores = vec!["reportGeneralIssue".to_string()];
        let violations = check_no_suppression_comments(&parsed, &config);
        assert!(violations.is_empty());
    }

    #[test]
    fn pyright_ignore_with_disallowed_code_caught() {
        let comment = format!("x = 1  # {}: {}[reportUnknown]", "pyright", "ignore");
        let parsed = parse(&comment);
        let config = default_config();
        let violations = check_no_suppression_comments(&parsed, &config);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("not in whitelist"));
    }

    #[test]
    fn blanket_pyright_ignore_caught() {
        let comment = format!("x = 1  # {}: {}", "pyright", "ignore");
        let parsed = parse(&comment);
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("blanket"));
    }

    #[test]
    fn clean_code_ok() {
        let parsed = parse("x = 1  # normal comment\n");
        let violations = check_no_suppression_comments(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn blacklisted_file_cannot_suppress() {
        let comment = format!("x = 1  # {}: {}[reportGeneralIssue]", "pyright", "ignore");
        let parsed = parse(&comment);
        let mut config = default_config();
        config.allowed_pyright_ignores = vec!["reportGeneralIssue".to_string()];
        config.suppression_blacklist = vec!["file.py".to_string()];
        let violations = check_no_suppression_comments(&parsed, &config);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("blacklisted"));
    }
}
