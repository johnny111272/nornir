//! Saga CLI: quality truth recorder.
//!
//! Runs quality tools on Python source, outputs .qa JSON report.
//!
//! Usage:
//!   saga <file.py>                          — run tools, JSON to stdout
//!   saga <file.py> --sidecar                — run tools, write .qa sidecar
//!   saga <dir>                              — generate missing .qa sidecars
//!   saga <dir> --force                      — regenerate all .qa sidecars
//!   echo "content" | saga --stdin <file.py> — run tools on stdin content
//!
//! File vs directory is auto-detected from the path.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process;

// =============================================================================
// CLI
// =============================================================================

struct Args {
    path: PathBuf,
    force: bool,
    write_sidecar: bool,
    read_stdin: bool,
    project_dir: Option<PathBuf>,
}

fn parse_args() -> Args {
    let raw: Vec<String> = std::env::args().skip(1).collect();

    let mut force = false;
    let mut write_sidecar = false;
    let mut read_stdin = false;
    let mut project_dir = None;
    let mut positional: Vec<String> = Vec::new();

    let mut idx = 0;
    while idx < raw.len() {
        match raw[idx].as_str() {
            "--force" => force = true,
            "--sidecar" => write_sidecar = true,
            "--stdin" => read_stdin = true,
            "--project-dir" => {
                idx += 1;
                project_dir = raw.get(idx).map(PathBuf::from);
            }
            "--help" | "-h" => {
                print_usage();
                process::exit(0);
            }
            arg if arg.starts_with('-') => {
                eprintln!("[saga] unknown flag: {}", arg);
                process::exit(2);
            }
            arg => positional.push(arg.to_string()),
        }
        idx += 1;
    }

    let path = match positional.first() {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };

    Args { path, force, write_sidecar, read_stdin, project_dir }
}

fn print_usage() {
    eprintln!(
        "saga — quality truth recorder

USAGE:
    saga <file.py> [--sidecar] [--project-dir <dir>]
    saga <dir> [--force]
    echo \"content\" | saga --stdin <file.py>

FILE:
    saga <file.py>              Run tools, report JSON to stdout
    saga <file.py> --sidecar    Run tools, write .qa sidecar

DIRECTORY:
    saga <dir>                  Generate .qa for files missing sidecars
    saga <dir> --force          Regenerate all .qa sidecars

OPTIONS:
    --project-dir <dir>    Project root (for relative paths)
    --stdin                Read file content from stdin
    --force                Regenerate even if .qa exists (directory only)"
    );
}

// =============================================================================
// Directory mode
// =============================================================================

fn find_python_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk_python_files(dir, &mut files);
    files.sort();
    files
}

fn walk_python_files(dir: &Path, results: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            if name.starts_with('.')
                || name == "__pycache__"
                || name == "node_modules"
                || name == ".venv"
                || name == "venv"
            {
                continue;
            }
            walk_python_files(&path, results);
        } else if name.ends_with(".py") {
            results.push(path);
        }
    }
}

fn run_directory(dir: &Path, force: bool) {
    let project_root = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let py_files = find_python_files(&project_root);

    if py_files.is_empty() {
        eprintln!("[saga] no .py files found in {}", dir.display());
        process::exit(0);
    }

    let mut generated = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for file in &py_files {
        let sidecar = saga_core::qa_path(file);
        if !force && sidecar.exists() {
            skipped += 1;
            continue;
        }

        let report = saga_core::generate_report(file, Some(&project_root));
        match saga_core::save_sidecar(&report) {
            Ok(path) => {
                let rel = path.strip_prefix(&project_root)
                    .unwrap_or(&path)
                    .display();
                eprintln!("  {}", rel);
                generated += 1;
            }
            Err(e) => {
                eprintln!("  FAIL {}: {}", file.display(), e);
                failed += 1;
            }
        }
    }

    eprintln!(
        "[saga] {} generated, {} skipped, {} failed (of {} .py files)",
        generated, skipped, failed, py_files.len()
    );
}

// =============================================================================
// Main
// =============================================================================

fn main() {
    let args = parse_args();

    if args.path.is_dir() {
        run_directory(&args.path, args.force);
    } else {
        let project_root = args.project_dir.as_deref();

        let report = if args.read_stdin {
            let mut content = String::new();
            if let Err(e) = std::io::stdin().read_to_string(&mut content) {
                eprintln!("[saga] failed to read stdin: {}", e);
                process::exit(1);
            }
            saga_core::generate_report_from_content(&args.path, &content, project_root)
        } else {
            if !args.path.exists() {
                eprintln!("[saga] file not found: {}", args.path.display());
                process::exit(1);
            }
            saga_core::generate_report(&args.path, project_root)
        };

        if args.write_sidecar {
            match saga_core::save_sidecar(&report) {
                Ok(path) => eprintln!("[saga] wrote {}", path.display()),
                Err(e) => eprintln!("[saga] sidecar write failed: {}", e),
            }
        }

        match serde_json::to_string_pretty(&report) {
            Ok(json) => println!("{}", json),
            Err(e) => {
                eprintln!("[saga] json serialization failed: {}", e);
                process::exit(1);
            }
        }
    }
}
