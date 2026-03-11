//! Guardrail check implementations for Rust source files.
//!
//! Each submodule contains checks for one concern area.
//! All checks share the signature: fn(&ParsedSource, &CheckConfig) -> Vec<Violation>

pub mod prohibited;
