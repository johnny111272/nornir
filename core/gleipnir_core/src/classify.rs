//! File classification by content and path.
//!
//! Every file is classified exactly once before any checks run.
//! The classification determines which checks apply (via the matrix).

use crate::structures::{FileKind, Level, V2Classification, Zone};

/// Classify a Python source file into one of 8 kinds.
///
/// Decision tree (order matters):
/// 1. PEP 723 shebang → Script
/// 2. "test" in filename → Test
/// 3. Zone path → DataStructure / UnsafeImpure / UnsafePure / ImpureFunction / PureFunction
/// 4. Fallback → Outside
pub fn classify_file(file_path: &str, first_line: &str) -> FileKind {
    if first_line.starts_with("#!/usr/bin/env -S uv run") {
        return FileKind::Script;
    }

    let filename = file_path.rsplit('/').next().unwrap_or(file_path);
    if filename.contains("test") {
        return FileKind::Test;
    }

    if file_path.contains("/structures/") {
        return FileKind::DataStructure;
    }
    if file_path.contains("/unsafe/impure/") {
        return FileKind::UnsafeImpure;
    }
    if file_path.contains("/unsafe/pure/") {
        return FileKind::UnsafePure;
    }
    if file_path.contains("/functions/impure/") {
        return FileKind::ImpureFunction;
    }
    if file_path.contains("/functions/pure/") {
        return FileKind::PureFunction;
    }

    FileKind::Outside
}

// -------------------------------------------------------------------------
// V2 zone architecture classification
// -------------------------------------------------------------------------

/// Zone markers for v2 classification.
/// Maps a path component to its zone.
const V2_ZONE_MARKERS: &[(&str, Zone)] = &[
    ("/logic/pure/", Zone::Pure),
    ("/logic/impure/", Zone::Impure),
    ("/logic/transform/", Zone::Transform),
    ("/logic/orchestrate/", Zone::Orchestrate),
    ("/structure/", Zone::Structure),
    ("/structures/", Zone::Structure),
];

/// Level from filename (stem without .py).
/// Layout: logic/{zone}/{module_name}/{level}.py
fn level_from_filename(file_path: &str) -> Level {
    let filename = file_path.rsplit('/').next().unwrap_or("");
    let stem = filename.strip_suffix(".py").unwrap_or(filename);
    match stem {
        "ffi" => Level::Ffi,
        "primitive" => Level::Primitive,
        "simple" => Level::Simple,
        "dispatch" => Level::Dispatch,
        "composed" => Level::Composed,
        "assembled" => Level::Assembled,
        "orchestrate" => Level::Orchestrate,
        _ => Level::Outside,
    }
}

/// Path markers kept for import path classification.
/// Import paths use the directory-as-level convention:
///   mypackage.logic.pure.composed.module
const V2_IMPORT_PATH_MARKERS: &[(&str, Level, Zone)] = &[
    // Pure track
    ("/logic/pure/ffi/", Level::Ffi, Zone::Pure),
    ("/logic/pure/primitive/", Level::Primitive, Zone::Pure),
    ("/logic/pure/simple/", Level::Simple, Zone::Pure),
    ("/logic/pure/dispatch/", Level::Dispatch, Zone::Pure),
    ("/logic/pure/composed/", Level::Composed, Zone::Pure),
    // Impure track
    ("/logic/impure/ffi/", Level::Ffi, Zone::Impure),
    ("/logic/impure/primitive/", Level::Primitive, Zone::Impure),
    ("/logic/impure/simple/", Level::Simple, Zone::Impure),
    ("/logic/impure/dispatch/", Level::Dispatch, Zone::Impure),
    ("/logic/impure/composed/", Level::Composed, Zone::Impure),
    // Transform track
    ("/logic/transform/ffi/", Level::Ffi, Zone::Transform),
    ("/logic/transform/primitive/", Level::Primitive, Zone::Transform),
    ("/logic/transform/simple/", Level::Simple, Zone::Transform),
    ("/logic/transform/dispatch/", Level::Dispatch, Zone::Transform),
    ("/logic/transform/composed/", Level::Composed, Zone::Transform),
    // Orchestrate
    ("/logic/orchestrate/", Level::Orchestrate, Zone::Orchestrate),
    // Structure (both singular and plural forms)
    ("/structure/", Level::Structure, Zone::Structure),
    ("/structures/", Level::Structure, Zone::Structure),
];

