//! File-arg diagnostic output contract for CLI check tools.
//!
//! Generic check tool that reads a file (or stdin), validates it,
//! and prints diagnostic output.

use std::io::Read;
use std::process::ExitCode;

use error_core::NornirError;
use serde::Serialize;

/// Result from a check operation.
#[derive(Debug, Serialize)]
pub struct CheckResult {
    pub valid: bool,
    pub message: String,
}

/// Run a check tool: read file or stdin, validate, print diagnostics.
///
/// - Reads file from first positional arg, or stdin if no arg
/// - Supports `--json` flag for structured output
/// - Exit 0 if valid, 1 if invalid, 2 if operational error
pub fn run_check<F>(validate_fn: F) -> ExitCode
where
    F: FnOnce(&str) -> Result<CheckResult, NornirError>,
{
    let args: Vec<String> = std::env::args().collect();

    let mut json_output = false;
    let mut file_path: Option<String> = None;

    for arg in &args[1..] {
        match arg.as_str() {
            "--json" => json_output = true,
            "--help" | "-h" => {
                print_help(&args[0]);
                return ExitCode::SUCCESS;
            }
            _ if arg.starts_with('-') => {
                eprintln!("Unknown option: {}", arg);
                return ExitCode::from(2);
            }
            _ => {
                if file_path.is_some() {
                    eprintln!("Multiple file arguments not supported");
                    return ExitCode::from(2);
                }
                file_path = Some(arg.clone());
            }
        }
    }

    // Read input
    let input = match file_path {
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => {
                let msg = format!("Cannot read '{}': {}", path, e);
                if json_output {
                    println!(
                        "{}",
                        serde_json::to_string(&serde_json::json!({
                            "ok": false,
                            "error": {"type": "io_error", "message": msg}
                        }))
                        .unwrap_or_default()
                    );
                } else {
                    eprintln!("{}", msg);
                }
                return ExitCode::from(2);
            }
        },
        None => {
            let mut buf = String::new();
            match std::io::stdin().read_to_string(&mut buf) {
                Ok(_) => buf,
                Err(e) => {
                    let msg = format!("Cannot read stdin: {}", e);
                    if json_output {
                        println!(
                            "{}",
                            serde_json::to_string(&serde_json::json!({
                                "ok": false,
                                "error": {"type": "io_error", "message": msg}
                            }))
                            .unwrap_or_default()
                        );
                    } else {
                        eprintln!("{}", msg);
                    }
                    return ExitCode::from(2);
                }
            }
        }
    };

    // Validate
    match validate_fn(&input) {
        Ok(result) => {
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({
                        "ok": true,
                        "data": {
                            "valid": result.valid,
                            "message": result.message,
                        }
                    }))
                    .unwrap_or_default()
                );
            } else {
                println!("{}", result.message);
            }
            if result.valid {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({
                        "ok": false,
                        "error": {"type": "validation_error", "message": e.to_string()}
                    }))
                    .unwrap_or_default()
                );
            } else {
                eprintln!("{}", e);
            }
            ExitCode::from(2)
        }
    }
}

fn print_help(program: &str) {
    println!("Validate a TOML agent definition file");
    println!();
    println!("USAGE:");
    println!("    {} [OPTIONS] [FILE]", program);
    println!();
    println!("ARGS:");
    println!("    [FILE]    TOML file to validate (reads stdin if omitted)");
    println!();
    println!("OPTIONS:");
    println!("    --json       Output structured JSON instead of educational text");
    println!("    --help, -h   Print help");
    println!();
    println!("EXIT CODES:");
    println!("    0    Valid");
    println!("    1    Invalid (validation errors found)");
    println!("    2    Operational error (can't read file, can't parse)");
}
