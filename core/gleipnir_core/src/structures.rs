//! Core data structures for gleipnir guardrail checks.

/// Classification of a Python source file by content and path (v1).
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

// -------------------------------------------------------------------------
// V2 zone architecture types
// -------------------------------------------------------------------------

/// Composition level in the v2 zone architecture.
///
/// Determines which other levels a file may import from.
/// Ffi and Primitive are peers — neither imports the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Structure,
    Ffi,
    Primitive,
    Simple,
    Dispatch,
    Composed,
    Assembled,
    Orchestrate,
    EntryPoint,
    Outside,
}

impl Level {
    /// Check whether a file at this level may import from `target` level.
    ///
    /// Encodes the level matrix from V2_ZONE_ARCHITECTURE.md.
    /// Structure zone has no level hierarchy — structure files freely import each other.
    /// Ffi and Primitive are peers — neither imports the other.
    /// Same-level imports are forbidden in logic zones (not structure).
    pub fn can_import(self, target: Level) -> bool {
        use Level::*;
        // Structure zone: no hierarchy, free to import from each other
        if self == Structure && target == Structure {
            return true;
        }
        if self == target {
            return false;
        }
        match self {
            Structure => false,
            Ffi => matches!(target, Structure),
            Primitive => matches!(target, Structure),
            Simple => matches!(target, Primitive | Ffi | Structure),
            Dispatch => matches!(target, Simple | Primitive | Ffi | Structure),
            Composed => matches!(target, Dispatch | Simple | Primitive | Ffi | Structure),
            Assembled => matches!(target, Composed | Dispatch | Simple | Primitive | Ffi | Structure),
            Orchestrate => matches!(target, Assembled | Composed | Dispatch | Simple | Primitive | Ffi | Structure),
            EntryPoint => matches!(target, Orchestrate | Structure),
            Outside => false,
        }
    }
}

/// Zone track in the v2 zone architecture.
///
/// Determines which other zones a file may import from.
/// Structure is reachable from all zones via the level matrix, not the zone matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Structure,
    Pure,
    Impure,
    Transform,
    Orchestrate,
}

impl Zone {
    /// Check whether a file in this zone may import from `target` zone.
    ///
    /// Encodes the zone matrix from V2_ZONE_ARCHITECTURE.md.
    /// Structure reachability is handled by Level::can_import, not here.
    pub fn can_reach(self, target: Zone) -> bool {
        use Zone::*;
        match self {
            Structure => false,
            Pure => matches!(target, Pure),
            Impure => matches!(target, Impure | Pure),
            Transform => matches!(target, Transform),
            Orchestrate => matches!(target, Orchestrate | Pure | Impure | Transform),
        }
    }
}

/// V2 file classification: zone + level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct V2Classification {
    pub level: Level,
    pub zone: Zone,
}

impl V2Classification {
    /// Check whether a file with this classification may import from a target.
    ///
    /// Both the level check and the zone check must pass.
    /// Structure targets are checked via level only (zone matrix skipped).
    pub fn can_import(self, target: V2Classification) -> bool {
        if !self.level.can_import(target.level) {
            return false;
        }
        if target.zone == Zone::Structure {
            return true;
        }
        self.zone.can_reach(target.zone)
    }
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

/// Per-file check configuration. Values loaded from gleipnir_statistics.toml.
#[derive(Debug, Clone)]
pub struct CheckConfig {
    pub max_function_lines: usize,
    pub max_function_params: usize,
    pub max_nesting_depth: usize,
    pub max_functions_outside_zones: usize,
    pub max_simple_union_members: usize,
    pub max_named_union_members: usize,
    pub min_param_length: usize,
    pub max_function_imports: usize,
    pub max_other_imports: usize,
    pub min_cc: usize,
    pub max_cc: usize,
    pub min_functions_for_pub_check: usize,
}

impl CheckConfig {
    /// Build config for a v1 Python FileKind from parsed statistics.
    pub fn for_kind(kind: FileKind, stats: &StatisticsToml) -> Self {
        let key = match kind {
            FileKind::PureFunction => Some("pure_function"),
            FileKind::ImpureFunction => Some("impure_function"),
            FileKind::UnsafePure => Some("unsafe_pure"),
            FileKind::UnsafeImpure => Some("unsafe_impure"),
            FileKind::DataStructure => Some("data_structure"),
            FileKind::Script => Some("script"),
            FileKind::Test => Some("test"),
            FileKind::Outside => Some("outside"),
        };
        let override_values = key.and_then(|k| {
            stats.v1.as_ref().and_then(|m| m.get(k))
        });
        Self::resolve(override_values, &stats.defaults)
    }

