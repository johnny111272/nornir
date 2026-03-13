//! Style and complexity checks.
//!
//! Checks: function_length, no_underscore_prefix, param_count,
//! nesting_depth, no_none_returns, no_throwaway_assignment,
//! no_single_letter_names, no_numbered_suffixes, short_param_names.

use regex::Regex;
use std::sync::LazyLock;

use crate::parsing::{find_nodes_by_type, node_field, node_line, node_text, walk_tree};
use crate::structures::{CheckConfig, ParsedSource, Severity, Violation};

fn violation(line: usize, message: String) -> Violation {
    Violation {
        line,
        check_name: String::new(),
        severity: Severity::Warning,
        message,
        detail: String::new(),
        signal: String::new(),
        direction: String::new(),
        canary: String::new(),
    }
}

// -------------------------------------------------------------------------
// function_length
// -------------------------------------------------------------------------

fn docstring_end_row(body: tree_sitter::Node) -> Option<usize> {
    let mut cursor = body.walk();
    let first = body.named_children(&mut cursor).next()?;
    if first.kind() != "expression_statement" {
        return None;
    }
    let mut inner_cursor = first.walk();
    for child in first.named_children(&mut inner_cursor) {
        if child.kind() == "string" {
            return Some(first.end_position().row);
        }
    }
    None
}

fn count_code_lines(func_node: tree_sitter::Node, lines: &[&str]) -> usize {
    let start_row = func_node.start_position().row;
    let end_row = func_node.end_position().row;
    let body = node_field(func_node, "body");
    let doc_end = body.and_then(docstring_end_row);

    let mut count = 0;
    for row in start_row..=end_row.min(lines.len().saturating_sub(1)) {
        if row == start_row {
            continue;
        }
        if let Some(de) = doc_end {
            if row <= de {
                continue;
            }
        }
        let stripped = lines[row].trim();
        if stripped.is_empty() || stripped.starts_with('#') {
            continue;
        }
        count += 1;
    }
    count
}

