use socket_emit::{Datagram, DatagramKind, Priority, emit_datagram, now, workspace_name};

// =============================================================================
// Types
// =============================================================================

struct Config {
    source: String,
    kind: DatagramKind,
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
        "report" => DatagramKind::Report,
        "canary" => DatagramKind::Canary,
        "notify" => DatagramKind::Notify,
        "exchange" => DatagramKind::Exchange,
        other => return Err(format!("Unknown --type: {other} (expected: alert, report, canary, notify, exchange)")),
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
        "Usage: send_datagram --source <s> --type <t> --priority <p> [--workspace <w>] [--detail <d>] [--speech <s>] [--payload <json>] [--payload-file <path>]"
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
        priority: config.priority,
        workspace: config.workspace.unwrap_or_else(workspace_name),
        detail: config.detail,
        speech: config.speech,
        payload,
    };

    // Validate against schema before sending
    let json_str = serde_json::to_string(&datagram)
        .map_err(|e| format!("Serialization error: {e}"))?;

    let result = schemas_embedded::DATAGRAM.validate(&json_str)
        .map_err(|e| format!("Schema validation error: {e}"))?;

    if !result.valid {
        return Err(result.message.clone());
    }

    emit_datagram(&datagram);
    Ok(())
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
