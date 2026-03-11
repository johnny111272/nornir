use datagram::{Datagram, DatagramKind, Priority, emit_validated, now, workspace_name};

// =============================================================================
// Types
// =============================================================================

#[derive(Debug)]
struct Config {
    source: String,
    kind: DatagramKind,
    classifier: Option<String>,
    priority: Priority,
    workspace: Option<String>,
    detail: Option<String>,
    speech: Option<String>,
    payload_file: Option<String>,
    payload_str: Option<String>,
}

// =============================================================================
// Arg parsing
// =============================================================================

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut source = None;
    let mut dtype_str = None;
    let mut priority_str = None;
    let mut workspace = None;
    let mut detail = None;
    let mut speech = None;
    let mut classifier = None;
    let mut payload_file = None;
    let mut payload_str = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--source" => { i += 1; source = Some(args.get(i).ok_or("--source requires a value")?.clone()); }
            "--type" => { i += 1; dtype_str = Some(args.get(i).ok_or("--type requires a value")?.clone()); }
            "--priority" => { i += 1; priority_str = Some(args.get(i).ok_or("--priority requires a value")?.clone()); }
            "--workspace" => { i += 1; workspace = Some(args.get(i).ok_or("--workspace requires a value")?.clone()); }
            "--detail" => { i += 1; detail = Some(args.get(i).ok_or("--detail requires a value")?.clone()); }
            "--speech" => { i += 1; speech = Some(args.get(i).ok_or("--speech requires a value")?.clone()); }
            "--classifier" => { i += 1; classifier = Some(args.get(i).ok_or("--classifier requires a value")?.clone()); }
            "--payload-file" => { i += 1; payload_file = Some(args.get(i).ok_or("--payload-file requires a value")?.clone()); }
            "--payload" => { i += 1; payload_str = Some(args.get(i).ok_or("--payload requires a value")?.clone()); }
            other => {
                return Err(format!("Unknown flag: {other}"));
            }
        }
        i += 1;
    }

    let source = source.ok_or("--source is required")?;
    let dtype_str = dtype_str.ok_or("--type is required")?;
    let priority_str = priority_str.ok_or("--priority is required")?;

    let kind = match dtype_str.as_str() {
        "alert" => DatagramKind::Alert,
        "quality" => DatagramKind::Quality,
        "canary" => DatagramKind::Canary,
        "notify" => DatagramKind::Notify,
        "traffic" => DatagramKind::Traffic,
        other => return Err(format!("Unknown --type: {other} (expected: alert, quality, canary, notify, traffic)")),
    };

    let priority = match priority_str.as_str() {
        "critical" => Priority::Critical,
        "high" => Priority::High,
        "normal" => Priority::Normal,
        "low" => Priority::Low,
        "trace" => Priority::Trace,
        other => return Err(format!("Unknown --priority: {other} (expected: critical, high, normal, low, trace)")),
    };

    Ok(Config {
        source,
        kind,
        classifier,
        priority,
        workspace,
        detail,
        speech,
        payload_file,
        payload_str,
    })
}

// =============================================================================
// Help
// =============================================================================

fn print_usage() {
    eprintln!(
        "Usage: send_datagram --source <s> --type <t> --priority <p> [--classifier <c>] [--workspace <w>] [--detail <d>] [--speech <s>] [--payload <json>] [--payload-file <path>]"
    );
}

// =============================================================================
// Core logic
// =============================================================================

fn run(config: Config) -> Result<(), String> {
    let payload = if let Some(path) = config.payload_file {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {path}: {e}"))?;
        Some(serde_json::from_str::<serde_json::Value>(&content)
            .map_err(|e| format!("Invalid JSON in {path}: {e}"))?)
    } else if let Some(raw) = config.payload_str {
        Some(serde_json::from_str::<serde_json::Value>(&raw)
            .map_err(|e| format!("Invalid --payload JSON: {e}"))?)
    } else {
        None
    };

    let datagram = Datagram {
        timestamp: now(),
        source: config.source,
        kind: config.kind,
        classifier: config.classifier,
        priority: config.priority,
        workspace: config.workspace.unwrap_or_else(workspace_name),
        detail: config.detail,
        speech: config.speech,
        payload,
    };

    emit_validated(&datagram)
}

