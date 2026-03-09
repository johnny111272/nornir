//! Type system enforcement checks.
//!
//! Checks: no_object, no_json_value, no_any_types, no_any_type_aliases,
//! no_bare_collections, union_member_count, no_implicit_type_aliases.

use crate::parsing::{
    annotation_contains_name, bare_names_in_annotation, count_union_members,
    find_nodes_by_type, find_type_annotations, node_field, node_line, node_text, walk_tree,
};
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

/// Check if file matches any pattern in unsafe_files config.
fn file_matches_unsafe(file_path: &str, unsafe_files: &[String]) -> bool {
    let filename = file_path.rsplit('/').next().unwrap_or(file_path);
    unsafe_files.iter().any(|pat| {
        if pat.contains('*') {
            // Simple glob: only support trailing *
            let prefix = pat.trim_end_matches('*');
            filename.starts_with(prefix)
        } else {
            filename == pat
        }
    })
}

pub fn check_no_object(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for type_node in find_type_annotations(source.tree.root_node(), source.source_bytes) {
        if annotation_contains_name(type_node, "object", source.source_bytes) {
            let text = node_text(type_node, source.source_bytes);
            violations.push(violation(
                node_line(type_node),
                format!("object in type annotation: {text}"),
            ));
        }
    }
    violations
}

pub fn check_no_json_value(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for type_node in find_type_annotations(source.tree.root_node(), source.source_bytes) {
        if annotation_contains_name(type_node, "JsonValue", source.source_bytes) {
            let text = node_text(type_node, source.source_bytes);
            violations.push(violation(
                node_line(type_node),
                format!("JsonValue in type annotation: {text}"),
            ));
        }
    }
    violations
}

pub fn check_no_any_types(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    // Config-based exemption for OUTSIDE files with exceptions
    if file_matches_unsafe(source.file_path, &config.unsafe_files) {
        return Vec::new();
    }

    let mut violations = Vec::new();

    for type_node in find_type_annotations(source.tree.root_node(), source.source_bytes) {
        if annotation_contains_name(type_node, "Any", source.source_bytes) {
            let text = node_text(type_node, source.source_bytes);
            violations.push(violation(
                node_line(type_node),
                format!("Any in type annotation: {text}"),
            ));
        }
    }
    violations
}

static BARE_COLLECTION_NAMES: &[&str] = &["dict", "list", "set", "tuple"];

pub fn check_no_bare_collections(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for type_node in find_type_annotations(source.tree.root_node(), source.source_bytes) {
        for (line, name) in
            bare_names_in_annotation(type_node, BARE_COLLECTION_NAMES, source.source_bytes)
        {
            violations.push(violation(line, format!("bare {name} in type annotation")));
        }
    }
    violations
}

/// Find top-level union binary_operator nodes within an annotation subtree.
/// Returns (line, member_count) for each union root.
fn find_union_roots(type_node: tree_sitter::Node) -> Vec<(usize, usize)> {
    let mut results = Vec::new();

    for node in walk_tree(type_node) {
        if node.kind() != "binary_operator" {
            continue;
        }
        // Skip if parent is also binary_operator (not the outermost union)
        if let Some(parent) = node.parent() {
            if parent.kind() == "binary_operator" {
                continue;
            }
        }
        let member_count = count_union_members(node);
        if member_count > 1 {
            results.push((node_line(node), member_count));
        }
    }
    results
}

