//! CLI check tool: check_anthropic_render
//! Validates a TOML file against the ANTHROPIC_RENDER schema and verifies paths.

use error_core::NornirError;
use format_core::toml_to_json;
use io_check::{run_check, CheckResult};
use schemas_embedded::ANTHROPIC_RENDER;

fn check(input: &str) -> Result<CheckResult, NornirError> {
    let json = toml_to_json(input)?;
    let result = ANTHROPIC_RENDER.validate(&json)?;
    if !result.valid {
        return Ok(CheckResult {
            valid: false,
            message: result.message,
        });
    }
    path_verify_io::verify_paths(ANTHROPIC_RENDER.schema_json(), &json)?;
    Ok(CheckResult {
        valid: true,
        message: result.message,
    })
}

fn main() -> std::process::ExitCode {
    run_check(check)
}
