use datagram_io::{Datagram, DatagramKind, Priority, emit, now, workspace_name};

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: send_notification <source> <message>");
        std::process::exit(1);
    }

    let detail = args.swap_remove(2);
    let source = args.swap_remove(1);

    let datagram = Datagram {
        timestamp: now(),
        source,
        kind: DatagramKind::Notify,
        classifier: None,
        priority: Priority::Normal,
        workspace: workspace_name(),
        detail: Some(detail),
        speech: None,
        payload: None,
    };

    emit(&datagram);
}
