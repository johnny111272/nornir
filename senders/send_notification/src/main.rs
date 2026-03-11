use datagram::{Datagram, DatagramKind, Priority, emit, now, workspace_name};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: send_notification <source> <message>");
        std::process::exit(1);
    }

    let datagram = Datagram {
        timestamp: now(),
        source: args[1].clone(),
        kind: DatagramKind::Notify,
        classifier: None,
        priority: Priority::Normal,
        workspace: workspace_name(),
        detail: Some(args[2].clone()),
        speech: None,
        payload: None,
    };

    emit(&datagram);
}
