//! Prohibited pattern checks for Rust source files.
//!
//! Checks: no_unwrap, no_println, no_clone_spam, no_string_abuse, no_pub_overuse

use crate::parsing::{find_nodes_by_type, node_line, node_text};
use crate::structures::{CheckConfig, ParsedSource, Severity, Violation};

fn violation(line: usize, message: impl Into<String>) -> Violation {
    Violation {
        line,
        check_name: String::new(),
        severity: Severity::Error,
        message: message.into(),
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
/// Check if a closure node is passed to an initialization method (get_or_init, get_or_try_init).
fn is_init_closure(closure: tree_sitter::Node, source: &[u8]) -> bool {
    let args = match closure.parent() {
        Some(node) if node.kind() == "arguments" => node,
        _ => return false,
    };
    let call_expr = match args.parent() {
        Some(node) => node,
        None => return false,
    };
    let func = match call_expr.child_by_field_name("function") {
        Some(node) if node.kind() == "field_expression" => node,
        _ => return false,
    };
    let field = match func.child_by_field_name("field") {
        Some(node) => node,
        None => return false,
    };
    let method = node_text(field, source);
    method == "get_or_init" || method == "get_or_try_init"
}

fn in_initialization_context(node: tree_sitter::Node, source: &[u8]) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        match ancestor.kind() {
            "static_item" | "const_item" => return true,
            "function_item" => return false,
            "closure_expression" if is_init_closure(ancestor, source) => return true,
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
            if in_initialization_context(node, source.source_bytes) {
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
    "dbg",
];

/// Check if the file path ends with main.rs (binary crate entry point).
fn is_binary_main(file_path: &str) -> bool {
    file_path.rsplit('/').next() == Some("main.rs")
}

/// Check if a node is inside a function whose name indicates stdout output.
fn in_output_function(node: tree_sitter::Node, source: &[u8]) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if ancestor.kind() == "function_item" {
            let name = ancestor
                .child_by_field_name("name")
                .map(|n| node_text(n, source))
                .unwrap_or("");
            return name.starts_with("print_")
                || name.starts_with("emit_")
                || name.starts_with("display_")
                || name == "print_help"
                || name == "print_usage";
        }
        current = ancestor.parent();
    }
    false
}

/// Detect println!() and dbg!() macro invocations.
///
/// Skips:
/// - Test contexts (#[test], #[cfg(test)])
/// - println! in main.rs files (binary crates use stdout as their interface)
/// - println! inside output functions (print_*, emit_*, display_*)
///
/// dbg!() is always flagged — it is never intentional in production code.
pub fn check_no_println(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    let in_binary = is_binary_main(source.file_path);

    for node in find_nodes_by_type(source.tree.root_node(), "macro_invocation") {
        let macro_node = match node.child_by_field_name("macro") {
            Some(m) => m,
            None => continue,
        };

        let macro_name = node_text(macro_node, source.source_bytes);
        let name = macro_name.trim_end_matches('!');

        if !FORBIDDEN_MACROS.contains(&name) {
            continue;
        }
        if in_test_context(node, source.source_bytes) {
            continue;
        }

        // println! gets contextual treatment; dbg! is always caught
        if name == "println" && (in_binary || in_output_function(node, source.source_bytes)) {
            continue;
        }

        violations.push(violation(
            node_line(node),
            format!("{}!() is debug output — use structured logging or remove", name),
        ));
    }

    violations
}

// -------------------------------------------------------------------------
// no_clone_spam
// -------------------------------------------------------------------------

/// Context-aware clone detection.
///
/// Skips ownership transfers (struct fields, function arguments, collection
/// inserts, iterator pipelines, return position, Arc::clone, enum wrapping,
/// fallback/error closures). Flags clones in assignments and let-bindings
/// where borrowing or restructuring would avoid the allocation.
pub fn check_no_clone_spam(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "call_expression") {
        let function_node = match node.child_by_field_name("function") {
            Some(f) => f,
            None => continue,
        };

        // Arc::clone(&x) / Rc::clone(&x) — always skip (refcount bump)
        if is_arc_rc_clone(function_node, source.source_bytes) {
            continue;
        }

        // .clone() — method call on a field_expression
        if !is_method_call(function_node, "clone", source.source_bytes) {
            continue;
        }
        if in_test_context(node, source.source_bytes) {
            continue;
        }
        if clone_is_ownership_transfer(node, source.source_bytes) {
            continue;
        }

        violations.push(violation(
            node_line(node),
            ".clone() — consider borrowing or restructuring ownership",
        ));
    }

    violations
}

