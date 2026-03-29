//! Prohibited pattern checks for TypeScript source.
//!
//! Checks: no_console_log

use crate::parsing::{find_nodes_by_type, node_line, node_text};
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
// no_console_log
// -------------------------------------------------------------------------

/// Detect console.log(), console.warn(), console.error() calls.
///
/// These are debug artifacts that should not be in production Svelte components.
pub fn check_no_console_log(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "call_expression") {
        let function_node = match node.child_by_field_name("function") {
            Some(f) => f,
            None => continue,
        };

        // console.log is a member_expression: object=console, property=log
        if function_node.kind() != "member_expression" {
            continue;
        }

        let object = match function_node.child_by_field_name("object") {
            Some(o) => o,
            None => continue,
        };

        if node_text(object, source.source_bytes) != "console" {
            continue;
        }

        let property = match function_node.child_by_field_name("property") {
            Some(p) => p,
            None => continue,
        };

        let method = node_text(property, source.source_bytes);
        match method {
            "log" | "warn" | "error" | "info" | "debug" | "trace" => {
                violations.push(violation(
                    node_line(node),
                    format!("console.{}() is debug output — remove", method),
                ));
            }
            _ => {}
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
    fn console_log_caught() {
        let parsed = parse("console.log('hello');");
        let violations = check_no_console_log(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("console.log"));
    }

    #[test]
    fn console_warn_caught() {
        let parsed = parse("console.warn('warning');");
        let violations = check_no_console_log(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("console.warn"));
    }

    #[test]
    fn console_error_caught() {
        let parsed = parse("console.error('error');");
        let violations = check_no_console_log(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn normal_function_call_ok() {
        let parsed = parse("doSomething();");
        let violations = check_no_console_log(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn method_call_not_console_ok() {
        let parsed = parse("logger.log('hello');");
        let violations = check_no_console_log(&parsed, &default_config());
        assert!(violations.is_empty());
    }
}
