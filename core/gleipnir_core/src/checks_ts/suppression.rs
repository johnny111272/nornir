//! Suppression comment checks for TypeScript source.
//!
//! Check: no_ts_suppression

use crate::structures::{CheckConfig, ParsedSource, Severity, Violation};

fn violation(line: usize, message: String) -> Violation {
    Violation {
        line,
        check_name: String::new(),
        severity: Severity::Error,
        message,
        detail: String::new(),
        signal: String::new(),
        direction: String::new(),
        canary: String::new(),
    }
}

// -------------------------------------------------------------------------
// no_ts_suppression
// -------------------------------------------------------------------------

/// Suppression patterns that LLMs use to silence TypeScript/ESLint warnings.
const SUPPRESSION_MARKERS: &[&str] = &[
    "@ts-ignore",
    "@ts-expect-error",
    "@ts-nocheck",
    "eslint-disable",
    "eslint-disable-next-line",
    "noinspection",
];

/// Detect suppression comments in TypeScript source.
///
/// Catches:
/// - // @ts-ignore
/// - // @ts-expect-error
/// - // @ts-nocheck
/// - // eslint-disable
/// - // eslint-disable-next-line
/// - /* eslint-disable */
pub fn check_no_ts_suppression(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for (line_num, line) in source.lines.iter().enumerate() {
        let trimmed = line.trim();

        // Check single-line comments
        if let Some(comment_start) = trimmed.find("//") {
            let comment = &trimmed[comment_start + 2..].trim();
            for marker in SUPPRESSION_MARKERS {
                if comment.starts_with(marker) {
                    violations.push(violation(
                        line_num + 1,
                        format!("{} suppresses compiler/linter feedback", marker),
                    ));
                    break;
                }
            }
        }

        // Check block comments on same line
        if let Some(comment_start) = trimmed.find("/*") {
            let comment = &trimmed[comment_start + 2..].trim();
            for marker in SUPPRESSION_MARKERS {
                if comment.starts_with(marker) {
                    violations.push(violation(
                        line_num + 1,
                        format!("{} suppresses compiler/linter feedback", marker),
                    ));
                    break;
                }
            }
        }
    }

    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::build_parsed_source_typescript;
    use crate::structures::FileKind;

    fn parse(code: &str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        build_parsed_source_typescript("/test/file.ts", source).unwrap()
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(FileKind::Outside, &crate::STATISTICS)
    }

    #[test]
    fn ts_ignore_caught() {
        let parsed = parse("// @ts-ignore\nlet x: any = 1;");
        let violations = check_no_ts_suppression(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("@ts-ignore"));
    }

    #[test]
    fn ts_expect_error_caught() {
        let parsed = parse("// @ts-expect-error\nlet x = bad();");
        let violations = check_no_ts_suppression(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn eslint_disable_caught() {
        let parsed = parse("/* eslint-disable */\nlet x = 1;");
        let violations = check_no_ts_suppression(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("eslint-disable"));
    }

    #[test]
    fn eslint_disable_next_line_caught() {
        let parsed = parse("// eslint-disable-next-line no-unused-vars\nlet x = 1;");
        let violations = check_no_ts_suppression(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn normal_comment_ok() {
        let parsed = parse("// This is a normal comment\nlet x = 1;");
        let violations = check_no_ts_suppression(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn no_comments_ok() {
        let parsed = parse("let x = 1;\nlet y = 2;");
        let violations = check_no_ts_suppression(&parsed, &default_config());
        assert!(violations.is_empty());
    }
}
