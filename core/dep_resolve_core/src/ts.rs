//! Tree-sitter primitives for Python AST traversal.
//!
//! Minimal set of helpers needed for dependency resolution.
//! These are intentionally self-contained — no dependency on gleipnir_core.

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

/// Extract UTF-8 text from a node's byte range.
pub fn node_text<'a>(node: Node, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()]).unwrap_or("")
}

/// Get a named field child from a node.
pub fn node_field<'a>(node: Node<'a>, field: &str) -> Option<Node<'a>> {
    node.child_by_field_name(field)
}

/// Depth-first search for all nodes of a specific kind.
pub fn find_nodes_by_type<'a>(root: Node<'a>, kind: &str) -> Vec<Node<'a>> {
    let mut results = Vec::new();
    collect_by_type(root, kind, &mut results);
    results
}

fn collect_by_type<'a>(node: Node<'a>, kind: &str, results: &mut Vec<Node<'a>>) {
    if node.kind() == kind {
        results.push(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_by_type(child, kind, results);
    }
}

/// Extract module path and relative import level from an import_from_statement.
///
/// Returns (module_path, level) where level > 0 means relative import.
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
/// For `from x import a, b, c` returns `["a", "b", "c"]`.
/// For `from x import *` returns `["*"]`.
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
        if past_import && child.kind() == "aliased_import" {
            // `from x import y as z` — we want the original name "y"
            if let Some(name_node) = child.child_by_field_name("name") {
                names.push(node_text(name_node, source).to_string());
            }
        }
        if past_import && child.kind() == "wildcard_import" {
            names.push("*".to_string());
        }
    }
    names
}
