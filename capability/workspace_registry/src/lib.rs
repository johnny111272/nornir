//! Workspace registry — SQLite-backed workspace and session tracking.
//!
//! DB location: `~/.ai/control/workspaces/registry.db`
//!
//! Provides:
//! - `register_workspace(name, path)` — upsert workspace mapping
//! - `register_session(session_id, workspace)` — record active session
//! - `resolve_workspace_from_path(path)` — longest-prefix match
//! - `workspace_from_session(session_id)` — session → workspace lookup

use std::path::PathBuf;

use rusqlite::{Connection, params};

// ---------------------------------------------------------------------------
// DB path
// ---------------------------------------------------------------------------

fn db_path() -> PathBuf {
    write_engine::ai_home()
        .join("control/workspaces/registry.db")
}

// ---------------------------------------------------------------------------
// Connection + schema
// ---------------------------------------------------------------------------

fn open() -> Result<Connection, String> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create_dir_all: {e}"))?;
    }

    let conn = Connection::open(&path)
        .map_err(|e| format!("open db: {e}"))?;

    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(|e| format!("WAL: {e}"))?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS workspaces (
            name TEXT PRIMARY KEY,
            path TEXT NOT NULL UNIQUE
        );
        CREATE TABLE IF NOT EXISTS sessions (
            session_id TEXT PRIMARY KEY,
            workspace  TEXT NOT NULL,
            started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
            FOREIGN KEY (workspace) REFERENCES workspaces(name)
        );"
    ).map_err(|e| format!("schema: {e}"))?;

    Ok(conn)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Register a workspace name → path mapping. Upserts on conflict.
pub fn register_workspace(name: &str, path: &str) -> Result<(), String> {
    let conn = open()?;
    conn.execute(
        "INSERT INTO workspaces (name, path) VALUES (?1, ?2)
         ON CONFLICT(name) DO UPDATE SET path = excluded.path",
        params![name, path],
    ).map_err(|e| format!("register_workspace: {e}"))?;
    Ok(())
}

/// Register a session → workspace mapping.
pub fn register_session(session_id: &str, workspace: &str) -> Result<(), String> {
    let conn = open()?;
    conn.execute(
        "INSERT INTO sessions (session_id, workspace) VALUES (?1, ?2)
         ON CONFLICT(session_id) DO UPDATE SET workspace = excluded.workspace",
        params![session_id, workspace],
    ).map_err(|e| format!("register_session: {e}"))?;
    Ok(())
}

/// Resolve a filesystem path to a workspace name via longest-prefix match.
pub fn resolve_workspace_from_path(path: &str) -> Result<Option<String>, String> {
    let conn = open()?;

    // Normalize: ensure no trailing slash for consistent matching
    let normalized = path.trim_end_matches('/');

    let mut stmt = conn.prepare(
        "SELECT name, path FROM workspaces ORDER BY LENGTH(path) DESC"
    ).map_err(|e| format!("prepare: {e}"))?;

    let result = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
        ))
    }).map_err(|e| format!("query: {e}"))?;

    for row in result {
        let (name, ws_path) = row.map_err(|e| format!("row: {e}"))?;
        let ws_normalized = ws_path.trim_end_matches('/');
        // Exact match or path is under workspace
        if normalized == ws_normalized
            || normalized.starts_with(&format!("{ws_normalized}/"))
        {
            return Ok(Some(name));
        }
    }

    Ok(None)
}

/// Look up workspace name from a session ID.
pub fn workspace_from_session(session_id: &str) -> Result<Option<String>, String> {
    let conn = open()?;
    let mut stmt = conn.prepare(
        "SELECT workspace FROM sessions WHERE session_id = ?1"
    ).map_err(|e| format!("prepare: {e}"))?;

    let result = stmt.query_row(params![session_id], |row| {
        row.get::<_, String>(0)
    });

    match result {
        Ok(ws) => Ok(Some(ws)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(format!("workspace_from_session: {e}")),
    }
}

/// Get the most recent session ID for a workspace.
pub fn latest_session_for_workspace(workspace: &str) -> Result<Option<String>, String> {
    let conn = open()?;
    let mut stmt = conn.prepare(
        "SELECT session_id FROM sessions WHERE workspace = ?1 ORDER BY started_at DESC LIMIT 1"
    ).map_err(|e| format!("prepare: {e}"))?;

    let result = stmt.query_row(params![workspace], |row| {
        row.get::<_, String>(0)
    });

    match result {
        Ok(sid) => Ok(Some(sid)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(format!("latest_session_for_workspace: {e}")),
    }
}

/// Get the control directory for a workspace: `~/.ai/control/workspaces/{name}/`
pub fn workspace_control_dir(name: &str) -> PathBuf {
    write_engine::ai_home()
        .join("control/workspaces")
        .join(name)
}

/// Get the registered path for a workspace name.
pub fn workspace_path(name: &str) -> Result<Option<String>, String> {
    let conn = open()?;
    let mut stmt = conn.prepare(
        "SELECT path FROM workspaces WHERE name = ?1"
    ).map_err(|e| format!("prepare: {e}"))?;

    let result = stmt.query_row(params![name], |row| {
        row.get::<_, String>(0)
    });

    match result {
        Ok(p) => Ok(Some(p)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(format!("workspace_path: {e}")),
    }
}
