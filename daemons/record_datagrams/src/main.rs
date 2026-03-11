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

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

// =============================================================================
// Constants
// =============================================================================

const DEFAULT_SOCKET_PATH: &str = datagram::SOCKET_PATH;
const DEFAULT_LOG_DIR: &str = ".ai/intercept/datagrams";
const READ_TIMEOUT: Duration = Duration::from_secs(2);

// =============================================================================
// Types
// =============================================================================

#[derive(Debug)]
struct Config {
    socket_path: PathBuf,
    log_dir: PathBuf,
}

// =============================================================================
// Arg parsing
// =============================================================================

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut socket_path = None;
    let mut log_dir = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--socket-path" => {
                i += 1;
                socket_path = Some(PathBuf::from(
                    args.get(i).ok_or("--socket-path requires a value")?,
                ));
            }
            "--log-dir" => {
                i += 1;
                log_dir = Some(PathBuf::from(
                    args.get(i).ok_or("--log-dir requires a value")?,
                ));
            }
            other => {
                return Err(format!("unknown argument: {other}"));
            }
        }
        i += 1;
    }

    let socket_path = socket_path.unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET_PATH));

    let log_dir = log_dir.unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(DEFAULT_LOG_DIR)
    });

    Ok(Config {
        socket_path,
        log_dir,
    })
}

fn print_usage() {
    eprintln!(
        "Usage: record_datagrams [--socket-path <path>] [--log-dir <path>]"
    );
}

// =============================================================================
// Log file management
// =============================================================================

/// Today's date as YYYY-MM-DD.
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let days = secs / 86400;
    // Civil date from days since epoch (simplified, handles 1970-2099)
    let mut y = 1970i64;
    let mut remaining = days as i64;
    loop {
        let year_days = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
        if remaining < year_days {
            break;
        }
        remaining -= year_days;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    let mut m = 0usize;
    for md in &month_days {
        if remaining < *md as i64 {
            break;
        }
        remaining -= *md as i64;
        m += 1;
    }
    format!("{y:04}-{:02}-{:02}", m + 1, remaining + 1)
}

/// Path to today's log file.
fn log_path(log_dir: &Path) -> PathBuf {
    log_dir.join(format!("datagrams_{}.jsonl", today()))
}

/// Append a line to the log file.
fn append_log(log_dir: &Path, line: &str) -> Result<(), String> {
    let path = log_path(log_dir);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    file.write_all(line.as_bytes())
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    if !line.ends_with('\n') {
        file.write_all(b"\n")
            .map_err(|e| format!("write newline {}: {e}", path.display()))?;
    }
    file.flush()
        .map_err(|e| format!("flush {}: {e}", path.display()))?;
    Ok(())
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

/// Install signal handler for clean shutdown.
fn install_shutdown_handler(socket_path: PathBuf) {
    // Store path for cleanup on ctrl-c
    ctrlc_cleanup(socket_path);
}

/// Register cleanup on SIGINT/SIGTERM using atexit-style approach.
/// On ctrl-c, remove the socket file then exit.
fn ctrlc_cleanup(socket_path: PathBuf) {
    // Use a simple atomic flag — on signal, set flag, main loop checks it
    unsafe {
        SHUTDOWN_SOCKET_PATH = Some(socket_path);
    }
    register_signal(SIGINT, shutdown_handler);
    register_signal(SIGTERM, shutdown_handler);
}

// Signal constants (macOS/Linux compatible)
const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;

extern "C" {
    fn signal(sig: i32, handler: usize) -> usize;
}

fn register_signal(sig: i32, handler: extern "C" fn(i32)) {
    unsafe {
        signal(sig, handler as *const () as usize);
    }
}

static mut SHUTDOWN_SOCKET_PATH: Option<PathBuf> = None;
static mut SHUTDOWN_FLAG: bool = false;

extern "C" fn shutdown_handler(_sig: i32) {
    unsafe {
        SHUTDOWN_FLAG = true;
        if let Some(ref path) = SHUTDOWN_SOCKET_PATH {
            let _ = fs::remove_file(path);
        }
    }
    std::process::exit(0);
}

fn should_shutdown() -> bool {
    unsafe { SHUTDOWN_FLAG }
}

// =============================================================================
// Main loop
// =============================================================================

fn run(config: &Config) -> Result<(), String> {
    fs::create_dir_all(&config.log_dir)
        .map_err(|e| format!("mkdir {}: {e}", config.log_dir.display()))?;

    cleanup_socket(&config.socket_path);
    install_shutdown_handler(config.socket_path.clone());

    let listener = UnixListener::bind(&config.socket_path)
        .map_err(|e| format!("bind {}: {e}", config.socket_path.display()))?;

    eprintln!(
        "record_datagrams listening on {}",
        config.socket_path.display()
    );
    eprintln!("logging to {}", config.log_dir.display());

    let mut count: u64 = 0;

    for stream in listener.incoming() {
        if should_shutdown() {
            break;
        }

        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("accept error: {e}");
                continue;
            }
        };

        // Best-effort timeout — some macOS socket configs reject this
        let _ = stream.set_read_timeout(Some(READ_TIMEOUT));

        let reader = BufReader::new(stream);
        for line in reader.lines() {
            match line {
                Ok(line) if !line.trim().is_empty() => {
                    if let Err(e) = append_log(&config.log_dir, &line) {
                        eprintln!("log error: {e}");
                    } else {
                        count += 1;
                        if count % 100 == 0 {
                            eprintln!("recorded {count} datagrams");
                        }
                    }
                }
                Ok(_) => {} // empty line, skip
                Err(e) => {
                    // Read timeout or connection reset — normal
                    if e.kind() != std::io::ErrorKind::WouldBlock
                        && e.kind() != std::io::ErrorKind::TimedOut
                    {
                        eprintln!("read error: {e}");
                    }
                    break;
                }
            }
        }
    }

    cleanup_socket(&config.socket_path);
    Ok(())
}