fn is_method_call(function_node: tree_sitter::Node, method: &str, source: &[u8]) -> bool {
    if function_node.kind() != "field_expression" {
        return false;
    }
    match function_node.child_by_field_name("field") {
        Some(f) => node_text(f, source) == method,
        None => false,
    }
}

fn is_arc_rc_clone(function_node: tree_sitter::Node, source: &[u8]) -> bool {
    if function_node.kind() != "scoped_identifier" {
        return false;
    }
    let text = node_text(function_node, source);
    text == "Arc::clone" || text == "Rc::clone"
}

/// Check if a closure is an argument to an iterator adaptor method.
fn is_iterator_adaptor_closure(closure: tree_sitter::Node, source: &[u8]) -> bool {
    let args = match closure.parent() {
        Some(n) if n.kind() == "arguments" => n,
        _ => return false,
    };
    let call = match args.parent() {
        Some(n) if n.kind() == "call_expression" => n,
        _ => return false,
    };
    is_method_call_to(call, source, &["map", "filter_map", "flat_map", "for_each", "and_then"])
}

/// Check if a closure is an argument to a fallback, error, or collection entry method.
fn is_fallback_or_error_closure(closure: tree_sitter::Node, source: &[u8]) -> bool {
    let args = match closure.parent() {
        Some(n) if n.kind() == "arguments" => n,
        _ => return false,
    };
    let call = match args.parent() {
        Some(n) if n.kind() == "call_expression" => n,
        _ => return false,
    };
    is_method_call_to(call, source, &[
        "unwrap_or_else", "ok_or_else", "map_err", "or_else",
        "or_insert_with", "get_or_insert_with",
    ])
}

fn is_method_call_to(call: tree_sitter::Node, source: &[u8], methods: &[&str]) -> bool {
    let func = match call.child_by_field_name("function") {
        Some(f) if f.kind() == "field_expression" => f,
        _ => return false,
    };
    let field = match func.child_by_field_name("field") {
        Some(f) => f,
        None => return false,
    };
    methods.contains(&node_text(field, source))
}

/// Check if a node is part of the last expression in a block (implicit return).
fn is_implicit_return(block: tree_sitter::Node, target: tree_sitter::Node) -> bool {
    // Counts as implicit return if block belongs to a function, match arm, or closure
    match block.parent() {
        Some(p) if matches!(p.kind(), "function_item" | "match_arm" | "closure_expression") => {}
        _ => return false,
    }
    let mut cursor = block.walk();
    match block.named_children(&mut cursor).last() {
        Some(last) => {
            target.start_byte() >= last.start_byte()
                && target.end_byte() <= last.end_byte()
        }
        None => false,
    }
}

/// Walk up the AST from a .clone() call to determine if it's an ownership transfer.
fn clone_is_ownership_transfer(clone_call: tree_sitter::Node, source: &[u8]) -> bool {
    let mut current = clone_call.parent();
    while let Some(node) = current {
        match node.kind() {
            // Struct literal field — building owned struct from borrowed data
            "field_initializer" => return true,

            // Explicit return
            "return_expression" => return true,

            // Match arm value — the clone IS the arm's output (no block wrapper)
            "match_arm" => return true,

            // Call expression — allocation consumed by any function/method call.
            // The callee takes ownership: Type::new(x.clone()), method(x.clone()),
            // free_fn(x.clone()), collection.insert(x.clone()) all need owned values.
            "call_expression" => return true,

            // Closure — skip if passed to an iterator adaptor or fallback/error combinator
            "closure_expression" => {
                return is_iterator_adaptor_closure(node, source)
                    || is_fallback_or_error_closure(node, source);
            }

            // Block — check for implicit return (last expression in function body)
            "block" => return is_implicit_return(node, clone_call),

            // Transparent wrappers — keep walking up
            "arguments" | "parenthesized_expression" | "reference_expression"
            | "try_expression" | "type_cast_expression" | "assignment_expression"
            | "tuple_expression" | "array_expression" => {
                current = node.parent();
                continue;
            }

            // Anything else — not a recognized transfer
            _ => return false,
        }
    }
    false
}

// -------------------------------------------------------------------------
// no_string_abuse
// -------------------------------------------------------------------------

