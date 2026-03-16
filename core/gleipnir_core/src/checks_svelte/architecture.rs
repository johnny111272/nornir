//! Svelte component architecture enforcement.
//!
//! Checks:
//!   no_missing_shared_imports — View components must import from @yggdrasil/ui
//!   no_raw_html_elements     — use shared components instead of raw HTML
//!   no_100vh_in_components   — 100vh only in page wrappers
//!   no_hardcoded_colors      — use CSS tokens, not hex colors
//!   no_margin_in_shared      — shared components must not impose external margin
//!   no_fixed_in_shared       — shared components must not use position: fixed

use crate::parsing::{find_nodes_by_type, node_line, node_text};
use crate::structures::{ParsedSource, Severity, Violation};

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
// no_missing_shared_imports
// -------------------------------------------------------------------------

/// View components must import from @yggdrasil/ui.
///
/// Scope: files whose path contains "View.svelte"
/// A View component that doesn't import shared UI is building its own.
pub fn check_no_missing_shared_imports(file_path: &str, source: &str) -> Vec<Violation> {
    if !file_path.contains("View.svelte") {
        return Vec::new();
    }

    if source.contains("@yggdrasil/ui") {
        return Vec::new();
    }

    vec![violation(1, "View component does not import from @yggdrasil/ui")]
}

// -------------------------------------------------------------------------
// no_raw_html_elements
// -------------------------------------------------------------------------

/// Raw HTML elements that have shared component equivalents.
const REPLACED_ELEMENTS: &[&str] = &["input", "select", "details", "button"];

/// Detect raw HTML elements that should use shared components.
///
/// Scope: all .svelte files EXCEPT those in ui/components/ (those ARE the implementations).
pub fn check_no_raw_html_elements(
    file_path: &str,
    parsed: &ParsedSource,
) -> Vec<Violation> {
    if file_path.contains("ui/components/") {
        return Vec::new();
    }

    let mut violations = Vec::new();

    for node in find_nodes_by_type(parsed.tree.root_node(), "tag_name") {
        // Only match opening tags — end_tag also has tag_name, skip to avoid double-counting
        let parent_kind = node.parent().map(|p| p.kind());
        if parent_kind != Some("start_tag") && parent_kind != Some("self_closing_tag") {
            continue;
        }

        let name = node_text(node, parsed.source_bytes);
        if REPLACED_ELEMENTS.contains(&name) {
            violations.push(violation(
                node_line(node),
                format!("<{}> has a shared component equivalent — use the shared version", name),
            ));
        }
    }

    violations
}

// -------------------------------------------------------------------------
// no_100vh_in_components
// -------------------------------------------------------------------------

/// Detect 100vh in component style blocks.
///
/// 100vh in components causes clipping bugs. Only page wrappers
/// (+page.svelte, +layout.svelte) may use it.
pub fn check_no_100vh_in_components(
    file_path: &str,
    parsed: &ParsedSource,
) -> Vec<Violation> {
    if is_page_file(file_path) || is_container_file(file_path) {
        return Vec::new();
    }

    let mut violations = Vec::new();

    for node in find_nodes_by_type(parsed.tree.root_node(), "integer_value") {
        let text = node_text(node, parsed.source_bytes);
        if text == "100vh" {
            violations.push(violation(
                node_line(node),
                "100vh causes clipping in components — only page wrappers should set viewport height",
            ));
        }
    }

    violations
}

fn is_page_file(file_path: &str) -> bool {
    let name = file_path.rsplit('/').next().unwrap_or("");
    name == "+page.svelte" || name == "+layout.svelte"
}

fn is_container_file(file_path: &str) -> bool {
    let name = file_path.rsplit('/').next().unwrap_or("");
    name.ends_with("Container.svelte")
}

// -------------------------------------------------------------------------
// no_hardcoded_colors
// -------------------------------------------------------------------------

/// Detect hardcoded hex colors in style blocks.
///
/// Hex colors should use CSS custom properties (design tokens).
/// Exempt: custom property definitions (--var-name: #hex) since those ARE tokens.
pub fn check_no_hardcoded_colors(
    _file_path: &str,
    parsed: &ParsedSource,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(parsed.tree.root_node(), "color_value") {
        if is_custom_property_definition(node, parsed.source_bytes) {
            continue;
        }

        violations.push(violation(
            node_line(node),
            format!(
                "hardcoded color {} — use a CSS custom property (design token)",
                node_text(node, parsed.source_bytes)
            ),
        ));
    }

    violations
}

/// Check if a color_value is inside a custom property definition (--var-name: #hex).
fn is_custom_property_definition(color_node: tree_sitter::Node, source: &[u8]) -> bool {
    let declaration = match color_node.parent() {
        Some(p) if p.kind() == "declaration" => p,
        _ => return false,
    };
    let mut cursor = declaration.walk();
    for child in declaration.named_children(&mut cursor) {
        if child.kind() == "property_name" {
            return node_text(child, source).starts_with("--");
        }
    }
    false
}