    /// Build config for a v2 level from parsed statistics.
    /// Zone is used to apply zone-specific overrides (e.g. transform has tighter CC/nesting).
    pub fn for_v2(level: Level, zone: Zone, stats: &StatisticsToml) -> Self {
        let key = match level {
            Level::Ffi => Some("ffi"),
            Level::Primitive => Some("primitive"),
            Level::Simple => Some("simple"),
            Level::Dispatch => Some("dispatch"),
            Level::Composed => Some("composed"),
            Level::Assembled => Some("assembled"),
            Level::Orchestrate => Some("orchestrate"),
            Level::EntryPoint | Level::Outside => Some("entry_point"),
            Level::Structure => None,
        };
        let v2_entry = key.and_then(|k| {
            stats.v2.as_ref().and_then(|m| m.get(k))
        });
        let override_values = v2_entry.map(|e| &e.values);
        let mut config = Self::resolve(override_values, &stats.defaults);
        if let Some(entry) = v2_entry {
            config.min_cc = entry.min_cc.unwrap_or(0);
            config.max_cc = entry.max_cc.unwrap_or(usize::MAX);
        }
        // Apply zone-specific overrides (e.g. [v2.transform.simple])
        if let Some(level_key) = key {
            if let Some(zone_entry) = stats.v2_zone_override(zone, level_key) {
                if let Some(cc) = zone_entry.max_cc {
                    config.max_cc = cc;
                }
                if let Some(cc) = zone_entry.min_cc {
                    config.min_cc = cc;
                }
                if let Some(nd) = zone_entry.values.max_nesting_depth {
                    config.max_nesting_depth = nd;
                }
            }
        }
        config
    }

    /// Build config for Rust files from parsed statistics.
    pub fn for_rust(stats: &StatisticsToml) -> Self {
        Self::resolve(stats.rust.as_ref(), &stats.defaults)
    }

    /// Build config for TypeScript/Svelte files from parsed statistics.
    pub fn for_typescript(stats: &StatisticsToml) -> Self {
        Self::resolve(stats.typescript.as_ref(), &stats.defaults)
    }

    /// Resolve a config by overlaying specific values onto defaults.
    fn resolve(specific: Option<&StatisticValues>, defaults: &StatisticValues) -> Self {
        let get = |f: fn(&StatisticValues) -> Option<usize>| -> usize {
            specific.and_then(|s| f(s)).or_else(|| f(defaults)).unwrap_or(0)
        };
        Self {
            max_function_lines: get(|v| v.max_function_lines),
            max_function_params: get(|v| v.max_function_params),
            max_nesting_depth: get(|v| v.max_nesting_depth),
            max_functions_outside_zones: get(|v| v.max_functions_outside_zones),
            max_simple_union_members: get(|v| v.max_simple_union_members),
            max_named_union_members: get(|v| v.max_named_union_members),
            min_param_length: get(|v| v.min_param_length),
            max_function_imports: get(|v| v.max_function_imports),
            max_other_imports: get(|v| v.max_other_imports),
            min_cc: 0,
            max_cc: usize::MAX,
            min_functions_for_pub_check: get(|v| v.min_functions_for_pub_check),
        }
    }
}

// -------------------------------------------------------------------------
// Statistics loaded from embedded TOML
// -------------------------------------------------------------------------

/// Top-level structure of gleipnir_statistics.toml.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct StatisticsToml {
    pub defaults: StatisticValues,
    #[serde(default)]
    pub rust: Option<StatisticValues>,
    #[serde(default)]
    pub typescript: Option<StatisticValues>,
    #[serde(default)]
    pub v1: Option<std::collections::HashMap<String, StatisticValues>>,
    #[serde(default)]
    pub v2: Option<std::collections::HashMap<String, V2LevelEntry>>,
}

