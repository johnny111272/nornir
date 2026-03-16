//! Architecture enforcement checks.
//!
//! Checks: no_methods_in_classes, pydantic_only, god_classes,
//! no_reexport_shims, hardcoded_config, structures_no_functions,
//! structures_import_boundary, classes_only_in_structures,
//! max_functions_outside_zones.

use crate::parsing::{
    extract_module_info, find_nodes_by_type, node_field, node_line, node_text,
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
// no_methods_in_classes
// -------------------------------------------------------------------------

pub fn check_no_methods_in_classes(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for class_node in find_nodes_by_type(source.tree.root_node(), "class_definition") {
        let class_name = node_field(class_node, "name")
            .map(|n| node_text(n, source.source_bytes))
            .unwrap_or("<unknown>");

        let body = match node_field(class_node, "body") {
            Some(b) => b,
            None => continue,
        };

        let mut cursor = body.walk();
        for child in body.named_children(&mut cursor) {
            if child.kind() == "function_definition" {
                let method_name = node_field(child, "name")
                    .map(|n| node_text(n, source.source_bytes))
                    .unwrap_or("<unknown>");
                violations.push(violation(
                    node_line(child),
                    format!("Method '{method_name}' in class '{class_name}'"),
                ));
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// pydantic_only
// -------------------------------------------------------------------------

// Reversed to avoid self-triggering
const REVERSED_PYDANTIC_FORBIDDEN: &[&str] = &[
    "tciDdepyT",
    "ssalcatad@",
    "elpuTdemaN",
    "sailAepyT",
    "enifed@",
    "nezorf@",
    "elbatum@",
    "srtta@",
    "tcurtS.cepsgsm",
    "tcurts.cepsgsm@",
];

fn pydantic_forbidden_patterns() -> Vec<String> {
    REVERSED_PYDANTIC_FORBIDDEN
        .iter()
        .map(|s| s.chars().rev().collect())
        .collect()
}

pub fn check_pydantic_only(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let patterns = pydantic_forbidden_patterns();
    let mut violations = Vec::new();

    for (line_num, line) in source.lines.iter().enumerate() {
        let line_num = line_num + 1;
        for pattern in &patterns {
            if line.contains(pattern.as_str()) {
                violations.push(violation(line_num, format!("{pattern} found")));
                break;
            }
        }
    }
    violations
}

// -------------------------------------------------------------------------
// god_classes
// -------------------------------------------------------------------------

const GOD_CLASS_ALLOWED_METHODS: &[&str] = &[
    "__init__", "__repr__", "__str__", "__eq__", "__hash__",
    "__lt__", "__le__", "__gt__", "__ge__", "__bool__",
    "__len__", "__iter__", "__getitem__", "__setitem__",
    "__contains__", "__post_init__",
    "model_validate", "model_json_schema",
    "model_copy", "model_post_init", "model_rebuild",
];

const GOD_CLASS_ALLOWED_DECORATORS: &[&str] = &[
    "property", "cached_property", "staticmethod", "classmethod",
    "validator", "field_validator", "model_validator",
    "computed_field", "root_validator",
];

fn extract_decorator_name_arch(decorator: tree_sitter::Node, source: &[u8]) -> String {
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

fn unwrap_decorated<'a>(
    node: tree_sitter::Node<'a>,
) -> Option<(tree_sitter::Node<'a>, Vec<tree_sitter::Node<'a>>)> {
    match node.kind() {
        "function_definition" => Some((node, vec![])),
        "decorated_definition" => {
            let mut cursor = node.walk();
            let mut func = None;
            let mut decorators = Vec::new();
            for child in node.named_children(&mut cursor) {
                match child.kind() {
                    "function_definition" => func = Some(child),
                    "decorator" => decorators.push(child),
                    _ => {}
                }
            }
            func.map(|f| (f, decorators))
        }
        _ => None,
    }
}

fn classify_class_methods(body: tree_sitter::Node, source: &[u8]) -> Vec<String> {
    let mut behavior = Vec::new();
    let mut cursor = body.walk();

    for child in body.named_children(&mut cursor) {
        let (func_node, decorators) = match unwrap_decorated(child) {
            Some(pair) => pair,
            None => continue,
        };

        let method_name = node_field(func_node, "name")
            .map(|n| node_text(n, source))
            .unwrap_or("");

        if GOD_CLASS_ALLOWED_METHODS.contains(&method_name) || method_name.starts_with('_') {
            continue;
        }

        let has_allowed_dec = decorators.iter().any(|dec| {
            let name = extract_decorator_name_arch(*dec, source);
            GOD_CLASS_ALLOWED_DECORATORS.contains(&name.as_str())
        });

        if !has_allowed_dec {
            behavior.push(method_name.to_string());
        }
    }
    behavior
}

pub fn check_god_classes(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut violations = Vec::new();

    for class_node in find_nodes_by_type(source.tree.root_node(), "class_definition") {
        let class_name = node_field(class_node, "name")
            .map(|n| node_text(n, source.source_bytes))
            .unwrap_or("");

        if class_name.starts_with("Test") || class_name.ends_with("Error") {
            continue;
        }

        let body = match node_field(class_node, "body") {
            Some(b) => b,
            None => continue,
        };

        let behavior = classify_class_methods(body, source.source_bytes);
        if !behavior.is_empty() {
            let methods_str = behavior.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            violations.push(violation(
                node_line(class_node),
                format!("{class_name} has behavior methods: {methods_str}"),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// no_reexport_shims
// -------------------------------------------------------------------------

fn classify_expression_stmt(stmt: tree_sitter::Node, source: &[u8]) -> &'static str {
    let mut cursor = stmt.walk();
    let children: Vec<_> = stmt.named_children(&mut cursor).collect();
    if children.is_empty() || children[0].kind() != "assignment" {
        return "other";
    }

    let assign = children[0];
    let left = match node_field(assign, "left") {
        Some(n) if n.kind() == "identifier" => n,
        _ => return "logic",
    };

    if node_text(left, source) == "__all__" {
        "all"
    } else {
        "logic"
    }
}

fn extract_module_docstring(root: tree_sitter::Node, source: &[u8]) -> String {
    let mut cursor = root.walk();
    let first = match root.named_children(&mut cursor).next() {
        Some(n) if n.kind() == "expression_statement" => n,
        _ => return String::new(),
    };
    let mut inner_cursor = first.walk();
    let inner = match first.named_children(&mut inner_cursor).next() {
        Some(n) if n.kind() == "string" => n,
        _ => return String::new(),
    };
    node_text(inner, source).to_string()
}

pub fn check_no_reexport_shims(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let mut has_imports = false;
    let mut has_all = false;
    let mut has_logic = false;

    let root = source.tree.root_node();
    let docstring = extract_module_docstring(root, source.source_bytes);

    let compat_keywords = ["backward compatibility", "deprecated", "re-export", "compatibility shim"];
    let doc_lower = docstring.to_lowercase();
    let has_compat_docstring = compat_keywords.iter().any(|kw| doc_lower.contains(kw));

    let mut cursor = root.walk();
    for stmt in root.named_children(&mut cursor) {
        match stmt.kind() {
            "import_statement" | "import_from_statement" => has_imports = true,
            "function_definition" | "class_definition" | "decorated_definition" => has_logic = true,
            "expression_statement" => {
                match classify_expression_stmt(stmt, source.source_bytes) {
                    "all" => has_all = true,
                    "logic" => has_logic = true,
                    _ => {}
                }
            }
            _ => {}
        }
    }

    let mut violations = Vec::new();

    if has_imports && has_all && !has_logic {
        violations.push(violation(
            1,
            "re-export shim detected (imports + __all__, no logic)".to_string(),
        ));
    } else if has_compat_docstring {
        violations.push(violation(
            1,
            "backward compatibility docstring found".to_string(),
        ));
    }

    violations
}

// -------------------------------------------------------------------------
// hardcoded_config
// -------------------------------------------------------------------------

const CONSTANT_NODE_TYPES: &[&str] = &[
    "string", "integer", "float", "true", "false", "none", "concatenated_string",
];

/// Collection constructor names that are equivalent to literal syntax.
/// `frozenset([...])` is the same intent as `{...}` — hardcoded config.
const COLLECTION_CONSTRUCTORS: &[&str] = &["frozenset", "set", "dict", "list", "tuple"];

/// Check if a `call` node is a collection constructor (frozenset, set, dict, list, tuple).
fn is_collection_constructor(call_node: tree_sitter::Node, source: &[u8]) -> bool {
    let func = match node_field(call_node, "function") {
        Some(n) if n.kind() == "identifier" => n,
        _ => return false,
    };
    COLLECTION_CONSTRUCTORS.contains(&node_text(func, source))
}

pub fn check_hardcoded_config(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
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

        let left = match node_field(assign, "left") {
            Some(n) if n.kind() == "identifier" => n,
            _ => continue,
        };
        let right = match node_field(assign, "right") {
            Some(n) => n,
            None => continue,
        };

        let name = node_text(left, source.source_bytes);

        // Skip dunder names
        if name.starts_with("__") && name.ends_with("__") {
            continue;
        }

        if right.kind() == "dictionary" || right.kind() == "list" {
            violations.push(violation(
                node_line(assign),
                format!("hard-coded config {name} found (dict/list at module level)"),
            ));
        } else if right.kind() == "call" && is_collection_constructor(right, source.source_bytes) {
            violations.push(violation(
                node_line(assign),
                format!("hard-coded config {name} found (collection constructor at module level)"),
            ));
        } else if name.len() > 1
            && name.chars().all(|c| c.is_ascii_uppercase() || c == '_')
            && CONSTANT_NODE_TYPES.contains(&right.kind())
        {
            violations.push(violation(
                node_line(assign),
                format!("hard-coded constant {name} found (ALL_CAPS value)"),
            ));
        }
    }
    violations
}

// -------------------------------------------------------------------------
// import_count
// -------------------------------------------------------------------------

/// Detect excessive import fan-in by category.
///
/// Counts import statements separately for functions/, structures/, and other.
/// High function-import count is the strongest signal of fragmented OOP —
/// a coordinator importing single-function siblings.
pub fn check_import_count(source: &ParsedSource, _config: &CheckConfig) -> Vec<Violation> {
    let root = source.tree.root_node();
    let mut cursor = root.walk();
    let mut fn_imports = 0;
    let mut other_imports = 0;

    for stmt in root.named_children(&mut cursor) {
        let module_path = match import_module_path(stmt, source.source_bytes) {
            Some(p) => p,
            None => continue,
        };

        if module_path.contains("functions.") || module_path.contains("functions/") {
            fn_imports += 1;
        } else if module_path.contains("structures.") || module_path.contains("structures/") {
            // No limit on structure imports — functions need their data types
        } else {
            other_imports += 1;
        }
    }

    let mut violations = Vec::new();

    if fn_imports > MAX_FUNCTION_IMPORTS {
        violations.push(violation(
            1,
            format!("{fn_imports} imports from functions/ — this module is coordinating, not computing"),
        ));
    }
    if other_imports > MAX_OTHER_IMPORTS {
        violations.push(violation(
            1,
            format!("{other_imports} external/stdlib imports — module has too many external dependencies"),
        ));
    }

    violations
}

const MAX_FUNCTION_IMPORTS: usize = 3;
const MAX_OTHER_IMPORTS: usize = 3;

/// Extract the module path from an import statement.
fn import_module_path<'a>(node: tree_sitter::Node, source: &'a [u8]) -> Option<&'a str> {
    if node.kind() == "import_from_statement" {
        return node_field(node, "module_name").map(|n| node_text(n, source));
    }
    if node.kind() == "import_statement" {
        let mut cursor = node.walk();
        return node.named_children(&mut cursor)
            .find(|c| c.kind() == "dotted_name")
            .map(|n| node_text(n, source));
    }
    None
}

// -------------------------------------------------------------------------
// structures_no_functions
// -------------------------------------------------------------------------

fn unwrap_function(node: tree_sitter::Node) -> Option<tree_sitter::Node> {
    if node.kind() == "function_definition" {
        return Some(node);
    }
    if node.kind() == "decorated_definition" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "function_definition" {
                return Some(child);
            }
        }
    }
    None
}

pub fn check_structures_no_functions(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    // Matrix handles dispatch — only called for DataStructure files
    let mut violations = Vec::new();
    let root = source.tree.root_node();
    let mut cursor = root.walk();

    for stmt in root.named_children(&mut cursor) {
        let func_node = match unwrap_function(stmt) {
            Some(f) => f,
            None => continue,
        };
        let func_name = node_field(func_node, "name")
            .map(|n| node_text(n, source.source_bytes))
            .unwrap_or("<unknown>");
        violations.push(violation(
            node_line(func_node),
            format!("function '{func_name}()' defined in structures/"),
        ));
    }
    violations
}

// -------------------------------------------------------------------------
// structures_import_boundary
// -------------------------------------------------------------------------

const STRUCTURES_ALLOWED_STDLIB: &[&str] = &[
    "pydantic", "typing", "collections", "enum", "tree_sitter",
    "typing_extensions", "annotated_types",
];

const STRUCTURES_ALLOWED_MODULES: &[&str] = &["pydantic_validators"];

fn is_structures_internal(module_path: &str) -> bool {
    let parts: Vec<&str> = module_path.split('.').collect();
    if parts.contains(&"structures") {
        return true;
    }
    let last = parts.last().copied().unwrap_or("");
    STRUCTURES_ALLOWED_MODULES.contains(&last)
}

pub fn check_structures_import_boundary(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    // Matrix handles dispatch — only called for DataStructure files
    let mut violations = Vec::new();

    for node in find_nodes_by_type(source.tree.root_node(), "import_from_statement") {
        let (module, _level) = extract_module_info(node, source.source_bytes);
        if module.is_empty() {
            continue;
        }

        let parts: Vec<&str> = module.split('.').collect();
        let top = parts.first().copied().unwrap_or("");

        if STRUCTURES_ALLOWED_STDLIB.contains(&top) {
            continue;
        }

        if !is_structures_internal(&module) {
            violations.push(violation(
                node_line(node),
                format!("structures/ file imports from outside data zone: {module}"),
            ));
        }
    }

    for node in find_nodes_by_type(source.tree.root_node(), "import_statement") {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() != "dotted_name" {
                continue;
            }
            let module = node_text(child, source.source_bytes);
            let parts: Vec<&str> = module.split('.').collect();
            let top = parts.first().copied().unwrap_or("");

            if STRUCTURES_ALLOWED_STDLIB.contains(&top) {
                continue;
            }

            if !is_structures_internal(module) {
                violations.push(violation(
                    node_line(node),
                    format!("structures/ file imports from outside data zone: {module}"),
                ));
            }
        }
    }

    violations
}

// -------------------------------------------------------------------------
// classes_only_in_structures
// -------------------------------------------------------------------------

pub fn check_classes_only_in_structures(
    source: &ParsedSource,
    _config: &CheckConfig,
) -> Vec<Violation> {
    // Matrix handles dispatch — only called for zone files (Pure/Impure/Unsafe)
    let mut violations = Vec::new();

    for class_node in find_nodes_by_type(source.tree.root_node(), "class_definition") {
        let class_name = node_field(class_node, "name")
            .map(|n| node_text(n, source.source_bytes))
            .unwrap_or("<unknown>");
        violations.push(violation(
            node_line(class_node),
            format!("class '{class_name}' defined outside structures/"),
        ));
    }
    violations
}

// -------------------------------------------------------------------------
// max_functions_outside_zones
// -------------------------------------------------------------------------

pub fn check_max_functions_outside_zones(
    source: &ParsedSource,
    config: &CheckConfig,
) -> Vec<Violation> {
    // Matrix handles dispatch — only called for Outside/Script files
    let mut functions: Vec<&str> = Vec::new();
    let root = source.tree.root_node();
    let mut cursor = root.walk();

    for stmt in root.named_children(&mut cursor) {
        let func_node = match unwrap_function(stmt) {
            Some(f) => f,
            None => continue,
        };
        let func_name = match node_field(func_node, "name") {
            Some(n) => node_text(n, source.source_bytes),
            None => continue,
        };
        if func_name.starts_with('_') || func_name.starts_with("test_") {
            continue;
        }
        functions.push(func_name);
    }

    if functions.len() <= config.max_functions_outside_zones {
        return Vec::new();
    }

    // Fuzzy message — no counts, no thresholds
    vec![violation(
        1,
        "too many top-level functions outside zones".to_string(),
    )]
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

    // -- no_methods_in_classes --

    #[test]
    fn method_in_class_caught() {
        let parsed = parse("class Foo:\n    def greet(self):\n        pass\n");
        let violations = check_no_methods_in_classes(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("greet"));
        assert!(violations[0].message.contains("Foo"));
    }

    #[test]
    fn class_without_methods_ok() {
        let parsed = parse("class Foo:\n    x: int = 0\n");
        let violations = check_no_methods_in_classes(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- pydantic_only --

    #[test]
    fn dataclass_caught() {
        let parsed = parse("@dataclass\nclass Foo:\n    x: int\n");
        let violations = check_pydantic_only(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn typed_dict_caught() {
        let parsed = parse("class Foo(TypedDict):\n    x: int\n");
        let violations = check_pydantic_only(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn pydantic_model_ok() {
        let parsed = parse("class Foo(BaseModel):\n    x: int\n");
        let violations = check_pydantic_only(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- god_classes --

    #[test]
    fn god_class_caught() {
        let parsed = parse("class Foo:\n    def process(self):\n        pass\n");
        let violations = check_god_classes(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Foo"));
        assert!(violations[0].message.contains("process"));
    }

    #[test]
    fn dunder_methods_ok() {
        let parsed = parse("class Foo:\n    def __init__(self):\n        pass\n    def __repr__(self):\n        pass\n");
        let violations = check_god_classes(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn test_class_skipped() {
        let parsed = parse("class TestFoo:\n    def process(self):\n        pass\n");
        let violations = check_god_classes(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn error_class_skipped() {
        let parsed = parse("class MyError:\n    def process(self):\n        pass\n");
        let violations = check_god_classes(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn property_decorator_ok() {
        let parsed = parse("class Foo:\n    @property\n    def name(self):\n        return self._name\n");
        let violations = check_god_classes(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- no_reexport_shims --

    #[test]
    fn reexport_shim_caught() {
        let parsed = parse("from foo import bar\n__all__ = ['bar']\n");
        let violations = check_no_reexport_shims(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("re-export shim"));
    }

    #[test]
    fn module_with_logic_ok() {
        let parsed = parse("from foo import bar\n__all__ = ['bar']\ndef process(): pass\n");
        let violations = check_no_reexport_shims(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn compat_docstring_caught() {
        let parsed = parse("\"\"\"Backward compatibility shim.\"\"\"\nfrom foo import bar\n");
        let violations = check_no_reexport_shims(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("backward compatibility"));
    }

    // -- hardcoded_config --

    #[test]
    fn module_level_dict_caught() {
        let parsed = parse("CONFIG = {'key': 'value'}\n");
        let violations = check_hardcoded_config(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("CONFIG"));
    }

    #[test]
    fn frozenset_constructor_caught() {
        let parsed = parse("KNOWN_SECTIONS = frozenset(['a', 'b', 'c'])\n");
        let violations = check_hardcoded_config(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("KNOWN_SECTIONS"));
    }

    #[test]
    fn set_constructor_caught() {
        let parsed = parse("VALID_TYPES = set(['input', 'output'])\n");
        let violations = check_hardcoded_config(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("VALID_TYPES"));
    }

    #[test]
    fn all_caps_constant_caught() {
        let parsed = parse("MAX_RETRIES = 3\n");
        let violations = check_hardcoded_config(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("MAX_RETRIES"));
    }

    #[test]
    fn dunder_name_ok() {
        let parsed = parse("__version__ = '1.0'\n");
        let violations = check_hardcoded_config(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn lowercase_assignment_ok() {
        let parsed = parse("result = compute()\n");
        let violations = check_hardcoded_config(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- import_count --

    #[test]
    fn few_function_imports_ok() {
        let parsed = parse(
            "from pkg.functions.pure.utils import helper\nfrom pkg.structures.types import MyType\ndef work(): pass\n",
        );
        let violations = check_import_count(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn many_function_imports_caught() {
        let code = (0..5)
            .map(|i| format!("from pkg.functions.pure.reshape_{i} import reshape_{i}\n"))
            .collect::<String>()
            + "def coordinate(): pass\n";
        let parsed = parse(&code);
        let violations = check_import_count(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("functions/"));
    }

    #[test]
    fn function_imports_at_threshold_ok() {
        let code = (0..3)
            .map(|i| format!("from pkg.functions.pure.mod_{i} import fn_{i}\n"))
            .collect::<String>()
            + "def work(): pass\n";
        let parsed = parse(&code);
        let violations = check_import_count(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn structure_imports_no_limit() {
        let code = (0..10)
            .map(|i| format!("from pkg.structures.types_{i} import Model_{i}\n"))
            .collect::<String>()
            + "def work(): pass\n";
        let parsed = parse(&code);
        let violations = check_import_count(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn many_stdlib_imports_caught() {
        let code = (0..5)
            .map(|i| format!("from stdlib_{i} import thing_{i}\n"))
            .collect::<String>()
            + "def work(): pass\n";
        let parsed = parse(&code);
        let violations = check_import_count(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("external/stdlib"));
    }

    // -- structures_no_functions --

    #[test]
    fn function_in_structures_caught() {
        let parsed = parse("def helper():\n    pass\n");
        let violations = check_structures_no_functions(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("helper"));
    }

    #[test]
    fn class_in_structures_ok() {
        let parsed = parse("class Foo:\n    x: int = 0\n");
        let violations = check_structures_no_functions(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- structures_import_boundary --

    #[test]
    fn import_from_functions_caught() {
        let parsed = parse("from myproject.functions.pure.helper import process\n");
        let violations = check_structures_import_boundary(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("outside data zone"));
    }

    #[test]
    fn pydantic_import_ok() {
        let parsed = parse("from pydantic import BaseModel\n");
        let violations = check_structures_import_boundary(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn structures_internal_import_ok() {
        let parsed = parse("from myproject.structures.base import BaseEntity\n");
        let violations = check_structures_import_boundary(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    #[test]
    fn pydantic_validators_ok() {
        let parsed = parse("from myproject.pydantic_validators import check_name\n");
        let violations = check_structures_import_boundary(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- classes_only_in_structures --

    #[test]
    fn class_outside_structures_caught() {
        let parsed = parse("class Foo:\n    x: int = 0\n");
        let violations = check_classes_only_in_structures(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Foo"));
        assert!(violations[0].message.contains("outside structures"));
    }

    #[test]
    fn no_classes_ok() {
        let parsed = parse("def helper(): pass\n");
        let violations = check_classes_only_in_structures(&parsed, &default_config());
        assert!(violations.is_empty());
    }

    // -- max_functions_outside_zones --

    #[test]
    fn few_functions_ok() {
        let parsed = parse("def a(): pass\ndef b(): pass\ndef c(): pass\n");
        let violations = check_max_functions_outside_zones(&parsed, &default_config());
        assert!(violations.is_empty()); // 3 functions, max 3
    }

    #[test]
    fn too_many_functions_caught() {
        let parsed = parse("def a(): pass\ndef b(): pass\ndef c(): pass\ndef d(): pass\n");
        let violations = check_max_functions_outside_zones(&parsed, &default_config());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("too many"));
        // Verify fuzzy — no counts in message
        assert!(!violations[0].message.contains("4"));
        assert!(!violations[0].message.contains("max"));
    }

    #[test]
    fn private_functions_skipped() {
        let parsed = parse("def _a(): pass\ndef _b(): pass\ndef _c(): pass\ndef _d(): pass\ndef public(): pass\n");
        let violations = check_max_functions_outside_zones(&parsed, &default_config());
        assert!(violations.is_empty()); // Only 1 public function
    }

    #[test]
    fn test_functions_skipped() {
        let parsed = parse("def test_a(): pass\ndef test_b(): pass\ndef test_c(): pass\ndef test_d(): pass\n");
        let violations = check_max_functions_outside_zones(&parsed, &default_config());
        assert!(violations.is_empty());
    }
}
