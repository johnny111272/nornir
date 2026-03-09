use socket_emit::{Datagram, DatagramKind, Priority, emit_datagram, now, workspace_name};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: send_heartbeat <source>");
        std::process::exit(1);
    }

    let datagram = Datagram {
        timestamp: now(),
        source: args[1].clone(),
        kind: DatagramKind::Canary,
        priority: Priority::Low,
        workspace: workspace_name(),
        detail: None,
        speech: None,
        payload: None,
    };

    emit_datagram(&datagram);
}
