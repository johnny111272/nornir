//! Check matrix — maps FileKind to applicable checks.
//!
//! Source of truth: GLEIPNIR_PROCESSING.md check matrix.
//! Individual checks never inspect file paths. The matrix decides dispatch.

use crate::checks::{architecture, imports, prohibited, style, suppression, type_safety};
use crate::structures::{CheckEntry, CheckFn, FileKind, Severity};

/// A check identifier with its function and severity.
struct MatrixEntry {
    name: &'static str,
    severity: Severity,
    check_fn: CheckFn,
}

// Shorthand constructors
const fn blocked(name: &'static str, f: CheckFn) -> MatrixEntry {
    MatrixEntry { name, severity: Severity::Blocked, check_fn: f }
}
const fn error(name: &'static str, f: CheckFn) -> MatrixEntry {
    MatrixEntry { name, severity: Severity::Error, check_fn: f }
}
const fn warning(name: &'static str, f: CheckFn) -> MatrixEntry {
    MatrixEntry { name, severity: Severity::Warning, check_fn: f }
}

// =========================================================================
// Per-FileKind check lists (from GLEIPNIR_PROCESSING.md matrix)
// =========================================================================

static SCRIPT_CHECKS: &[MatrixEntry] = &[
    // TYPE SAFETY
    error("no_any_types", type_safety::check_no_any_types),
    error("no_any_type_aliases", type_safety::check_no_any_type_aliases),
    error("no_object", type_safety::check_no_object),
    error("no_json_value", type_safety::check_no_json_value),
    error("no_bare_collections", type_safety::check_no_bare_collections),
    error("no_implicit_type_aliases", type_safety::check_no_implicit_type_aliases),
    warning("union_member_count", type_safety::check_union_member_count),
    error("no_cast", prohibited::check_no_cast),
    // IMPORTS
    blocked("no_unsafe_imports", imports::check_no_unsafe_imports),
    blocked("no_relative_imports", imports::check_no_relative_imports),
    error("no_type_checking_imports", imports::check_no_type_checking_imports),
    error("no_parent_imports", imports::check_no_parent_imports),
    // PROHIBITED
    error("no_bare_except", prohibited::check_no_bare_except),
    error("no_broad_exceptions", prohibited::check_no_broad_exceptions),
    error("no_print", prohibited::check_no_print_calls),
    error("no_model_dump", prohibited::check_no_model_dump),
    error("no_overload", prohibited::check_no_overload),
    error("no_future_annotations", prohibited::check_no_future_annotations),
    error("init_files_empty", prohibited::check_init_files_empty),
    error("no_dunder_all", prohibited::check_no_dunder_all),
    error("no_suppression_comments", suppression::check_no_suppression_comments),
    // ARCHITECTURE (no hardcoded_config — scripts are self-contained)
    error("no_methods_in_classes", architecture::check_no_methods_in_classes),
    error("pydantic_only", architecture::check_pydantic_only),
    error("god_classes", architecture::check_god_classes),
    // STYLE
    warning("function_length", style::check_function_length),
    warning("param_count", style::check_param_count),
    warning("nesting_depth", style::check_nesting_depth),
    warning("no_underscore_prefix", style::check_no_underscore_prefix),
    warning("no_none_returns", style::check_no_none_returns),
    warning("no_throwaway_assignment", style::check_no_throwaway_assignment),
    warning("no_single_letter_names", style::check_no_single_letter_names),
    warning("no_numbered_suffixes", style::check_no_numbered_suffixes),
    warning("short_param_names", style::check_short_param_names),
];

static TEST_CHECKS: &[MatrixEntry] = &[
    // TYPE SAFETY (subset)
    error("no_any_types", type_safety::check_no_any_types),
    error("no_object", type_safety::check_no_object),
    error("no_json_value", type_safety::check_no_json_value),
    error("no_bare_collections", type_safety::check_no_bare_collections),
    // IMPORTS
    error("no_type_checking_imports", imports::check_no_type_checking_imports),
    // PROHIBITED (subset)
    error("no_bare_except", prohibited::check_no_bare_except),
    error("no_broad_exceptions", prohibited::check_no_broad_exceptions),
    error("no_future_annotations", prohibited::check_no_future_annotations),
    error("no_suppression_comments", suppression::check_no_suppression_comments),
];

