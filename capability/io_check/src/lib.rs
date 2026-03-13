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
    let (json_output, file_path) = match parse_check_args(&args) {
        Some(parsed) => parsed,
        None => return ExitCode::SUCCESS, // --help was printed
    };

    let input = match read_input(file_path.as_deref(), json_output) {
        Some(content) => content,
        None => return ExitCode::from(2),
    };

    match validate_fn(&input) {
        Ok(result) => emit_result(&result, json_output),
        Err(error) => {
            emit_error("validation_error", &error.to_string(), json_output);
            ExitCode::from(2)
        }
    }
}

/// Parse CLI args into (json_output, file_path). Returns None if help was printed.
fn parse_check_args(args: &[String]) -> Option<(bool, Option<String>)> {
    let mut json_output = false;
    let mut file_path: Option<String> = None;

    for arg in &args[1..] {
        match arg.as_str() {
            "--json" => json_output = true,
            "--help" | "-h" => {
                print_help(&args[0]);
                return None;
            }
            _ if arg.starts_with('-') => {
                eprintln!("Unknown option: {}", arg);
                return Some((json_output, file_path));
            }
            _ => {
                if file_path.is_some() {
                    eprintln!("Multiple file arguments not supported");
                    return Some((json_output, file_path));
                }
                file_path = Some(arg.clone());
            }
        }
    }

    Some((json_output, file_path))
}

/// Read input from file path or stdin. Returns None on error (already printed).
fn read_input(file_path: Option<&str>, json_output: bool) -> Option<String> {
    match file_path {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(content) => Some(content),
            Err(error) => {
                emit_error("io_error", &format!("Cannot read '{}': {}", path, error), json_output);
                None
            }
        },
        None => {
            let mut buffer = String::new();
            match std::io::stdin().read_to_string(&mut buffer) {
                Ok(_) => Some(buffer),
                Err(error) => {
                    emit_error("io_error", &format!("Cannot read stdin: {}", error), json_output);
                    None
                }
            }
        }
    }
}

/// Emit a successful check result.
fn emit_result(result: &CheckResult, json_output: bool) -> ExitCode {
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
    if result.valid { ExitCode::SUCCESS } else { ExitCode::from(1) }
}

/// Emit an error in json or text format.
fn emit_error(error_type: &str, message: &str, json_output: bool) {
    if json_output {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "ok": false,
                "error": {"type": error_type, "message": message}
            }))
            .unwrap_or_default()
        );
    } else {
        eprintln!("{}", message);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // =========================================================================
    // parse_check_args
    // =========================================================================

    #[test]
    fn parse_no_args() {
        let a = args(&["tool"]);
        let (json, file) = parse_check_args(&a).unwrap();
        assert!(!json);
        assert!(file.is_none());
    }

    #[test]
    fn parse_file_arg() {
        let a = args(&["tool", "file.toml"]);
        let (json, file) = parse_check_args(&a).unwrap();
        assert!(!json);
        assert_eq!(file.as_deref(), Some("file.toml"));
    }

    #[test]
    fn parse_json_flag() {
        let a = args(&["tool", "--json"]);
        let (json, file) = parse_check_args(&a).unwrap();
        assert!(json);
        assert!(file.is_none());
    }

    #[test]
    fn parse_json_and_file() {
        let a = args(&["tool", "--json", "file.toml"]);
        let (json, file) = parse_check_args(&a).unwrap();
        assert!(json);
        assert_eq!(file.as_deref(), Some("file.toml"));
    }

    #[test]
    fn parse_file_then_json() {
        let a = args(&["tool", "file.toml", "--json"]);
        let (json, file) = parse_check_args(&a).unwrap();
        assert!(json);
        assert_eq!(file.as_deref(), Some("file.toml"));
    }

    #[test]
    fn parse_help_returns_none() {
        let a = args(&["tool", "--help"]);
        assert!(parse_check_args(&a).is_none());
    }

    #[test]
    fn parse_help_short_returns_none() {
        let a = args(&["tool", "-h"]);
        assert!(parse_check_args(&a).is_none());
    }

    #[test]
    fn parse_unknown_flag_continues() {
        let a = args(&["tool", "--banana"]);
        let (json, file) = parse_check_args(&a).unwrap();
        assert!(!json);
        assert!(file.is_none());
    }

    #[test]
    fn parse_multiple_files_keeps_first() {
        let a = args(&["tool", "a.toml", "b.toml"]);
        let (_, file) = parse_check_args(&a).unwrap();
        assert_eq!(file.as_deref(), Some("a.toml"));
    }

    // =========================================================================
    // CheckResult serialization
    // =========================================================================

    #[test]
    fn check_result_valid_serializes() {
        let result = CheckResult { valid: true, message: "OK".into() };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"valid\":true"));
        assert!(json.contains("\"message\":\"OK\""));
    }

    #[test]
    fn check_result_invalid_serializes() {
        let result = CheckResult { valid: false, message: "bad field".into() };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"valid\":false"));
    }

    // =========================================================================
    // emit_result exit codes
    // =========================================================================

    #[test]
    fn emit_result_valid_returns_success() {
        let result = CheckResult { valid: true, message: "OK".into() };
        let code = emit_result(&result, false);
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn emit_result_invalid_returns_one() {
        let result = CheckResult { valid: false, message: "error".into() };
        let code = emit_result(&result, false);
        assert_eq!(code, ExitCode::from(1));
    }
}