impl StatisticsToml {
    /// Look up a zone-specific override for a v2 level.
    /// Returns the zone override entry if `[v2.{zone}.{level}]` exists.
    pub fn v2_zone_override(&self, zone: Zone, level_key: &str) -> Option<&V2LevelEntry> {
        let zone_key = match zone {
            Zone::Transform => "transform",
            _ => return None, // Only transform has overrides for now
        };
        let compound_key = format!("{zone_key}.{level_key}");
        self.v2.as_ref().and_then(|m| m.get(&compound_key))
    }
}

/// Numeric parameters. All fields optional — absence means "inherit from defaults".
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct StatisticValues {
    pub max_function_lines: Option<usize>,
    pub max_function_params: Option<usize>,
    pub max_nesting_depth: Option<usize>,
    pub min_param_length: Option<usize>,
    pub max_simple_union_members: Option<usize>,
    pub max_named_union_members: Option<usize>,
    pub max_functions_outside_zones: Option<usize>,
    pub max_function_imports: Option<usize>,
    pub max_other_imports: Option<usize>,
    pub min_functions_for_pub_check: Option<usize>,
}

/// V2 level entry: standard values plus CC bounds.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct V2LevelEntry {
    pub min_cc: Option<usize>,
    pub max_cc: Option<usize>,
    #[serde(flatten)]
    pub values: StatisticValues,
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

#[cfg(test)]
mod tests {
    use super::*;

    // -- Level matrix truth table --

    #[test]
    fn structure_imports_structure_only() {
        assert!(Level::Structure.can_import(Level::Structure), "Structure should import Structure");
        for target in [Level::Ffi, Level::Primitive, Level::Simple, Level::Dispatch, Level::Composed, Level::Assembled, Level::Orchestrate, Level::EntryPoint] {
            assert!(!Level::Structure.can_import(target), "Structure should not import {target:?}");
        }
    }

    #[test]
    fn ffi_imports_only_structure() {
        assert!(Level::Ffi.can_import(Level::Structure));
        assert!(!Level::Ffi.can_import(Level::Ffi));
        assert!(!Level::Ffi.can_import(Level::Primitive));
        assert!(!Level::Ffi.can_import(Level::Simple));
    }

    #[test]
    fn primitive_imports_only_structure() {
        assert!(Level::Primitive.can_import(Level::Structure));
        assert!(!Level::Primitive.can_import(Level::Primitive));
        assert!(!Level::Primitive.can_import(Level::Ffi));
        assert!(!Level::Primitive.can_import(Level::Simple));
    }

    #[test]
    fn ffi_and_primitive_are_peers() {
        assert!(!Level::Ffi.can_import(Level::Primitive));
        assert!(!Level::Primitive.can_import(Level::Ffi));
    }

    #[test]
    fn simple_imports_primitive_ffi_structure() {
        assert!(Level::Simple.can_import(Level::Primitive));
        assert!(Level::Simple.can_import(Level::Ffi));
        assert!(Level::Simple.can_import(Level::Structure));
        assert!(!Level::Simple.can_import(Level::Simple));
        assert!(!Level::Simple.can_import(Level::Composed));
        assert!(!Level::Simple.can_import(Level::Orchestrate));
    }