static DATA_STRUCTURE_CHECKS: &[MatrixEntry] = &[
    // TYPE SAFETY
    error("no_any_types", type_safety::check_no_any_types),
    error("no_any_type_aliases", type_safety::check_no_any_type_aliases),
    error("no_object", type_safety::check_no_object),
    error("no_json_value", type_safety::check_no_json_value),
    error("no_bare_collections", type_safety::check_no_bare_collections),
    error("no_implicit_type_aliases", type_safety::check_no_implicit_type_aliases),
    warning("union_member_count", type_safety::check_union_member_count),
    error("no_cast", prohibited::check_no_cast),
    // IMPORTS
    blocked("no_unsafe_imports", imports::check_no_unsafe_imports),
    error("no_type_checking_imports", imports::check_no_type_checking_imports),
    error("no_parent_imports", imports::check_no_parent_imports),
    // PROHIBITED
    error("no_bare_except", prohibited::check_no_bare_except),
    error("no_broad_exceptions", prohibited::check_no_broad_exceptions),
    error("no_print", prohibited::check_no_print_calls),
    error("no_model_dump", prohibited::check_no_model_dump),
    error("no_overload", prohibited::check_no_overload),
    error("no_future_annotations", prohibited::check_no_future_annotations),
    error("init_files_empty", prohibited::check_init_files_empty),
    error("no_dunder_all", prohibited::check_no_dunder_all),
    error("no_suppression_comments", suppression::check_no_suppression_comments),
    // ARCHITECTURE
    error("no_methods_in_classes", architecture::check_no_methods_in_classes),
    error("pydantic_only", architecture::check_pydantic_only),
    error("hardcoded_config", architecture::check_hardcoded_config),
    warning("structures_no_functions", architecture::check_structures_no_functions),
    error("structures_import_boundary", architecture::check_structures_import_boundary),
];

static UNSAFE_IMPURE_CHECKS: &[MatrixEntry] = &[
    // TYPE SAFETY (no_any_types and no_cast exempt)
    error("no_object", type_safety::check_no_object),
    error("no_json_value", type_safety::check_no_json_value),
    error("no_bare_collections", type_safety::check_no_bare_collections),
    error("no_implicit_type_aliases", type_safety::check_no_implicit_type_aliases),
    warning("union_member_count", type_safety::check_union_member_count),
    // IMPORTS
    error("no_type_checking_imports", imports::check_no_type_checking_imports),
    error("no_parent_imports", imports::check_no_parent_imports),
    // PROHIBITED
    error("no_bare_except", prohibited::check_no_bare_except),
    error("no_broad_exceptions", prohibited::check_no_broad_exceptions),
    error("no_print", prohibited::check_no_print_calls),
    error("no_overload", prohibited::check_no_overload),
    error("no_future_annotations", prohibited::check_no_future_annotations),
    error("init_files_empty", prohibited::check_init_files_empty),
    error("no_dunder_all", prohibited::check_no_dunder_all),
    error("no_suppression_comments", suppression::check_no_suppression_comments),
    // ARCHITECTURE
    error("pydantic_only", architecture::check_pydantic_only),
    error("god_classes", architecture::check_god_classes),
    error("no_reexport_shims", architecture::check_no_reexport_shims),
    error("hardcoded_config", architecture::check_hardcoded_config),
    warning("classes_only_in_structures", architecture::check_classes_only_in_structures),
    // STYLE
    warning("function_length", style::check_function_length),
    warning("param_count", style::check_param_count),
    warning("nesting_depth", style::check_nesting_depth),
    warning("no_underscore_prefix", style::check_no_underscore_prefix),
    warning("no_none_returns", style::check_no_none_returns),
    warning("no_throwaway_assignment", style::check_no_throwaway_assignment),
    warning("no_single_letter_names", style::check_no_single_letter_names),
    warning("no_numbered_suffixes", style::check_no_numbered_suffixes),
    warning("short_param_names", style::check_short_param_names),
];

static UNSAFE_PURE_CHECKS: &[MatrixEntry] = &[
    // TYPE SAFETY (no_any_types and no_cast exempt)
    error("no_object", type_safety::check_no_object),
    error("no_json_value", type_safety::check_no_json_value),
    error("no_bare_collections", type_safety::check_no_bare_collections),
    error("no_implicit_type_aliases", type_safety::check_no_implicit_type_aliases),
    warning("union_member_count", type_safety::check_union_member_count),
    // IMPORTS
    error("impure_module_quarantine", imports::check_impure_module_quarantine),
    error("no_type_checking_imports", imports::check_no_type_checking_imports),
    error("no_parent_imports", imports::check_no_parent_imports),
    // PROHIBITED
    error("no_bare_except", prohibited::check_no_bare_except),
    error("no_broad_exceptions", prohibited::check_no_broad_exceptions),
    error("no_print", prohibited::check_no_print_calls),
    error("no_overload", prohibited::check_no_overload),
    error("no_future_annotations", prohibited::check_no_future_annotations),
    error("init_files_empty", prohibited::check_init_files_empty),
    error("no_dunder_all", prohibited::check_no_dunder_all),
    error("no_suppression_comments", suppression::check_no_suppression_comments),
    // ARCHITECTURE
    error("pydantic_only", architecture::check_pydantic_only),
    error("god_classes", architecture::check_god_classes),
    error("no_reexport_shims", architecture::check_no_reexport_shims),
    error("hardcoded_config", architecture::check_hardcoded_config),
    warning("classes_only_in_structures", architecture::check_classes_only_in_structures),
    // STYLE
    warning("function_length", style::check_function_length),
    warning("param_count", style::check_param_count),
    warning("nesting_depth", style::check_nesting_depth),
    warning("no_underscore_prefix", style::check_no_underscore_prefix),
    warning("no_none_returns", style::check_no_none_returns),
    warning("no_throwaway_assignment", style::check_no_throwaway_assignment),
    warning("no_single_letter_names", style::check_no_single_letter_names),
    warning("no_numbered_suffixes", style::check_no_numbered_suffixes),
    warning("short_param_names", style::check_short_param_names),
];

