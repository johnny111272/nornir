use datagram_io::{Datagram, DatagramKind, Priority, emit, now, workspace_name};

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: send_alert <source> <message>");
        std::process::exit(1);
    }

    let message = args.swap_remove(2);
    let source = args.swap_remove(1);

    let speech = format!("ALERT from {source}: {message}");
    let datagram = Datagram {
        timestamp: now(),
        source,
        kind: DatagramKind::Alert,
        classifier: None,
        priority: Priority::Critical,
        workspace: workspace_name(),
        detail: Some(message),
        speech: Some(speech),
        payload: None,
    };

    emit(&datagram);
}
