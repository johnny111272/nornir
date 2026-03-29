//! Import boundary checks.
//!
//! Checks: no_unsafe_imports, impure_module_quarantine,
//! no_type_checking_imports, no_parent_imports, no_disallowed_stdlib.

use crate::classify;
use crate::parsing::{
    extract_imported_names, extract_module_info, find_nodes_by_type, node_line, node_text,
};
use crate::structures::{CheckConfig, Level, ParsedSource, Severity, V2Classification, Violation};

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

fn file_in_quarantine_zone(file_path: &str) -> bool {
    file_path.contains("unsafe/")
}

pub fn check_no_unsafe_imports(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    if file_in_quarantine_zone(source.file_path) {
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
    "aiohttp", "time", "datetime", "random", "importlib", "types",
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

// -------------------------------------------------------------------------
// no_disallowed_stdlib — global ban on superseded stdlib modules
// -------------------------------------------------------------------------

/// Stdlib modules that have been replaced by project-standard alternatives.
/// Each entry is (module_name, replacement).
const DISALLOWED_STDLIB: &[(&str, &str)] = &[
    ("argparse", "typer"),
    ("logging", "loguru"),
    ("unittest", "pytest"),
    ("configparser", "toml + pydantic"),
    ("json", "pydantic model_dump_json() / model_validate_json()"),
];

pub fn check_no_disallowed_stdlib(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        let (module, _level) = extract_module_info(node, source.source_bytes);
        let top = module.split('.').next().unwrap_or("");
        if let Some((_, replacement)) = DISALLOWED_STDLIB.iter().find(|(name, _)| *name == top) {
            violations.push(violation(
                node_line(node),
                format!("'{top}' is superseded — use {replacement}"),
            ));
        }
    }

    for node in find_nodes_by_type(source.tree.root_node(), "import_statement") {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "dotted_name" {
                let module = node_text(child, source.source_bytes);
                let top = module.split('.').next().unwrap_or("");
                if let Some((_, replacement)) = DISALLOWED_STDLIB.iter().find(|(name, _)| *name == top) {
                    violations.push(violation(
                        node_line(node),
                        format!("'{top}' is superseded — use {replacement}"),
                    ));
                }
            }
        }
    }

    violations
}

// -------------------------------------------------------------------------
// v2 import boundary check
// -------------------------------------------------------------------------

/// Resolve a relative import path to a dotted path using the source file's location.
///
/// For `from .sibling import foo` in `/project/src/pkg/logic/pure/simple/module.py`,
/// the relative part `.sibling` resolves against the directory containing module.py.
/// Level 1 = same directory, level 2 = parent directory, etc.
fn resolve_relative_import(source_path: &str, module: &str, level: usize) -> String {
    let parts: Vec<&str> = source_path.split('/').collect();
    // Strip filename, then go up `level` directories
    let dir_parts = if parts.len() > level {
        &parts[..parts.len() - level]
    } else {
        &[]
    };

    // Find the src/package boundary — look for common patterns
    // We need the dotted package path from the project package root
    let mut package_start = 0;
    for (idx, part) in dir_parts.iter().enumerate() {
        if *part == "src" && idx + 1 < dir_parts.len() {
            package_start = idx + 1;
            break;
        }
    }

    let package_parts = &dir_parts[package_start..];
    let base = package_parts.join(".");
    if module.is_empty() {
        base
    } else {
        format!("{base}.{module}")
    }
}

/// Check v2 import boundaries using the two-axis (level + zone) intersection.
///
/// Every import must satisfy both the level matrix and the zone matrix.
/// Files outside the v2 zone layout are violations themselves.
/// External imports (stdlib, third-party) are skipped.
pub fn check_v2_import_boundaries(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let source_class = classify::classify_file_v2(source.file_path);

    let mut violations = Vec::new();

    // __init__.py files: package markers, no imports to check
    // (init_files_empty enforces they're empty separately)
    let filename = source.file_path.rsplit('/').next().unwrap_or("");
    if filename == "__init__.py" {
        return violations;
    }

    if source_class.level == Level::Outside {
        violations.push(violation(
            1,
            "file outside v2 zone layout".to_string(),
        ));
        return violations;
    }

    // Check import_from_statement nodes
    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        let (module, level) = extract_module_info(node, source.source_bytes);

        let dotted = if level > 0 {
            resolve_relative_import(source.file_path, &module, level)
        } else {
            module.clone()
        };

        if let Some(target_class) = classify::classify_import_path(&dotted) {
            check_import_legality(
                source_class,
                target_class,
                &dotted,
                node_line(node),
                &mut violations,
            );
        }
    }

    // Check bare import_statement nodes
    for node in find_nodes_by_type(source.tree.root_node(), "import_statement") {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "dotted_name" {
                let dotted = node_text(child, source.source_bytes).to_string();
                if let Some(target_class) = classify::classify_import_path(&dotted) {
                    check_import_legality(
                        source_class,
                        target_class,
                        &dotted,
                        node_line(node),
                        &mut violations,
                    );
                }
            }
        }
    }

    violations
}

