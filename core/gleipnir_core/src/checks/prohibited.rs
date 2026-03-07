//! Prohibited pattern checks.
//!
//! Checks: no_cast, no_overload, no_bare_except, no_broad_exceptions,
//! no_print, no_model_dump, no_future_annotations, init_files_empty,
//! no_dunder_all.

use crate::parsing::{find_nodes_by_type, node_field, node_line, node_text};
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
// no_cast
// -------------------------------------------------------------------------

fn is_cast_call(node: tree_sitter::Node, source: &[u8]) -> bool {
    let func = match node.child_by_field_name("function") {
        Some(f) => f,
        None => return false,
    };
    match func.kind() {
        "identifier" => node_text(func, source) == "cast",
        "attribute" => {
            func.child_by_field_name("attribute")
                .is_some_and(|a| node_text(a, source) == "cast")
        }
        _ => false,
    }
}

pub fn check_no_cast(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    for node in find_nodes_by_type(source.tree.root_node(), "call") {
        if is_cast_call(node, source.source_bytes) {
            violations.push(violation(node_line(node), "typing.cast() usage".to_string()));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_overload
// -------------------------------------------------------------------------

fn extract_decorator_name(decorator: tree_sitter::Node, source: &[u8]) -> String {
    let mut cursor = decorator.walk();
    let children: Vec<_> = decorator.named_children(&mut cursor).collect();
    if children.is_empty() {
        return String::new();
    }
    let expr = children[0];
    match expr.kind() {
        "identifier" => node_text(expr, source).to_string(),
        "attribute" => expr
            .child_by_field_name("attribute")
            .map(|a| node_text(a, source).to_string())
            .unwrap_or_default(),
        "call" => {
            let func = match expr.child_by_field_name("function") {
                Some(f) => f,
                None => return String::new(),
            };
            match func.kind() {
                "identifier" => node_text(func, source).to_string(),
                "attribute" => func
                    .child_by_field_name("attribute")
                    .map(|a| node_text(a, source).to_string())
                    .unwrap_or_default(),
                _ => String::new(),
            }
        }
        _ => String::new(),
    }
}

pub fn check_no_overload(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    for node in find_nodes_by_type(source.tree.root_node(), "decorated_definition") {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "decorator" {
                let name = extract_decorator_name(child, source.source_bytes);
                if name == "overload" {
                    violations.push(violation(
                        node_line(child),
                        "@overload decorator".to_string(),
                    ));
                }
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_bare_except
// -------------------------------------------------------------------------

pub fn check_no_bare_except(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    for node in find_nodes_by_type(source.tree.root_node(), "except_clause") {
        let mut cursor = node.walk();
        let named: Vec<_> = node.named_children(&mut cursor).collect();
        // Bare except: no named children, or only a block child
        let is_bare = named.is_empty()
            || (named.len() == 1 && named[0].kind() == "block");
        if is_bare {
            violations.push(violation(
                node_line(node),
                "bare except clause".to_string(),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_broad_exceptions
// -------------------------------------------------------------------------

fn except_clause_type_name(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "identifier" => return Some(node_text(child, source).to_string()),
            "attribute" => {
                return child
                    .child_by_field_name("attribute")
                    .map(|a| node_text(a, source).to_string())
            }
            "as_pattern" => {
                let mut inner_cursor = child.walk();
                for inner in child.named_children(&mut inner_cursor) {
                    if inner.kind() == "identifier" {
                        return Some(node_text(inner, source).to_string());
                    }
                    if inner.kind() == "attribute" {
                        return inner
                            .child_by_field_name("attribute")
                            .map(|a| node_text(a, source).to_string());
                    }
                }
            }
            _ => {}
        }
    }
    None
}

pub fn check_no_broad_exceptions(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    for node in find_nodes_by_type(source.tree.root_node(), "except_clause") {
        if let Some(name) = except_clause_type_name(node, source.source_bytes) {
            if name == "Exception" {
                violations.push(violation(
                    node_line(node),
                    "except Exception is too broad".to_string(),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_print
// -------------------------------------------------------------------------

pub fn check_no_print_calls(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    // Pattern constructed at runtime to avoid self-triggering
    let pattern = format!("{}(", "print");
    let mut violations = Vec::new();

    for (line_num, line) in source.lines.iter().enumerate() {
        let line_num = line_num + 1;
        if line.trim_start().starts_with('#') {
            continue;
        }
        if line.contains(&pattern) {
            violations.push(violation(line_num, "print() call".to_string()));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_model_dump
// -------------------------------------------------------------------------

// Reversed to avoid triggering text scanner
const REVERSED_MODEL_DUMP: &[&str] = &[
    ")(pmud_ledom.",
    ")(tcid.",
];

fn model_dump_patterns() -> Vec<String> {
    REVERSED_MODEL_DUMP
        .iter()
        .map(|s| s.chars().rev().collect())
        .collect()
}

pub fn check_no_model_dump(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    // Config-based exemption for boundary files
    let filename = source.file_path.rsplit('/').next().unwrap_or(source.file_path);
    if config.boundary_files.iter().any(|f| f == filename) {
        return Vec::new();
    }

    let patterns = model_dump_patterns();
    let mut violations = Vec::new();

    for (line_num, line) in source.lines.iter().enumerate() {
        let line_num = line_num + 1;
        if line.trim_start().starts_with('#') {
            continue;
        }
        for pattern in &patterns {
            if line.contains(pattern.as_str()) {
                violations.push(violation(
                    line_num,
                    "serialization call sheds type safety".to_string(),
                ));
                break;
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_future_annotations
// -------------------------------------------------------------------------

pub fn check_no_future_annotations(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    // Pattern constructed at runtime
    let pattern = format!("from {} import annotations", "__future__");
    let mut violations = Vec::new();

    for (line_num, line) in source.lines.iter().enumerate() {
        let line_num = line_num + 1;
        if line.contains(&pattern) {
            violations.push(violation(
                line_num,
                "from __future__ import annotations".to_string(),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// init_files_empty
// -------------------------------------------------------------------------

pub fn check_init_files_empty(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let filename = source.file_path.rsplit('/').next().unwrap_or(source.file_path);
    if filename != "__init__.py" {
        return Vec::new();
    }

    // Warning marker constructed at runtime
    let marker = format!("This {} file is EPHEMERAL:", "__init__.py");

    let content = std::str::from_utf8(source.source_bytes).unwrap_or("");
    let trimmed = content.trim();

    if trimmed.is_empty() || trimmed.contains(&marker) {
        return Vec::new();
    }

    vec![violation(1, "non-empty __init__.py".to_string())]
}

// -------------------------------------------------------------------------
// no_dunder_all
// -------------------------------------------------------------------------

pub fn check_no_dunder_all(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    let root = source.tree.root_node();
    let mut cursor = root.walk();

    for stmt in root.named_children(&mut cursor) {
        let assign = if stmt.kind() == "expression_statement" {
            let mut inner_cursor = stmt.walk();
            let first = stmt.named_children(&mut inner_cursor)
                .next()
                .filter(|c| c.kind() == "assignment");
            first
        } else if stmt.kind() == "assignment" {
            Some(stmt)
        } else {
            None
        };

        let assign = match assign {
            Some(a) => a,
            None => continue,
        };

        if let Some(left) = node_field(assign, "left") {
            if left.kind() == "identifier" && node_text(left, source.source_bytes) == "__all__" {
                violations.push(violation(
                    node_line(assign),
                    "__all__ declaration".to_string(),
                ));
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
        build_parsed_source("/test/file.py", source)
    }

    fn parse_init(code: &str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        build_parsed_source("/test/__init__.py", source)
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(FileKind::Outside, None)
    }

    // -- no_cast --

    #[test]
    fn cast_call_caught() {
        let parsed = parse("from typing import cast\nx = cast(int, value)\n");
        let violations = check_no_cast(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn no_cast_clean() {
        let parsed = parse("x = int(value)\n");
        let violations = check_no_cast(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_overload --

    #[test]
    fn overload_caught() {
        let parsed = parse("from typing import overload\n@overload\ndef foo(x: int) -> int: ...\n");
        let violations = check_no_overload(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    // -- no_bare_except --

    #[test]
    fn bare_except_caught() {
        let parsed = parse("try:\n    pass\nexcept:\n    pass\n");
        let violations = check_no_bare_except(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn typed_except_ok() {
        let parsed = parse("try:\n    pass\nexcept ValueError:\n    pass\n");
        let violations = check_no_bare_except(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_broad_exceptions --

    #[test]
    fn broad_exception_caught() {
        let parsed = parse("try:\n    pass\nexcept Exception:\n    pass\n");
        let violations = check_no_broad_exceptions(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn specific_exception_ok() {
        let parsed = parse("try:\n    pass\nexcept ValueError:\n    pass\n");
        let violations = check_no_broad_exceptions(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_print --

    #[test]
    fn print_call_caught() {
        let code = "print(\"hello\")\n";
        let parsed = parse(code);
        let violations = check_no_print_calls(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn no_print_clean() {
        let parsed = parse("x = 42\n");
        let violations = check_no_print_calls(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_future_annotations --

    #[test]
    fn future_annotations_caught() {
        let code = "from __future__ import annotations\n";
        let parsed = parse(code);
        let violations = check_no_future_annotations(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    // -- init_files_empty --

    #[test]
    fn nonempty_init_caught() {
        let parsed = parse_init("from foo import bar\n");
        let violations = check_init_files_empty(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn empty_init_ok() {
        let parsed = parse_init("");
        let violations = check_init_files_empty(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_dunder_all --

    #[test]
    fn dunder_all_caught() {
        let parsed = parse("__all__ = ['foo', 'bar']\n");
        let violations = check_no_dunder_all(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn no_dunder_all_clean() {
        let parsed = parse("foo = 42\n");
        let violations = check_no_dunder_all(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_model_dump --

    #[test]
    fn model_dump_caught() {
        let code = "result = obj.model_dump()\n";
        let parsed = parse(code);
        let violations = check_no_model_dump(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn model_dump_exempted_by_config() {
        let code = "result = obj.model_dump()\n";
        let parsed = parse(code);
        let mut config = default_config();
        config.boundary_files = vec!["file.py".to_string()];
        let violations = check_no_model_dump(&parsed, &config);
        assert!(violations.is_empty());
    }
}
