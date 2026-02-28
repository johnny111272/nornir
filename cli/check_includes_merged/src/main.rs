//! CLI check tool: check_includes_merged
//! Validates a TOML file against the INCLUDES_MERGED schema and verifies paths.

use error_core::NornirError;
use format_core::toml_to_json;
use io_check::{run_check, CheckResult};
use schemas_embedded::INCLUDES_MERGED;

fn check(input: &str) -> Result<CheckResult, NornirError> {
    let json = toml_to_json(input)?;
    let result = INCLUDES_MERGED.validate(&json)?;
    if !result.valid {
        return Ok(CheckResult {
            valid: false,
            message: result.message,
        });
    }
    path_verify::verify_paths(INCLUDES_MERGED.schema_json(), &json)?;
    Ok(CheckResult {
        valid: true,
        message: result.message,
    })
}

fn main() -> std::process::ExitCode {
    run_check(check)
}