static IMPURE_FUNCTION_CHECKS: &[MatrixEntry] = &[
    // TYPE SAFETY
    error("no_any_types", type_safety::check_no_any_types),
    error("no_any_type_aliases", type_safety::check_no_any_type_aliases),
    error("no_object", type_safety::check_no_object),
    error("no_json_value", type_safety::check_no_json_value),
    error("no_bare_collections", type_safety::check_no_bare_collections),
    error("no_implicit_type_aliases", type_safety::check_no_implicit_type_aliases),
    warning("union_member_count", type_safety::check_union_member_count),
    error("no_cast", prohibited::check_no_cast),
    // IMPORTS
    blocked("no_unsafe_imports", imports::check_no_unsafe_imports),
    error("no_type_checking_imports", imports::check_no_type_checking_imports),
    error("no_parent_imports", imports::check_no_parent_imports),
    // PROHIBITED
    error("no_bare_except", prohibited::check_no_bare_except),
    error("no_broad_exceptions", prohibited::check_no_broad_exceptions),
    error("no_print", prohibited::check_no_print_calls),
    error("no_model_dump", prohibited::check_no_model_dump),
    error("no_overload", prohibited::check_no_overload),
    error("no_future_annotations", prohibited::check_no_future_annotations),
    error("init_files_empty", prohibited::check_init_files_empty),
    error("no_dunder_all", prohibited::check_no_dunder_all),
    error("no_suppression_comments", suppression::check_no_suppression_comments),
    // ARCHITECTURE
    error("pydantic_only", architecture::check_pydantic_only),
    error("god_classes", architecture::check_god_classes),
    error("no_reexport_shims", architecture::check_no_reexport_shims),
    error("hardcoded_config", architecture::check_hardcoded_config),
    warning("classes_only_in_structures", architecture::check_classes_only_in_structures),
    // STYLE
    warning("function_length", style::check_function_length),
    warning("param_count", style::check_param_count),
    warning("nesting_depth", style::check_nesting_depth),
    warning("no_underscore_prefix", style::check_no_underscore_prefix),
    warning("no_none_returns", style::check_no_none_returns),
    warning("no_throwaway_assignment", style::check_no_throwaway_assignment),
    warning("no_single_letter_names", style::check_no_single_letter_names),
    warning("no_numbered_suffixes", style::check_no_numbered_suffixes),
    warning("short_param_names", style::check_short_param_names),
];

static PURE_FUNCTION_CHECKS: &[MatrixEntry] = &[
    // TYPE SAFETY
    error("no_any_types", type_safety::check_no_any_types),
    error("no_any_type_aliases", type_safety::check_no_any_type_aliases),
    error("no_object", type_safety::check_no_object),
    error("no_json_value", type_safety::check_no_json_value),
    error("no_bare_collections", type_safety::check_no_bare_collections),
    error("no_implicit_type_aliases", type_safety::check_no_implicit_type_aliases),
    warning("union_member_count", type_safety::check_union_member_count),
    error("no_cast", prohibited::check_no_cast),
    // IMPORTS
    blocked("no_unsafe_imports", imports::check_no_unsafe_imports),
    error("impure_module_quarantine", imports::check_impure_module_quarantine),
    error("no_type_checking_imports", imports::check_no_type_checking_imports),
    error("no_parent_imports", imports::check_no_parent_imports),
    // PROHIBITED
    error("no_bare_except", prohibited::check_no_bare_except),
    error("no_broad_exceptions", prohibited::check_no_broad_exceptions),
    error("no_print", prohibited::check_no_print_calls),
    error("no_model_dump", prohibited::check_no_model_dump),
    error("no_overload", prohibited::check_no_overload),
    error("no_future_annotations", prohibited::check_no_future_annotations),
    error("init_files_empty", prohibited::check_init_files_empty),
    error("no_dunder_all", prohibited::check_no_dunder_all),
    error("no_suppression_comments", suppression::check_no_suppression_comments),
    // ARCHITECTURE
    error("pydantic_only", architecture::check_pydantic_only),
    error("god_classes", architecture::check_god_classes),
    error("no_reexport_shims", architecture::check_no_reexport_shims),
    error("hardcoded_config", architecture::check_hardcoded_config),
    warning("classes_only_in_structures", architecture::check_classes_only_in_structures),
    // STYLE
    warning("function_length", style::check_function_length),
    warning("param_count", style::check_param_count),
    warning("nesting_depth", style::check_nesting_depth),
    warning("no_underscore_prefix", style::check_no_underscore_prefix),
    warning("no_none_returns", style::check_no_none_returns),
    warning("no_throwaway_assignment", style::check_no_throwaway_assignment),
    warning("no_single_letter_names", style::check_no_single_letter_names),
    warning("no_numbered_suffixes", style::check_no_numbered_suffixes),
    warning("short_param_names", style::check_short_param_names),
];

