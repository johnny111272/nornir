//! Persistent datagram logger.
//!
//! Listens on a Unix stream socket, appends every received datagram to a
//! daily-rotated JSONL log file. This is the permanent archive — every
//! datagram ever emitted by any nornir tool gets captured here, regardless
//! of whether Hlidskjalf is running.
//!
//! Usage:
//!     record_datagrams
//!     record_datagrams --socket-path /tmp/ai_logger.sock --log-dir ~/.ai/intercept/datagrams
//!
//! Protocol: one JSON line per connection (connect, write line, disconnect).
//! Same protocol as datagram's try_emit().

use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;

// =============================================================================
// Constants
// =============================================================================

const DEFAULT_SOCKET_PATH: &str = datagram_io::SOCKET_PATH;
const READ_TIMEOUT: Duration = Duration::from_secs(2);

// =============================================================================
// Types
// =============================================================================

fn default_log_dir() -> PathBuf {
    write_engine::ai_home().join("intercept/datagrams")
}

/// Persistent datagram logger — listens on a Unix socket and writes daily JSONL logs.
#[derive(Debug, Parser)]
#[command(name = "record_datagrams")]
struct Args {
    /// Unix socket path to listen on
    #[arg(long, default_value = DEFAULT_SOCKET_PATH)]
    socket_path: PathBuf,

    /// Directory for daily JSONL log files
    #[arg(long, default_value_os_t = default_log_dir())]
    log_dir: PathBuf,
}

// =============================================================================
// Log file management
// =============================================================================

/// Today's date as YYYY-MM-DD (UTC).
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    time_core::civil_date(secs)
}

/// Path to today's log file.
fn log_path(log_dir: &Path) -> PathBuf {
    log_dir.join(format!("datagrams_{}-Z.jsonl", today()))
}

// =============================================================================
// Socket management
// =============================================================================

/// Remove stale socket file if it exists.
fn cleanup_socket(path: &Path) {
    if path.exists() {
        if let Err(e) = fs::remove_file(path) {
            eprintln!("warning: failed to remove stale socket {}: {e}", path.display());
        }
    }
}

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

static SHUTDOWN_FLAG: AtomicBool = AtomicBool::new(false);
static SHUTDOWN_SOCKET_PATH: OnceLock<PathBuf> = OnceLock::new();

extern "C" fn shutdown_handler(_sig: i32) {
    SHUTDOWN_FLAG.store(true, Ordering::Relaxed);
}

/// Store socket path and register SIGINT/SIGTERM handlers.
fn install_shutdown_handler(socket_path: PathBuf) {
    let _ = SHUTDOWN_SOCKET_PATH.set(socket_path);
    unsafe {
        libc::signal(libc::SIGINT, shutdown_handler as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, shutdown_handler as *const () as libc::sighandler_t);
    }
}

// =============================================================================
// Main loop
// =============================================================================

fn is_transient_io_error(error: &std::io::Error) -> bool {
    matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)
}

fn process_stream(stream: std::os::unix::net::UnixStream, log_dir: &Path, count: &mut u64) {
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let text = match line {
            Ok(text) => text,
            Err(e) => {
                if !is_transient_io_error(&e) {
                    eprintln!("read error: {e}");
                }
                break;
            }
        };
        if text.trim().is_empty() {
            continue;
        }
        match write_engine::append_line_fsync(&log_path(log_dir), &text) {
            Err(e) => eprintln!("log error: {e}"),
            Ok(()) => {
                *count += 1;
                if *count % 100 == 0 {
                    eprintln!("recorded {} datagrams", *count);
                }
            }
        }
    }
}

fn run(args: Args) -> Result<(), String> {
    fs::create_dir_all(&args.log_dir)
        .map_err(|e| format!("mkdir {}: {e}", args.log_dir.display()))?;

    cleanup_socket(&args.socket_path);

    let listener = UnixListener::bind(&args.socket_path)
        .map_err(|e| format!("bind {}: {e}", args.socket_path.display()))?;

    eprintln!("record_datagrams listening on {}", args.socket_path.display());
    eprintln!("logging to {}", args.log_dir.display());

    let log_dir = args.log_dir;
    install_shutdown_handler(args.socket_path);

    listener
        .set_nonblocking(true)
        .map_err(|e| format!("set_nonblocking: {e}"))?;

    let mut count: u64 = 0;

    loop {
        if SHUTDOWN_FLAG.load(Ordering::Relaxed) {
            break;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
                process_stream(stream, &log_dir, &mut count);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }

    if let Some(path) = SHUTDOWN_SOCKET_PATH.get() {
        cleanup_socket(path);
    }
    eprintln!("shutdown after {count} datagrams");
    Ok(())
}

// =============================================================================
// Entry point
// =============================================================================

fn main() {
    let args = Args::parse();

    match run(args) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // arg parsing (clap)
    // =========================================================================

    #[test]
    fn parse_args_defaults() {
        let args = Args::try_parse_from(["record_datagrams"]).unwrap();
        assert_eq!(args.socket_path, PathBuf::from(DEFAULT_SOCKET_PATH));
    }

    #[test]
    fn parse_args_custom_socket() {
        let args = Args::try_parse_from(["record_datagrams", "--socket-path", "/tmp/custom.sock"]).unwrap();
        assert_eq!(args.socket_path, PathBuf::from("/tmp/custom.sock"));
    }

    #[test]
    fn parse_args_custom_log_dir() {
        let args = Args::try_parse_from(["record_datagrams", "--log-dir", "/tmp/logs"]).unwrap();
        assert_eq!(args.log_dir, PathBuf::from("/tmp/logs"));
    }

    #[test]
    fn parse_args_unknown_flag() {
        let result = Args::try_parse_from(["record_datagrams", "--banana"]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--banana"), "should mention unknown flag: {err}");
    }

    #[test]
    fn parse_args_socket_path_missing_value() {
        let result = Args::try_parse_from(["record_datagrams", "--socket-path"]);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("socket-path"), "should mention flag: {err}");
    }

    // =========================================================================
    // today()
    // =========================================================================

    #[test]
    fn today_format() {
        let d = today();
        assert_eq!(d.len(), 10, "should be YYYY-MM-DD: {d}");
        assert_eq!(&d[4..5], "-");
        assert_eq!(&d[7..8], "-");
    }

    // =========================================================================
    // log_path
    // =========================================================================

    #[test]
    fn log_path_includes_date() {
        let dir = Path::new("/tmp/logs");
        let path = log_path(dir);
        let filename = path.file_name().unwrap().to_string_lossy();
        assert!(filename.starts_with("datagrams_"), "filename: {filename}");
        assert!(filename.ends_with("-Z.jsonl"), "filename: {filename}");
    }

    // =========================================================================
    // append via write_engine
    // =========================================================================

    #[test]
    fn append_creates_and_appends_via_write_engine() {
        let dir = std::env::temp_dir().join("rd_test_append");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let path = log_path(&dir);
        write_engine::append_line_fsync(&path, r#"{"test": 1}"#).unwrap();
        write_engine::append_line_fsync(&path, r#"{"test": 2}"#).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], r#"{"test": 1}"#);
        assert_eq!(lines[1], r#"{"test": 2}"#);

        let _ = fs::remove_dir_all(&dir);
    }
}
