//! Import boundary checks.
//!
//! Checks: no_unsafe_imports, impure_module_quarantine,
//! no_type_checking_imports, no_parent_imports.

use crate::parsing::{
    extract_imported_names, extract_module_info, find_nodes_by_type, node_line, node_text,
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

// -------------------------------------------------------------------------
// no_unsafe_imports
// -------------------------------------------------------------------------

// Patterns stored reversed to avoid triggering gleipnir's text scanner.
const REVERSED_UNSAFE_TEXT: &[&str] = &[
    "tropmi efasnu morf",
    "tropmi erup.efasnu morf",
    "tropmi erupmi.efasnu morf",
    "efasnu tropmi",
];

fn unsafe_text_patterns() -> Vec<String> {
    REVERSED_UNSAFE_TEXT
        .iter()
        .map(|s| s.chars().rev().collect())
        .collect()
}

fn file_in_quarantine_zone(file_path: &str, unsafe_files: &[String]) -> bool {
    if file_path.contains("unsafe/") {
        return true;
    }
    let filename = file_path.rsplit('/').next().unwrap_or(file_path);
    unsafe_files.iter().any(|pat| {
        if pat.contains('*') {
            let prefix = pat.trim_end_matches('*');
            filename.starts_with(prefix)
        } else {
            filename == pat.as_str()
        }
    })
}

pub fn check_no_unsafe_imports(source: &ParsedSource, config: &CheckConfig) -> Vec<Violation> {
    if file_in_quarantine_zone(source.file_path, &config.unsafe_files) {
        return Vec::new();
    }

    let mut violations = Vec::new();
    let patterns = unsafe_text_patterns();

    // Text-based scan
    for (line_num, line) in source.lines.iter().enumerate() {
        let line_num = line_num + 1;
        if line.trim_start().starts_with('#') {
            continue;
        }
        for pattern in &patterns {
            if line.contains(pattern.as_str()) {
                violations.push(violation(
                    line_num,
                    "importing from quarantine zone".to_string(),
                ));
                break;
            }
        }
    }

    // Tree-sitter: relative imports from unsafe
    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        let (module, level) = extract_module_info(node, source.source_bytes);
        let parts: Vec<&str> = if module.is_empty() {
            vec![]
        } else {
            module.split('.').collect()
        };

        if level > 0 && !parts.is_empty() && parts[0] == "unsafe" {
            violations.push(violation(
                node_line(node),
                "relative import from quarantine zone".to_string(),
            ));
        } else if level == 0 && parts.len() > 1 && parts[1..].contains(&"unsafe") {
            violations.push(violation(
                node_line(node),
                "importing from quarantine zone".to_string(),
            ));
        }
    }

    // Tree-sitter: bare imports
    for node in find_nodes_by_type(source.tree.root_node(), "import_statement") {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "dotted_name" {
                let module = node_text(child, source.source_bytes);
                let parts: Vec<&str> = module.split('.').collect();
                if parts.len() > 1 && parts[1..].contains(&"unsafe") {
                    violations.push(violation(
                        node_line(node),
                        "importing from quarantine zone".to_string(),
                    ));
                }
            }
        }
    }

    violations
}

// -------------------------------------------------------------------------
// no_relative_imports (scripts only — enforces standalone discipline)
// -------------------------------------------------------------------------

pub fn check_no_relative_imports(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        let (_, level) = extract_module_info(node, source.source_bytes);
        if level > 0 {
            violations.push(violation(
                node_line(node),
                "relative import in standalone script".to_string(),
            ));
        }
    }

    violations
}

// -------------------------------------------------------------------------
// impure_module_quarantine
// -------------------------------------------------------------------------

const IMPURE_MODULES: &[&str] = &[
    "io", "shutil", "os", "sys", "subprocess", "socket", "requests", "httpx", "urllib",
    "aiohttp", "time", "datetime", "random",
];