fn check_import_legality(
    source: V2Classification,
    target: V2Classification,
    dotted_path: &str,
    line: usize,
    violations: &mut Vec<Violation>,
) {
    if target.level == Level::Outside {
        violations.push(violation(
            line,
            format!("import from outside v2 zone layout: {dotted_path}"),
        ));
        return;
    }

    let level_ok = source.level.can_import(target.level);
    let zone_ok = if target.zone == crate::structures::Zone::Structure {
        true
    } else {
        source.zone.can_reach(target.zone)
    };

    if !level_ok {
        violations.push(violation(
            line,
            format!(
                "level violation: {:?} cannot import {:?} ({dotted_path})",
                source.level, target.level,
            ),
        ));
    }

    if !zone_ok {
        violations.push(violation(
            line,
            format!(
                "zone violation: {:?} cannot import {:?} ({dotted_path})",
                source.zone, target.zone,
            ),
        ));
    }
}

// -------------------------------------------------------------------------
// no_before_validators
// -------------------------------------------------------------------------

pub fn check_no_before_validators(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        let imported = extract_imported_names(node, source.source_bytes);
        for name in &imported {
            if *name == "BeforeValidator" {
                violations.push(violation(
                    node_line(node),
                    "BeforeValidator import — use explicit transforms instead of implicit coercion".to_string(),
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

    fn parse_with_path<'a>(code: &str, path: &'a str) -> ParsedSource<'static> {
        let source: &'static [u8] = Box::leak(code.as_bytes().to_vec().into_boxed_slice());
        let path: &'static str = Box::leak(path.to_string().into_boxed_str());
        build_parsed_source(path, source).unwrap()
    }

    fn default_config() -> CheckConfig {
        CheckConfig::for_kind(FileKind::Outside, &crate::STATISTICS)
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

    // -- v2 import boundaries --

    #[test]
    fn v2_pure_simple_imports_pure_primitive_ok() {
        let code = "from mypackage.logic.pure.primitive.helpers import add\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/pure/check/simple.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_pure_simple_imports_structure_ok() {
        let code = "from mypackage.structures.models import MyModel\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/pure/check/simple.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_same_level_import_blocked() {
        let code = "from mypackage.logic.pure.simple.other import foo\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/pure/check/simple.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("level violation"));
    }

    #[test]
    fn v2_transform_to_pure_blocked() {
        let code = "from mypackage.logic.pure.primitive.helpers import add\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/transform/coerce/simple.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("zone violation"));
    }

    #[test]
    fn v2_impure_simple_to_pure_primitive_ok() {
        let code = "from mypackage.logic.pure.primitive.helpers import add\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/impure/io_ops/simple.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_impure_to_pure_same_level_blocked() {
        let code = "from mypackage.logic.pure.simple.helpers import add\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/impure/io_ops/simple.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("level violation"));
    }

    #[test]
    fn v2_orchestrate_to_composed_ok() {
        let code = "from mypackage.logic.pure.composed.pipeline import run\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/orchestrate/main.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_orchestrate_to_simple_ok() {
        let code = "from mypackage.logic.pure.simple.helpers import add\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/orchestrate/main.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert!(violations.is_empty(), "orchestrate should reach any lower level");
    }

    #[test]
    fn v2_structure_imports_structure_ok() {
        let code = "from regin.structure.gen.schema.agent_paths_resolved import Capabilities\nfrom regin.structure.model.capability_tiers import CapabilityTiers\n";
        let parsed = parse_with_path(code, "/project/src/regin/structure/config/capability_tiers.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert!(violations.is_empty(), "structure-to-structure imports should be allowed, got: {:?}", violations.iter().map(|v| &v.message).collect::<Vec<_>>());
    }

    #[test]
    fn v2_external_import_skipped() {
        let code = "from pydantic import BaseModel\nimport json\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/pure/check/simple.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn v2_outside_file_is_violation() {
        let code = "import json\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/random_file.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("outside v2 zone layout"));
    }

    #[test]
    fn v2_pure_composed_unreachable_from_impure() {
        let code = "from mypackage.logic.pure.composed.pipeline import run\n";
        let parsed = parse_with_path(code, "/project/src/mypackage/logic/impure/workflow/composed.py");
        let violations = check_v2_import_boundaries(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("level violation"));
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

    // -- no_disallowed_stdlib --

    #[test]
    fn argparse_caught() {
        let parsed = parse("import argparse\n");
        let violations = check_no_disallowed_stdlib(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("typer"));
    }

    #[test]
    fn logging_caught() {
        let parsed = parse("import logging\n");
        let violations = check_no_disallowed_stdlib(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("loguru"));
    }

    #[test]
    fn json_from_import_caught() {
        let parsed = parse("from json import loads\n");
        let violations = check_no_disallowed_stdlib(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("pydantic"));
    }

    #[test]
    fn unittest_caught() {
        let parsed = parse("import unittest\n");
        let violations = check_no_disallowed_stdlib(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("pytest"));
    }

    #[test]
    fn configparser_caught() {
        let parsed = parse("import configparser\n");
        let violations = check_no_disallowed_stdlib(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("pydantic"));
    }

    #[test]
    fn pydantic_import_not_flagged() {
        let parsed = parse("from pydantic import BaseModel\n");
        let violations = check_no_disallowed_stdlib(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn typer_import_not_flagged() {
        let parsed = parse("import typer\n");
        let violations = check_no_disallowed_stdlib(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_before_validators --

    #[test]
    fn before_validator_caught() {
        let parsed = parse("from pydantic import BeforeValidator\n");
        let violations = check_no_before_validators(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("BeforeValidator"));
    }

    #[test]
    fn pydantic_basemodel_ok() {
        let parsed = parse("from pydantic import BaseModel\n");
        let violations = check_no_before_validators(&parsed, &default_config());
        assert!(violations.is_empty());
    }
}
