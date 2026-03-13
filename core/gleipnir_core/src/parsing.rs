//! Tree-sitter parsing primitives for source analysis.
//!
//! These are the building blocks all checks use.
//! Grammar-specific parsers (Python, Rust) live here.
//! Helper functions (find_nodes_by_type, node_text, etc.) are grammar-agnostic.

use crate::structures::ParsedSource;
use tree_sitter::{Node, Parser, Tree};

/// Parse Python source bytes into a tree-sitter Tree.
pub fn parse_python(source_bytes: &[u8]) -> Result<Tree, String> {
    let mut parser = Parser::new();
    let language = tree_sitter_python::LANGUAGE;
    parser
        .set_language(&language.into())
        .map_err(|e| format!("failed to set Python language: {e}"))?;
    parser
        .parse(source_bytes, None)
        .ok_or_else(|| "tree-sitter parse returned None".to_string())
}

/// Parse Rust source bytes into a tree-sitter Tree.
pub fn parse_rust(source_bytes: &[u8]) -> Result<Tree, String> {
    let mut parser = Parser::new();
    let language = tree_sitter_rust::LANGUAGE;
    parser
        .set_language(&language.into())
        .map_err(|e| format!("failed to set Rust language: {e}"))?;
    parser
        .parse(source_bytes, None)
        .ok_or_else(|| "tree-sitter parse returned None".to_string())
}

/// Build a ParsedSource from file path and source content (Python).
pub fn build_parsed_source<'a>(file_path: &'a str, source: &'a [u8]) -> Result<ParsedSource<'a>, String> {
    let tree = parse_python(source)?;
    let lines = std::str::from_utf8(source)
        .unwrap_or("")
        .lines()
        .collect();
    Ok(ParsedSource {
        file_path,
        source_bytes: source,
        lines,
        tree,
    })
}

/// Build a ParsedSource from file path and source content (Rust).
pub fn build_parsed_source_rust<'a>(file_path: &'a str, source: &'a [u8]) -> Result<ParsedSource<'a>, String> {
    let tree = parse_rust(source)?;
    let lines = std::str::from_utf8(source)
        .unwrap_or("")
        .lines()
        .collect();
    Ok(ParsedSource {
        file_path,
        source_bytes: source,
        lines,
        tree,
    })
}

/// Parse TypeScript source bytes into a tree-sitter Tree.
pub fn parse_typescript(source_bytes: &[u8]) -> Result<Tree, String> {
    let mut parser = Parser::new();
    let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT;
    parser
        .set_language(&language.into())
        .map_err(|e| format!("failed to set TypeScript language: {e}"))?;
    parser
        .parse(source_bytes, None)
        .ok_or_else(|| "tree-sitter parse returned None".to_string())
}

/// Build a ParsedSource from file path and TypeScript source content.
pub fn build_parsed_source_typescript<'a>(
    file_path: &'a str,
    source: &'a [u8],
) -> Result<ParsedSource<'a>, String> {
    let tree = parse_typescript(source)?;
    let lines = std::str::from_utf8(source)
        .unwrap_or("")
        .lines()
        .collect();
    Ok(ParsedSource {
        file_path,
        source_bytes: source,
        lines,
        tree,
    })
}

/// Extracted `<script>` block from a Svelte file.
pub struct SvelteScript {
    /// The TypeScript content inside the script tags.
    pub content: String,
    /// 0-indexed line number of the line AFTER `<script...>` in the original file.
    /// Used to offset violation line numbers back to the .svelte file.
    pub line_offset: usize,
}

/// Extract the primary `<script>` block content from a Svelte file.
///
/// Finds the first `<script>` or `<script lang="ts">` tag (excluding
/// `<script context="module">` which is a separate concern) and returns
/// the content between the opening and closing tags.
///
/// Returns None if no script block is found.
pub fn extract_svelte_script(source: &str) -> Option<SvelteScript> {
    let mut script_start = None;
    let mut in_tag = false;

    for (line_num, line) in source.lines().enumerate() {
        let trimmed = line.trim();

        if !in_tag {
            // Look for <script> or <script lang="ts"> (not context="module")
            if trimmed.starts_with("<script")
                && !trimmed.contains("context=")
            {
                if trimmed.contains('>') {
                    // Single-line opening tag: <script lang="ts">
                    script_start = Some(line_num + 1);
                } else {
                    // Multi-line opening tag (rare)
                    in_tag = true;
                }
            }
        } else if trimmed.contains('>') {
            script_start = Some(line_num + 1);
            in_tag = false;
        }

        if script_start.is_some() && trimmed == "</script>" {
            let start = script_start?;
            let content: String = source
                .lines()
                .skip(start)
                .take(line_num - start)
                .collect::<Vec<_>>()
                .join("\n");
            return Some(SvelteScript {
                content,
                line_offset: start,
            });
        }
    }
    None
}

