//! Core data structures for gleipnir guardrail checks.

/// Classification of a Python source file by content and path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Script,
    Test,
    DataStructure,
    UnsafeImpure,
    UnsafePure,
    ImpureFunction,
    PureFunction,
    Outside,
}

/// Violation severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Blocked,
    Error,
    Warning,
}

/// A single guardrail violation detected by a check.
#[derive(Debug, Clone)]
pub struct Violation {
    pub line: usize,
    pub check_name: String,
    pub severity: Severity,
    pub message: String,
    pub detail: String,
    pub signal: String,
    pub direction: String,
    pub canary: String,
}

/// Per-file check configuration. Thresholds derived from FileKind.
#[derive(Debug, Clone)]
pub struct CheckConfig {
    pub unsafe_files: Vec<String>,
    pub boundary_files: Vec<String>,
    pub allowed_pyright_ignores: Vec<String>,
    pub suppression_whitelist: Vec<String>,
    pub suppression_blacklist: Vec<String>,
    pub max_function_lines: usize,
    pub max_function_params: usize,
    pub max_nesting_depth: usize,
    pub max_functions_outside_zones: usize,
    pub max_union_members: usize,
    pub min_param_length: usize,
}

impl CheckConfig {
    /// Build config with thresholds appropriate for a file kind.
    pub fn for_kind(kind: FileKind) -> Self {
        let max_function_lines = match kind {
            FileKind::UnsafeImpure
            | FileKind::UnsafePure
            | FileKind::ImpureFunction
            | FileKind::PureFunction => 25,
            _ => 50,
        };

        Self {
            unsafe_files: vec![],
            boundary_files: vec![],
            allowed_pyright_ignores: vec![],
            suppression_whitelist: vec![],
            suppression_blacklist: vec![],
            max_function_lines,
            max_function_params: 5,
            max_nesting_depth: 4,
            max_functions_outside_zones: 3,
            max_union_members: 5,
            min_param_length: 4,
        }
    }
}

/// Parsed Python source file. Borrows source bytes for zero-copy access.
pub struct ParsedSource<'a> {
    pub file_path: &'a str,
    pub source_bytes: &'a [u8],
    pub lines: Vec<&'a str>,
    pub tree: tree_sitter::Tree,
}

/// Check function signature. Every check conforms to this.
pub type CheckFn = fn(&ParsedSource, &CheckConfig) -> Vec<Violation>;

/// Registry entry for a single check.
#[derive(Clone)]
pub struct CheckEntry {
    pub name: &'static str,
    pub severity: Severity,
    pub check_fn: CheckFn,
}

// -------------------------------------------------------------------------
// Messages loaded from embedded TOML
// -------------------------------------------------------------------------

/// Static message fields for a check, loaded from gleipnir_messages.toml.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct CheckMessages {
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub signal: String,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub canary: String,
}
