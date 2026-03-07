//! Guardrail check implementations.
//!
//! Each submodule contains checks for one concern area.
//! All checks share the signature: fn(&ParsedSource, &CheckConfig) -> Vec<Violation>

pub mod architecture;
pub mod imports;
pub mod prohibited;
pub mod style;
pub mod suppression;
pub mod type_safety;
