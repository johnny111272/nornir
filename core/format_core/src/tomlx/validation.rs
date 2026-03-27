//! Zero-ambiguity validation rules for .tomlx.
//!
//! Enforces:
//! 1. Orphan detection (field annotations without section target)
//! 2. Missing annotations (fields in targeted sections without annotations)
//! 3. Base redeclaration prevention

use std::collections::HashMap;

use super::annotation::{
    extract_field_annotations, extract_section_annotations, FieldLineParse, SectionHeaderParse,
};
use super::diagnostics::{
    BaseRedeclaration, MalformedFieldAnnotation, MalformedSectionAnnotation,
    MissingFieldAnnotation, OrphanPathAnnotation, OrphanUnitAnnotation, TomlxIssues,
};
use super::types::{
    FieldAnnotation, FieldAnnotationType, PathRegistry, SectionAnnotation, TargetFamily,
};

/// Validated section with its fields.
#[derive(Debug)]
pub struct ValidatedSection {
    pub annotation: Option<SectionAnnotation>,
    pub fields: Vec<FieldAnnotation>,
    pub unannotated_fields: Vec<(usize, String)>,
}

/// Result of validation pass.
#[derive(Debug)]
pub struct ValidationResult {
    pub sections: HashMap<String, ValidatedSection>,
    pub path_registry: PathRegistry,
    pub issues: TomlxIssues,
}

/// Mutable state accumulated during validation.
struct ValidationState {
    sections: HashMap<String, ValidatedSection>,
    issues: TomlxIssues,
    path_registry: PathRegistry,
}

/// Validate a .tomlx source file.
pub fn validate_tomlx(source: &str, file: Option<&str>) -> ValidationResult {
    let mut state = ValidationState {
        issues: match file {
            Some(f) => TomlxIssues::with_file(f),
            None => TomlxIssues::new(),
        },
        sections: HashMap::new(),
        path_registry: PathRegistry::new(),
    };

    let section_headers = extract_section_annotations(source);

    let section_ranges: Vec<(String, usize, Option<usize>, Option<SectionAnnotation>)> =
        section_headers
            .iter()
            .enumerate()
            .map(|(i, (line, parse))| {
                let end = section_headers.get(i + 1).map(|(l, _)| *l);
                match parse {
                    SectionHeaderParse::NoAnnotation { section_path } => {
                        (section_path.clone(), *line, end, None)
                    }
                    SectionHeaderParse::Annotated(ann) => {
                        (ann.section_path.clone(), *line, end, Some(ann.clone()))
                    }
                    SectionHeaderParse::Error { section_path, raw, reason } => {
                        state.issues.malformed_section_annotations.push(MalformedSectionAnnotation {
                            location: state.issues.loc(*line),
                            section_name: section_path.clone(),
                            raw_annotation: raw.clone(),
                            reason: reason.clone(),
                        });
                        (section_path.clone(), *line, end, None)
                    }
                }
            })
            .collect();

    // Handle fields before first section (root level)
    let first_section_line = section_ranges.first().map(|(_, l, _, _)| *l).unwrap_or(usize::MAX);
    if first_section_line > 1 {
        let root_fields = extract_field_annotations(source, 0, Some(first_section_line), "");
        validate_section_fields("", None, &root_fields, &mut state);
    }

    for (section_path, start, end, annotation) in section_ranges {
        let field_parses = extract_field_annotations(source, start, end, &section_path);
        validate_section_fields(&section_path, annotation, &field_parses, &mut state);
    }

    ValidationResult {
        sections: state.sections,
        path_registry: state.path_registry,
        issues: state.issues,
    }
}

fn report_missing_annotations(
    section_path: &str,
    target: &TargetFamily,
    unannotated: &[(usize, String)],
    issues: &mut TomlxIssues,
) {
    let expected_type = match target {
        TargetFamily::Path => "path",
        _ => "unit",
    };
    for (line, field_name) in unannotated {
        issues.missing_field_annotations.push(MissingFieldAnnotation {
            location: issues.loc(*line),
            field_name: field_name.clone(),
            section_name: section_path.to_string(),
            expected_type: expected_type.to_string(),
        });
    }
}

fn register_path_bases(
    section_path: &str,
    annotation: &mut SectionAnnotation,
    state: &mut ValidationState,
) {
    if state.path_registry.declared_at.is_none() {
        state.path_registry.user_defined = std::mem::take(&mut annotation.path_bases);
        state.path_registry.expand_mode = annotation.expand_mode;
        state.path_registry.declared_at = Some(annotation.line);
    } else {
        for (base_name, _) in &annotation.path_bases {
            if let Some(original_value) = state.path_registry.user_defined.get(base_name) {
                state.issues.base_redeclarations.push(BaseRedeclaration {
                    location: state.issues.loc(annotation.line),
                    section_name: section_path.to_string(),
                    base_name: base_name.clone(),
                    original_line: state.path_registry.declared_at.unwrap_or(0),
                    original_value: original_value.clone(),
                });
            }
        }
    }
}

