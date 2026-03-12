use clap::{Parser, ValueEnum};
use datagram::{Datagram, DatagramKind, Priority, emit_validated, now, workspace_name};

// =============================================================================
// CLI enum types (local ValueEnum wrappers for tier isolation)
// =============================================================================

#[derive(Debug, Clone, ValueEnum)]
enum Kind {
    Alert,
    Warning,
    Quality,
    Canary,
    Notify,
    Traffic,
}

impl From<Kind> for DatagramKind {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::Alert => DatagramKind::Alert,
            Kind::Warning => DatagramKind::Warning,
            Kind::Quality => DatagramKind::Quality,
            Kind::Canary => DatagramKind::Canary,
            Kind::Notify => DatagramKind::Notify,
            Kind::Traffic => DatagramKind::Traffic,
        }
    }
}

#[derive(Debug, Clone, ValueEnum)]
enum PriorityLevel {
    Critical,
    High,
    Normal,
    Low,
    Trace,
}

impl From<PriorityLevel> for Priority {
    fn from(level: PriorityLevel) -> Self {
        match level {
            PriorityLevel::Critical => Priority::Critical,
            PriorityLevel::High => Priority::High,
            PriorityLevel::Normal => Priority::Normal,
            PriorityLevel::Low => Priority::Low,
            PriorityLevel::Trace => Priority::Trace,
        }
    }
}

// =============================================================================
// Types
// =============================================================================

/// Emit a validated datagram to the Hlidskjalf messaging system.
#[derive(Debug, Parser)]
#[command(name = "send_datagram")]
struct Args {
    /// Datagram source identifier
    #[arg(long)]
    source: String,

    /// Datagram kind
    #[arg(long)]
    kind: Kind,

    /// Datagram priority
    #[arg(long)]
    priority: PriorityLevel,

    /// Optional classifier tag
    #[arg(long)]
    classifier: Option<String>,

    /// Workspace name (auto-derived if omitted)
    #[arg(long)]
    workspace: Option<String>,

    /// Human-readable detail message
    #[arg(long)]
    detail: Option<String>,

    /// Speech text for audio alerts
    #[arg(long)]
    speech: Option<String>,

    /// Path to JSON file for payload
    #[arg(long)]
    payload_file: Option<String>,

    /// Inline JSON string for payload
    #[arg(long)]
    payload: Option<String>,
}

// =============================================================================
// Core logic
// =============================================================================

fn run(args: Args) -> Result<(), String> {
    let payload = if let Some(path) = args.payload_file {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {path}: {e}"))?;
        Some(serde_json::from_str::<serde_json::Value>(&content)
            .map_err(|e| format!("Invalid JSON in {path}: {e}"))?)
    } else if let Some(raw) = args.payload {
        Some(serde_json::from_str::<serde_json::Value>(&raw)
            .map_err(|e| format!("Invalid --payload JSON: {e}"))?)
    } else {
        None
    };

    let datagram = Datagram {
        timestamp: now(),
        source: args.source,
        kind: args.kind.into(),
        classifier: args.classifier,
        priority: args.priority.into(),
        workspace: args.workspace.unwrap_or_else(workspace_name),
        detail: args.detail,
        speech: args.speech,
        payload,
    };

    emit_validated(&datagram)
}

// =============================================================================
// Entry point
// =============================================================================

