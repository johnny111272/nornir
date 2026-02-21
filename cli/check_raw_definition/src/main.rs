//! CLI check tool: check_raw_definition
//! Validates a TOML file against the RAW_DEFINITION schema.

use error_core::NornirError;
use format_core::toml_to_json;
use io_check::{run_check, CheckResult};
use schemas_embedded::RAW_DEFINITION;

fn check(input: &str) -> Result<CheckResult, NornirError> {
    let json = toml_to_json(input)?;
    let result = RAW_DEFINITION.validate(&json)?;
    Ok(CheckResult {
        valid: result.valid,
        message: result.message,
    })
}

fn main() -> std::process::ExitCode {
    run_check(check)
}
