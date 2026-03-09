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

fn parse_args() -> Result<Args, String> {
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
            arg if arg.starts_with('-') => {
                return Err(format!("[saga] unknown flag: {}", arg));
            }
            arg => positional.push(arg.to_string()),
        }
        idx += 1;
    }

    let path = match positional.first() {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };

    Ok(Args { path, force, write_sidecar, read_stdin, project_dir })
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
    saga_core::find_files(dir, &[], &|name| name.ends_with(".py"))
}

fn run_directory(dir: &Path, force: bool) -> Result<(), String> {
    let project_root = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());

    // Remove orphaned .qa sidecars before generation
    let orphans = saga_core::remove_orphaned_sidecars(&project_root);
    if !orphans.is_empty() {
        eprintln!("[saga] removed {} orphaned .qa sidecars", orphans.len());
    }

    let py_files = find_python_files(&project_root);

    if py_files.is_empty() {
        eprintln!("[saga] no .py files found in {}", dir.display());
        return Ok(());
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
    Ok(())
}

// =============================================================================
// Main
// =============================================================================

fn run_file(args: &Args) -> Result<(), String> {
    let project_root = args.project_dir.as_deref();

    let report = if args.read_stdin {
        let mut content = String::new();
        std::io::stdin().read_to_string(&mut content)
            .map_err(|e| format!("[saga] failed to read stdin: {}", e))?;
        saga_core::generate_report_from_content(&args.path, &content, project_root)
    } else {
        if !args.path.exists() {
            return Err(format!("[saga] file not found: {}", args.path.display()));
        }
        saga_core::generate_report(&args.path, project_root)
    };

    if args.write_sidecar {
        match saga_core::save_sidecar(&report) {
            Ok(path) => eprintln!("[saga] wrote {}", path.display()),
            Err(e) => eprintln!("[saga] sidecar write failed: {}", e),
        }
    }

    let json = serde_json::to_string_pretty(&report)
        .map_err(|e| format!("[saga] json serialization failed: {}", e))?;
    println!("{}", json);
    Ok(())
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        process::exit(0);
    }

    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(2);
        }
    };

    let result = if args.path.is_dir() {
        run_directory(&args.path, args.force)
    } else {
        run_file(&args)
    };

    match result {
        Ok(()) => process::exit(0),
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    }
}