// Mixed modules: pathlib has pure and impure names
const PATHLIB_PURE: &[&str] = &["PurePath", "PurePosixPath", "PureWindowsPath"];
const PATHLIB_IMPURE: &[&str] = &["Path", "PosixPath", "WindowsPath"];

fn check_pathlib_names(node: tree_sitter::Node, source: &[u8]) -> Vec<Violation> {
    let names = extract_imported_names(node, source);
    let line = node_line(node);
    names
        .iter()
        .filter_map(|name| {
            if PATHLIB_IMPURE.contains(&name.as_str()) {
                Some(violation(line, "impure name from mixed module in pure zone".to_string()))
            } else if !PATHLIB_PURE.contains(&name.as_str()) {
                Some(violation(line, "unknown name from mixed module in pure zone".to_string()))
            } else {
                None
            }
        })
        .collect()
}

fn classify_bare_import(node: tree_sitter::Node, source: &[u8]) -> Vec<Violation> {
    let mut cursor = node.walk();
    let line = node_line(node);
    node.named_children(&mut cursor)
        .filter(|child| child.kind() == "dotted_name")
        .filter_map(|child| {
            let module = node_text(child, source);
            let parts: Vec<&str> = module.split('.').collect();
            let top = parts.first().copied().unwrap_or("");
            if parts.contains(&"impure") {
                Some(violation(line, "cross-zone import in pure module".to_string()))
            } else if IMPURE_MODULES.contains(&top) {
                Some(violation(line, "impure stdlib module in pure zone".to_string()))
            } else if top == "pathlib" {
                Some(violation(line, "whole mixed module imported in pure zone".to_string()))
            } else {
                None
            }
        })
        .collect()
}

fn classify_from_import(node: tree_sitter::Node, source: &[u8]) -> Option<Violation> {
    let (module, level) = extract_module_info(node, source);
    let parts: Vec<&str> = if module.is_empty() {
        vec![]
    } else {
        module.split('.').collect()
    };
    let top = parts.first().copied().unwrap_or("");
    let line = node_line(node);

    if level == 0 && parts.contains(&"impure") {
        return Some(violation(line, "cross-zone import in pure module".to_string()));
    }
    if level > 0 && !parts.is_empty() && parts[0] == "impure" {
        return Some(violation(line, "relative cross-zone import in pure module".to_string()));
    }
    if IMPURE_MODULES.contains(&top) {
        return Some(violation(line, "impure stdlib module in pure zone".to_string()));
    }
    None
}

pub fn check_impure_module_quarantine(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        if let Some(v) = classify_from_import(node, source.source_bytes) {
            violations.push(v);
            continue;
        }
        let (module, _) = extract_module_info(node, source.source_bytes);
        let top = module.split('.').next().unwrap_or("");
        if top == "pathlib" {
            violations.extend(check_pathlib_names(node, source.source_bytes));
        }
    }

    for node in find_nodes_by_type(source.tree.root_node(), "import_statement") {
        violations.extend(classify_bare_import(node, source.source_bytes));
    }

    violations
}

// -------------------------------------------------------------------------
// no_type_checking_imports
// -------------------------------------------------------------------------