    #[test]
    fn dispatch_imports_simple_primitive_ffi_structure() {
        assert!(Level::Dispatch.can_import(Level::Simple));
        assert!(Level::Dispatch.can_import(Level::Primitive));
        assert!(Level::Dispatch.can_import(Level::Ffi));
        assert!(Level::Dispatch.can_import(Level::Structure));
        assert!(!Level::Dispatch.can_import(Level::Dispatch));
        assert!(!Level::Dispatch.can_import(Level::Composed));
        assert!(!Level::Dispatch.can_import(Level::Orchestrate));
    }

    #[test]
    fn composed_imports_dispatch_simple_primitive_ffi_structure() {
        assert!(Level::Composed.can_import(Level::Dispatch));
        assert!(Level::Composed.can_import(Level::Simple));
        assert!(Level::Composed.can_import(Level::Primitive));
        assert!(Level::Composed.can_import(Level::Ffi));
        assert!(Level::Composed.can_import(Level::Structure));
        assert!(!Level::Composed.can_import(Level::Composed));
        assert!(!Level::Composed.can_import(Level::Assembled));
        assert!(!Level::Composed.can_import(Level::Orchestrate));
    }

    #[test]
    fn assembled_imports_composed_and_below() {
        assert!(Level::Assembled.can_import(Level::Composed));
        assert!(Level::Assembled.can_import(Level::Dispatch));
        assert!(Level::Assembled.can_import(Level::Simple));
        assert!(Level::Assembled.can_import(Level::Primitive));
        assert!(Level::Assembled.can_import(Level::Ffi));
        assert!(Level::Assembled.can_import(Level::Structure));
        assert!(!Level::Assembled.can_import(Level::Assembled));
        assert!(!Level::Assembled.can_import(Level::Orchestrate));
        assert!(!Level::Assembled.can_import(Level::EntryPoint));
    }

    #[test]
    fn orchestrate_imports_any_lower_level() {
        assert!(Level::Orchestrate.can_import(Level::Assembled));
        assert!(Level::Orchestrate.can_import(Level::Composed));
        assert!(Level::Orchestrate.can_import(Level::Dispatch));
        assert!(Level::Orchestrate.can_import(Level::Simple));
        assert!(Level::Orchestrate.can_import(Level::Primitive));
        assert!(Level::Orchestrate.can_import(Level::Ffi));
        assert!(Level::Orchestrate.can_import(Level::Structure));
        assert!(!Level::Orchestrate.can_import(Level::Orchestrate));
        assert!(!Level::Orchestrate.can_import(Level::EntryPoint));
    }

    #[test]
    fn entry_point_imports_orchestrate_structure_only() {
        assert!(Level::EntryPoint.can_import(Level::Orchestrate));
        assert!(Level::EntryPoint.can_import(Level::Structure));
        assert!(!Level::EntryPoint.can_import(Level::Composed));
        assert!(!Level::EntryPoint.can_import(Level::Simple));
        assert!(!Level::EntryPoint.can_import(Level::EntryPoint));
    }

    #[test]
    fn outside_imports_nothing() {
        for target in [Level::Structure, Level::Ffi, Level::Primitive, Level::Simple, Level::Dispatch, Level::Composed, Level::Assembled, Level::Orchestrate, Level::EntryPoint, Level::Outside] {
            assert!(!Level::Outside.can_import(target), "Outside should not import {target:?}");
        }
    }

    // -- Zone matrix truth table --

    #[test]
    fn pure_reaches_pure_only() {
        assert!(Zone::Pure.can_reach(Zone::Pure));
        assert!(!Zone::Pure.can_reach(Zone::Impure));
        assert!(!Zone::Pure.can_reach(Zone::Transform));
        assert!(!Zone::Pure.can_reach(Zone::Orchestrate));
    }