pub fn check_union_member_count(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for type_node in find_type_annotations(source.tree.root_node(), source.source_bytes) {
        for (line, count) in find_union_roots(type_node) {
            if count > config.max_union_members {
                violations.push(violation(
                    line,
                    "union has too many members".to_string(),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_any_type_aliases
// -------------------------------------------------------------------------

pub fn check_no_any_type_aliases(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "type_alias_statement") {
        // Extract alias name (second named child of kind "type" wrapping an identifier)
        // and value (last named child of kind "type")
        let mut cursor = node.walk();
        let type_children: Vec<_> = node
            .named_children(&mut cursor)
            .filter(|c| c.kind() == "type")
            .collect();

        // Need at least 2: name wrapper and value wrapper
        if type_children.len() < 2 {
            continue;
        }

        let name_node = type_children[0];
        let value_node = *type_children.last().unwrap();

        let alias_name = name_node
            .named_child(0)
            .map(|n| node_text(n, source.source_bytes))
            .unwrap_or("<unknown>");

        if annotation_contains_name(value_node, "Any", source.source_bytes) {
            violations.push(violation(
                node_line(node),
                format!("type alias '{alias_name}' launders Any — use Any directly so type holes cluster visibly"),
            ));
        }
    }
    violations
}

/// Classify a node as a type alias value.
/// Returns alias type name or empty string.
fn is_type_alias_value(node: tree_sitter::Node, source: &[u8]) -> &'static str {
    if node.kind() == "subscript" {
        if let Some(value) = node.child_by_field_name("value") {
            if value.kind() == "identifier" {
                match node_text(value, source) {
                    "Literal" => return "Literal",
                    "dict" => return "dict",
                    "list" => return "list",
                    "set" => return "set",
                    "tuple" => return "tuple",
                    _ => {}
                }
            }
        }
    }
    if node.kind() == "binary_operator" {
        let child_count = node.child_count();
        for i in 0..child_count {
            if let Some(child) = node.child(i) {
                if !child.is_named() && child.kind() == "|" {
                    return "Union";
                }
            }
        }
    }
    ""
}

pub fn check_no_implicit_type_aliases(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let root = source.tree.root_node();
    let mut cursor = root.walk();

    for stmt in root.named_children(&mut cursor) {
        let assign = if stmt.kind() == "expression_statement" {
            let mut inner_cursor = stmt.walk();
            let first_child = stmt.named_children(&mut inner_cursor).next();
            match first_child {
                Some(child) if child.kind() == "assignment" => Some(child),
                _ => None,
            }
        } else if stmt.kind() == "assignment" {
            Some(stmt)
        } else {
            None
        };

        let assign = match assign {
            Some(a) => a,
            None => continue,
        };

        let left = match node_field(assign, "left") {
            Some(n) if n.kind() == "identifier" => n,
            _ => continue,
        };
        let right = match node_field(assign, "right") {
            Some(n) => n,
            None => continue,
        };

        let alias_type = is_type_alias_value(right, source.source_bytes);
        if !alias_type.is_empty() {
            let name = node_text(left, source.source_bytes);
            violations.push(violation(
                node_line(assign),
                format!("implicit type alias {name} = {alias_type}[...]"),
            ));
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::build_parsed_source;

    fn parse(code: &str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        build_parsed_source("/test/file.py", source)
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(crate::structures::FileKind::Outside, None)
    }

    // -- no_object --

    #[test]
    fn object_in_annotation() {
        let parsed = parse("def foo(x: object) -> None:\n    pass\n");
        let violations = check_no_object(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("object"));
    }

    #[test]
    fn no_object_clean() {
        let parsed = parse("def foo(x: str) -> str:\n    return x\n");
        let violations = check_no_object(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_json_value --

    #[test]
    fn json_value_in_annotation() {
        let parsed = parse("x: JsonValue = {}\n");
        let violations = check_no_json_value(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("JsonValue"));
    }

    #[test]
    fn no_json_value_clean() {
        let parsed = parse("x: dict[str, int] = {}\n");
        let violations = check_no_json_value(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_any_types --

    #[test]
    fn any_in_annotation() {
        let parsed = parse("def foo(x: Any) -> Any:\n    return x\n");
        let violations = check_no_any_types(&parsed, &default_config());
        assert!(!violations.is_empty());
    }

    #[test]
    fn any_exempted_by_config() {
        let parsed = parse("def foo(x: Any) -> Any:\n    return x\n");
        let mut config = default_config();
        config.unsafe_files = vec!["file.py".to_string()];
        let violations = check_no_any_types(&parsed, &config);
        assert!(violations.is_empty());
    }

    #[test]
    fn no_any_clean() {
        let parsed = parse("def foo(x: str) -> int:\n    return 1\n");
        let violations = check_no_any_types(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn any_in_type_alias_caught() {
        let parsed = parse("type JsonNode = dict[str, Any]\n");
        let violations = check_no_any_types(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Any"));
    }

    // -- no_bare_collections --

    #[test]
    fn bare_dict_caught() {
        let parsed = parse("x: dict\n");
        let violations = check_no_bare_collections(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("dict"));
    }

    #[test]
    fn subscripted_dict_ok() {
        let parsed = parse("x: dict[str, int]\n");
        let violations = check_no_bare_collections(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn bare_list_caught() {
        let parsed = parse("def foo(items: list) -> None:\n    pass\n");
        let violations = check_no_bare_collections(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("list"));
    }

    // -- union_member_count --

    #[test]
    fn small_union_ok() {
        let parsed = parse("x: int | str\n");
        let violations = check_union_member_count(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn large_union_caught() {
        let parsed = parse("x: int | str | float | bool | bytes | list\n");
        let violations = check_union_member_count(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("too many members"));
    }

    #[test]
    fn exactly_at_threshold_ok() {
        let parsed = parse("x: int | str | float | bool | bytes\n");
        let violations = check_union_member_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 5 members, max 5
    }

    // -- no_implicit_type_aliases --

    #[test]
    fn literal_alias_caught() {
        let parsed = parse("Status = Literal['active', 'inactive']\n");
        let violations = check_no_implicit_type_aliases(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Literal"));
    }

    #[test]
    fn union_alias_caught() {
        let parsed = parse("Result = int | str\n");
        let violations = check_no_implicit_type_aliases(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Union"));
    }

    #[test]
    fn normal_assignment_ok() {
        let parsed = parse("x = 42\n");
        let violations = check_no_implicit_type_aliases(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn dict_alias_caught() {
        let parsed = parse("Config = dict[str, int]\n");
        let violations = check_no_implicit_type_aliases(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    // -- no_any_type_aliases --

    #[test]
    fn any_type_alias_caught() {
        let parsed = parse("type JsonNode = dict[str, Any]\n");
        let violations = check_no_any_type_aliases(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("JsonNode"));
        assert!(violations[0].message.contains("launders Any"));
    }

    #[test]
    fn nested_any_type_alias_caught() {
        let parsed = parse("type Payload = list[dict[str, Any]]\n");
        let violations = check_no_any_type_aliases(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Payload"));
    }

    #[test]
    fn clean_type_alias_ok() {
        let parsed = parse("type UserId = str\n");
        let violations = check_no_any_type_aliases(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn type_alias_no_any_ok() {
        let parsed = parse("type Config = dict[str, int]\n");
        let violations = check_no_any_type_aliases(&parsed, &default_config());
        assert!(violations.is_empty());
    }
}