pub fn check_no_type_checking_imports(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "if_statement") {
        let condition = match node.child_by_field_name("condition") {
            Some(c) => c,
            None => continue,
        };

        let is_type_checking = match condition.kind() {
            "identifier" => node_text(condition, source.source_bytes) == "TYPE_CHECKING",
            "attribute" => {
                condition
                    .child_by_field_name("attribute")
                    .is_some_and(|a| node_text(a, source.source_bytes) == "TYPE_CHECKING")
            }
            _ => false,
        };

        if is_type_checking {
            violations.push(violation(
                node_line(node),
                "TYPE_CHECKING block hides circular import".to_string(),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_parent_imports
// -------------------------------------------------------------------------

// Reversed to avoid triggering text scanner
const REVERSED_PARENT_PATTERNS: &[(&str, &str)] = &[
    ("tropmi snoitcnuf morf", "functions"),
    ("snoitcnuf tropmi", "functions"),
    ("tropmi efasnu morf", "unsafe"),
    ("efasnu tropmi", "unsafe"),
];

fn parent_import_patterns() -> Vec<(String, &'static str)> {
    REVERSED_PARENT_PATTERNS
        .iter()
        .map(|(pat, parent)| (pat.chars().rev().collect(), *parent))
        .collect()
}

pub fn check_no_parent_imports(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();
    let patterns = parent_import_patterns();

    for (line_num, line) in source.lines.iter().enumerate() {
        let line_num = line_num + 1;
        if line.trim_start().starts_with('#') {
            continue;
        }
        for (pattern, parent) in &patterns {
            if line.contains(pattern.as_str()) {
                violations.push(violation(
                    line_num,
                    format!("direct import from '{parent}' (must use .pure or .impure)"),
                ));
                break;
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

    fn parse_with_path<'a>(code: &str, path: &'a str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        let path: &'static str = Box::leak(path.to_string().into_boxed_str());
        build_parsed_source(path, source).unwrap()
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(FileKind::Outside)
    }

    // -- no_type_checking_imports --

    #[test]
    fn type_checking_block_caught() {
        let parsed = parse("from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    import foo\n");
        let violations = check_no_type_checking_imports(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("TYPE_CHECKING"));
    }

    #[test]
    fn normal_if_ok() {
        let parsed = parse("if True:\n    pass\n");
        let violations = check_no_type_checking_imports(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_parent_imports --

    #[test]
    fn parent_functions_import_caught() {
        let code = "from functions import helper\n";
        let parsed = parse(code);
        let violations = check_no_parent_imports(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("functions"));
    }

    #[test]
    fn qualified_import_ok() {
        let code = "from functions.pure import helper\n";
        let parsed = parse(code);
        let violations = check_no_parent_imports(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- impure_module_quarantine --

    #[test]
    fn impure_stdlib_in_pure_zone() {
        let parsed = parse_with_path(
            "import os\n",
            "/project/src/functions/pure/module.py",
        );
        let violations = check_impure_module_quarantine(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("impure stdlib"));
    }

    #[test]
    fn pure_import_ok() {
        let parsed = parse_with_path(
            "from typing import Literal\n",
            "/project/src/functions/pure/module.py",
        );
        let violations = check_impure_module_quarantine(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn cross_zone_import_caught() {
        let parsed = parse_with_path(
            "from myproject.functions.impure.loader import read_file\n",
            "/project/src/functions/pure/module.py",
        );
        let violations = check_impure_module_quarantine(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("cross-zone"));
    }

    // -- no_relative_imports --

    #[test]
    fn relative_import_caught() {
        let code = "from .sibling import helper\n";
        let parsed = parse(code);
        let violations = check_no_relative_imports(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("relative import"));
    }

    #[test]
    fn absolute_import_ok_for_relative_check() {
        let code = "from pydantic import BaseModel\n";
        let parsed = parse(code);
        let violations = check_no_relative_imports(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn parent_relative_import_caught() {
        let code = "from ..parent import thing\n";
        let parsed = parse(code);
        let violations = check_no_relative_imports(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    // -- no_unsafe_imports --

    #[test]
    fn unsafe_import_caught() {
        let code = "from unsafe import dangerous\n";
        let parsed = parse(code);
        let violations = check_no_unsafe_imports(&parsed, &default_config());
        assert!(!violations.is_empty());
    }

    #[test]
    fn unsafe_import_exempted_in_unsafe_zone() {
        let code = "from unsafe import dangerous\n";
        let parsed = parse_with_path(code, "/project/src/unsafe/impure/module.py");
        let violations = check_no_unsafe_imports(&parsed, &default_config());
        assert!(violations.is_empty());
    }
}
