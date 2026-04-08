//! dep_resolve — CLI for inspecting Python dependency resolution.
//!
//! Shows what the LLM would see when reading a Python file:
//! resolved dependency signatures or full bodies, in topological order.
//!
//! Usage:
//!     dep_resolve <file> [--mode signatures|full] [--simulate] [--project-root <path>]

use std::path::{Path, PathBuf};
use std::process;

use clap::Parser;

use dep_resolve_core::resolve::ResolveConfig;
use dep_resolve_core::ResolveMode;

#[derive(Parser)]
#[command(name = "dep_resolve", about = "Resolve Python dependency interfaces")]
struct Args {
    /// Python file to analyze.
    file: PathBuf,

    /// Resolution mode: "signatures" or "full".
    #[arg(long, default_value = "signatures")]
    mode: String,

    /// Project root directory (auto-detected if omitted).
    #[arg(long)]
    project_root: Option<PathBuf>,

    /// Maximum transitive resolution depth.
    #[arg(long, default_value = "10")]
    max_depth: usize,

    /// Show injection + file content with banners (what the LLM would see).
    #[arg(long)]
    simulate: bool,
}

fn main() {
    let args = Args::parse();
    if let Err(message) = run(&args) {
        eprintln!("dep_resolve: {message}");
        process::exit(1);
    }
}

fn run(args: &Args) -> Result<(), String> {
    let file_path = args
        .file
        .canonicalize()
        .map_err(|err| format!("{}: {err}", args.file.display()))?;

    if !file_path.is_file() {
        return Err(format!("{} is not a file", file_path.display()));
    }

    let project_root = match &args.project_root {
        Some(root) => root
            .canonicalize()
            .map_err(|err| format!("{}: {err}", root.display()))?,
        None => detect_project_root(&file_path)
            .ok_or_else(|| "could not detect project root (no pyproject.toml found)".to_string())?,
    };

    let mode = parse_mode(&args.mode)?;

    let source = std::fs::read(&file_path)
        .map_err(|err| format!("read {}: {err}", file_path.display()))?;

    let config = ResolveConfig {
        mode,
        max_depth: args.max_depth,
    };

    let file_str = file_path.to_string_lossy();
    let root_str = project_root.to_string_lossy();

    let result = dep_resolve_core::resolve::resolve_dependencies(
        &file_str,
        &source,
        &root_str,
        |path| std::fs::read(path).ok(),
        &config,
    );

    if args.simulate {
        let file_content = std::str::from_utf8(&source).unwrap_or("");
        print!("{}", dep_resolve_core::render::render_simulate(&result, file_content, mode));
    } else {
        print!("{}", dep_resolve_core::render::render_injection(&result));
    }

    Ok(())
}

fn parse_mode(value: &str) -> Result<ResolveMode, String> {
    match value {
        "signatures" | "sig" => Ok(ResolveMode::Signatures),
        "hybrid" => Ok(ResolveMode::Hybrid),
        "full" => Ok(ResolveMode::Full),
        other => Err(format!(
            "unknown mode '{other}' (expected: signatures, hybrid, full)"
        )),
    }
}

fn detect_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.parent()?;
    loop {
        if current.join("pyproject.toml").exists() {
            return Some(current.to_path_buf());
        }
        if current.join("setup.py").exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}
