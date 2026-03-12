//! Guardrail check implementations for TypeScript source (extracted from .svelte files).
//!
//! Each submodule contains checks for one concern area.
//! All checks share the signature: fn(&ParsedSource, &CheckConfig) -> Vec<Violation>

pub mod prohibited;
pub mod style;
pub mod suppression;
