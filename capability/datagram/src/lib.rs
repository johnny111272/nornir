//! Datagram types and transport for the Hlidskjalf messaging protocol.
//!
//! Two emit paths:
//!   `emit()`           — fire-and-forget, no validation (hardcoded senders)
//!   `emit_validated()` — schema-validated, rejects malformed datagrams (dynamic payloads)
//!
//! Dual transport:
//!   1. Unix stream socket → record_datagrams daemon (persistent archive)
//!   2. UDP multicast 239.0.0.1:9899 → live consumers (Hlidskjalf, etc.)
//!
//! Both channels fire-and-forget. Either can fail silently.
//! Protocol: compact JSON + newline (one datagram per line).

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::net::{Ipv4Addr, UdpSocket};
use std::os::unix::net::UnixStream;
use std::time::Duration;

const SOCKET_PATH: &str = "/tmp/ai_logger.sock";
const WRITE_TIMEOUT: Duration = Duration::from_millis(200);

const MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(239, 0, 0, 1);
const MULTICAST_PORT: u16 = 9899;

/// Content type — what this datagram IS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DatagramKind {
    Alert,
    Quality,
    Canary,
    Notify,
    Traffic,
}

/// Severity level for threshold filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Trace,
    Low,
    Normal,
    High,
    Critical,
}

/// A standardized datagram for the Hlidskjalf messaging protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Datagram {
    pub timestamp: f64,
    pub source: String,
    pub kind: DatagramKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifier: Option<String>,
    pub priority: Priority,
    pub workspace: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speech: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

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