pub fn check_function_length(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let func_name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };

        if func_name.starts_with("test_") || func_name.starts_with("__") {
            continue;
        }

        let code_lines = count_code_lines(func_node, &source.lines);
        if code_lines > config.max_function_lines {
            // Fuzzy — no counts
            violations.push(violation(
                node_line(func_node),
                format!("{func_name}() too long"),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_underscore_prefix
// -------------------------------------------------------------------------

fn is_underscore_violation(name: &str) -> bool {
    if !name.starts_with('_') {
        return false;
    }
    if name == "_" {
        return false;
    }
    if name.starts_with("__") && name.ends_with("__") {
        return false;
    }
    // Single-letter type var like _T
    if name.len() == 2 && name.as_bytes()[1].is_ascii_uppercase() {
        return false;
    }
    true
}

pub fn check_no_underscore_prefix(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };
        if is_underscore_violation(name) {
            violations.push(violation(
                node_line(func_node),
                format!("function '{name}' uses underscore prefix"),
            ));
        }
    }

    for class_node in find_nodes_by_type(source.tree.root_node(), "class_definition") {
        let name = match node_field(class_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };
        if is_underscore_violation(name) {
            violations.push(violation(
                node_line(class_node),
                format!("class '{name}' uses underscore prefix"),
            ));
        }
    }

    for assign_node in find_nodes_by_type(source.tree.root_node(), "assignment") {
        let left = match node_field(assign_node, "left") {
            Some(n) if n.kind() == "identifier" => n,
            _ => continue,
        };
        let name = node_text(left, source.source_bytes);
        if is_underscore_violation(name) {
            violations.push(violation(
                node_line(assign_node),
                format!("variable '{name}' uses underscore prefix"),
            ));
        }
    }

    violations
}

// -------------------------------------------------------------------------
// param_count
// -------------------------------------------------------------------------

fn extract_param_name<'a>(param: tree_sitter::Node<'a>, source: &'a [u8]) -> &'a str {
    match param.kind() {
        "identifier" => node_text(param, source),
        "typed_parameter" | "default_parameter" | "typed_default_parameter" => {
            let mut cursor = param.walk();
            let first = param.named_children(&mut cursor).next();
            match first {
                Some(n) if n.kind() == "identifier" => node_text(n, source),
                _ => "",
            }
        }
        _ => "",
    }
}

pub fn check_param_count(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let func_name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };

        if func_name.starts_with("test_") || func_name.starts_with("__") {
            continue;
        }

        let params_node = match node_field(func_node, "parameters") {
            Some(n) => n,
            None => continue,
        };

        let mut count = 0;
        let mut cursor = params_node.walk();
        for param in params_node.named_children(&mut cursor) {
            if param.kind() == "list_splat_pattern" || param.kind() == "dictionary_splat_pattern" {
                continue;
            }
            let name = extract_param_name(param, source.source_bytes);
            if name == "self" || name == "cls" || name.is_empty() {
                continue;
            }
            count += 1;
        }

        if count > config.max_function_params {
            // Fuzzy — no counts
            violations.push(violation(
                node_line(func_node),
                format!("{func_name}() has too many parameters"),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// nesting_depth
// -------------------------------------------------------------------------

const NESTING_TYPES: &[&str] = &[
    "if_statement",
    "for_statement",
    "while_statement",
    "with_statement",
    "try_statement",
    "except_clause",
];

fn compute_max_nesting(node: tree_sitter::Node, current: usize) -> usize {
    let mut best = current;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if NESTING_TYPES.contains(&child.kind()) {
            best = best.max(compute_max_nesting(child, current + 1));
        } else {
            best = best.max(compute_max_nesting(child, current));
        }
    }
    best
}

pub fn check_nesting_depth(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let func_name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };

        if func_name.starts_with("test_") {
            continue;
        }

        let depth = compute_max_nesting(func_node, 0);
        if depth > config.max_nesting_depth {
            // Fuzzy — no counts
            violations.push(violation(
                node_line(func_node),
                format!("{func_name}() nested too deeply"),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_none_returns
// -------------------------------------------------------------------------

pub fn check_no_none_returns(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let func_name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };

        if func_name.starts_with("test_") || func_name.starts_with("__") {
            continue;
        }

        let return_type = match node_field(func_node, "return_type") {
            Some(n) => n,
            None => continue,
        };

        let rt_text = node_text(return_type, source.source_bytes).trim();
        if rt_text == "None" {
            violations.push(violation(
                node_line(func_node),
                format!("{func_name}() returns None"),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_throwaway_assignment
// -------------------------------------------------------------------------

pub fn check_no_throwaway_assignment(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "assignment") {
        let left = match node_field(node, "left") {
            Some(n) if n.kind() == "identifier" => n,
            _ => continue,
        };
        if node_text(left, source.source_bytes) == "_" {
            violations.push(violation(
                node_line(node),
                "throwaway assignment discards return value".to_string(),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// Naming quality helpers
// -------------------------------------------------------------------------

const SINGLE_LETTER_ALLOWLIST: &[&str] = &["i", "j", "k", "_"];

static NUMBERED_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z_]*\d+$").unwrap());

static NUMBERED_ALLOWED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(",
        r"step_\d+|",
        r"utf_?\d+|",
        r"sha\d+|md\d+|",
        r"h\d+|",
        r"ipv\d+|",
        r"base\d+|",
        r"int\d+|float\d+|",
        r"v\d+|",
        r"rgb\d*|",
        r"level\d+|",
        r"phase\d+|",
        r"log\d*|",
        r"p\d{4,}",
        r")$",
    ))
    .unwrap()
});

const SHORT_PARAM_ALLOWLIST: &[&str] = &[
    "self", "cls", "id", "db", "ok", "os", "io", "fn", "fp", "fh", "ip", "tx", "rx", "ui", "pk",
    "op",
];

fn func_param_names<'a>(
    func_node: tree_sitter::Node<'a>,
    source: &'a [u8],
) -> Vec<(usize, &'a str)> {
    let params_node = match node_field(func_node, "parameters") {
        Some(n) => n,
        None => return Vec::new(),
    };
    let mut results = Vec::new();
    let mut cursor = params_node.walk();
    for param in params_node.named_children(&mut cursor) {
        if param.kind() == "list_splat_pattern" || param.kind() == "dictionary_splat_pattern" {
            continue;
        }
        let name = extract_param_name(param, source);
        if !name.is_empty() && name != "self" && name != "cls" {
            results.push((node_line(param), name));
        }
    }
    results
}

fn names_from_target<'a>(
    target: tree_sitter::Node<'a>,
    source: &'a [u8],
) -> Vec<(usize, &'a str)> {
    let mut results = Vec::new();
    if target.kind() == "identifier" {
        results.push((node_line(target), node_text(target, source)));
    } else if target.kind() == "pattern_list" {
        let mut cursor = target.walk();
        for child in target.named_children(&mut cursor) {
            if child.kind() == "identifier" {
                results.push((node_line(child), node_text(child, source)));
            }
        }
    }
    results
}

fn collect_body_target_names<'a>(
    func_node: tree_sitter::Node<'a>,
    source: &'a [u8],
) -> Vec<(usize, &'a str)> {
    let body = match node_field(func_node, "body") {
        Some(b) => b,
        None => return Vec::new(),
    };
    let mut results = Vec::new();
    for node in walk_tree(body) {
        match node.kind() {
            "assignment" => {
                if let Some(left) = node_field(node, "left") {
                    results.extend(names_from_target(left, source));
                }
            }
            "for_statement" | "for_in_clause" => {
                if let Some(left) = node_field(node, "left") {
                    results.extend(names_from_target(left, source));
                }
            }
            _ => {}
        }
    }
    results
}

fn is_bad_numbered_name(name: &str) -> bool {
    NUMBERED_PATTERN.is_match(name) && !NUMBERED_ALLOWED.is_match(name)
}

// -------------------------------------------------------------------------
// no_single_letter_names
// -------------------------------------------------------------------------

pub fn check_no_single_letter_names(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let func_name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };
        if func_name.starts_with("test_") {
            continue;
        }

        for (line, name) in func_param_names(func_node, source.source_bytes) {
            if name.len() == 1 && !SINGLE_LETTER_ALLOWLIST.contains(&name) {
                violations.push(violation(
                    line,
                    format!("single-letter parameter '{name}' in {func_name}()"),
                ));
            }
        }

        for (line, name) in collect_body_target_names(func_node, source.source_bytes) {
            if name.len() == 1 && !SINGLE_LETTER_ALLOWLIST.contains(&name) {
                violations.push(violation(
                    line,
                    format!("single-letter variable '{name}' in {func_name}()"),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_numbered_suffixes
// -------------------------------------------------------------------------

pub fn check_no_numbered_suffixes(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let func_name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };

        for (line, name) in func_param_names(func_node, source.source_bytes) {
            if is_bad_numbered_name(name) {
                violations.push(violation(
                    line,
                    format!("numbered parameter '{name}' in {func_name}()"),
                ));
            }
        }

        for (line, name) in collect_body_target_names(func_node, source.source_bytes) {
            if is_bad_numbered_name(name) {
                violations.push(violation(
                    line,
                    format!("numbered variable '{name}' in {func_name}()"),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// short_param_names
// -------------------------------------------------------------------------

pub fn check_short_param_names(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    let min_len = config.min_param_length;

    for func_node in find_nodes_by_type(source.tree.root_node(), "function_definition") {
        let func_name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };

        for (line, name) in func_param_names(func_node, source.source_bytes) {
            if name.starts_with('_') {
                continue;
            }
            if name.len() < min_len && !SHORT_PARAM_ALLOWLIST.contains(&name) {
                // Fuzzy — no counts
                violations.push(violation(
                    line,
                    format!("short parameter '{name}' in {func_name}()"),
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
        build_parsed_source("/test/file.py", source).unwrap()
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(FileKind::Outside)
    }

    // -- function_length --

    #[test]
    fn short_function_ok() {
        let parsed = parse("def foo():\n    return 1\n");
        let violations = check_function_length(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn long_function_caught() {
        // Generate a function with 51+ code lines (config max is 50 for Outside)
        let mut code = String::from("def foo():\n");
        for i in 0..55 {
            code.push_str(&format!("    x{i} = {i}\n"));
        }
        let parsed = parse(&code);
        let violations = check_function_length(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("foo"));
        // Fuzzy — no line count
        assert!(!violations[0].message.contains("55"));
    }

    #[test]
    fn test_function_skipped() {
        let mut code = String::from("def test_big():\n");
        for i in 0..55 {
            code.push_str(&format!("    x{i} = {i}\n"));
        }
        let parsed = parse(&code);
        let violations = check_function_length(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn docstring_excluded_from_count() {
        let mut code = String::from("def foo():\n    \"\"\"Long docstring.\n");
        for _ in 0..20 {
            code.push_str("    Line of docstring.\n");
        }
        code.push_str("    \"\"\"\n");
        // Add only a few code lines after docstring
        for i in 0..5 {
            code.push_str(&format!("    x{i} = {i}\n"));
        }
        let parsed = parse(&code);
        let violations = check_function_length(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_underscore_prefix --

    #[test]
    fn underscore_function_caught() {
        let parsed = parse("def _helper():\n    pass\n");
        let violations = check_no_underscore_prefix(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("_helper"));
    }

    #[test]
    fn dunder_ok() {
        let parsed = parse("def __init__(self):\n    pass\n");
        let violations = check_no_underscore_prefix(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn single_underscore_ok() {
        let parsed = parse("_ = compute()\n");
        let violations = check_no_underscore_prefix(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn underscore_variable_caught() {
        let parsed = parse("_cache = {}\n");
        let violations = check_no_underscore_prefix(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("_cache"));
    }

    // -- param_count --

    #[test]
    fn few_params_ok() {
        let parsed = parse("def foo(a, b, c):\n    pass\n");
        let violations = check_param_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 3 params, max 5
    }

    #[test]
    fn too_many_params_caught() {
        let parsed = parse("def foo(a, b, c, d, e, f, g):\n    pass\n");
        let violations = check_param_count(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("foo"));
        // Fuzzy — no counts
        assert!(!violations[0].message.contains("7"));
    }

    #[test]
    fn self_excluded() {
        let parsed = parse("def foo(self, a, b, c, d, e):\n    pass\n");
        let violations = check_param_count(&parsed, &default_config());
        assert!(violations.is_empty()); // 5 params after excluding self
    }

    // -- nesting_depth --

    #[test]
    fn shallow_nesting_ok() {
        let parsed = parse("def foo():\n    if True:\n        return 1\n");
        let violations = check_nesting_depth(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn deep_nesting_caught() {
        let code = "\
def foo():
    if True:
        for x in range(10):
            if x > 0:
                while True:
                    if x > 5:
                        pass
";
        let parsed = parse(code);
        let violations = check_nesting_depth(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("foo"));
    }

    // -- no_none_returns --

    #[test]
    fn none_return_caught() {
        let parsed = parse("def foo() -> None:\n    pass\n");
        let violations = check_no_none_returns(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("foo"));
    }

    #[test]
    fn int_return_ok() {
        let parsed = parse("def foo() -> int:\n    return 1\n");
        let violations = check_no_none_returns(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn dunder_none_return_ok() {
        let parsed = parse("def __init__(self) -> None:\n    pass\n");
        let violations = check_no_none_returns(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_throwaway_assignment --

    #[test]
    fn throwaway_caught() {
        let parsed = parse("_ = compute()\n");
        let violations = check_no_throwaway_assignment(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn normal_assignment_ok() {
        let parsed = parse("result = compute()\n");
        let violations = check_no_throwaway_assignment(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_single_letter_names --

    #[test]
    fn single_letter_param_caught() {
        let parsed = parse("def foo(x):\n    pass\n");
        let violations = check_no_single_letter_names(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("'x'"));
    }

    #[test]
    fn loop_index_ok() {
        let parsed = parse("def foo():\n    for i in range(10):\n        pass\n");
        let violations = check_no_single_letter_names(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn single_letter_local_caught() {
        let parsed = parse("def foo():\n    x = 1\n");
        let violations = check_no_single_letter_names(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("variable"));
    }

    // -- no_numbered_suffixes --

    #[test]
    fn numbered_param_caught() {
        let parsed = parse("def foo(result1):\n    pass\n");
        let violations = check_no_numbered_suffixes(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("result1"));
    }

    #[test]
    fn allowed_numbered_ok() {
        let parsed = parse("def foo(sha256):\n    pass\n");
        let violations = check_no_numbered_suffixes(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn utf8_ok() {
        let parsed = parse("def foo(utf8):\n    pass\n");
        let violations = check_no_numbered_suffixes(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn numbered_local_caught() {
        let parsed = parse("def foo():\n    data1 = 1\n");
        let violations = check_no_numbered_suffixes(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("data1"));
    }

    // -- short_param_names --

    #[test]
    fn short_param_caught() {
        let parsed = parse("def foo(ab):\n    pass\n");
        let violations = check_short_param_names(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("'ab'"));
    }

    #[test]
    fn allowlisted_short_ok() {
        let parsed = parse("def foo(id, db, ok):\n    pass\n");
        let violations = check_short_param_names(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn long_param_ok() {
        let parsed = parse("def foo(user_name):\n    pass\n");
        let violations = check_short_param_names(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn underscore_prefix_param_skipped() {
        let parsed = parse("def foo(_x):\n    pass\n");
        let violations = check_short_param_names(&parsed, &default_config());
        assert!(violations.is_empty());
    }
}