static OUTSIDE_CHECKS: &[MatrixEntry] = &[
    // TYPE SAFETY (lazy — excusable via user.toml)
    error("no_any_types", type_safety::check_no_any_types),
    error("no_any_type_aliases", type_safety::check_no_any_type_aliases),
    error("no_object", type_safety::check_no_object),
    error("no_json_value", type_safety::check_no_json_value),
    error("no_bare_collections", type_safety::check_no_bare_collections),
    error("no_implicit_type_aliases", type_safety::check_no_implicit_type_aliases),
    warning("union_member_count", type_safety::check_union_member_count),
    error("no_cast", prohibited::check_no_cast),
    // IMPORTS (lazy — excusable)
    blocked("no_unsafe_imports", imports::check_no_unsafe_imports),
    error("no_type_checking_imports", imports::check_no_type_checking_imports),
    error("no_parent_imports", imports::check_no_parent_imports),
    // PROHIBITED
    error("no_bare_except", prohibited::check_no_bare_except),
    error("no_broad_exceptions", prohibited::check_no_broad_exceptions),
    error("no_print", prohibited::check_no_print_calls),
    error("no_model_dump", prohibited::check_no_model_dump),
    error("no_overload", prohibited::check_no_overload),
    error("no_future_annotations", prohibited::check_no_future_annotations),
    error("init_files_empty", prohibited::check_init_files_empty),
    error("no_dunder_all", prohibited::check_no_dunder_all),
    error("no_suppression_comments", suppression::check_no_suppression_comments),
    // ARCHITECTURE
    error("pydantic_only", architecture::check_pydantic_only),
    error("god_classes", architecture::check_god_classes),
    error("no_reexport_shims", architecture::check_no_reexport_shims),
    error("hardcoded_config", architecture::check_hardcoded_config),
    warning("classes_only_in_structures", architecture::check_classes_only_in_structures),
    warning("max_functions_outside_zones", architecture::check_max_functions_outside_zones),
    // STYLE
    warning("function_length", style::check_function_length),
    warning("param_count", style::check_param_count),
    warning("nesting_depth", style::check_nesting_depth),
    warning("no_underscore_prefix", style::check_no_underscore_prefix),
    warning("no_none_returns", style::check_no_none_returns),
    warning("no_throwaway_assignment", style::check_no_throwaway_assignment),
    warning("no_single_letter_names", style::check_no_single_letter_names),
    warning("no_numbered_suffixes", style::check_no_numbered_suffixes),
    warning("short_param_names", style::check_short_param_names),
];

/// Get the list of checks applicable to a file kind.
pub fn checks_for_kind(kind: FileKind) -> Vec<CheckEntry> {
    let entries: &[MatrixEntry] = match kind {
        FileKind::Script => SCRIPT_CHECKS,
        FileKind::Test => TEST_CHECKS,
        FileKind::DataStructure => DATA_STRUCTURE_CHECKS,
        FileKind::UnsafeImpure => UNSAFE_IMPURE_CHECKS,
        FileKind::UnsafePure => UNSAFE_PURE_CHECKS,
        FileKind::ImpureFunction => IMPURE_FUNCTION_CHECKS,
        FileKind::PureFunction => PURE_FUNCTION_CHECKS,
        FileKind::Outside => OUTSIDE_CHECKS,
    };

    entries
        .iter()
        .map(|e| CheckEntry {
            name: e.name,
            severity: e.severity,
            check_fn: e.check_fn,
        })
        .collect()
}
