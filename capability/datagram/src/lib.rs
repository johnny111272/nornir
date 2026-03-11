//! Datagram types and transport for the Hlidskjalf messaging protocol.
//!
//! Three emit paths:
//!   `emit()`                  — fire-and-forget, no validation (hardcoded senders)
//!   `emit_validated()`        — schema-validated, returns Err on malformed (caller handles)
//!   `emit_validated_or_alert()` — schema-validated, emits alert on failure (autonomous callers)
//!
//! Dual transport:
//!   1. Unix stream socket → record_datagrams daemon (persistent archive)
//!   2. UDP multicast 239.0.0.1:9899 → live consumers (Hlidskjalf, etc.)
//!
//! Both channels fire-and-forget. Either can fail silently.
//! Protocol: compact JSON + newline (one datagram per line).

use std::io::Write;
use std::net::{Ipv4Addr, UdpSocket};
use std::os::unix::net::UnixStream;
use std::time::Duration;

// Re-export types from core crate — all consumers get them through this crate
pub use datagram_types::{Datagram, DatagramKind, Priority};

pub const SOCKET_PATH: &str = "/tmp/ai_logger.sock";
const WRITE_TIMEOUT: Duration = Duration::from_millis(200);

const MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(239, 0, 0, 1);
const MULTICAST_PORT: u16 = 9899;

/// Send a datagram. Fire-and-forget — never panics, never blocks.
/// No schema validation. Use for hardcoded senders with known-good shapes.
pub fn emit(datagram: &Datagram) {
    let _ = try_emit(datagram);
}

/// Send a datagram after validating against the compiled schema.
/// Returns Err with validation message if the datagram is malformed.
/// Use for dynamic payloads (Traffic, Quality) constructed at runtime.
pub fn emit_validated(datagram: &Datagram) -> Result<(), String> {
    let json_str = serde_json::to_string(datagram)
        .map_err(|e| format!("serialization error: {e}"))?;

    let result = schemas_embedded::DATAGRAM
        .validate(&json_str)
        .map_err(|e| format!("schema error: {e}"))?;

    if !result.valid {
        return Err(result.message);
    }

    emit(datagram);
    Ok(())
}

/// Validate and emit, alerting on failure. For autonomous callers (watchers, syn)
/// where no human sees stderr. Returns true if emitted, false if validation failed.
///
/// On validation failure:
///   1. Emits a high-priority alert datagram (via emit(), bypasses validation)
///   2. The alert includes the validation error and the source that tried to emit
///   3. Returns false so callers can track accurate counts
pub fn emit_validated_or_alert(datagram: &Datagram, caller: &str) -> bool {
    match emit_validated(datagram) {
        Ok(()) => true,
        Err(e) => {
            let alert = Datagram {
                timestamp: now(),
                source: caller.into(),
                kind: DatagramKind::Alert,
                classifier: None,
                priority: Priority::High,
                workspace: datagram.workspace.clone(),
                detail: Some(format!("Schema validation failed: {e}")),
                speech: Some(format!(
                    "WARNING: {} emitted a malformed datagram. Schema validation failed.",
                    caller
                )),
                payload: None,
            };
            emit(&alert);
            false
        }
    }
}

fn try_emit(datagram: &Datagram) -> Result<(), Box<dyn std::error::Error>> {
    let mut json = serde_json::to_vec(datagram)?;
    json.push(b'\n');

    // Channel 1: Unix stream to logging daemon (permanent archive)
    let _ = try_unix_stream(&json);

    // Channel 2: UDP multicast on loopback (live consumers)
    let _ = try_udp_multicast(&json);

    Ok(())
}

fn try_unix_stream(json: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = UnixStream::connect(SOCKET_PATH)?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    stream.write_all(json)?;
    Ok(())
}

fn try_udp_multicast(json: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_multicast_ttl_v4(1)?;
    set_multicast_interface(&socket, Ipv4Addr::LOCALHOST)?;
    socket.send_to(json, (MULTICAST_ADDR, MULTICAST_PORT))?;
    Ok(())
}

/// Bind multicast output to a specific interface via IP_MULTICAST_IF.
fn set_multicast_interface(
    socket: &UdpSocket,
    interface: Ipv4Addr,
) -> Result<(), std::io::Error> {
    use std::os::unix::io::AsRawFd;
    let fd = socket.as_raw_fd();
    let addr = interface.octets();
    let ret = unsafe {
        libc::setsockopt(
            fd,
            libc::IPPROTO_IP,
            libc::IP_MULTICAST_IF,
            addr.as_ptr() as *const libc::c_void,
            4, // sizeof(struct in_addr)
        )
    };
    if ret == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Current time as Unix timestamp (f64).
pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Get the workspace name from CLAUDE_PROJECT_DIR env var.
pub fn workspace_name() -> String {
    std::env::var("CLAUDE_PROJECT_DIR")
        .ok()
        .and_then(|p| {
            std::path::Path::new(&p)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
        })
        .unwrap_or_default()
}