/// Entry point filenames at project root level.
const ENTRY_POINT_NAMES: &[&str] = &["cli.py", "__main__.py"];

/// Classify a Python source file into the v2 zone architecture.
///
/// Two-step classification:
/// 1. Zone from path directory (/logic/pure/, /structure/, etc.)
/// 2. Level from filename (composed.py, simple.py, etc.)
///
/// Special cases:
/// - Structure zone: level is always Structure (no per-file level)
/// - Orchestrate zone: only orchestrate.py and dispatch.py are valid levels
/// - Pure/Impure/Transform zones: ffi, primitive, simple, dispatch, composed, assembled
/// - Entry point files (cli.py, __main__.py): EntryPoint + Orchestrate
/// - __init__.py: inherits zone, level is Outside (no checks needed)
/// - Invalid level for zone (e.g. orchestrate.py in pure/): forced to Outside
///
/// Files matching no zone get Level::Outside — a sentinel that fails
/// all import checks. In a v2 project, every file must be in a zone.
pub fn classify_file_v2(file_path: &str) -> V2Classification {
    let filename = file_path.rsplit('/').next().unwrap_or(file_path);

    // Entry points detected by filename
    if ENTRY_POINT_NAMES.contains(&filename) {
        return V2Classification {
            level: Level::EntryPoint,
            zone: Zone::Orchestrate,
        };
    }

    // Find zone from path
    let zone = V2_ZONE_MARKERS.iter().find_map(|&(pattern, zone)| {
        if file_path.contains(pattern) { Some(zone) } else { None }
    });

    let Some(zone) = zone else {
        return V2Classification {
            level: Level::Outside,
            zone: Zone::Structure,
        };
    };

    // Structure zone has a fixed level (no per-file hierarchy)
    if zone == Zone::Structure {
        return V2Classification { level: Level::Structure, zone };
    }

    // __init__.py files inherit zone but have no level checks
    if filename == "__init__.py" {
        return V2Classification { level: Level::Outside, zone };
    }

    // Level from filename
    let level = level_from_filename(file_path);

    // Validate level is legal for this zone
    let valid = match zone {
        Zone::Pure | Zone::Impure | Zone::Transform => matches!(
            level,
            Level::Ffi | Level::Primitive | Level::Simple | Level::Dispatch
            | Level::Composed | Level::Assembled | Level::Outside
        ),
        Zone::Orchestrate => matches!(
            level,
            Level::Orchestrate | Level::Dispatch | Level::Outside
        ),
        Zone::Structure => true, // handled above
    };
    let level = if valid { level } else { Level::Outside };

    V2Classification { level, zone }
}