fn main() {
    let args = Args::parse();

    match run(args) {
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

    // =========================================================================
    // arg parsing (clap) — required flags present
    // =========================================================================

    #[test]
    fn parse_args_all_required() {
        let args = Args::try_parse_from(["send_datagram", "--source", "saga", "--kind", "alert", "--priority", "high"]).unwrap();
        assert_eq!(args.source, "saga");
        assert!(matches!(args.kind, Kind::Alert));
        assert!(matches!(args.priority, PriorityLevel::High));
        assert!(args.workspace.is_none());
        assert!(args.detail.is_none());
        assert!(args.speech.is_none());
        assert!(args.payload_file.is_none());
        assert!(args.payload.is_none());
    }

    #[test]
    fn parse_args_all_flags() {
        let args = Args::try_parse_from([
            "send_datagram",
            "--source", "hook",
            "--kind", "quality",
            "--priority", "normal",
            "--workspace", "odinn",
            "--detail", "something happened",
            "--speech", "alert spoken text",
            "--payload", r#"{"key":"val"}"#,
        ]).unwrap();
        assert_eq!(args.source, "hook");
        assert!(matches!(args.kind, Kind::Quality));
        assert!(matches!(args.priority, PriorityLevel::Normal));
        assert_eq!(args.workspace.as_deref(), Some("odinn"));
        assert_eq!(args.detail.as_deref(), Some("something happened"));
        assert_eq!(args.speech.as_deref(), Some("alert spoken text"));
        assert_eq!(args.payload.as_deref(), Some(r#"{"key":"val"}"#));
    }

    // =========================================================================
    // arg parsing — missing required flags
    // =========================================================================

    #[test]
    fn parse_args_missing_source() {
        let result = Args::try_parse_from(["send_datagram", "--kind", "alert", "--priority", "high"]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--source"), "error should mention --source: {err}");
    }

    #[test]
    fn parse_args_missing_kind() {
        let result = Args::try_parse_from(["send_datagram", "--source", "saga", "--priority", "high"]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--kind"), "error should mention --kind: {err}");
    }

    #[test]
    fn parse_args_missing_priority() {
        let result = Args::try_parse_from(["send_datagram", "--source", "saga", "--kind", "alert"]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--priority"), "error should mention --priority: {err}");
    }

    // =========================================================================
    // arg parsing — invalid enum values
    // =========================================================================

    #[test]
    fn parse_args_invalid_kind() {
        let result = Args::try_parse_from(["send_datagram", "--source", "s", "--kind", "bogus", "--priority", "high"]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("bogus"), "error should mention the bad value: {err}");
    }

    #[test]
    fn parse_args_invalid_priority() {
        let result = Args::try_parse_from(["send_datagram", "--source", "s", "--kind", "alert", "--priority", "mega"]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("mega"), "error should mention the bad value: {err}");
    }

    // =========================================================================
    // arg parsing — unknown flags
    // =========================================================================

    #[test]
    fn parse_args_unknown_flag() {
        let result = Args::try_parse_from(["send_datagram", "--source", "s", "--kind", "alert", "--priority", "high", "--banana"]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--banana"), "error should mention unknown flag: {err}");
    }

    // =========================================================================
    // arg parsing — all datagram kinds
    // =========================================================================

    #[test]
    fn parse_args_all_datagram_kinds() {
        for name in ["alert", "quality", "canary", "notify", "traffic"] {
            let args = Args::try_parse_from(["send_datagram", "--source", "s", "--kind", name, "--priority", "low"]).unwrap();
            // Verify the conversion to DatagramKind works
            let _: DatagramKind = args.kind.into();
        }
    }

    // =========================================================================
    // arg parsing — all priorities
    // =========================================================================

    #[test]
    fn parse_args_all_priorities() {
        for name in ["critical", "high", "normal", "low", "trace"] {
            let args = Args::try_parse_from(["send_datagram", "--source", "s", "--kind", "alert", "--priority", name]).unwrap();
            let _: Priority = args.priority.into();
        }
    }

    // =========================================================================
    // arg parsing — detail and speech are optional
    // =========================================================================

    #[test]
    fn parse_args_optional_detail_speech() {
        let args = Args::try_parse_from(["send_datagram", "--source", "s", "--kind", "alert", "--priority", "low"]).unwrap();
        assert!(args.detail.is_none());
        assert!(args.speech.is_none());
    }

    // =========================================================================
    // arg parsing — flag without value
    // =========================================================================

    #[test]
    fn parse_args_source_without_value() {
        let result = Args::try_parse_from(["send_datagram", "--source"]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--source"), "error should mention --source: {err}");
    }
}
