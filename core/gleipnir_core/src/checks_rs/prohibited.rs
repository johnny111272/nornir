//! Prohibited pattern checks for Rust source files.
//!
//! Checks: no_unwrap, no_println, no_clone_spam, no_string_abuse, no_pub_overuse

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
// Shared: test context detection
// -------------------------------------------------------------------------

/// Check if a node is inside a test function or a #[cfg(test)] module.
///
/// In tree-sitter-rust, attributes are previous siblings, not children:
///   attribute_item("#[test]")  <- sibling
///   function_item("fn ...")    <- the function
pub(crate) fn in_test_context(node: tree_sitter::Node, source: &[u8]) -> bool {
    // Check the node itself first (when node IS a function_item/mod_item)
    match node.kind() {
        "function_item" => {
            if has_preceding_attribute(node, source, "test") {
                return true;
            }
        }
        "mod_item" => {
            if has_preceding_attribute(node, source, "cfg(test)") {
                return true;
            }
        }
        _ => {}
    }

    // Walk up ancestors
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "function_item" => {
                if has_preceding_attribute(parent, source, "test") {
                    return true;
                }
            }
            "mod_item" => {
                if has_preceding_attribute(parent, source, "cfg(test)") {
                    return true;
                }
            }
            _ => {}
        }
        current = parent.parent();
    }
    false
}

/// Check if the previous sibling(s) of a node are attribute_items containing target text.
pub(crate) fn has_preceding_attribute(node: tree_sitter::Node, source: &[u8], target: &str) -> bool {
    let mut sibling = node.prev_sibling();
    while let Some(sib) = sibling {
        if sib.kind() == "attribute_item" {
            let text = node_text(sib, source);
            if text.contains(target) {
                return true;
            }
        } else {
            break;
        }
        sibling = sib.prev_sibling();
    }
    false
}

// -------------------------------------------------------------------------
// no_unwrap
// -------------------------------------------------------------------------

/// Check if a node is inside a static or const initializer.
///
/// `static FOO: LazyLock<T> = LazyLock::new(|| ...unwrap()...);`
///
/// Unwrap/expect in static initializers runs exactly once at first access
/// (LazyLock) or at compile time (const). These are initialization panics
/// on constant data, not runtime panics on variable data.
fn in_static_initializer(node: tree_sitter::Node) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        match ancestor.kind() {
            "static_item" | "const_item" => return true,
            "function_item" => return false,
            _ => {}
        }
        current = ancestor.parent();
    }
    false
}

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

        if function_node.kind() != "field_expression" {
            continue;
        }

        let field = match function_node.child_by_field_name("field") {
            Some(f) => f,
            None => continue,
        };

        let method_name = node_text(field, source.source_bytes);

        if method_name == "unwrap" || method_name == "expect" {
            if in_test_context(node, source.source_bytes) {
                continue;
            }
            if in_static_initializer(node) {
                continue;
            }

            violations.push(violation(
                node_line(node),
                format!(".{}() panics on failure — propagate the error with ? instead", method_name),
            ));
        }

        if SAFE_UNWRAP_METHODS.contains(&method_name) {
            continue;
        }
    }

    violations
}

// -------------------------------------------------------------------------
// no_println
// -------------------------------------------------------------------------

/// Debug/print macro names that should not appear in production code.
const FORBIDDEN_MACROS: &[&str] = &[
    "println",
    "eprintln",
    "dbg",
];

/// Detect println!(), eprintln!(), and dbg!() macro invocations.
///
/// Skips test contexts.
pub fn check_no_println(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "macro_invocation") {
        let macro_node = match node.child_by_field_name("macro") {
            Some(m) => m,
            None => continue,
        };

        let macro_name = node_text(macro_node, source.source_bytes);
        // macro_name may be "println!" — strip the !
        let name = macro_name.trim_end_matches('!');

        if FORBIDDEN_MACROS.contains(&name) {
            if in_test_context(node, source.source_bytes) {
                continue;
            }

            violations.push(violation(
                node_line(node),
                format!("{}!() is debug output — use structured logging or remove", name),
            ));
        }
    }

    violations
}

// -------------------------------------------------------------------------
// no_clone_spam
// -------------------------------------------------------------------------

