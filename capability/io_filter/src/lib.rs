//! Stdin->validate->stdout/stderr filter contract for pipeline gates.
//!
//! Generic filter that reads all stdin, passes through a validation function,
//! and writes the result to stdout (success) or stderr (failure).

use std::io::Read;
use std::process::ExitCode;

use error_core::NornirError;

/// Run a filter gate: read stdin, validate, write stdout or stderr.
///
/// - Success: writes returned `String` to stdout, exit 0
/// - Validation failure: writes educational error to stderr, exit 1
/// - IO error: message to stderr, exit 2
pub fn run_filter<F>(validate_fn: F) -> ExitCode
where
    F: FnOnce(&str) -> Result<String, NornirError>,
{
    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("Cannot read stdin: {}", e);
        return ExitCode::from(2);
    }

    match validate_fn(&input) {
        Ok(output) => {
            print!("{}", output);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{}", e);
            ExitCode::from(1)
        }
    }
}
