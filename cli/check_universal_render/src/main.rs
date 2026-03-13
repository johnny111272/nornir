//! CLI check tool: check_universal_render
//! Validates a TOML file against the UNIVERSAL_RENDER schema and verifies paths.

use error_core::NornirError;
use format_core::toml_to_json;
use io_check::{run_check, CheckResult};
use schemas_embedded::UNIVERSAL_RENDER;

fn check(input: &str) -> Result<CheckResult, NornirError> {
    let json = toml_to_json(input)?;
    let result = UNIVERSAL_RENDER.validate(&json)?;
    if !result.valid {
        return Ok(CheckResult {
            valid: false,
            message: result.message,
        });
    }
    path_verify_io::verify_paths(UNIVERSAL_RENDER.schema_json(), &json)?;
    Ok(CheckResult {
        valid: true,
        message: result.message,
    })
}

fn main() -> std::process::ExitCode {
    run_check(check)
}
