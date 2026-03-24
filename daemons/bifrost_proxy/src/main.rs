//! bifrost_proxy — Rust reverse proxy replacing mitmproxy for Claude Code API interception.
//!
//! Sits between nginx TLS termination and nginx upstream forwarding:
//!   Claude Code → nginx (:443) → bifrost_proxy (:4000) → nginx (:8443) → Anthropic
//!
//! For /v1/messages POST requests: buffers body, runs interception pipeline
//! (raw capture, schema validation, classify, route, compaction rewrite),
//! then forwards (possibly rewritten) body upstream.
//!
//! All other requests pass through transparently without buffering.

use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use clap::Parser;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

use compaction_inject_core::inject_compaction_system_block;
use intercept_core::{classify_exchange, ExchangeKind};
use schema_core::EmbeddedValidator;

// =============================================================================
// CLI args
// =============================================================================

#[derive(Parser, Debug)]
#[command(name = "bifrost_proxy", about = "Bifrost reverse proxy — intercepts Claude Code API traffic")]
struct Args {
    /// Listen address
    #[arg(long, default_value = "0.0.0.0:4000")]
    listen: SocketAddr,

    /// Upstream address (nginx → Anthropic)
    #[arg(long, default_value = "127.0.0.1:8443")]
    upstream: String,

    /// Intercept data directory
    #[arg(long)]
    intercept_dir: Option<PathBuf>,

    /// Watcher binary name
    #[arg(long, default_value = "watch_and_diff_exchange_intercepts")]
    watcher_binary: String,
}

// =============================================================================
// Shared state
// =============================================================================

type HttpClient = Client<hyper_util::client::legacy::connect::HttpConnector, Full<Bytes>>;

struct ProxyState {
    upstream: String,
    intercept_dir: PathBuf,
    watcher_binary: String,
    http_client: HttpClient,
    /// session_id → workspace (in-memory supplement to registry.db)
    session_cache: Mutex<HashMap<String, String>>,
    /// session_id → watcher child process
    watchers: Mutex<HashMap<String, Child>>,
}

// =============================================================================
// Wire schema (compiled in)
// =============================================================================

static WIRE_SCHEMA: EmbeddedValidator =
    EmbeddedValidator::new(include_str!("../cc_wire_schema.json"), "cc-wire-format");

// =============================================================================
// Signal handling
// =============================================================================

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

fn install_signal_handlers() {
    unsafe {
        libc::signal(libc::SIGINT, signal_handler as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, signal_handler as *const () as libc::sighandler_t);
    }
}

extern "C" fn signal_handler(_sig: i32) {
    SHUTDOWN.store(true, Ordering::Relaxed);
}

// =============================================================================
// Extraction helpers (replicate Python bifrost.functions.pure.extract)
// =============================================================================

/// Extract session_id from metadata.user_id field.
///
/// Two formats: JSON object {"session_id": "..."} or prefixed string "session_<uuid>".
fn extract_session_id(value: &serde_json::Value) -> String {
    let user_id = value
        .pointer("/metadata/user_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if user_id.is_empty() {
        return "unknown".to_string();
    }

    // JSON object format: {"session_id": "...", ...}
    if user_id.starts_with('{') {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(user_id) {
            if let Some(sid) = parsed.get("session_id").and_then(|v| v.as_str()) {
                return sid.to_string();
            }
        }
        return "unknown".to_string();
    }

    // Prefixed string format: "session_<uuid>"
    if let Some(pos) = user_id.find("session_") {
        return user_id[pos + 8..].to_string();
    }

    "unknown".to_string()
}

/// Extract workspace path from system blocks containing "Primary working directory:".
fn extract_workspace_path(value: &serde_json::Value) -> Option<String> {
    let system = value.get("system")?;

    if let Some(text) = system.as_str() {
        return workspace_path_from_text(text);
    }

    if let Some(blocks) = system.as_array() {
        for block in blocks {
            if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                if let Some(path) = workspace_path_from_text(text) {
                    return Some(path);
                }
            }
        }
    }

    None
}

