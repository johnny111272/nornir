//! Educational error diagnostics for .tomlx parsing.
//!
//! Error format:
//! ```text
//! ERROR: <file>:<line>
//!   <What is wrong>
//!   Rule: <The constraint violated>
//!   Fix: <Actionable guidance>
//! ```

use std::fmt;

#[derive(Debug, Clone)]
pub struct Location {
    pub file: Option<String>,
    pub line: usize,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.file {
            Some(file) => write!(f, "{}:{}", file, self.line),
            None => write!(f, "line {}", self.line),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MalformedSectionAnnotation {
    pub location: Location,
    pub section_name: String,
    pub raw_annotation: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct MalformedFieldAnnotation {
    pub location: Location,
    pub field_name: String,
    pub raw_annotation: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct OrphanUnitAnnotation {
    pub location: Location,
    pub field_name: String,
    pub unit: String,
    pub section_name: String,
}

#[derive(Debug, Clone)]
pub struct OrphanPathAnnotation {
    pub location: Location,
    pub field_name: String,
    pub path_ref: String,
    pub section_name: String,
}

#[derive(Debug, Clone)]
pub struct MissingFieldAnnotation {
    pub location: Location,
    pub field_name: String,
    pub section_name: String,
    pub expected_type: String,
}

#[derive(Debug, Clone)]
pub struct BaseRedeclaration {
    pub location: Location,
    pub section_name: String,
    pub base_name: String,
    pub original_line: usize,
    pub original_value: String,
}

#[derive(Debug, Clone)]
pub struct UnknownUnit {
    pub location: Location,
    pub field_name: String,
    pub unit: String,
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IncompatibleUnitFamily {
    pub location: Location,
    pub field_name: String,
    pub unit: String,
    pub unit_family: String,
    pub target: String,
    pub target_family: String,
}

#[derive(Debug, Clone)]
pub struct UndefinedPathReference {
    pub location: Location,
    pub field_name: String,
    pub reference: String,
    pub defined_refs: Vec<String>,
}

/// Collection of all .tomlx parsing issues.
#[derive(Debug, Default)]
pub struct TomlxIssues {
    pub file: Option<String>,
    pub malformed_section_annotations: Vec<MalformedSectionAnnotation>,
    pub malformed_field_annotations: Vec<MalformedFieldAnnotation>,
    pub orphan_unit_annotations: Vec<OrphanUnitAnnotation>,
    pub orphan_path_annotations: Vec<OrphanPathAnnotation>,
    pub missing_field_annotations: Vec<MissingFieldAnnotation>,
    pub base_redeclarations: Vec<BaseRedeclaration>,
    pub unknown_units: Vec<UnknownUnit>,
    pub incompatible_unit_families: Vec<IncompatibleUnitFamily>,
    pub undefined_path_references: Vec<UndefinedPathReference>,
}

impl TomlxIssues {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_file(file: impl Into<String>) -> Self {
        Self { file: Some(file.into()), ..Self::default() }
    }

    pub fn loc(&self, line: usize) -> Location {
        Location { file: self.file.clone(), line }
    }

    pub fn has_issues(&self) -> bool {
        !self.malformed_section_annotations.is_empty()
            || !self.malformed_field_annotations.is_empty()
            || !self.orphan_unit_annotations.is_empty()
            || !self.orphan_path_annotations.is_empty()
            || !self.missing_field_annotations.is_empty()
            || !self.base_redeclarations.is_empty()
            || !self.unknown_units.is_empty()
            || !self.incompatible_unit_families.is_empty()
            || !self.undefined_path_references.is_empty()
    }

    pub fn count(&self) -> usize {
        self.malformed_section_annotations.len()
            + self.malformed_field_annotations.len()
            + self.orphan_unit_annotations.len()
            + self.orphan_path_annotations.len()
            + self.missing_field_annotations.len()
            + self.base_redeclarations.len()
            + self.unknown_units.len()
            + self.incompatible_unit_families.len()
            + self.undefined_path_references.len()
    }

    /// Format all issues as an educational error message.
    pub fn format(&self) -> String {
        let mut sections = Vec::new();

        for issue in &self.malformed_section_annotations {
            sections.push(format!(
                "ERROR: {}\n  \
                 Section [{}] has malformed annotation.\n\n  \
                 Found: {}\n  \
                 Problem: {}\n\n  \
                 Fix: Use format: [section]  # target=seconds\n       \
                      Or: [section]  # target=path, base=~/app/",
                issue.location, issue.section_name, issue.raw_annotation, issue.reason
            ));
        }

        for issue in &self.malformed_field_annotations {
            sections.push(format!(
                "ERROR: {}\n  \
                 Field '{}' has malformed annotation.\n\n  \
                 Found: {}\n  \
                 Problem: {}\n\n  \
                 Fix: Use format: field = value  # unit=seconds\n       \
                      Or: field = \"path/\"  # path=base",
                issue.location, issue.field_name, issue.raw_annotation, issue.reason
            ));
        }

        for issue in &self.orphan_unit_annotations {
            sections.push(format!(
                "ERROR: {}\n  \
                 Field '{}' has unit annotation but section [{}] has no target.\n\n  \
                 Rule: Unit annotations require a section target declaration.\n\n  \
                 Fix: Add target to section: [{}]  # target=seconds\n       \
                      Or remove the annotation if no conversion needed.",
                issue.location, issue.field_name, issue.section_name, issue.section_name
            ));
        }

        for issue in &self.orphan_path_annotations {
            sections.push(format!(
                "ERROR: {}\n  \
                 Field '{}' has path annotation but section [{}] has no target.\n\n  \
                 Rule: Path annotations require a section target=path declaration.\n\n  \
                 Fix: Add target to section: [{}]  # target=path, base=~/app/\n       \
                      Or remove the annotation if no path expansion needed.",
                issue.location, issue.field_name, issue.section_name, issue.section_name
            ));
        }

        for issue in &self.missing_field_annotations {
            sections.push(format!(
                "ERROR: {}\n  \
                 Section [{}] has target but field '{}' has no {} annotation.\n\n  \
                 Rule: All fields in targeted sections must have annotations.\n       \
                       All units must be convertible to the section target.\n\n  \
                 Fix: Add annotation: {} = value  # {}=...\n       \
                      Or move to a non-targeted section if this field doesn't need conversion.",
                issue.location, issue.section_name, issue.field_name, issue.expected_type,
                issue.field_name, issue.expected_type
            ));
        }

        for issue in &self.base_redeclarations {
            sections.push(format!(
                "ERROR: {}\n  \
                 Section [{}] attempts to redefine '{}' but path bases are document-level constants.\n\n  \
                 Rule: Path bases must be declared in the FIRST target=path section only.\n       \
                       Subsequent target=path sections inherit these bases.\n\n  \
                 '{}' was defined at line {}: {}={}\n\n  \
                 Fix: Remove '{}=...' from this section header.\n       \
                      Use: [{}]  # target=path",
                issue.location, issue.section_name, issue.base_name,
                issue.base_name, issue.original_line, issue.base_name, issue.original_value,
                issue.base_name, issue.section_name
            ));
        }

        for issue in &self.unknown_units {
            let suggestions = if issue.suggestions.is_empty() {
                String::new()
            } else {
                format!("\n\n  Did you mean: {}?", issue.suggestions.join(", "))
            };

            sections.push(format!(
                "ERROR: {}\n  \
                 Unknown unit '{}' on field '{}'.{}\n\n  \
                 Supported time units: ms, s, sec, seconds, m, min, minutes, h, hr, hours, d, days, w, weeks\n  \
                 Supported size units: b, bytes, kb, mb, gb, tb, kib, mib, gib, tib",
                issue.location, issue.unit, issue.field_name, suggestions
            ));
        }

        for issue in &self.incompatible_unit_families {
            sections.push(format!(
                "ERROR: {}\n  \
                 Field '{}' has unit={} but section has target={}.\n\n  \
                 Rule: Unit must be convertible to section target.\n\n  \
                 {} units ({}) cannot convert to {} units.\n  \
                 Move this field to a section with target={}.",
                issue.location, issue.field_name, issue.unit, issue.target,
                issue.unit_family.to_uppercase(), issue.unit, issue.target_family,
                if issue.unit_family == "size" { "bytes" } else { "seconds" }
            ));
        }

        for issue in &self.undefined_path_references {
            let defined = if issue.defined_refs.is_empty() {
                "none".to_string()
            } else {
                issue.defined_refs.join(", ")
            };

            sections.push(format!(
                "ERROR: {}\n  \
                 Field '{}' uses path={} but '{}' is not defined in section header.\n\n  \
                 Defined references: {}\n  \
                 Built-in references: home, absolute, config\n\n  \
                 Fix: Add to section header: [section]  # target=path, base=..., {}=/path/\n       \
                      Or use a defined reference: # path=base",
                issue.location, issue.field_name, issue.reference, issue.reference,
                defined, issue.reference
            ));
        }

        if sections.is_empty() {
            "No issues found.".to_string()
        } else {
            sections.join("\n\n---\n\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_issues() {
        let issues = TomlxIssues::new();
        assert!(!issues.has_issues());
        assert_eq!(issues.count(), 0);
    }

    #[test]
    fn test_orphan_unit_format() {
        let mut issues = TomlxIssues::with_file("config.tomlx");
        issues.orphan_unit_annotations.push(OrphanUnitAnnotation {
            location: issues.loc(7),
            field_name: "timeout".to_string(),
            unit: "minutes".to_string(),
            section_name: "settings".to_string(),
        });

        assert!(issues.has_issues());
        let formatted = issues.format();
        assert!(formatted.contains("config.tomlx:7"));
        assert!(formatted.contains("timeout"));
        assert!(formatted.contains("Rule:"));
        assert!(formatted.contains("Fix:"));
    }
}
