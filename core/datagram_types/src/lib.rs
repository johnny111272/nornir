//! Core datagram types for the Hlidskjalf messaging protocol.
//!
//! Pure type definitions with serde derives. No I/O, no transport.
//! Used by core crates that need datagram types without pulling in transport.

use serde::{Deserialize, Serialize};

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