/// Extract the full path from text containing "Primary working directory:".
fn workspace_path_from_text(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(pos) = line.find("Primary working directory:") {
            let path = line[pos + 26..].trim().trim_end_matches('/');
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
}

// =============================================================================
// Workspace resolution
// =============================================================================

/// Resolve workspace name from system blocks or session memory.
///
/// Returns (workspace_name, just_resolved).
/// just_resolved is true when transitioning from unknown to known.
fn resolve_workspace(
    session_id: &str,
    value: &serde_json::Value,
    state: &ProxyState,
) -> (String, bool) {
    let workspace_path = extract_workspace_path(value);

    // Try to resolve path → workspace name via registry.db
    let workspace_from_blocks = workspace_path
        .as_deref()
        .and_then(|p| workspace_registry::resolve_workspace_from_path(p).ok().flatten());

    let mut cache = state.session_cache.lock().unwrap_or_else(|e| e.into_inner());
    let previous = cache.get(session_id).cloned().unwrap_or_else(|| "unknown".to_string());

    if let Some(ref ws) = workspace_from_blocks {
        cache.insert(session_id.to_string(), ws.clone());

        // Register session → workspace in persistent DB
        let _ = workspace_registry::register_session(session_id, ws);

        let just_resolved = previous == "unknown";
        return (ws.clone(), just_resolved);
    }

    // Compaction path: no system blocks, check in-memory cache then DB
    if previous != "unknown" {
        return (previous, false);
    }

    // Cache miss — try registry.db
    if let Ok(Some(ws)) = workspace_registry::workspace_from_session(session_id) {
        cache.insert(session_id.to_string(), ws.clone());
        return (ws, true);
    }

    ("unknown".to_string(), false)
}

// =============================================================================
// Interception pipeline
// =============================================================================

/// Run the full interception pipeline on a /v1/messages request body.
///
/// Returns Ok(Some(rewritten_bytes)) for compaction requests,
/// Ok(None) for non-compaction requests (forward original body).
fn intercept(
    body: &[u8],
    session_id: &str,
    workspace: &str,
    intercept_dir: &Path,
) -> Result<Option<Vec<u8>>, String> {
    let session_dir = intercept_dir.join("sessions").join(session_id);
    fs::create_dir_all(&session_dir)
        .map_err(|e| format!("mkdir {}: {e}", session_dir.display()))?;

    let json_str = std::str::from_utf8(body).map_err(|e| format!("invalid UTF-8: {e}"))?;

    // 1. Raw capture — UNCONDITIONAL, FIRST (before validation)
    let raw_path = session_dir.join("raw_session_log.jsonl");
    write_engine::append_line_fsync(&raw_path, json_str)?;

    // 2. Schema validation
    let validation = WIRE_SCHEMA
        .validate(json_str)
        .map_err(|e| format!("schema error: {e}"))?;

    if !validation.valid {
        // Fail-open: log but forward original bytes
        eprintln!(
            "bifrost_proxy: CC wire format changed — schema validation failed:\n{}",
            validation.message
        );
        return Ok(None);
    }

    let mut value = validation
        .data
        .ok_or("schema validated but data was missing")?;

    // 3. Classify
    let kind = match classify_exchange(&value) {
        None => return Ok(None),
        Some(k) => k,
    };

    // 4. Route
    match kind {
        ExchangeKind::Main => {
            session_io::append_exchange(&session_dir, &value)?;
            Ok(None)
        }
        ExchangeKind::Subagent => {
            session_io::append_subagent(&session_dir, &value)?;
            Ok(None)
        }
        ExchangeKind::Compaction => {
            session_io::record_compaction(&session_dir, &value, workspace)?;
            inject_compaction_system_block(&mut value)?;
            let output = serde_json::to_vec(&value)
                .map_err(|e| format!("serialize rewritten JSON: {e}"))?;
            Ok(Some(output))
        }
    }
}

// =============================================================================
// Workspace symlinks
// =============================================================================

fn create_workspace_symlinks(
    session_id: &str,
    workspace: &str,
    intercept_dir: &Path,
) {
    let workspace_dir = intercept_dir.join("traffic").join(workspace);
    if let Err(e) = fs::create_dir_all(&workspace_dir) {
        eprintln!("bifrost_proxy: mkdir traffic/{workspace}: {e}");
        return;
    }

    let link_path = workspace_dir.join(session_id);
    let target = PathBuf::from("../..").join("sessions").join(session_id);

    if link_path.is_symlink() {
        let _ = fs::remove_file(&link_path);
    } else if link_path.exists() {
        eprintln!(
            "bifrost_proxy: skipping symlink — real path exists at {}",
            link_path.display()
        );
        return;
    }

    if let Err(e) = unix_fs::symlink(&target, &link_path) {
        eprintln!("bifrost_proxy: symlink {}: {e}", link_path.display());
    }
}

// =============================================================================
// Watcher spawning
// =============================================================================

fn spawn_watcher(
    session_id: &str,
    workspace: &str,
    state: &ProxyState,
) {
    let mut watchers = state.watchers.lock().unwrap_or_else(|e| e.into_inner());

    // Check if existing watcher is alive
    if let Some(child) = watchers.get_mut(session_id) {
        match child.try_wait() {
            Ok(None) => return, // still running
            _ => { watchers.remove(session_id); }
        }
    }

    let jsonl_path = state
        .intercept_dir
        .join("sessions")
        .join(session_id)
        .join("main_exchange_log.jsonl");

    match Command::new(&state.watcher_binary)
        .args([
            "--watch",
            "--jsonl-path",
            &jsonl_path.to_string_lossy(),
            "--workspace",
            workspace,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => {
            eprintln!(
                "bifrost_proxy: watcher spawned: {workspace}/{session_id} (PID {})",
                child.id()
            );
            watchers.insert(session_id.to_string(), child);
        }
        Err(e) => {
            eprintln!("bifrost_proxy: watcher spawn failed: {e}");
        }
    }
}

// =============================================================================
// HTTP handler — gate + dispatch
// =============================================================================

async fn handle_request(
    request: Request<Incoming>,
    state: Arc<ProxyState>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let (parts, body) = request.into_parts();
    let body_bytes = body.collect().await?.to_bytes();

    let is_messages =
        parts.uri.path().starts_with("/v1/messages") && parts.method == Method::POST;

    let forward_body = if is_messages && !body_bytes.is_empty() {
        intercept_messages_request(&body_bytes, &state)
    } else {
        body_bytes
    };

    forward_upstream(&parts, forward_body, &state.upstream, &state.http_client).await
}

// =============================================================================
// Messages interception — identity + pipeline + side effects
// =============================================================================

/// Run interception pipeline on a /v1/messages request body.
/// Returns the body to forward (original or rewritten for compactions).
fn intercept_messages_request(body_bytes: &Bytes, state: &ProxyState) -> Bytes {
    let body_ref = body_bytes.as_ref();

    let maybe_value = serde_json::from_slice::<serde_json::Value>(body_ref).ok();

    let (session_id, workspace, just_resolved) = match &maybe_value {
        Some(value) => {
            let sid = extract_session_id(value);
            let (ws, jr) = resolve_workspace(&sid, value, state);
            (sid, ws, jr)
        }
        None => {
            save_unparseable_request(body_ref, &state.intercept_dir);
            return Bytes::copy_from_slice(body_ref);
        }
    };

    let rewritten = if session_id != "unknown" {
        match intercept(body_ref, &session_id, &workspace, &state.intercept_dir) {
            Ok(rw) => rw,
            Err(e) => {
                eprintln!("bifrost_proxy: intercept error: {e}");
                None
            }
        }
    } else {
        None
    };

    if just_resolved && workspace != "unknown" {
        create_workspace_symlinks(&session_id, &workspace, &state.intercept_dir);
    }

    if session_id != "unknown" && workspace != "unknown" {
        spawn_watcher(&session_id, &workspace, state);
    }

    match rewritten {
        Some(bytes) => Bytes::from(bytes),
        None => Bytes::copy_from_slice(body_ref),
    }
}

fn save_unparseable_request(body: &[u8], intercept_dir: &Path) {
    let unresolved_dir = intercept_dir.join("sessions").join("unresolved");
    let _ = fs::create_dir_all(&unresolved_dir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file_path = unresolved_dir.join(format!("{ts}_req.raw"));
    let _ = fs::write(&file_path, body);
    eprintln!(
        "bifrost_proxy: unparseable request saved to {}",
        file_path.display()
    );
}

// =============================================================================
// Upstream forwarding
// =============================================================================

fn bad_gateway(message: String) -> Result<Response<Full<Bytes>>, hyper::Error> {
    Ok(Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .body(Full::new(Bytes::from(message)))
        .unwrap_or_else(|_| Response::new(Full::new(Bytes::new()))))
}

async fn forward_upstream(
    original_parts: &hyper::http::request::Parts,
    body: Bytes,
    upstream_addr: &str,
    http_client: &HttpClient,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let upstream_uri = format!("http://{}{}", upstream_addr, original_parts.uri.path());
    let uri: hyper::Uri = match upstream_uri.parse() {
        Ok(u) => u,
        Err(e) => return bad_gateway(format!("bad upstream URI: {e}")),
    };

    let mut builder = Request::builder()
        .method(&original_parts.method)
        .uri(uri);

    for (key, val) in &original_parts.headers {
        if key != hyper::header::HOST {
            builder = builder.header(key, val);
        }
    }

    let upstream_request = match builder.body(Full::new(body)) {
        Ok(r) => r,
        Err(e) => return bad_gateway(format!("request build error: {e}")),
    };

    match http_client.request(upstream_request).await {
        Ok(upstream_response) => {
            let (resp_parts, resp_body) = upstream_response.into_parts();
            let resp_bytes = match resp_body.collect().await {
                Ok(b) => b.to_bytes(),
                Err(e) => {
                    eprintln!("bifrost_proxy: read upstream response: {e}");
                    Bytes::from(format!("upstream read error: {e}"))
                }
            };

            let mut builder = Response::builder().status(resp_parts.status);
            for (key, val) in &resp_parts.headers {
                builder = builder.header(key, val);
            }
            Ok(builder
                .body(Full::new(resp_bytes))
                .unwrap_or_else(|_| Response::new(Full::new(Bytes::new()))))
        }
        Err(e) => {
            eprintln!("bifrost_proxy: upstream error: {e}");
            bad_gateway(format!("upstream error: {e}"))
        }
    }
}

// =============================================================================
// Main
// =============================================================================

fn parse_args() -> Result<Args, clap::Error> {
    Args::try_parse()
}

fn run(args: Args) -> Result<(), String> {
    install_signal_handlers();

    let intercept_dir = args.intercept_dir.unwrap_or_else(|| {
        write_engine::ai_home().join("intercept")
    });

    // Ensure base directories exist
    fs::create_dir_all(intercept_dir.join("sessions"))
        .map_err(|e| format!("mkdir sessions: {e}"))?;
    fs::create_dir_all(intercept_dir.join("sessions/unresolved"))
        .map_err(|e| format!("mkdir unresolved: {e}"))?;
    fs::create_dir_all(intercept_dir.join("traffic"))
        .map_err(|e| format!("mkdir traffic: {e}"))?;

    let http_client = Client::builder(TokioExecutor::new()).build_http();

    let state = Arc::new(ProxyState {
        upstream: args.upstream,
        intercept_dir,
        watcher_binary: args.watcher_binary,
        http_client,
        session_cache: Mutex::new(HashMap::new()),
        watchers: Mutex::new(HashMap::new()),
    });

    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("tokio runtime: {e}"))?;

    rt.block_on(run_server(args.listen, state))
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            e.print().ok();
            std::process::exit(if e.use_stderr() { 2 } else { 0 });
        }
    };

    if let Err(e) = run(args) {
        eprintln!("bifrost_proxy: {e}");
        std::process::exit(1);
    }
}