fn report_orphan_annotations(
    section_path: &str,
    annotated_fields: &[FieldAnnotation],
    issues: &mut TomlxIssues,
) {
    for ann in annotated_fields {
        let field_name = ann.field_path.rsplit('.').next().unwrap_or(&ann.field_path);
        match &ann.annotation_type {
            FieldAnnotationType::Unit(unit) => {
                issues.orphan_unit_annotations.push(OrphanUnitAnnotation {
                    location: issues.loc(ann.line),
                    field_name: field_name.to_string(),
                    unit: unit.clone(),
                    section_name: section_path.to_string(),
                });
            }
            FieldAnnotationType::Path(path_ref) => {
                issues.orphan_path_annotations.push(OrphanPathAnnotation {
                    location: issues.loc(ann.line),
                    field_name: field_name.to_string(),
                    path_ref: path_ref.clone(),
                    section_name: section_path.to_string(),
                });
            }
        }
    }
}

fn validate_section_fields(
    section_path: &str,
    mut annotation: Option<SectionAnnotation>,
    field_parses: &[(usize, FieldLineParse)],
    state: &mut ValidationState,
) {
    let mut annotated_fields: Vec<FieldAnnotation> = Vec::new();
    let mut unannotated_fields: Vec<(usize, String)> = Vec::new();

    for (line, parse) in field_parses {
        match parse {
            FieldLineParse::NotAField => continue,
            FieldLineParse::NoAnnotation { field_name } => {
                unannotated_fields.push((*line, field_name.clone()));
            }
            FieldLineParse::Annotated(ann) => {
                annotated_fields.push(ann.clone());
            }
            FieldLineParse::Error { field_name, raw, reason } => {
                state.issues.malformed_field_annotations.push(MalformedFieldAnnotation {
                    location: state.issues.loc(*line),
                    field_name: field_name.clone(),
                    raw_annotation: raw.clone(),
                    reason: reason.clone(),
                });
                unannotated_fields.push((*line, field_name.clone()));
            }
        }
    }

    match &mut annotation {
        Some(ann) => {
            if let Some(target) = &ann.target {
                report_missing_annotations(section_path, target, &unannotated_fields, &mut state.issues);
            }
            if matches!(ann.target, Some(TargetFamily::Path)) {
                register_path_bases(section_path, ann, state);
            }
        }
        None => {
            report_orphan_annotations(section_path, &annotated_fields, &mut state.issues);
        }
    }

    state.sections.insert(
        section_path.to_string(),
        ValidatedSection { annotation, fields: annotated_fields, unannotated_fields },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_unit_section() {
        let source = "\n[ttl]  # target=seconds\nsession = 15  # unit=minutes\ncache = 4     # unit=hours\n";
        let result = validate_tomlx(source, None);
        assert!(!result.issues.has_issues());
    }

    #[test]
    fn test_orphan_unit() {
        let source = "\n[settings]\ntimeout = 30  # unit=seconds\n";
        let result = validate_tomlx(source, None);
        assert!(result.issues.has_issues());
        assert_eq!(result.issues.orphan_unit_annotations.len(), 1);
    }

    #[test]
    fn test_missing_annotation() {
        let source = "\n[ttl]  # target=seconds\nsession = 15  # unit=minutes\nretry_count = 3\n";
        let result = validate_tomlx(source, None);
        assert!(result.issues.has_issues());
        assert_eq!(result.issues.missing_field_annotations.len(), 1);
    }

    #[test]
    fn test_base_redeclaration() {
        let source = "\n[paths]  # target=path, base=~/.ai/\ncli = \"cli/\"  # path=base\n\n[logging]  # target=path, base=/var/log/\napp = \"app.log\"  # path=base\n";
        let result = validate_tomlx(source, None);
        assert!(result.issues.has_issues());
        assert_eq!(result.issues.base_redeclarations.len(), 1);
    }

    #[test]
    fn test_subsequent_path_inherits() {
        let source = "\n[paths]  # target=path, base=~/.ai/\ncli = \"cli/\"  # path=base\n\n[logging]  # target=path\napp = \"app.log\"  # path=base\n";
        let result = validate_tomlx(source, None);
        assert!(!result.issues.has_issues());
    }
}