/// Depth-first walk of all nodes in a subtree (including the root).
pub fn walk_tree<'a>(root: Node<'a>) -> Vec<Node<'a>> {
    let mut nodes = Vec::new();
    collect_all_nodes(root, &mut nodes);
    nodes
}

fn collect_all_nodes<'a>(node: Node<'a>, results: &mut Vec<Node<'a>>) {
    results.push(node);
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_all_nodes(child, results);
    }
}

/// Extract text from a node's byte range.
pub fn node_text<'a>(node: Node, source: &'a [u8]) -> &'a str {
    let range = node.byte_range();
    std::str::from_utf8(&source[range]).unwrap_or("")
}

/// Get 1-indexed line number of a node.
pub fn node_line(node: Node) -> usize {
    node.start_position().row + 1
}

/// Get 1-indexed end line number of a node.
pub fn node_end_line(node: Node) -> usize {
    node.end_position().row + 1
}

/// Get a named field child from a node.
pub fn node_field<'a>(node: Node<'a>, field: &str) -> Option<Node<'a>> {
    node.child_by_field_name(field)
}

/// Depth-first search for all nodes of a specific kind.
pub fn find_nodes_by_type<'a>(root: Node<'a>, kind: &str) -> Vec<Node<'a>> {
    let mut results = Vec::new();
    let mut cursor = root.walk();
    collect_by_type(root, kind, &mut results, &mut cursor);
    results
}

fn collect_by_type<'a>(
    node: Node<'a>,
    kind: &str,
    results: &mut Vec<Node<'a>>,
    cursor: &mut tree_sitter::TreeCursor<'a>,
) {
    if node.kind() == kind {
        results.push(node);
    }
    for child in node.named_children(cursor) {
        let mut child_cursor = child.walk();
        collect_by_type(child, kind, results, &mut child_cursor);
    }
}

/// Find all type annotation nodes in a tree.
///
/// Collects type nodes from:
/// - Function parameters (type field of typed_parameter, typed_default_parameter)
/// - Return types (return_type field of function_definition)
/// - Variable annotations (type field of assignment with type)
/// - PEP 695 type alias values (type X = ...)
pub fn find_type_annotations<'a>(root: Node<'a>, source: &[u8]) -> Vec<Node<'a>> {
    let mut annotations = Vec::new();
    collect_type_annotations(root, source, &mut annotations);
    annotations
}