// =============================================================================
// Entry point
// =============================================================================

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let config = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            print_usage();
            std::process::exit(2);
        }
    };

    match run(&config) {
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

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // =========================================================================
    // parse_args
    // =========================================================================

    #[test]
    fn parse_args_defaults() {
        let a = args(&[]);
        let config = parse_args(&a).unwrap();
        assert_eq!(config.socket_path, PathBuf::from(DEFAULT_SOCKET_PATH));
    }

    #[test]
    fn parse_args_custom_socket() {
        let a = args(&["--socket-path", "/tmp/custom.sock"]);
        let config = parse_args(&a).unwrap();
        assert_eq!(config.socket_path, PathBuf::from("/tmp/custom.sock"));
    }

    #[test]
    fn parse_args_custom_log_dir() {
        let a = args(&["--log-dir", "/tmp/logs"]);
        let config = parse_args(&a).unwrap();
        assert_eq!(config.log_dir, PathBuf::from("/tmp/logs"));
    }

    #[test]
    fn parse_args_unknown_flag() {
        let a = args(&["--banana"]);
        let err = parse_args(&a).unwrap_err();
        assert!(err.contains("--banana"), "should mention unknown flag: {err}");
    }

    #[test]
    fn parse_args_socket_path_missing_value() {
        let a = args(&["--socket-path"]);
        let err = parse_args(&a).unwrap_err();
        assert!(err.contains("--socket-path"), "should mention flag: {err}");
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
        assert!(filename.ends_with(".jsonl"), "filename: {filename}");
    }

    // =========================================================================
    // append_log
    // =========================================================================

    #[test]
    fn append_log_creates_and_appends() {
        let dir = std::env::temp_dir().join("rd_test_append");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        append_log(&dir, r#"{"test": 1}"#).unwrap();
        append_log(&dir, r#"{"test": 2}"#).unwrap();

        let path = log_path(&dir);
        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], r#"{"test": 1}"#);
        assert_eq!(lines[1], r#"{"test": 2}"#);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_log_adds_newline_if_missing() {
        let dir = std::env::temp_dir().join("rd_test_newline");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        append_log(&dir, "line without newline").unwrap();
        append_log(&dir, "line with newline\n").unwrap();

        let path = log_path(&dir);
        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);

        let _ = fs::remove_dir_all(&dir);
    }
}
