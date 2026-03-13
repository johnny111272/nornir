//! CLI check tool: check_paths_verified
//! Validates a TOML file against the PATHS_RESOLVED schema and verifies paths.

use error_core::NornirError;
use format_core::toml_to_json;
use io_check::{run_check, CheckResult};
use schemas_embedded::PATHS_RESOLVED;

fn check(input: &str) -> Result<CheckResult, NornirError> {
    let json = toml_to_json(input)?;
    let result = PATHS_RESOLVED.validate(&json)?;
    if !result.valid {
        return Ok(CheckResult {
            valid: false,
            message: result.message,
        });
    }
    path_verify_io::verify_paths(PATHS_RESOLVED.schema_json(), &json)?;
    Ok(CheckResult {
        valid: true,
        message: result.message,
    })
}

fn main() -> std::process::ExitCode {
    run_check(check)
}