fn collect_type_annotations<'a>(
    node: Node<'a>,
    source: &[u8],
    results: &mut Vec<Node<'a>>,
) {
    match node.kind() {
        "typed_parameter" | "typed_default_parameter" => {
            if let Some(type_node) = node.child_by_field_name("type") {
                results.push(type_node);
            }
        }
        "function_definition" => {
            if let Some(return_type) = node.child_by_field_name("return_type") {
                results.push(return_type);
            }
        }
        "assignment" => {
            if let Some(type_node) = node.child_by_field_name("type") {
                results.push(type_node);
            }
        }
        "type_alias_statement" => {
            // PEP 695: type X = value
            // The value is the last named "type" child (after keyword and name)
            let mut cursor = node.walk();
            let type_children: Vec<_> = node
                .named_children(&mut cursor)
                .filter(|c| c.kind() == "type")
                .collect();
            if let Some(&value_node) = type_children.last() {
                results.push(value_node);
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_type_annotations(child, source, results);
    }
}

/// Check if a type annotation subtree contains a specific identifier name.
pub fn annotation_contains_name(type_node: Node, name: &str, source: &[u8]) -> bool {
    if type_node.kind() == "identifier" && node_text(type_node, source) == name {
        return true;
    }
    let mut cursor = type_node.walk();
    for child in type_node.named_children(&mut cursor) {
        if annotation_contains_name(child, name, source) {
            return true;
        }
    }
    false
}

/// Find bare (non-subscripted) usages of target names in a type annotation.
///
/// Returns (line, name) for each bare identifier that is NOT the value child
/// of a generic_type (subscript) node.
pub fn bare_names_in_annotation<'a>(
    type_node: Node<'a>,
    target_names: &[&str],
    source: &'a [u8],
) -> Vec<(usize, &'a str)> {
    let mut results = Vec::new();
    collect_bare_names(type_node, target_names, source, &mut results);
    results
}

fn collect_bare_names<'a>(
    node: Node<'a>,
    target_names: &[&str],
    source: &'a [u8],
    results: &mut Vec<(usize, &'a str)>,
) {
    if node.kind() == "identifier" {
        let name = node_text(node, source);
        if target_names.contains(&name) {
            let parent = node.parent();
            let is_subscripted = parent.is_some_and(|p| {
                p.kind() == "generic_type" || p.kind() == "subscript"
            });
            if !is_subscripted {
                results.push((node_line(node), name));
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_bare_names(child, target_names, source, results);
    }
}

/// Count members in a union type (A | B | C → 3).
///
/// Flattens nested binary_operator chains with "|" operator.
/// Unwraps "type" wrapper nodes that tree-sitter puts around annotations.
pub fn count_union_members(node: Node) -> usize {
    // Unwrap "type" wrapper node
    if node.kind() == "type" {
        return node.named_child(0)
            .map(count_union_members)
            .unwrap_or(1);
    }
    if node.kind() != "binary_operator" {
        return 1;
    }
    // The "|" is an anonymous child between left and right.
    // Check by iterating all children (named + anonymous).
    let child_count = node.child_count();
    let mut has_pipe = false;
    for i in 0..child_count {
        if let Some(child) = node.child(i) {
            if !child.is_named() && child.kind() == "|" {
                has_pipe = true;
                break;
            }
        }
    }
    if !has_pipe {
        return 1;
    }
    let left = node.child_by_field_name("left");
    let right = node.child_by_field_name("right");
    let left_count = left.map_or(1, |n| count_union_members(n));
    let right_count = right.map_or(1, |n| count_union_members(n));
    left_count + right_count
}

/// Extract module name and relative level from an import_from_statement.
///
/// Returns (module_name, level). Level is the count of leading dots
/// for relative imports, 0 for absolute.
pub fn extract_module_info(node: Node, source: &[u8]) -> (String, usize) {
    let module_node = match node.child_by_field_name("module_name") {
        Some(n) => n,
        None => return (String::new(), 0),
    };

    let full_text = node_text(module_node, source);

    if module_node.kind() == "relative_import" {
        let level = full_text.chars().take_while(|&c| c == '.').count();
        return (full_text[level..].to_string(), level);
    }

    (full_text.to_string(), 0)
}

/// Get names imported by an import_from_statement.
///
/// For `from X import A, B, C`, returns vec!["A", "B", "C"].
pub fn extract_imported_names(node: Node, source: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let mut past_import = false;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "import" {
            past_import = true;
            continue;
        }
        if past_import && child.kind() == "dotted_name" {
            names.push(node_text(child, source).to_string());
        }
        if past_import && child.kind() == "wildcard_import" {
            names.push("*".to_string());
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(code: &str) -> ParsedSource<'static> {
        // Leak for test convenience — tests don't care about cleanup
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        build_parsed_source("/test/file.py", source).unwrap()
    }

    #[test]
    fn find_class_definitions() {
        let parsed = parse("class Foo:\n    pass\nclass Bar:\n    pass\n");
        let classes = find_nodes_by_type(parsed.tree.root_node(), "class_definition");
        assert_eq!(classes.len(), 2);
    }

    #[test]
    fn node_text_extraction() {
        let parsed = parse("x = 42\n");
        let root = parsed.tree.root_node();
        let assignments = find_nodes_by_type(root, "assignment");
        assert_eq!(assignments.len(), 1);
        let left = node_field(assignments[0], "left").unwrap();
        assert_eq!(node_text(left, parsed.source_bytes), "x");
    }

    #[test]
    fn node_line_numbers() {
        let parsed = parse("a = 1\nb = 2\nc = 3\n");
        let assignments = find_nodes_by_type(parsed.tree.root_node(), "assignment");
        assert_eq!(assignments.len(), 3);
        assert_eq!(node_line(assignments[0]), 1);
        assert_eq!(node_line(assignments[1]), 2);
        assert_eq!(node_line(assignments[2]), 3);
    }

    #[test]
    fn count_union_two_members() {
        let parsed = parse("x: int | str\n");
        let annotations = find_type_annotations(parsed.tree.root_node(), parsed.source_bytes);
        assert_eq!(annotations.len(), 1);
        assert_eq!(count_union_members(annotations[0]), 2);
    }

    #[test]
    fn count_union_four_members() {
        let parsed = parse("x: int | str | float | bool\n");
        let annotations = find_type_annotations(parsed.tree.root_node(), parsed.source_bytes);
        assert_eq!(annotations.len(), 1);
        assert_eq!(count_union_members(annotations[0]), 4);
    }

    #[test]
    fn bare_dict_in_annotation() {
        let parsed = parse("x: dict\n");
        let annotations = find_type_annotations(parsed.tree.root_node(), parsed.source_bytes);
        let bare = bare_names_in_annotation(annotations[0], &["dict", "list"], parsed.source_bytes);
        assert_eq!(bare.len(), 1);
        assert_eq!(bare[0].1, "dict");
    }

    #[test]
    fn subscripted_dict_not_bare() {
        let parsed = parse("x: dict[str, int]\n");
        let annotations = find_type_annotations(parsed.tree.root_node(), parsed.source_bytes);
        let bare = bare_names_in_annotation(annotations[0], &["dict", "list"], parsed.source_bytes);
        assert_eq!(bare.len(), 0);
    }
}