async fn run_server(addr: SocketAddr, state: Arc<ProxyState>) -> Result<(), String> {
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| format!("bind {addr}: {e}"))?;

    eprintln!("bifrost_proxy: listening on {addr}, upstream {}", state.upstream);

    loop {
        if SHUTDOWN.load(Ordering::Relaxed) {
            eprintln!("bifrost_proxy: shutting down");
            break;
        }

        let accept = tokio::select! {
            result = listener.accept() => result,
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => continue,
        };

        let (stream, _peer) = match accept {
            Ok(s) => s,
            Err(e) => {
                eprintln!("bifrost_proxy: accept: {e}");
                continue;
            }
        };

        let conn_state = Arc::clone(&state);
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let svc = service_fn(move |request| {
                let request_state = Arc::clone(&conn_state);
                handle_request(request, request_state)
            });

            if let Err(e) = http1::Builder::new()
                .serve_connection(io, svc)
                .await
            {
                if !e.to_string().contains("connection closed") {
                    eprintln!("bifrost_proxy: connection error: {e}");
                }
            }
        });
    }

    // Cleanup: kill watcher processes
    let mut watchers = state.watchers.lock().unwrap_or_else(|e| e.into_inner());
    for (sid, child) in watchers.iter_mut() {
        let _ = child.kill();
        eprintln!("bifrost_proxy: killed watcher for {sid}");
    }
    watchers.clear();

    Ok(())
}