// =============================================================================
// Entry point
// =============================================================================

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let config = match parse_args(&args[1..]) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            print_usage();
            std::process::exit(1);
        }
    };

    match run(config) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // =========================================================================
    // parse_args — required flags present
    // =========================================================================

    #[test]
    fn parse_args_all_required() {
        let a = args(&["--source", "saga", "--type", "alert", "--priority", "high"]);
        let config = parse_args(&a).unwrap();
        assert_eq!(config.source, "saga");
        assert_eq!(config.kind, DatagramKind::Alert);
        assert_eq!(config.priority, Priority::High);
        assert!(config.workspace.is_none());
        assert!(config.detail.is_none());
        assert!(config.speech.is_none());
        assert!(config.payload_file.is_none());
        assert!(config.payload_str.is_none());
    }

    #[test]
    fn parse_args_all_flags() {
        let a = args(&[
            "--source", "hook",
            "--type", "quality",
            "--priority", "normal",
            "--workspace", "odinn",
            "--detail", "something happened",
            "--speech", "alert spoken text",
            "--payload", r#"{"key":"val"}"#,
        ]);
        let config = parse_args(&a).unwrap();
        assert_eq!(config.source, "hook");
        assert_eq!(config.kind, DatagramKind::Quality);
        assert_eq!(config.priority, Priority::Normal);
        assert_eq!(config.workspace.as_deref(), Some("odinn"));
        assert_eq!(config.detail.as_deref(), Some("something happened"));
        assert_eq!(config.speech.as_deref(), Some("alert spoken text"));
        assert_eq!(config.payload_str.as_deref(), Some(r#"{"key":"val"}"#));
    }

    // =========================================================================
    // parse_args — missing required flags
    // =========================================================================

    #[test]
    fn parse_args_missing_source() {
        let a = args(&["--type", "alert", "--priority", "high"]);
        let err = parse_args(&a).unwrap_err();
        assert!(err.contains("--source"), "error should mention --source: {err}");
    }

    #[test]
    fn parse_args_missing_type() {
        let a = args(&["--source", "saga", "--priority", "high"]);
        let err = parse_args(&a).unwrap_err();
        assert!(err.contains("--type"), "error should mention --type: {err}");
    }

    #[test]
    fn parse_args_missing_priority() {
        let a = args(&["--source", "saga", "--type", "alert"]);
        let err = parse_args(&a).unwrap_err();
        assert!(err.contains("--priority"), "error should mention --priority: {err}");
    }

    // =========================================================================
    // parse_args — invalid enum values
    // =========================================================================

    #[test]
    fn parse_args_invalid_type() {
        let a = args(&["--source", "s", "--type", "bogus", "--priority", "high"]);
        let err = parse_args(&a).unwrap_err();
        assert!(err.contains("bogus"), "error should mention the bad value: {err}");
    }

    #[test]
    fn parse_args_invalid_priority() {
        let a = args(&["--source", "s", "--type", "alert", "--priority", "mega"]);
        let err = parse_args(&a).unwrap_err();
        assert!(err.contains("mega"), "error should mention the bad value: {err}");
    }

    // =========================================================================
    // parse_args — unknown flags
    // =========================================================================

    #[test]
    fn parse_args_unknown_flag() {
        let a = args(&["--source", "s", "--type", "alert", "--priority", "high", "--banana"]);
        let err = parse_args(&a).unwrap_err();
        assert!(err.contains("--banana"), "error should mention unknown flag: {err}");
    }

    // =========================================================================
    // parse_args — all datagram kinds
    // =========================================================================

    #[test]
    fn parse_args_all_datagram_kinds() {
        for (name, expected) in [
            ("alert", DatagramKind::Alert),
            ("quality", DatagramKind::Quality),
            ("canary", DatagramKind::Canary),
            ("notify", DatagramKind::Notify),
            ("traffic", DatagramKind::Traffic),
        ] {
            let a = args(&["--source", "s", "--type", name, "--priority", "low"]);
            let config = parse_args(&a).unwrap();
            assert_eq!(config.kind, expected, "kind mismatch for --type {name}");
        }
    }

    // =========================================================================
    // parse_args — all priorities
    // =========================================================================

    #[test]
    fn parse_args_all_priorities() {
        for (name, expected) in [
            ("critical", Priority::Critical),
            ("high", Priority::High),
            ("normal", Priority::Normal),
            ("low", Priority::Low),
            ("trace", Priority::Trace),
        ] {
            let a = args(&["--source", "s", "--type", "alert", "--priority", name]);
            let config = parse_args(&a).unwrap();
            assert_eq!(config.priority, expected, "priority mismatch for --priority {name}");
        }
    }

    // =========================================================================
    // parse_args — detail and speech are optional
    // =========================================================================

    #[test]
    fn parse_args_optional_detail_speech() {
        let a = args(&["--source", "s", "--type", "alert", "--priority", "low"]);
        let config = parse_args(&a).unwrap();
        assert!(config.detail.is_none());
        assert!(config.speech.is_none());
    }

    // =========================================================================
    // parse_args — flag without value
    // =========================================================================

    #[test]
    fn parse_args_source_without_value() {
        let a = args(&["--source"]);
        let err = parse_args(&a).unwrap_err();
        assert!(err.contains("--source"), "error should mention --source: {err}");
    }
}