/// Detect wasteful String construction from string literals.
///
/// Catches "literal".to_string(), String::from("literal"), "literal".to_owned().
/// These are LLM patterns — allocating heap strings when &str or const would suffice.
pub fn check_no_string_abuse(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "call_expression") {
        if in_test_context(node, source.source_bytes) {
            continue;
        }
        if let Some(msg) = detect_literal_method_abuse(node, source.source_bytes) {
            // Same ownership taxonomy as clone — if the allocation flows into a
            // struct field, return, collection method, or enum constructor, the
            // owned String is required and flagging it is a false positive.
            if !clone_is_ownership_transfer(node, source.source_bytes) {
                violations.push(violation(node_line(node), msg));
            }
            continue;
        }
        if detect_string_from_literal(node, source.source_bytes) {
            if !clone_is_ownership_transfer(node, source.source_bytes) {
                violations.push(violation(
                    node_line(node),
                    "String::from(\"...\") allocates — use &str or const",
                ));
            }
        }
    }

    violations
}

/// "literal".to_string() / "literal".to_owned() / "literal".into()
fn detect_literal_method_abuse(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let func = node.child_by_field_name("function")?;
    if func.kind() != "field_expression" {
        return None;
    }
    let field = func.child_by_field_name("field")?;
    let value = func.child_by_field_name("value")?;
    let method = node_text(field, source);
    if !matches!(method, "to_string" | "to_owned" | "into") {
        return None;
    }
    if value.kind() != "string_literal" {
        return None;
    }
    Some(format!("\"...\".{method}() allocates — use &str or const"))
}

/// String::from("literal")
fn detect_string_from_literal(node: tree_sitter::Node, source: &[u8]) -> bool {
    let func = match node.child_by_field_name("function") {
        Some(f) if f.kind() == "scoped_identifier" => f,
        _ => return false,
    };
    if node_text(func, source) != "String::from" {
        return false;
    }
    let args = match node.child_by_field_name("arguments") {
        Some(a) => a,
        None => return false,
    };
    let mut cursor = args.walk();
    let has_string = args.named_children(&mut cursor).any(|c| c.kind() == "string_literal");
    has_string
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
    // lib.rs and mod.rs are structural entry points — pub is expected
    let filename = source.file_path.rsplit('/').next().unwrap_or(source.file_path);
    if filename == "lib.rs" || filename == "mod.rs" {
        return Vec::new();
    }

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

        if is_pub_unrestricted(node, source.source_bytes) {
            pub_fns += 1;
        }
    }

    // Only flag when there are 4+ functions AND all are bare pub
    if total_fns >= 4 && pub_fns == total_fns {
        vec![violation(
            1,
            "every function is pub — design module boundaries instead".to_string(),
        )]
    } else {
        Vec::new()
    }
}