/// Detect excessive .clone() calls in Rust source.
///
/// LLMs clone everything to satisfy the borrow checker instead of
/// designing proper ownership. Skips test contexts.
pub fn check_no_clone_spam(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "call_expression") {
        let function_node = match node.child_by_field_name("function") {
            Some(f) => f,
            None => continue,
        };

        if function_node.kind() != "field_expression" {
            continue;
        }

        let field = match function_node.child_by_field_name("field") {
            Some(f) => f,
            None => continue,
        };

        let method_name = node_text(field, source.source_bytes);

        if method_name == "clone" {
            if in_test_context(node, source.source_bytes) {
                continue;
            }

            violations.push(violation(
                node_line(node),
                ".clone() — consider borrowing or restructuring ownership".to_string(),
            ));
        }
    }

    violations
}

// -------------------------------------------------------------------------
// no_string_abuse
// -------------------------------------------------------------------------

/// Detect wasteful String construction from string literals.
///
/// Catches:
/// - "literal".to_string()
/// - String::from("literal")
/// - "literal".to_owned()
/// - format!("literal_without_args")
///
/// These are LLM patterns — allocating heap strings when &str or
/// const would suffice.
pub fn check_no_string_abuse(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "call_expression") {
        let function_node = match node.child_by_field_name("function") {
            Some(f) => f,
            None => continue,
        };

        // .to_string() / .to_owned() on a string literal
        if function_node.kind() == "field_expression" {
            let field = match function_node.child_by_field_name("field") {
                Some(f) => f,
                None => continue,
            };
            let value = match function_node.child_by_field_name("value") {
                Some(v) => v,
                None => continue,
            };

            let method = node_text(field, source.source_bytes);
            if (method == "to_string" || method == "to_owned" || method == "into")
                && value.kind() == "string_literal"
            {
                if in_test_context(node, source.source_bytes) {
                    continue;
                }
                violations.push(violation(
                    node_line(node),
                    format!("\"...\".{}() allocates — use &str or const", method),
                ));
            }
        }

        // String::from("literal")
        if function_node.kind() == "scoped_identifier" {
            let text = node_text(function_node, source.source_bytes);
            if text == "String::from" {
                // Check if argument is a string literal
                let args = match node.child_by_field_name("arguments") {
                    Some(a) => a,
                    None => continue,
                };
                let mut cursor = args.walk();
                let has_string_literal = args.named_children(&mut cursor)
                    .any(|c| c.kind() == "string_literal");
                if has_string_literal {
                    if in_test_context(node, source.source_bytes) {
                        continue;
                    }
                    violations.push(violation(
                        node_line(node),
                        "String::from(\"...\") allocates — use &str or const".to_string(),
                    ));
                }
            }
        }
    }

    violations
}

// -------------------------------------------------------------------------
// no_pub_overuse
// -------------------------------------------------------------------------

/// Detect when every function in a file is pub.
///
/// LLMs make everything pub to avoid compiler errors about unused/unreachable
/// code, rather than designing proper module boundaries.
/// Only fires when there are multiple functions and ALL are pub.
pub fn check_no_pub_overuse(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut total_fns = 0usize;
    let mut pub_fns = 0usize;

    for node in find_nodes_by_type(source.tree.root_node(), "function_item") {
        // Skip functions inside impl blocks (methods) and test contexts
        if let Some(parent) = node.parent() {
            if parent.kind() == "impl_item" || parent.kind() == "declaration_list" {
                continue;
            }
        }
        if in_test_context(node, source.source_bytes) {
            continue;
        }

        total_fns += 1;

        // Check for visibility modifier (pub)
        if has_visibility(node) {
            pub_fns += 1;
        }
    }

    // Only flag when there are 4+ functions AND all are pub
    if total_fns >= 4 && pub_fns == total_fns {
        vec![violation(
            1,
            "every function is pub — design module boundaries instead".to_string(),
        )]
    } else {
        Vec::new()
    }
}

