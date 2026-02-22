//! All schemas embedded via `include_str!()`.
//!
//! Pipeline schemas (gates): 11 deployed.
//! Agent output schemas (writers): 2 deployed.

use schema_core::EmbeddedValidator;

// =============================================================================
// 9 deployed schemas
// =============================================================================

static RAW_DEF_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../schemas/agent-raw-definition.schema.json"
));
pub static RAW_DEFINITION: EmbeddedValidator =
    EmbeddedValidator::new(RAW_DEF_JSON, "raw-definition");

static PATHS_RES_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../schemas/agent-paths-resolved.schema.json"
));
pub static PATHS_RESOLVED: EmbeddedValidator =
    EmbeddedValidator::new(PATHS_RES_JSON, "paths-resolved");

static GUARD_RED_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../schemas/agent-guardrails-reduced.schema.json"
));
pub static GUARDRAILS_REDUCED: EmbeddedValidator =
    EmbeddedValidator::new(GUARD_RED_JSON, "guardrails-reduced");

static SF_RED_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../schemas/agent-sf-reduced.schema.json"
));
pub static SF_REDUCED: EmbeddedValidator =
    EmbeddedValidator::new(SF_RED_JSON, "sf-reduced");

static CRIT_MERG_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../schemas/agent-criteria-merged.schema.json"
));
pub static CRITERIA_MERGED: EmbeddedValidator =
    EmbeddedValidator::new(CRIT_MERG_JSON, "criteria-merged");

static INST_RED_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../schemas/agent-instructions-reduced.schema.json"
));
pub static INSTRUCTIONS_REDUCED: EmbeddedValidator =
    EmbeddedValidator::new(INST_RED_JSON, "instructions-reduced");

static EXAM_RED_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../schemas/agent-examples-reduced.schema.json"
));
pub static EXAMPLES_REDUCED: EmbeddedValidator =
    EmbeddedValidator::new(EXAM_RED_JSON, "examples-reduced");

static EXEC_MERG_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../schemas/agent-execution-merged.schema.json"
));
pub static EXECUTION_MERGED: EmbeddedValidator =
    EmbeddedValidator::new(EXEC_MERG_JSON, "execution-merged");

static INCL_RES_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../schemas/agent-includes-resolved.schema.json"
));
pub static INCLUDES_RESOLVED: EmbeddedValidator =
    EmbeddedValidator::new(INCL_RES_JSON, "includes-resolved");

static PERM_RES_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../schemas/agent-permissions-resolved.schema.json"
));
pub static PERMISSIONS_RESOLVED: EmbeddedValidator =
    EmbeddedValidator::new(PERM_RES_JSON, "permissions-resolved");

static UNIV_FMT_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../schemas/agent-universal-format.schema.json"
));
pub static UNIVERSAL_FORMAT: EmbeddedValidator =
    EmbeddedValidator::new(UNIV_FMT_JSON, "universal-format");

static UNIV_REND_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../schemas/agent-universal-render.schema.json"
));
pub static UNIVERSAL_RENDER: EmbeddedValidator =
    EmbeddedValidator::new(UNIV_REND_JSON, "universal-render");

// =============================================================================
// Agent output schemas (writers)
// =============================================================================

static QC_REPORT_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../schemas/qc-report.schema.json"
));
pub static QC_REPORT: EmbeddedValidator =
    EmbeddedValidator::new(QC_REPORT_JSON, "qc-report");

static GLOSSARY_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../schemas/glossary.schema.json"
));
pub static GLOSSARY: EmbeddedValidator =
    EmbeddedValidator::new(GLOSSARY_JSON, "glossary");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_definition_compiles() {
        assert_eq!(RAW_DEFINITION.schema_name(), "raw-definition");
        assert!(!RAW_DEFINITION.schema_json().is_empty());
    }

    #[test]
    fn test_paths_resolved_compiles() {
        assert_eq!(PATHS_RESOLVED.schema_name(), "paths-resolved");
        assert!(!PATHS_RESOLVED.schema_json().is_empty());
    }

    #[test]
    fn test_guardrails_reduced_compiles() {
        assert_eq!(GUARDRAILS_REDUCED.schema_name(), "guardrails-reduced");
    }

    #[test]
    fn test_sf_reduced_compiles() {
        assert_eq!(SF_REDUCED.schema_name(), "sf-reduced");
    }

    #[test]
    fn test_criteria_merged_compiles() {
        assert_eq!(CRITERIA_MERGED.schema_name(), "criteria-merged");
    }

    #[test]
    fn test_instructions_reduced_compiles() {
        assert_eq!(INSTRUCTIONS_REDUCED.schema_name(), "instructions-reduced");
    }

    #[test]
    fn test_examples_reduced_compiles() {
        assert_eq!(EXAMPLES_REDUCED.schema_name(), "examples-reduced");
    }

    #[test]
    fn test_execution_merged_compiles() {
        assert_eq!(EXECUTION_MERGED.schema_name(), "execution-merged");
    }

    #[test]
    fn test_includes_resolved_compiles() {
        assert_eq!(INCLUDES_RESOLVED.schema_name(), "includes-resolved");
    }

    #[test]
    fn test_permissions_resolved_compiles() {
        assert_eq!(PERMISSIONS_RESOLVED.schema_name(), "permissions-resolved");
        assert!(!PERMISSIONS_RESOLVED.schema_json().is_empty());
    }

    #[test]
    fn test_universal_format_compiles() {
        assert_eq!(UNIVERSAL_FORMAT.schema_name(), "universal-format");
        assert!(!UNIVERSAL_FORMAT.schema_json().is_empty());
    }

    #[test]
    fn test_universal_render_compiles() {
        assert_eq!(UNIVERSAL_RENDER.schema_name(), "universal-render");
        assert!(!UNIVERSAL_RENDER.schema_json().is_empty());
    }

    // Agent output schemas

    #[test]
    fn test_qc_report_compiles() {
        assert_eq!(QC_REPORT.schema_name(), "qc-report");
        assert!(!QC_REPORT.schema_json().is_empty());
    }

    #[test]
    fn test_glossary_compiles() {
        assert_eq!(GLOSSARY.schema_name(), "glossary");
        assert!(!GLOSSARY.schema_json().is_empty());
    }
}
