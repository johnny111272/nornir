use socket_emit::{Datagram, DatagramKind, Priority, emit_datagram, now, workspace_name};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: send_alert <source> <message>");
        std::process::exit(1);
    }

    let source = &args[1];
    let message = &args[2];

    let datagram = Datagram {
        timestamp: now(),
        source: source.clone(),
        kind: DatagramKind::Alert,
        priority: Priority::Critical,
        workspace: workspace_name(),
        detail: Some(message.clone()),
        speech: Some(format!("ALERT from {source}: {message}")),
        payload: None,
    };

    emit_datagram(&datagram);
}
