//! Text rendering for resolved dependencies.
//!
//! Two output formats:
//!   render_injection — additionalContext string for hook injection
//!   render_simulate — CLI output with banners separating injection from file content

use crate::{ResolveMode, ResolutionResult};

/// Render the dependency context for hook injection (additionalContext).
///
/// Returns empty string if there are no resolved dependencies.
pub fn render_injection(result: &ResolutionResult) -> String {
    if result.groups.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();
    lines.push("The following are resolved dependencies for the file being read — use these to understand the interfaces before modifying the code.".to_string());
    lines.push(String::new());
    lines.push(format!(
        "\u{2500}\u{2500} Dependency interfaces for: {} \u{2500}\u{2500}",
        result.target_file
    ));
    lines.push(String::new());

    for group in &result.groups {
        lines.push(format!(
            "\u{2500}\u{2500} {} \u{2500}\u{2500}",
            group.source_file
        ));
        for (idx, sym) in group.symbols.iter().enumerate() {
            if idx > 0 {
                lines.push(String::new());
            }
            lines.push(sym.rendered.clone());
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

/// Render the simulate view: injection banner + dependency context + file content.
pub fn render_simulate(
    result: &ResolutionResult,
    file_content: &str,
    mode: ResolveMode,
) -> String {
    let mode_label = match mode {
        ResolveMode::Signatures => "signatures",
        ResolveMode::Hybrid => "hybrid",
        ResolveMode::Full => "full",
    };

    let mut lines = Vec::new();

    if result.groups.is_empty() {
        lines.push(format!(
            "\u{2550}\u{2550}\u{2550} INJECTION ({mode_label}) \u{2550}\u{2550}\u{2550}"
        ));
        lines.push(String::new());
        lines.push("(no project-local dependencies)".to_string());
    } else {
        lines.push(format!(
            "\u{2550}\u{2550}\u{2550} INJECTION ({mode_label}) \u{2550}\u{2550}\u{2550}"
        ));
        lines.push(String::new());

        for group in &result.groups {
            lines.push(format!(
                "\u{2500}\u{2500} {} \u{2500}\u{2500}",
                group.source_file
            ));
            for (idx, sym) in group.symbols.iter().enumerate() {
                if idx > 0 {
                    lines.push(String::new());
                }
                lines.push(sym.rendered.clone());
            }
            lines.push(String::new());
        }
    }

    lines.push(
        "\u{2550}\u{2550}\u{2550} FILE CONTENT \u{2550}\u{2550}\u{2550}".to_string(),
    );
    lines.push(String::new());
    lines.push(file_content.to_string());

    lines.join("\n")
}
