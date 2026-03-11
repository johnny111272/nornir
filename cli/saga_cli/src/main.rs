//! Saga CLI: quality truth recorder.
//!
//! Runs quality tools on source files (Python, Rust), outputs .qa JSON report.
//!
//! Usage:
//!   saga <file>                              — run tools, JSON to stdout
//!   saga <file> --sidecar                    — run tools, write .qa sidecar
//!   saga <dir> [--kind py|rs|all]            — remove orphaned .qa, generate missing
//!   saga <dir> --force [--kind py|rs|all]    — remove orphaned .qa, regenerate all
//!   echo "content" | saga --stdin <file>     — run tools on stdin content
//!
//! File vs directory is auto-detected from the path.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process;

// =============================================================================
// CLI
// =============================================================================

#[derive(Clone, Copy)]
enum Kind {
    Py,
    Rs,
    All,
}

struct Args {
    path: PathBuf,
    force: bool,
    write_sidecar: bool,
    read_stdin: bool,
    project_dir: Option<PathBuf>,
    kind: Kind,
}

fn parse_args() -> Result<Args, String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();

    let mut force = false;
    let mut write_sidecar = false;
    let mut read_stdin = false;
    let mut project_dir = None;
    let mut kind = Kind::All;
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
            "--kind" => {
                idx += 1;
                kind = match raw.get(idx).map(|s| s.as_str()) {
                    Some("py") => Kind::Py,
                    Some("rs") => Kind::Rs,
                    Some("all") => Kind::All,
                    Some(other) => return Err(format!("[saga] unknown kind: {} (use py, rs, or all)", other)),
                    None => return Err("[saga] --kind requires a value (py, rs, or all)".into()),
                };
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

    Ok(Args { path, force, write_sidecar, read_stdin, project_dir, kind })
}

fn print_usage() {
    eprintln!(
        "saga — quality truth recorder

USAGE:
    saga <file> [--sidecar] [--project-dir <dir>]
    saga <dir> [--kind py|rs|all] [--force]
    echo \"content\" | saga --stdin <file>

FILE:
    saga <file>                Run tools, report JSON to stdout
    saga <file> --sidecar      Run tools, write .qa sidecar

DIRECTORY:
    saga <dir>                  Remove orphaned .qa, generate missing sidecars
    saga <dir> --force          Remove orphaned .qa, regenerate all sidecars

OPTIONS:
    --kind <py|rs|all>     File types to process in directory mode (default: all)
    --project-dir <dir>    Project root (for relative paths)
    --stdin                Read file content from stdin
    --force                Regenerate even if .qa exists (directory only)"
    );
}

// =============================================================================
// Directory mode
// =============================================================================

fn find_source_files(dir: &Path, kind: Kind) -> Vec<PathBuf> {
    saga_runner::find_files(dir, &[], &|name| match kind {
        Kind::Py => name.ends_with(".py"),
        Kind::Rs => name.ends_with(".rs"),
        Kind::All => name.ends_with(".py") || name.ends_with(".rs"),
    })
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Py => ".py",
        Kind::Rs => ".rs",
        Kind::All => ".py/.rs",
    }
}

fn run_directory(dir: &Path, force: bool, kind: Kind) -> Result<(), String> {
    let project_root = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());

    // Remove orphaned .qa sidecars before generation
    let orphans = saga_runner::remove_orphaned_sidecars(&project_root);
    if !orphans.is_empty() {
        eprintln!("[saga] removed {} orphaned .qa sidecars", orphans.len());
    }

    let files = find_source_files(&project_root, kind);

    if files.is_empty() {
        eprintln!("[saga] no {} files found in {}", kind_label(kind), dir.display());
        return Ok(());
    }

    let mut generated = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for file in &files {
        let sidecar = saga_runner::qa_path(file);
        if !force && sidecar.exists() {
            skipped += 1;
            continue;
        }

        let report = saga_runner::generate_report(file, Some(&project_root));
        match saga_runner::save_sidecar(&report) {
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
        "[saga] {} generated, {} skipped, {} failed (of {} {} files)",
        generated, skipped, failed, files.len(), kind_label(kind)
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
        saga_runner::generate_report_from_content(&args.path, &content, project_root)
    } else {
        if !args.path.exists() {
            return Err(format!("[saga] file not found: {}", args.path.display()));
        }
        saga_runner::generate_report(&args.path, project_root)
    };

    if args.write_sidecar {
        match saga_runner::save_sidecar(&report) {
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
        run_directory(&args.path, args.force, args.kind)
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
