//! Pure text processing utilities.
//!
//! No I/O, no side effects. String transformations for preprocessing text
//! before TTS, display, or other consumption.

use regex::Regex;

/// Strip markdown formatting from text, keeping the readable content.
///
/// Removes: fenced code blocks, tables, inline code backticks, bold/italic
/// markers, header prefixes. Collapses excessive blank lines.
pub fn strip_markdown(text: &str) -> String {
    // Strip fenced code blocks (```...```) — (?s) enables dot-matches-newline
    let Ok(fenced) = Regex::new(r"(?s)```[^\n]*\n.*?```") else { return text.to_string() };
    let text = fenced.replace_all(text, "");

    // Strip markdown tables (lines starting with |)
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim_start().starts_with('|'))
        .collect();
    let text = lines.join("\n");

    // Strip inline code backticks — keep inner text
    let Ok(inline) = Regex::new(r"`([^`\n]+)`") else { return text };
    let text = inline.replace_all(&text, "$1");

    // Strip bold (**text**) — keep inner text
    let Ok(bold) = Regex::new(r"\*\*([^*\n]+)\*\*") else { return text.to_string() };
    let text = bold.replace_all(&text, "$1");

    // Strip italic (*text*) — keep inner text (after bold stripped)
    let Ok(italic) = Regex::new(r"\*([^*\n]+)\*") else { return text.to_string() };
    let text = italic.replace_all(&text, "$1");

    // Strip markdown header prefixes (# ## etc.) — keep heading text
    let Ok(headers) = Regex::new(r"(?m)^#{1,6}\s+(.+)$") else { return text.to_string() };
    let text = headers.replace_all(&text, "$1");

    // Collapse 3+ consecutive blank lines → one blank line
    let Ok(blank_lines) = Regex::new(r"\n{3,}") else { return text.to_string() };
    let text = blank_lines.replace_all(&text, "\n\n");

    text.trim().to_string()
}

/// Transform paths into speakable TTS text.
///
/// `~/ai/control/voice/` → `path: home '.' ai '.' control '.' voice`
/// `./src/main.rs`         → `path: relative '.' src '.' main.rs`
/// `@workspace/file`       → `path: workspace '.' file`
pub fn tts_clean(text: &str) -> String {
    let Ok(path_re) = Regex::new(r"[~@.]?/[\w.\-/]+|~/") else { return text.to_string() };
    let text = path_re.replace_all(text, |caps: &regex::Captures| {
        speakable_path(&caps[0])
    });
    text.to_string()
}

fn speakable_path(path: &str) -> String {
    let stripped = path.trim_end_matches('/');

    // Determine prefix from first character(s)
    let (prefix, remainder) = if stripped.starts_with("~/") || stripped == "~" {
        ("home", stripped.trim_start_matches("~/").trim_start_matches('~'))
    } else if stripped.starts_with("@/") || stripped.starts_with('@') {
        ("workspace", stripped.trim_start_matches("@/").trim_start_matches('@'))
    } else if stripped.starts_with("./") {
        ("relative", stripped.trim_start_matches("./"))
    } else {
        ("", stripped.trim_start_matches('/'))
    };

    let parts: Vec<&str> = remainder.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_start_matches('.'))
        .collect();

    let mut components = vec![prefix.to_string()];
    components.extend(parts.iter().map(|s| s.to_string()));

    format!("path: {}", components.join(" '.' "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_fenced_code_block() {
        let input = "Here is some code:\n```rust\nfn main() {}\n```\nAnd some text.";
        let result = strip_markdown(input);
        assert!(!result.contains("fn main"));
        assert!(result.contains("Here is some code"));
        assert!(result.contains("And some text"));
    }

    #[test]
    fn strips_markdown_table() {
        let input = "Before\n| col1 | col2 |\n| ---- | ---- |\n| a    | b    |\nAfter";
        let result = strip_markdown(input);
        assert!(!result.contains('|'));
        assert!(result.contains("Before"));
        assert!(result.contains("After"));
    }

    #[test]
    fn strips_inline_code_keeps_text() {
        let result = strip_markdown("Use the `announce` binary.");
        assert!(!result.contains('`'));
        assert!(result.contains("announce"));
    }

    #[test]
    fn strips_bold_keeps_text() {
        let result = strip_markdown("This is **important** text.");
        assert!(!result.contains("**"));
        assert!(result.contains("important"));
    }

    #[test]
    fn strips_italic_keeps_text() {
        let result = strip_markdown("This is *emphasized* text.");
        assert!(!result.contains('*'));
        assert!(result.contains("emphasized"));
    }

    #[test]
    fn strips_header_prefix_keeps_text() {
        let result = strip_markdown("## My Section\nSome content.");
        assert!(!result.contains('#'));
        assert!(result.contains("My Section"));
        assert!(result.contains("Some content"));
    }

    #[test]
    fn collapses_excess_blank_lines() {
        let result = strip_markdown("First\n\n\n\nSecond");
        assert!(!result.contains("\n\n\n"));
    }

    #[test]
    fn plain_text_passes_through() {
        let input = "Hello, this is a plain sentence.";
        assert_eq!(strip_markdown(input), input);
    }

    #[test]
    fn empty_after_filtering_returns_empty() {
        let input = "```rust\nfn main() {}\n```";
        assert!(strip_markdown(input).trim().is_empty());
    }

    // --- tts_clean ---

    #[test]
    fn tts_clean_home_path() {
        let result = tts_clean("Check ~/ai/control/voice/");
        assert_eq!(result, "Check path: home '.' ai '.' control '.' voice");
    }

    #[test]
    fn tts_clean_relative_path() {
        let result = tts_clean("Edit ./src/main.rs");
        assert_eq!(result, "Edit path: relative '.' src '.' main.rs");
    }

    #[test]
    fn tts_clean_plain_text_unchanged() {
        let input = "This is a normal sentence.";
        assert_eq!(tts_clean(input), input);
    }
}
