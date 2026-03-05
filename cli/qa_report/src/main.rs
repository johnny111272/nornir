//! CLI tool for reading .qa sidecar files and producing formatted reports.
//!
//! Usage:
//!     qa-report <path>                    # colored terminal (default)
//!     qa-report --plain <path>            # plain text for LLM context
//!     qa-report --json <path>             # re-grouped JSON
//!     qa-report --tool gleipnir <path>    # filter to one tool
//!
//! <path> can be a single .qa file, a .py file (finds its sidecar),
//! or a directory (finds all sidecars recursively).

use std::path::{Path, PathBuf};
use std::process;

use qa_core::{
    filter_by_tool, group_issues, ColoredFormatter, JsonFormatter, PlainFormatter, QaFormatter,
    SanityReport,
};

// =============================================================================
// CLI argument parsing — no deps, just args
// =============================================================================

enum OutputMode {
    Colored,
    Plain,
    Json,
}

struct Args {
    mode: OutputMode,
    tool_filter: Option<String>,
    target: PathBuf,
}

fn parse_args() -> Option<Args> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return None;
    }

    let mut mode = OutputMode::Colored;
    let mut tool_filter: Option<String> = None;
    let mut target: Option<PathBuf> = None;
    let mut idx = 0;

    while idx < args.len() {
        match args[idx].as_str() {
            "--plain" => mode = OutputMode::Plain,
            "--json" => mode = OutputMode::Json,
            "--colored" => mode = OutputMode::Colored,
            "--tool" if idx + 1 < args.len() => {
                tool_filter = Some(args[idx + 1].clone());
                idx += 1;
            }
            "--help" | "-h" => return None,
            other if !other.starts_with('-') => {
                target = Some(PathBuf::from(other));
            }
            _ => {
                eprintln!("Unknown flag: {}", args[idx]);
                return None;
            }
        }
        idx += 1;
    }

    target.map(|t| Args {
        mode,
        tool_filter,
        target: t,
    })
}

// =============================================================================
// .qa file discovery
// =============================================================================

/// Find the .qa sidecar for a .py file.
fn sidecar_for_py(py_path: &Path) -> Option<PathBuf> {
    let parent = py_path.parent()?;
    let name = py_path.file_name()?.to_str()?;
    let sidecar = parent.join(format!(".{}.qa", name));
    if sidecar.exists() {
        Some(sidecar)
    } else {
        None
    }
}

/// Recursively find all .qa files under a directory.
fn find_sidecars_recursive(dir: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    walk_for_sidecars(dir, &mut results);
    results.sort();
    results
}

fn walk_for_sidecars(dir: &Path, results: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip hidden dirs, __pycache__, .venv, node_modules
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.')
                || name_str == "__pycache__"
                || name_str == ".venv"
                || name_str == "node_modules"
            {
                continue;
            }
            walk_for_sidecars(&path, results);
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.ends_with(".qa") && name.starts_with('.') {
                results.push(path);
            }
        }
    }
}

/// Resolve the target path into a list of .qa files.
fn resolve_targets(target: &Path) -> Vec<PathBuf> {
    if target.is_dir() {
        find_sidecars_recursive(target)
    } else if target.extension().map(|e| e == "qa").unwrap_or(false) {
        // Direct .qa file
        vec![target.to_path_buf()]
    } else if target.extension().map(|e| e == "py").unwrap_or(false) {
        // .py file — find its sidecar
        match sidecar_for_py(target) {
            Some(sidecar) => vec![sidecar],
            None => {
                eprintln!("No .qa sidecar found for {}", target.display());
                vec![]
            }
        }
    } else {
        eprintln!("Unknown target type: {}", target.display());
        vec![]
    }
}

// =============================================================================
// Report reading
// =============================================================================

fn read_report(path: &Path) -> Option<SanityReport> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

// =============================================================================
// Main
// =============================================================================

fn main() {
    let args = match parse_args() {
        Some(a) => a,
        None => {
            eprintln!("Usage: qa-report [--plain|--json|--colored] [--tool <name>] <path>");
            eprintln!();
            eprintln!("  <path>    .qa file, .py file (finds sidecar), or directory (recursive)");
            eprintln!();
            eprintln!("Output modes:");
            eprintln!("  --colored   ANSI terminal output (default)");
            eprintln!("  --plain     grouped text for LLM context injection");
            eprintln!("  --json      re-grouped JSON for machines");
            eprintln!();
            eprintln!("Filters:");
            eprintln!("  --tool <name>   only show issues from this tool (gleipnir, ruff, basedpyright)");
            process::exit(2);
        }
    };

    let qa_files = resolve_targets(&args.target);
    if qa_files.is_empty() {
        eprintln!("No .qa files found at {}", args.target.display());
        process::exit(2);
    }

    let reports: Vec<SanityReport> = qa_files.iter().filter_map(|p| read_report(p)).collect();

    if reports.is_empty() {
        eprintln!("No valid .qa reports found");
        process::exit(2);
    }

    let mut groups = group_issues(&reports);

    if let Some(ref tool) = args.tool_filter {
        groups = filter_by_tool(groups, tool);
    }

    let output = match args.mode {
        OutputMode::Colored => ColoredFormatter.format(&groups),
        OutputMode::Plain => PlainFormatter.format(&groups),
        OutputMode::Json => JsonFormatter.format(&groups),
    };

    println!("{}", output);

    // Exit 1 if violations found, 0 if clean
    if groups.is_empty() || groups.iter().all(|g| g.issues.is_empty()) {
        process::exit(0);
    } else {
        process::exit(1);
    }
}