/// Classify a dotted import path (e.g. "regin.logic.pure.codegen_transforms.composed")
/// into v2 zone/level. Returns None if the path contains no v2 markers.
///
/// Two strategies:
/// 1. Level from last segment: "regin.logic.pure.module.composed" → Composed + Pure
/// 2. Level from directory position: "regin.logic.pure.composed.module" → Composed + Pure
/// Strategy 1 matches the file layout (level as filename).
/// Strategy 2 is kept for backward compatibility.
pub fn classify_import_path(dotted_path: &str) -> Option<V2Classification> {
    let slashed = dotted_path.replace('.', "/");
    let probe = format!("/{slashed}/");

    // Find zone first
    let zone = V2_ZONE_MARKERS.iter().find_map(|&(pattern, zone)| {
        if probe.contains(pattern) { Some(zone) } else { None }
    })?;

    // Structure zone has a fixed level
    if zone == Zone::Structure {
        return Some(V2Classification { level: Level::Structure, zone });
    }

    // Try level from last segment (filename convention)
    let last_segment = dotted_path.rsplit('.').next().unwrap_or("");
    let level_from_last = match last_segment {
        "ffi" => Some(Level::Ffi),
        "primitive" => Some(Level::Primitive),
        "simple" => Some(Level::Simple),
        "dispatch" => Some(Level::Dispatch),
        "composed" => Some(Level::Composed),
        "assembled" => Some(Level::Assembled),
        "orchestrate" => Some(Level::Orchestrate),
        _ => None,
    };
    if let Some(level) = level_from_last {
        return Some(V2Classification { level, zone });
    }

    // Try level from directory position (backward compat)
    for &(pattern, level, pzone) in V2_IMPORT_PATH_MARKERS {
        if probe.contains(pattern) && pzone == zone {
            return Some(V2Classification { level, zone });
        }
    }

    // In zone but level unknown — caller decides significance
    Some(V2Classification { level: Level::Outside, zone })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_detected_by_shebang() {
        assert_eq!(
            classify_file("/project/tools/run.py", "#!/usr/bin/env -S uv run"),
            FileKind::Script,
        );
    }

    #[test]
    fn shebang_takes_priority_over_path() {
        assert_eq!(
            classify_file(
                "/project/src/pkg/functions/pure/tool.py",
                "#!/usr/bin/env -S uv run",
            ),
            FileKind::Script,
        );
    }

    #[test]
    fn test_file_detected_by_name() {
        assert_eq!(
            classify_file("/project/tests/test_foo.py", "import pytest"),
            FileKind::Test,
        );
    }

    #[test]
    fn test_in_name_takes_priority_over_path() {
        assert_eq!(
            classify_file(
                "/project/src/pkg/structures/test_models.py",
                "from pydantic import BaseModel",
            ),
            FileKind::Test,
        );
    }

    #[test]
    fn structures_path() {
        assert_eq!(
            classify_file("/project/src/pkg/structures/models.py", "from pydantic import BaseModel"),
            FileKind::DataStructure,
        );
    }

    #[test]
    fn unsafe_impure_path() {
        assert_eq!(
            classify_file("/project/src/pkg/unsafe/impure/io.py", "import os"),
            FileKind::UnsafeImpure,
        );
    }

    #[test]
    fn unsafe_pure_path() {
        assert_eq!(
            classify_file("/project/src/pkg/unsafe/pure/cast.py", "from typing import cast"),
            FileKind::UnsafePure,
        );
    }

    #[test]
    fn functions_impure_path() {
        assert_eq!(
            classify_file("/project/src/pkg/functions/impure/loader.py", "import json"),
            FileKind::ImpureFunction,
        );
    }

    #[test]
    fn functions_pure_path() {
        assert_eq!(
            classify_file("/project/src/pkg/functions/pure/compute.py", "def add(a, b):"),
            FileKind::PureFunction,
        );
    }

    #[test]
    fn outside_file() {
        assert_eq!(
            classify_file("/project/src/app.py", "import sys"),
            FileKind::Outside,
        );
    }

    // -- V2 classification --
    // Layout: logic/{zone}/{module_name}/{level}.py
    // Zone from path directory, level from filename.

    #[test]
    fn v2_pure_primitive() {
        let c = classify_file_v2("/project/src/pkg/logic/pure/helpers/primitive.py");
        assert_eq!(c.level, Level::Primitive);
        assert_eq!(c.zone, Zone::Pure);
    }

    #[test]
    fn v2_pure_simple() {
        let c = classify_file_v2("/project/src/pkg/logic/pure/validators/simple.py");
        assert_eq!(c.level, Level::Simple);
        assert_eq!(c.zone, Zone::Pure);
    }

    #[test]
    fn v2_pure_composed() {
        let c = classify_file_v2("/project/src/pkg/logic/pure/pipeline/composed.py");
        assert_eq!(c.level, Level::Composed);
        assert_eq!(c.zone, Zone::Pure);
    }

    #[test]
    fn v2_pure_ffi() {
        let c = classify_file_v2("/project/src/pkg/logic/pure/rust_binding/ffi.py");
        assert_eq!(c.level, Level::Ffi);
        assert_eq!(c.zone, Zone::Pure);
    }

    #[test]
    fn v2_impure_primitive() {
        let c = classify_file_v2("/project/src/pkg/logic/impure/io_ops/primitive.py");
        assert_eq!(c.level, Level::Primitive);
        assert_eq!(c.zone, Zone::Impure);
    }

    #[test]
    fn v2_impure_composed() {
        let c = classify_file_v2("/project/src/pkg/logic/impure/workflow/composed.py");
        assert_eq!(c.level, Level::Composed);
        assert_eq!(c.zone, Zone::Impure);
    }

    #[test]
    fn v2_transform_simple() {
        let c = classify_file_v2("/project/src/pkg/logic/transform/coerce/simple.py");
        assert_eq!(c.level, Level::Simple);
        assert_eq!(c.zone, Zone::Transform);
    }

    #[test]
    fn v2_transform_primitive() {
        let c = classify_file_v2("/project/src/pkg/logic/transform/reshape/primitive.py");
        assert_eq!(c.level, Level::Primitive);
        assert_eq!(c.zone, Zone::Transform);
    }

    #[test]
    fn v2_pure_dispatch() {
        let c = classify_file_v2("/project/src/pkg/logic/pure/converters/dispatch.py");
        assert_eq!(c.level, Level::Dispatch);
        assert_eq!(c.zone, Zone::Pure);
    }

    #[test]
    fn v2_transform_dispatch() {
        let c = classify_file_v2("/project/src/pkg/logic/transform/field_map/dispatch.py");
        assert_eq!(c.level, Level::Dispatch);
        assert_eq!(c.zone, Zone::Transform);
    }

    #[test]
    fn v2_orchestrate() {
        let c = classify_file_v2("/project/src/pkg/logic/orchestrate/main_pipeline/orchestrate.py");
        assert_eq!(c.level, Level::Orchestrate);
        assert_eq!(c.zone, Zone::Orchestrate);
    }

    #[test]
    fn v2_orchestrate_dispatch() {
        let c = classify_file_v2("/project/src/pkg/logic/orchestrate/main_pipeline/dispatch.py");
        assert_eq!(c.level, Level::Dispatch);
        assert_eq!(c.zone, Zone::Orchestrate);
    }

    #[test]
    fn v2_orchestrate_py_in_pure_rejected() {
        let c = classify_file_v2("/project/src/pkg/logic/pure/module/orchestrate.py");
        assert_eq!(c.level, Level::Outside); // orchestrate.py invalid in pure zone
        assert_eq!(c.zone, Zone::Pure);
    }

    #[test]
    fn v2_primitive_in_orchestrate_rejected() {
        let c = classify_file_v2("/project/src/pkg/logic/orchestrate/module/primitive.py");
        assert_eq!(c.level, Level::Outside); // primitive.py invalid in orchestrate zone
        assert_eq!(c.zone, Zone::Orchestrate);
    }

    #[test]
    fn v2_assembled_in_orchestrate_rejected() {
        let c = classify_file_v2("/project/src/pkg/logic/orchestrate/module/assembled.py");
        assert_eq!(c.level, Level::Outside); // assembled.py invalid in orchestrate zone
        assert_eq!(c.zone, Zone::Orchestrate);
    }

    #[test]
    fn v2_composed_in_orchestrate_rejected() {
        let c = classify_file_v2("/project/src/pkg/logic/orchestrate/module/composed.py");
        assert_eq!(c.level, Level::Outside); // composed.py invalid in orchestrate zone
        assert_eq!(c.zone, Zone::Orchestrate);
    }

    #[test]
    fn v2_init_inherits_zone() {
        let c = classify_file_v2("/project/src/pkg/logic/pure/helpers/__init__.py");
        assert_eq!(c.zone, Zone::Pure);
        // __init__.py gets Outside level — no checks applied
        assert_eq!(c.level, Level::Outside);
    }

    #[test]
    fn v2_structure_singular() {
        let c = classify_file_v2("/project/src/pkg/structure/models.py");
        assert_eq!(c.level, Level::Structure);
        assert_eq!(c.zone, Zone::Structure);
    }

    #[test]
    fn v2_structure_plural() {
        let c = classify_file_v2("/project/src/pkg/structures/schema.py");
        assert_eq!(c.level, Level::Structure);
        assert_eq!(c.zone, Zone::Structure);
    }

    #[test]
    fn v2_entry_point_cli() {
        let c = classify_file_v2("/project/src/pkg/cli.py");
        assert_eq!(c.level, Level::EntryPoint);
        assert_eq!(c.zone, Zone::Orchestrate);
    }

    #[test]
    fn v2_entry_point_main() {
        let c = classify_file_v2("/project/src/pkg/__main__.py");
        assert_eq!(c.level, Level::EntryPoint);
        assert_eq!(c.zone, Zone::Orchestrate);
    }

    #[test]
    fn v2_outside_file() {
        let c = classify_file_v2("/project/src/pkg/random_file.py");
        assert_eq!(c.level, Level::Outside);
    }

    // -- Import path classification --

    // Import path: level from last segment (filename convention)
    #[test]
    fn import_path_pure_composed_from_filename() {
        // regin.logic.pure.codegen_transforms.composed → Composed + Pure
        let c = classify_import_path("regin.logic.pure.codegen_transforms.composed").unwrap();
        assert_eq!(c.level, Level::Composed);
        assert_eq!(c.zone, Zone::Pure);
    }

    #[test]
    fn import_path_impure_primitive_from_filename() {
        let c = classify_import_path("regin.logic.impure.gates.primitive").unwrap();
        assert_eq!(c.level, Level::Primitive);
        assert_eq!(c.zone, Zone::Impure);
    }

    #[test]
    fn import_path_transform_simple_from_filename() {
        let c = classify_import_path("regin.logic.transform.coerce.simple").unwrap();
        assert_eq!(c.level, Level::Simple);
        assert_eq!(c.zone, Zone::Transform);
    }

    // Import path: level from directory position (backward compat)
    #[test]
    fn import_path_pure_simple_from_directory() {
        let c = classify_import_path("mypackage.logic.pure.simple.helpers").unwrap();
        assert_eq!(c.level, Level::Simple);
        assert_eq!(c.zone, Zone::Pure);
    }

    #[test]
    fn import_path_impure_primitive_from_directory() {
        let c = classify_import_path("mypackage.logic.impure.primitive.io").unwrap();
        assert_eq!(c.level, Level::Primitive);
        assert_eq!(c.zone, Zone::Impure);
    }

    #[test]
    fn import_path_structures() {
        let c = classify_import_path("mypackage.structures.models").unwrap();
        assert_eq!(c.level, Level::Structure);
        assert_eq!(c.zone, Zone::Structure);
    }

    #[test]
    fn import_path_structure_singular() {
        let c = classify_import_path("regin.structure.gen.schema.agent_raw_definition").unwrap();
        assert_eq!(c.level, Level::Structure);
        assert_eq!(c.zone, Zone::Structure);
    }

    #[test]
    fn import_path_external_returns_none() {
        assert!(classify_import_path("pydantic.BaseModel").is_none());
        assert!(classify_import_path("os.path").is_none());
    }
}
