//! Import resolution, transitive walk, and topological ordering.
//!
//! Given a Python file's imports, resolves each to a source file under the
//! project root, recursively follows their imports, and returns dependency
//! groups in topological order (deepest first).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::extract::{extract_all_top_level_names, extract_symbols};
use crate::ts::{extract_imported_names, extract_module_info, find_nodes_by_type, parse_python};
use crate::{DependencyGroup, ResolveMode, ResolutionResult, ResolvedSymbol};

/// Configuration for dependency resolution.
pub struct ResolveConfig {
    /// What to extract: signatures or full bodies.
    pub mode: ResolveMode,
    /// Maximum transitive resolution depth.
    pub max_depth: usize,
}

/// A single import found in a source file.
struct ImportEntry {
    /// Dotted module path (e.g. "draupnir.structure.models").
    module_path: String,
    /// Names imported from that module.
    symbol_names: Vec<String>,
    /// Resolved file path on disk (if project-local).
    resolved_file: Option<PathBuf>,
}

/// Accumulated state for the recursive resolution walk.
struct ResolveState<'a, F: Fn(&str) -> Option<Vec<u8>>> {
    project_root: &'a Path,
    read_file: &'a F,
    mode: ResolveMode,
    max_depth: usize,
    visited: HashSet<String>,
    order: Vec<String>,
    groups: HashMap<String, Vec<ResolvedSymbol>>,
}

/// Resolve all project-local dependencies for a Python file.
///
/// The `read_file` callback provides source bytes for a given absolute path.
/// Returns groups in topological order: deepest dependencies first.
pub fn resolve_dependencies<F>(
    file_path: &str,
    source: &[u8],
    project_root: &str,
    read_file: F,
    config: &ResolveConfig,
) -> ResolutionResult
where
    F: Fn(&str) -> Option<Vec<u8>>,
{
    let root = Path::new(project_root);
    let target = Path::new(file_path);

    let target_rel = target
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| file_path.to_string());

    let mut state = ResolveState {
        project_root: root,
        read_file: &read_file,
        mode: config.mode,
        max_depth: config.max_depth,
        visited: HashSet::new(),
        order: Vec::new(),
        groups: HashMap::new(),
    };

    walk_file(file_path, source, 0, &mut state);

    let mut result_groups: Vec<DependencyGroup> = state
        .order
        .into_iter()
        .filter_map(|rel_path| {
            let symbols = state.groups.remove(&rel_path)?;
            if symbols.is_empty() {
                return None;
            }
            Some(DependencyGroup {
                source_file: rel_path,
                symbols,
            })
        })
        .collect();

    dedup_groups(&mut result_groups);

    ResolutionResult {
        target_file: target_rel,
        groups: result_groups,
    }
}

// ── Recursive walker ────────────────────────────────────────────

fn walk_file<F: Fn(&str) -> Option<Vec<u8>>>(
    file_path: &str,
    source: &[u8],
    depth: usize,
    state: &mut ResolveState<F>,
) {
    if depth > state.max_depth {
        return;
    }
    let canonical = file_path.to_string();
    if state.visited.contains(&canonical) {
        return;
    }
    state.visited.insert(canonical);

    let tree = match parse_python(source) {
        Ok(tree) => tree,
        Err(_) => return,
    };

    let imports = collect_local_imports(file_path, source, &tree, state.project_root);

    // Recurse into dependencies first (depth-first → deepest appear first)
    recurse_into_deps(&imports, depth, state);

    // Extract symbols from this file's direct dependencies
    extract_from_deps(&imports, depth, state);
}

fn recurse_into_deps<F: Fn(&str) -> Option<Vec<u8>>>(
    imports: &[ImportEntry],
    depth: usize,
    state: &mut ResolveState<F>,
) {
    for entry in imports {
        let dep_path = match &entry.resolved_file {
            Some(path) => path.to_string_lossy().to_string(),
            None => continue,
        };
        if state.visited.contains(&dep_path) {
            continue;
        }
        if let Some(dep_source) = (state.read_file)(&dep_path) {
            walk_file(&dep_path, &dep_source, depth + 1, state);
        }
    }
}

