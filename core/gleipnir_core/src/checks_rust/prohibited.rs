//! Prohibited pattern checks for Rust source files.
//!
//! Checks: no_unwrap

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
// no_unwrap
// -------------------------------------------------------------------------

/// Safe method names that start with "unwrap" but don't panic.
const SAFE_UNWRAP_METHODS: &[&str] = &[
    "unwrap_or",
    "unwrap_or_default",
    "unwrap_or_else",
];

/// Detect .unwrap() and .expect() calls in Rust source.
///
/// Skips:
/// - Test functions (#[test] or #[cfg(test)] modules)
/// - unwrap_or, unwrap_or_default, unwrap_or_else (safe alternatives)
pub fn check_no_unwrap(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "call_expression") {
        let function_node = match node.child_by_field_name("function") {
            Some(f) => f,
            None => continue,
        };

        // Method call: field_expression with a field_identifier
        if function_node.kind() != "field_expression" {
            continue;
        }

        let field = match function_node.child_by_field_name("field") {
            Some(f) => f,
            None => continue,
        };

        let method_name = node_text(field, source.source_bytes);

        if method_name == "unwrap" || method_name == "expect" {
            // Skip if inside a test context
            if in_test_context(node, source.source_bytes) {
                continue;
            }

            violations.push(violation(
                node_line(node),
                format!(".{}() panics on failure — propagate the error with ? instead", method_name),
            ));
        }

        // Guard against safe variants being caught by text matching
        // (not needed here since we match exact method name, but defensive)
        if SAFE_UNWRAP_METHODS.contains(&method_name) {
            // These are fine — explicitly not flagged
            continue;
        }
    }

    violations
}

/// Check if a node is inside a test function or a #[cfg(test)] module.
fn in_test_context(node: tree_sitter::Node, source: &[u8]) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "function_item" => {
                if has_test_attribute(parent, source) {
                    return true;
                }
            }
            "mod_item" => {
                if has_cfg_test_attribute(parent, source) {
                    return true;
                }
            }
            _ => {}
        }
        current = parent.parent();
    }
    false
}

/// Check if a function has a #[test] attribute.
fn has_test_attribute(func_node: tree_sitter::Node, source: &[u8]) -> bool {
    let mut cursor = func_node.walk();
    for child in func_node.children(&mut cursor) {
        if child.kind() == "attribute_item" {
            let text = node_text(child, source);
            if text.contains("test") {
                return true;
            }
        }
    }
    false
}

/// Check if a module has a #[cfg(test)] attribute.
fn has_cfg_test_attribute(mod_node: tree_sitter::Node, source: &[u8]) -> bool {
    let mut cursor = mod_node.walk();
    for child in mod_node.children(&mut cursor) {
        if child.kind() == "attribute_item" {
            let text = node_text(child, source);
            if text.contains("cfg") && text.contains("test") {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::build_parsed_source_rust;
    use crate::structures::FileKind;

    fn parse(code: &str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        build_parsed_source_rust("/test/file.rs", source)
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(FileKind::Outside, None)
    }

    #[test]
    fn unwrap_caught() {
        let parsed = parse("fn main() { let x: Option<i32> = Some(1); x.unwrap(); }");
        let violations = check_no_unwrap(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("unwrap"));
    }

    #[test]
    fn expect_caught() {
        let parsed = parse(r#"fn main() { let x: Option<i32> = Some(1); x.expect("bad"); }"#);
        let violations = check_no_unwrap(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("expect"));
    }

    #[test]
    fn unwrap_or_ok() {
        let parsed = parse("fn main() { let x: Option<i32> = Some(1); x.unwrap_or(0); }");
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn unwrap_or_default_ok() {
        let parsed = parse("fn main() { let x: Option<i32> = Some(1); x.unwrap_or_default(); }");
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn unwrap_in_test_skipped() {
        let parsed = parse(
            r#"
            #[cfg(test)]
            mod tests {
                #[test]
                fn it_works() {
                    let x: Option<i32> = Some(1);
                    x.unwrap();
                }
            }
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn unwrap_in_test_function_skipped() {
        let parsed = parse(
            r#"
            #[test]
            fn it_works() {
                let x: Option<i32> = Some(1);
                x.unwrap();
            }
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn unwrap_outside_test_caught() {
        let parsed = parse(
            r#"
            fn production_code() {
                let x: Option<i32> = Some(1);
                x.unwrap();
            }

            #[cfg(test)]
            mod tests {
                #[test]
                fn it_works() {
                    let x: Option<i32> = Some(1);
                    x.unwrap();
                }
            }
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        // Only the production unwrap should be caught, not the test one
        assert_eq!(violations.len(), 1);
    }
}
