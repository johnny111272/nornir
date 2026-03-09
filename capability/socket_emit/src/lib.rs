//! Fire-and-forget event emitter for Hlidskjalf watchtower.
//!
//! Sends newline-delimited JSON datagrams over a Unix stream socket.
//! If Hlidskjalf isn't running, the send silently fails — never blocks the hook.
//!
//! Protocol: compact JSON + newline (one datagram per line).

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::time::Duration;

const SOCKET_PATH: &str = "/tmp/hlidskjalf.sock";
const WRITE_TIMEOUT: Duration = Duration::from_millis(200);

/// Event class — what kind of datagram this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DatagramKind {
    Alert,
    Report,
    Canary,
    Notify,
    Exchange,
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
    #[serde(rename = "type")]
    pub kind: DatagramKind,
    pub priority: Priority,
    pub workspace: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speech: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

/// Send a datagram to Hlidskjalf. Fire-and-forget — never panics, never blocks.
pub fn emit(datagram: &Datagram) {
    let _ = try_emit(datagram);
}

/// Backward-compatible alias during migration.
pub fn emit_datagram(datagram: &Datagram) {
    emit(datagram);
}

fn try_emit(datagram: &Datagram) -> Result<(), Box<dyn std::error::Error>> {
    let mut json = serde_json::to_vec(datagram)?;
    json.push(b'\n');

    let mut stream = UnixStream::connect(SOCKET_PATH)?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;

    stream.write_all(&json)?;

    Ok(())
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