fn extract_from_deps<F: Fn(&str) -> Option<Vec<u8>>>(
    imports: &[ImportEntry],
    depth: usize,
    state: &mut ResolveState<F>,
) {
    // In hybrid mode: depth 0 (direct deps of the target file) get full bodies,
    // deeper transitive deps get signatures only.
    let effective_mode = match state.mode {
        ResolveMode::Hybrid if depth > 0 => ResolveMode::Signatures,
        other => other,
    };

    for entry in imports {
        let dep_path = match &entry.resolved_file {
            Some(path) => path,
            None => continue,
        };
        let dep_str = dep_path.to_string_lossy().to_string();
        let dep_source = match (state.read_file)(&dep_str) {
            Some(bytes) => bytes,
            None => continue,
        };
        let dep_tree = match parse_python(&dep_source) {
            Ok(tree) => tree,
            Err(_) => continue,
        };

        let rel_path = dep_path
            .strip_prefix(state.project_root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| dep_str.clone());

        let names = resolve_wildcard_names(&entry.symbol_names, &dep_source, &dep_tree);
        let mut symbols = extract_symbols(&dep_source, &dep_tree, &names, effective_mode);

        for sym in &mut symbols {
            sym.import_path = format!("{}.{}", entry.module_path, sym.name);
        }

        if !state.order.contains(&rel_path) {
            state.order.push(rel_path.clone());
        }
        state.groups.entry(rel_path).or_default().extend(symbols);
    }
}

// ── Import collection ───────────────────────────────────────────

fn collect_local_imports(
    file_path: &str,
    source: &[u8],
    tree: &tree_sitter::Tree,
    project_root: &Path,
) -> Vec<ImportEntry> {
    let root_node = tree.root_node();
    let import_nodes = find_nodes_by_type(root_node, "import_from_statement");
    let mut entries = Vec::new();

    for node in import_nodes {
        let (module_path, level) = extract_module_info(node, source);
        let symbol_names = extract_imported_names(node, source);

        let dotted = if level > 0 {
            resolve_relative_module(file_path, &module_path, level)
        } else {
            module_path.clone()
        };

        let resolved = resolve_module_to_file(&dotted, project_root);

        entries.push(ImportEntry {
            module_path: dotted,
            symbol_names,
            resolved_file: resolved,
        });
    }

    entries
}

// ── Module path → file path resolution ──────────────────────────

fn resolve_module_to_file(dotted: &str, project_root: &Path) -> Option<PathBuf> {
    let parts: Vec<&str> = dotted.split('.').collect();

    // Try progressively shorter prefixes as file paths
    for split in (1..=parts.len()).rev() {
        let rel_path: PathBuf = parts[..split].iter().collect();

        let candidate = project_root.join(&rel_path).with_extension("py");
        if candidate.is_file() {
            return Some(candidate);
        }

        let candidate = project_root.join(&rel_path).join("__init__.py");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    // Also try with src/ prefix (common layout)
    for split in (1..=parts.len()).rev() {
        let rel_path: PathBuf = parts[..split].iter().collect();

        let candidate = project_root.join("src").join(&rel_path).with_extension("py");
        if candidate.is_file() {
            return Some(candidate);
        }

        let candidate = project_root
            .join("src")
            .join(&rel_path)
            .join("__init__.py");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn resolve_relative_module(file_path: &str, module: &str, level: usize) -> String {
    let parent = Path::new(file_path).parent().unwrap_or(Path::new(""));

    // Walk up `level` directories (level=1 means current package)
    let mut base = parent.to_path_buf();
    for _ in 1..level {
        base = base.parent().unwrap_or(Path::new("")).to_path_buf();
    }

    let base_dotted = base
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(".");

    if module.is_empty() {
        base_dotted
    } else if base_dotted.is_empty() {
        module.to_string()
    } else {
        format!("{base_dotted}.{module}")
    }
}

// ── Wildcard resolution ─────────────────────────────────────────

fn resolve_wildcard_names(
    names: &[String],
    dep_source: &[u8],
    dep_tree: &tree_sitter::Tree,
) -> Vec<String> {
    if names.iter().any(|n| n == "*") {
        extract_all_top_level_names(dep_source, dep_tree)
    } else {
        names.to_vec()
    }
}

// ── Deduplication ───────────────────────────────────────────────

/// Remove duplicate symbols within each group (same name extracted multiple times).
pub fn dedup_groups(groups: &mut [DependencyGroup]) {
    for group in groups.iter_mut() {
        let mut seen = HashSet::new();
        group.symbols.retain(|sym| seen.insert(sym.name.clone()));
    }
}
