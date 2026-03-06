//! Fire-and-forget event emitter for Hlidskjalf watchtower.
//!
//! Sends newline-delimited JSON events over a Unix stream socket.
//! If Hlidskjalf isn't running, the send silently fails — never blocks the hook.
//!
//! Protocol: compact JSON + newline (one event per line).

use serde::Serialize;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::time::Duration;

const SOCKET_PATH: &str = "/tmp/hlidskjalf.sock";
const WRITE_TIMEOUT: Duration = Duration::from_millis(200);

/// A hook event destined for Hlidskjalf.
///
/// Matches the HookEvent struct on the receiver side.
#[derive(Debug, Clone, Serialize)]
pub struct WatchtowerEvent {
    pub timestamp: f64,
    pub category: String,
    pub decision: String,
    pub event_name: String,
    pub workspace: String,
    pub detail: String,
    pub context_injected: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speech: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

impl WatchtowerEvent {
    /// Create a simple event (no payload, no context injection).
    pub fn simple(
        category: &str,
        decision: &str,
        event_name: &str,
        workspace: &str,
        detail: &str,
    ) -> Self {
        Self {
            timestamp: now(),
            category: category.to_string(),
            decision: decision.to_string(),
            event_name: event_name.to_string(),
            workspace: workspace.to_string(),
            detail: detail.to_string(),
            context_injected: String::new(),
            speech: None,
            payload: None,
        }
    }
}

/// Send an event to Hlidskjalf. Fire-and-forget — never panics, never blocks.
pub fn emit(event: &WatchtowerEvent) {
    let _ = try_emit(event);
}

fn try_emit(event: &WatchtowerEvent) -> Result<(), Box<dyn std::error::Error>> {
    let mut json = serde_json::to_vec(event)?;
    json.push(b'\n');

    let mut stream = UnixStream::connect(SOCKET_PATH)?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;

    stream.write_all(&json)?;

    Ok(())
}

/// Current time as Unix timestamp (f64).
fn now() -> f64 {
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