/// Check if a node has bare `pub` visibility (not `pub(crate)` or `pub(super)`).
/// Restricted visibility shows intentional boundary design and should not be flagged.
fn is_pub_unrestricted(node: tree_sitter::Node, source: &[u8]) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            let text = node_text(child, source);
            // bare "pub" vs "pub(crate)", "pub(super)", "pub(in ...)"
            return text == "pub";
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
        build_parsed_source_rust("/test/file.rs", source).unwrap()
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(FileKind::Outside)
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

    #[test]
    fn expect_in_get_or_init_ok() {
        let parsed = parse(
            r#"
            use std::sync::OnceLock;
            struct Schema { validator: OnceLock<String> }
            impl Schema {
                fn get_validator(&self) -> &String {
                    self.validator.get_or_init(|| {
                        let data = "test".to_string();
                        data.parse().expect("must be valid")
                    })
                }
            }
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert!(violations.is_empty(), "expect in get_or_init closure should be skipped: {:?}",
            violations.iter().map(|v| &v.message).collect::<Vec<_>>());
    }

    #[test]
    fn unwrap_in_regular_closure_still_caught() {
        let parsed = parse(
            r#"
            fn process() {
                let items: Vec<i32> = vec![1, 2, 3];
                let results: Vec<i32> = items.iter().map(|x| {
                    Some(*x).unwrap()
                }).collect();
            }
            "#,
        );
        let violations = check_no_unwrap(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "unwrap in regular closure should still be caught");
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
    fn eprintln_ok() {
        let parsed = parse(r#"fn main() { eprintln!("error"); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty(), "eprintln is legitimate error output for CLI tools");
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
    fn println_in_main_rs_ok() {
        let source: &'static [u8] =
            Box::leak(b"fn main() { println!(\"starting\"); }".to_vec().into_boxed_slice());
        let parsed = build_parsed_source_rust("/app/src/main.rs", source).unwrap();
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty(), "println in main.rs should be skipped");
    }

    #[test]
    fn dbg_in_main_rs_still_caught() {
        let source: &'static [u8] =
            Box::leak(b"fn main() { let x = 1; dbg!(x); }".to_vec().into_boxed_slice());
        let parsed = build_parsed_source_rust("/app/src/main.rs", source).unwrap();
        let violations = check_no_println(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "dbg! should be caught even in main.rs");
    }

    #[test]
    fn println_in_output_function_ok() {
        let parsed = parse(r#"fn print_help() { println!("Usage: tool [options]"); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty(), "println in print_help should be skipped");
    }

    #[test]
    fn println_in_emit_function_ok() {
        let parsed = parse(r#"fn emit_report() { println!("Report:"); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty(), "println in emit_report should be skipped");
    }

    #[test]
    fn println_in_regular_function_caught() {
        let parsed = parse(r#"fn process() { println!("debug"); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "println in regular function should be caught");
    }

    #[test]
    fn format_macro_ok() {
        let parsed = parse(r#"fn main() { let s = format!("hello {}", 42); }"#);
        let violations = check_no_println(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_clone_spam --

    #[test]
    fn bare_clone_caught() {
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

    // -- clone skip rules --

    #[test]
    fn clone_in_struct_field_ok() {
        let code = r#"
            struct Issue { tool: String }
            fn build(item: &Issue) -> Issue {
                Issue { tool: item.tool.clone() }
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "struct field clone should be skipped");
    }

    #[test]
    fn clone_in_collection_insert_ok() {
        let code = r#"
            fn build() {
                let mut map = std::collections::HashMap::new();
                let key = String::from("k");
                map.insert(key.clone(), 42);
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "collection insert clone should be skipped");
    }

    #[test]
    fn clone_in_vec_push_ok() {
        let code = r#"
            fn build() {
                let mut items: Vec<String> = Vec::new();
                let val = String::from("v");
                items.push(val.clone());
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "vec push clone should be skipped");
    }

    #[test]
    fn clone_in_iter_map_ok() {
        let code = r#"
            fn transform(items: &[String]) -> Vec<String> {
                items.iter().map(|s| s.clone()).collect()
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "iterator map clone should be skipped");
    }

    #[test]
    fn clone_in_return_ok() {
        let code = r#"
            fn get_name(data: &String) -> String {
                return data.clone();
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "explicit return clone should be skipped");
    }

    #[test]
    fn clone_in_implicit_return_ok() {
        let code = r#"
            fn get_name(data: &String) -> String {
                data.clone()
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "implicit return clone should be skipped");
    }

    #[test]
    fn arc_clone_ok() {
        let code = r#"
            fn share(data: &std::sync::Arc<String>) -> std::sync::Arc<String> {
                Arc::clone(data)
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "Arc::clone should be skipped");
    }

    #[test]
    fn clone_in_some_ok() {
        let code = r#"
            fn wrap(path: &String) -> Option<String> {
                Some(path.clone())
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "Some() clone should be skipped");
    }

    #[test]
    fn clone_in_match_arm_ok() {
        let code = r#"
            fn get_value(opt: &Option<String>) -> String {
                match opt {
                    Some(s) => s.clone(),
                    None => String::new(),
                }
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "clone in match arm should be skipped");
    }

    #[test]
    fn clone_in_match_arm_block_ok() {
        let code = r#"
            fn get_value(opt: &Option<String>) -> String {
                match opt {
                    Some(s) => {
                        let _len = s.len();
                        s.clone()
                    }
                    None => String::new(),
                }
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "clone as last expr in match arm block should be skipped");
    }

    #[test]
    fn clone_in_tuple_ok() {
        let code = r#"
            fn build(name: &String, value: &String) -> (String, String) {
                (name.clone(), value.clone())
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "clone in tuple expression should be skipped");
    }

    #[test]
    fn clone_in_ok_variant_ok() {
        let code = r#"
            fn wrap(data: &String) -> Result<String, ()> {
                Ok(data.clone())
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "Ok() clone should be skipped");
    }

    #[test]
    fn clone_as_function_arg_ok() {
        let code = r#"
            fn setup(path: String) {}
            fn main() {
                let config_path = String::from("/tmp/socket");
                setup(config_path.clone());
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "clone passed as function argument should be skipped");
    }

    #[test]
    fn clone_in_type_constructor_ok() {
        let code = r#"
            fn convert(name: &String) {
                let val = Value::String(name.clone());
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "clone in type constructor should be skipped");
    }

    #[test]
    fn clone_in_or_insert_with_ok() {
        let code = r#"
            fn group(items: &[Issue]) {
                let mut groups = std::collections::HashMap::new();
                for item in items {
                    groups.entry("key").or_insert_with(|| item.clone());
                }
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert!(violations.is_empty(), "clone in or_insert_with closure should be skipped");
    }

    #[test]
    fn clone_in_assignment_still_caught() {
        let code = r#"
            struct Data { field: String }
            fn update(data: &mut Data, source: &Data) {
                data.field = source.field.clone();
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_clone_spam(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "clone in assignment should still be caught");
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

    #[test]
    fn to_string_in_struct_field_ok() {
        let code = r#"
            struct Config { name: String }
            fn build() -> Config {
                Config { name: "default".to_string() }
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "to_string in struct field should be skipped");
    }

    #[test]
    fn into_in_map_insert_ok() {
        let code = r#"
            fn build() {
                let mut map = std::collections::HashMap::new();
                map.insert("key".into(), 42);
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "into() in map insert should be skipped");
    }

    #[test]
    fn to_string_in_return_ok() {
        let code = r#"
            fn default_name() -> String {
                return "unknown".to_string();
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "to_string in return should be skipped");
    }

    #[test]
    fn to_string_in_some_ok() {
        let code = r#"
            fn maybe_name() -> Option<String> {
                Some("default".to_string())
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "to_string in Some() should be skipped");
    }

    #[test]
    fn string_from_in_struct_field_ok() {
        let code = r#"
            struct Config { name: String }
            fn build() -> Config {
                Config { name: String::from("default") }
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "String::from in struct field should be skipped");
    }

    #[test]
    fn to_string_as_function_arg_ok() {
        let code = r#"
            fn setup(name: String) {}
            fn main() {
                setup("default".to_string());
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "to_string passed as function arg should be skipped");
    }

    #[test]
    fn into_in_or_insert_with_closure_ok() {
        let code = r#"
            fn build() {
                let mut map = std::collections::HashMap::new();
                map.entry("key").or_insert_with(|| "default".to_string());
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert!(violations.is_empty(), "to_string in or_insert_with closure should be skipped");
    }

    #[test]
    fn to_string_in_let_binding_still_caught() {
        let code = r#"
            fn main() {
                let name = "hello".to_string();
            }
        "#;
        let parsed = parse(code);
        let violations = check_no_string_abuse(&parsed, &default_config());
        assert_eq!(violations.len(), 1, "to_string in bare let binding should still be caught");
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

    #[test]
    fn pub_crate_shows_boundary_design() {
        let source: &'static [u8] = Box::leak(
            b"pub fn a() {} pub fn b() {} pub fn c() {} pub(crate) fn d() {}".to_vec().into_boxed_slice(),
        );
        let parsed = build_parsed_source_rust("/test/file.rs", source).unwrap();
        let violations = check_no_pub_overuse(&parsed, &default_config());
        assert!(violations.is_empty()); // pub(crate) means intentional scoping
    }

    #[test]
    fn lib_rs_exempt() {
        let source: &'static [u8] = Box::leak(
            b"pub fn a() {} pub fn b() {} pub fn c() {} pub fn d() {}".to_vec().into_boxed_slice(),
        );
        let parsed = build_parsed_source_rust("/test/lib.rs", source).unwrap();
        let violations = check_no_pub_overuse(&parsed, &default_config());
        assert!(violations.is_empty()); // lib.rs is a crate entry point
    }

    #[test]
    fn mod_rs_exempt() {
        let source: &'static [u8] = Box::leak(
            b"pub fn a() {} pub fn b() {} pub fn c() {} pub fn d() {}".to_vec().into_boxed_slice(),
        );
        let parsed = build_parsed_source_rust("/test/mod.rs", source).unwrap();
        let violations = check_no_pub_overuse(&parsed, &default_config());
        assert!(violations.is_empty()); // mod.rs is a module entry point
    }
}