    #[test]
    fn impure_reaches_impure_and_pure() {
        assert!(Zone::Impure.can_reach(Zone::Impure));
        assert!(Zone::Impure.can_reach(Zone::Pure));
        assert!(!Zone::Impure.can_reach(Zone::Transform));
        assert!(!Zone::Impure.can_reach(Zone::Orchestrate));
    }

    #[test]
    fn transform_reaches_transform_only() {
        assert!(Zone::Transform.can_reach(Zone::Transform));
        assert!(!Zone::Transform.can_reach(Zone::Pure));
        assert!(!Zone::Transform.can_reach(Zone::Impure));
        assert!(!Zone::Transform.can_reach(Zone::Orchestrate));
    }

    #[test]
    fn orchestrate_reaches_self_pure_impure_transform() {
        assert!(Zone::Orchestrate.can_reach(Zone::Orchestrate));
        assert!(Zone::Orchestrate.can_reach(Zone::Pure));
        assert!(Zone::Orchestrate.can_reach(Zone::Impure));
        assert!(Zone::Orchestrate.can_reach(Zone::Transform));
    }

    #[test]
    fn structure_zone_reaches_nothing() {
        assert!(!Zone::Structure.can_reach(Zone::Pure));
        assert!(!Zone::Structure.can_reach(Zone::Impure));
        assert!(!Zone::Structure.can_reach(Zone::Transform));
        assert!(!Zone::Structure.can_reach(Zone::Structure));
    }

    // -- V2Classification combined check --

    #[test]
    fn cross_track_same_level_blocked() {
        let impure_simple = V2Classification { level: Level::Simple, zone: Zone::Impure };
        let pure_simple = V2Classification { level: Level::Simple, zone: Zone::Pure };
        assert!(!impure_simple.can_import(pure_simple), "same-level cross-track blocked");
    }

    #[test]
    fn cross_track_lower_level_allowed() {
        let impure_simple = V2Classification { level: Level::Simple, zone: Zone::Impure };
        let pure_primitive = V2Classification { level: Level::Primitive, zone: Zone::Pure };
        assert!(impure_simple.can_import(pure_primitive));
    }

    #[test]
    fn transform_to_pure_blocked() {
        let transform_simple = V2Classification { level: Level::Simple, zone: Zone::Transform };
        let pure_primitive = V2Classification { level: Level::Primitive, zone: Zone::Pure };
        assert!(!transform_simple.can_import(pure_primitive), "transform isolated from pure");
    }

    #[test]
    fn structure_imports_structure_combined() {
        let source = V2Classification { level: Level::Structure, zone: Zone::Structure };
        let target = V2Classification { level: Level::Structure, zone: Zone::Structure };
        assert!(source.can_import(target), "structure files should freely import each other");
    }

    #[test]
    fn all_zones_can_import_structure() {
        let structure = V2Classification { level: Level::Structure, zone: Zone::Structure };
        for zone in [Zone::Pure, Zone::Impure, Zone::Transform, Zone::Orchestrate] {
            let source = V2Classification { level: Level::Simple, zone };
            assert!(source.can_import(structure), "{zone:?}/simple should import structure");
        }
    }

    #[test]
    fn orchestrate_reaches_composed_in_all_zones() {
        let orch = V2Classification { level: Level::Orchestrate, zone: Zone::Orchestrate };
        for zone in [Zone::Pure, Zone::Impure, Zone::Transform] {
            let target = V2Classification { level: Level::Composed, zone };
            assert!(orch.can_import(target), "orchestrate should reach {zone:?}/composed");
        }
    }

    #[test]
    fn pure_composed_unreachable_from_impure() {
        let pure_composed = V2Classification { level: Level::Composed, zone: Zone::Pure };
        for level in [Level::Ffi, Level::Primitive, Level::Simple, Level::Composed] {
            let impure = V2Classification { level, zone: Zone::Impure };
            assert!(!impure.can_import(pure_composed), "impure/{level:?} should not reach pure/composed");
        }
    }
}

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
