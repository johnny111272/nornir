//! CLI check tool: check_paths_resolved
//! Validates a TOML file against the PATHS_RESOLVED schema.

use error_core::NornirError;
use format_core::toml_to_json;
use io_check::{run_check, CheckResult};
use schemas_embedded::PATHS_RESOLVED;

fn check(input: &str) -> Result<CheckResult, NornirError> {
    let json = toml_to_json(input)?;
    let result = PATHS_RESOLVED.validate(&json)?;
    Ok(CheckResult {
        valid: result.valid,
        message: result.message,
    })
}

fn main() -> std::process::ExitCode {
    run_check(check)
}