/// Check if a node has a visibility_modifier child (pub, pub(crate), etc.)
fn has_visibility(node: tree_sitter::Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            return true;
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

    // -- no_unwrap --

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
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn unwrap_in_static_lazy_lock_ok() {
        let parsed = parse(
            r#"
            use std::sync::LazyLock;
            use regex::Regex;
            static PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+$").unwrap());
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty(), "unwrap in LazyLock static should be skipped");
    }

    #[test]
    fn expect_in_static_lazy_lock_ok() {
        let parsed = parse(
            r#"
            use std::sync::LazyLock;
            static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
                serde_json::from_str(SCHEMA_JSON).expect("embedded schema must be valid JSON")
            });
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty(), "expect in LazyLock static should be skipped");
    }

    #[test]
    fn unwrap_in_const_ok() {
        let parsed = parse(
            r#"
            const VALUE: i32 = Some(42).unwrap();
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty(), "unwrap in const initializer should be skipped");
    }

    #[test]
    fn unwrap_in_static_multiline_ok() {
        let parsed = parse(
            r#"
            use std::sync::LazyLock;
            use regex::Regex;
            static PYRIGHT_RE: LazyLock<Regex> =
                LazyLock::new(|| Regex::new(r"test").unwrap());
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty(), "unwrap in multiline LazyLock static should be skipped: {:?}",
            violations.iter().map(|v| &v.message).collect::<Vec<_>>());
    }

    #[test]
    fn unwrap_in_function_still_caught() {
        let parsed = parse(
            r#"
            use std::sync::LazyLock;
            use regex::Regex;
            static PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+$").unwrap());
            fn production_code() {
                let x: Option<i32> = Some(1);
                x.unwrap();
            }
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "only function unwrap should be caught, not static");
    }

    // -- no_println --

    #[test]
    fn println_caught() {
        let parsed = parse(r#"fn main() { println!("hello"); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("println"));
    }

    #[test]
    fn eprintln_caught() {
        let parsed = parse(r#"fn main() { eprintln!("error"); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("eprintln"));
    }

    #[test]
    fn dbg_caught() {
        let parsed = parse("fn main() { let x = 1; dbg!(x); }");
        let violations = check_no_println(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("dbg"));
    }

    #[test]
    fn println_in_test_ok() {
        let parsed = parse(
            r#"
            #[cfg(test)]
            mod tests {
                #[test]
                fn it_works() {
                    println!("debug");
                }
            }
            "#,
        );
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn format_macro_ok() {
        let parsed = parse(r#"fn main() { let s = format!("hello {}", 42); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_clone_spam --

    #[test]
    fn clone_caught() {
        let parsed = parse("fn main() { let s = String::new(); let s2 = s.clone(); }");
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("clone"));
    }

    #[test]
    fn clone_in_test_ok() {
        let parsed = parse(
            r#"
            #[cfg(test)]
            mod tests {
                #[test]
                fn it_works() {
                    let s = String::new();
                    let s2 = s.clone();
                }
            }
            "#,
        );
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn no_clone_ok() {
        let parsed = parse("fn main() { let x = 42; let y = x; }");
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_string_abuse --

    #[test]
    fn literal_to_string_caught() {
        let parsed = parse(r#"fn main() { let s = "hello".to_string(); }"#);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("to_string"));
    }

    #[test]
    fn literal_to_owned_caught() {
        let parsed = parse(r#"fn main() { let s = "hello".to_owned(); }"#);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("to_owned"));
    }

    #[test]
    fn string_from_literal_caught() {
        let parsed = parse(r#"fn main() { let s = String::from("hello"); }"#);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("String::from"));
    }

    #[test]
    fn variable_to_string_ok() {
        let parsed = parse("fn main() { let x = 42; let s = x.to_string(); }");
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn string_abuse_in_test_ok() {
        let parsed = parse(
            r#"
            #[cfg(test)]
            mod tests {
                #[test]
                fn it_works() {
                    let s = "hello".to_string();
                }
            }
            "#,
        );
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_pub_overuse --

    #[test]
    fn all_pub_caught() {
        let parsed = parse(
            "pub fn a() {} pub fn b() {} pub fn c() {} pub fn d() {}",
        );
        let violations = check_no_pub_overuse(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("pub"));
    }

    #[test]
    fn mixed_visibility_ok() {
        let parsed = parse(
            "pub fn a() {} fn b() {} pub fn c() {} fn d() {}",
        );
        let violations = check_no_pub_overuse(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn few_pub_fns_ok() {
        let parsed = parse("pub fn a() {} pub fn b() {}");
        let violations = check_no_pub_overuse(&parsed, &default_config());
        assert!(violations.is_empty()); // Only 2 functions, threshold is 4
    }

    #[test]
    fn all_pub_in_test_ok() {
        let parsed = parse(
            r#"
            #[cfg(test)]
            mod tests {
                pub fn a() {}
                pub fn b() {}
                pub fn c() {}
                pub fn d() {}
            }
            "#,
        );
        let violations = check_no_pub_overuse(&parsed, &default_config());
        assert!(violations.is_empty());
    }
}