// -------------------------------------------------------------------------
// no_margin_in_shared
// -------------------------------------------------------------------------

/// Shared components must not impose external margin.
///
/// Scope: files in ui/components/
/// margin-top and margin-bottom leak spacing into parent layouts.
pub fn check_no_margin_in_shared(
    file_path: &str,
    parsed: &ParsedSource,
) -> Vec<Violation> {
    if !file_path.contains("ui/components/") {
        return Vec::new();
    }

    let mut violations = Vec::new();

    for node in find_nodes_by_type(parsed.tree.root_node(), "property_name") {
        let name = node_text(node, parsed.source_bytes);
        if name == "margin-top" || name == "margin-bottom" {
            violations.push(violation(
                node_line(node),
                format!("{} in shared component leaks spacing into parent layout", name),
            ));
        }
    }

    violations
}

// -------------------------------------------------------------------------
// no_fixed_in_shared
// -------------------------------------------------------------------------

/// Shared components must not use position: fixed.
///
/// Scope: files in ui/components/
/// Fixed positioning escapes containers — architectural bug in shared components.
pub fn check_no_fixed_in_shared(
    file_path: &str,
    parsed: &ParsedSource,
) -> Vec<Violation> {
    if !file_path.contains("ui/components/") {
        return Vec::new();
    }

    let mut violations = Vec::new();

    for node in find_nodes_by_type(parsed.tree.root_node(), "declaration") {
        let (mut has_position, mut has_fixed) = (false, false);
        let mut prop_line = 0;

        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "property_name" if node_text(child, parsed.source_bytes) == "position" => {
                    has_position = true;
                    prop_line = node_line(child);
                }
                "plain_value" if node_text(child, parsed.source_bytes) == "fixed" => {
                    has_fixed = true;
                }
                _ => {}
            }
        }

        if has_position && has_fixed {
            violations.push(violation(
                prop_line,
                "position: fixed in shared component escapes container layout",
            ));
        }
    }

    violations
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::{build_parsed_source_css, build_parsed_source_html};

    fn parse_css(code: &str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        build_parsed_source_css("/test/file.css", source).unwrap()
    }

    fn parse_css_with_path<'a>(code: &'a str, path: &'a str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        let path: &'static str = Box::leak(path.to_string().into_boxed_str());
        build_parsed_source_css(path, source).unwrap()
    }

    fn parse_html_with_path<'a>(code: &'a str, path: &'a str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        let path: &'static str = Box::leak(path.to_string().into_boxed_str());
        build_parsed_source_html(path, source).unwrap()
    }

    // -- no_missing_shared_imports --

    #[test]
    fn view_with_import_ok() {
        let v = check_no_missing_shared_imports(
            "/app/src/lib/HlidskjalfView.svelte",
            "import { Panel, Badge } from '@yggdrasil/ui';\nlet x = 1;",
        );
        assert!(v.is_empty());
    }

    #[test]
    fn view_without_import_caught() {
        let v = check_no_missing_shared_imports(
            "/app/src/lib/HlidskjalfView.svelte",
            "let x = 1;\nconst y = 2;",
        );
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("@yggdrasil/ui"));
    }

    #[test]
    fn non_view_file_skipped() {
        let v = check_no_missing_shared_imports(
            "/app/src/lib/SchemaInspector.svelte",
            "let x = 1;",
        );
        assert!(v.is_empty());
    }

    // -- no_raw_html_elements --

    #[test]
    fn raw_input_caught() {
        let parsed = parse_html_with_path(
            "<div><input type=\"text\" /></div>",
            "/app/src/lib/MyView.svelte",
        );
        let v = check_no_raw_html_elements("/app/src/lib/MyView.svelte", &parsed);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("<input>"));
    }

    #[test]
    fn raw_button_caught() {
        let parsed = parse_html_with_path(
            "<button>Click</button>",
            "/app/src/lib/MyView.svelte",
        );
        let v = check_no_raw_html_elements("/app/src/lib/MyView.svelte", &parsed);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("<button>"));
    }

    #[test]
    fn raw_select_caught() {
        let parsed = parse_html_with_path(
            "<select><option>A</option></select>",
            "/app/src/lib/MyView.svelte",
        );
        let v = check_no_raw_html_elements("/app/src/lib/MyView.svelte", &parsed);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("<select>"));
    }

    #[test]
    fn shared_component_file_exempt() {
        let parsed = parse_html_with_path(
            "<input type=\"text\" />",
            "/ui/components/Input.svelte",
        );
        let v = check_no_raw_html_elements("/ui/components/Input.svelte", &parsed);
        assert!(v.is_empty());
    }

    #[test]
    fn div_not_caught() {
        let parsed = parse_html_with_path(
            "<div>hello</div>",
            "/app/src/lib/MyView.svelte",
        );
        let v = check_no_raw_html_elements("/app/src/lib/MyView.svelte", &parsed);
        assert!(v.is_empty());
    }

    // -- no_100vh_in_components --

    #[test]
    fn vh_100_in_component_caught() {
        let parsed = parse_css_with_path(
            ".container { height: 100vh; }",
            "/app/src/lib/MyView.svelte",
        );
        let v = check_no_100vh_in_components("/app/src/lib/MyView.svelte", &parsed);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("100vh"));
    }

    #[test]
    fn vh_100_in_page_ok() {
        let parsed = parse_css_with_path(
            ".wrapper { height: 100vh; }",
            "/app/src/routes/+page.svelte",
        );
        let v = check_no_100vh_in_components("/app/src/routes/+page.svelte", &parsed);
        assert!(v.is_empty());
    }

    #[test]
    fn vh_100_in_layout_ok() {
        let parsed = parse_css_with_path(
            ".wrapper { height: 100vh; }",
            "/app/src/routes/+layout.svelte",
        );
        let v = check_no_100vh_in_components("/app/src/routes/+layout.svelte", &parsed);
        assert!(v.is_empty());
    }

    #[test]
    fn normal_height_ok() {
        let parsed = parse_css_with_path(
            ".foo { height: 100%; }",
            "/app/src/lib/MyView.svelte",
        );
        let v = check_no_100vh_in_components("/app/src/lib/MyView.svelte", &parsed);
        assert!(v.is_empty());
    }

    #[test]
    fn vh_100_in_container_ok() {
        let parsed = parse_css_with_path(
            ".wrapper { height: 100vh; }",
            "/ui/components/SoloContainer.svelte",
        );
        let v = check_no_100vh_in_components("/ui/components/SoloContainer.svelte", &parsed);
        assert!(v.is_empty());
    }

    // -- no_hardcoded_colors --

    #[test]
    fn hex_color_caught() {
        let parsed = parse_css(".foo { color: #ff0000; }");
        let v = check_no_hardcoded_colors("/app/src/lib/MyView.svelte", &parsed);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("#ff0000"));
    }

    #[test]
    fn short_hex_caught() {
        let parsed = parse_css(".foo { background: #333; }");
        let v = check_no_hardcoded_colors("/app/src/lib/MyView.svelte", &parsed);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("#333"));
    }

    #[test]
    fn custom_property_definition_ok() {
        let parsed = parse_css(":root { --my-color: #ff0000; }");
        let v = check_no_hardcoded_colors("/app/src/lib/tokens.svelte", &parsed);
        assert!(v.is_empty());
    }

    #[test]
    fn var_usage_ok() {
        let parsed = parse_css(".foo { color: var(--text-primary); }");
        let v = check_no_hardcoded_colors("/app/src/lib/MyView.svelte", &parsed);
        assert!(v.is_empty());
    }

    // -- no_margin_in_shared --

    #[test]
    fn margin_top_in_shared_caught() {
        let parsed = parse_css_with_path(
            ".card { margin-top: 8px; }",
            "/ui/components/Card.svelte",
        );
        let v = check_no_margin_in_shared("/ui/components/Card.svelte", &parsed);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("margin-top"));
    }

    #[test]
    fn margin_bottom_in_shared_caught() {
        let parsed = parse_css_with_path(
            ".card { margin-bottom: 16px; }",
            "/ui/components/Card.svelte",
        );
        let v = check_no_margin_in_shared("/ui/components/Card.svelte", &parsed);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("margin-bottom"));
    }

    #[test]
    fn margin_in_app_ok() {
        let parsed = parse_css_with_path(
            ".card { margin-top: 8px; }",
            "/app/src/lib/MyView.svelte",
        );
        let v = check_no_margin_in_shared("/app/src/lib/MyView.svelte", &parsed);
        assert!(v.is_empty());
    }

    #[test]
    fn margin_left_in_shared_ok() {
        let parsed = parse_css_with_path(
            ".card { margin-left: 8px; }",
            "/ui/components/Card.svelte",
        );
        let v = check_no_margin_in_shared("/ui/components/Card.svelte", &parsed);
        assert!(v.is_empty()); // only top/bottom are external spacing leaks
    }

    // -- no_fixed_in_shared --

    #[test]
    fn fixed_in_shared_caught() {
        let parsed = parse_css_with_path(
            ".overlay { position: fixed; }",
            "/ui/components/Modal.svelte",
        );
        let v = check_no_fixed_in_shared("/ui/components/Modal.svelte", &parsed);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("position: fixed"));
    }

    #[test]
    fn absolute_in_shared_ok() {
        let parsed = parse_css_with_path(
            ".tooltip { position: absolute; }",
            "/ui/components/Tooltip.svelte",
        );
        let v = check_no_fixed_in_shared("/ui/components/Tooltip.svelte", &parsed);
        assert!(v.is_empty());
    }

    #[test]
    fn fixed_in_app_ok() {
        let parsed = parse_css_with_path(
            ".overlay { position: fixed; }",
            "/app/src/lib/MyView.svelte",
        );
        let v = check_no_fixed_in_shared("/app/src/lib/MyView.svelte", &parsed);
        assert!(v.is_empty());
    }
}
