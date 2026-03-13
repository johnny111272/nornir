use datagram_io::{Datagram, DatagramKind, Priority, emit, now, workspace_name};

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: send_heartbeat <source>");
        std::process::exit(1);
    }

    let source = args.swap_remove(1);

    let datagram = Datagram {
        timestamp: now(),
        source,
        kind: DatagramKind::Canary,
        classifier: None,
        priority: Priority::Low,
        workspace: workspace_name(),
        detail: None,
        speech: None,
        payload: None,
    };

    emit(&datagram);
}
