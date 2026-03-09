//! Saga: quality truth recorder.
//!
//! Runs external quality tools (ruff, basedpyright, gleipnir) on source files,
//! normalizes their output to a common Issue format, and writes .qa sidecar files.
//!
//! Saga records ALL issues — no filtering. Filtering is Syn's job.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

// =============================================================================
// Data structures — the .qa sidecar format
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub tool: String,
    pub code: String,
    pub severity: String,
    pub line: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub fixable: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub signal: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub direction: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub canary: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SanityReport {
    pub file: String,
    #[serde(default)]
    pub relative_path: String,
    #[serde(default)]
    pub content_hash: String,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub elapsed_ms: u64,
    #[serde(default)]
    pub issues: Vec<Issue>,
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub by_tool: HashMap<String, usize>,
    #[serde(default)]
    pub by_severity: HashMap<String, usize>,
    #[serde(default)]
    pub by_category: HashMap<String, usize>,
}

// =============================================================================
// Embedded configs — compiled into the binary, written to tmp at runtime
// =============================================================================

static RUFF_TOML: &str = include_str!("../ruff.toml");
static PYRIGHTCONFIG_JSON: &str = include_str!("../pyrightconfig.json");

fn saga_tmp() -> PathBuf {
    std::env::temp_dir().join("saga")
}

fn ensure_embedded_config(filename: &str, content: &str) -> PathBuf {
    let dir = saga_tmp();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(filename);
    let _ = std::fs::write(&path, content);
    path
}

fn ruff_config() -> PathBuf {
    ensure_embedded_config("ruff.toml", RUFF_TOML)
}

fn pyright_config() -> PathBuf {
    ensure_embedded_config("pyrightconfig.json", PYRIGHTCONFIG_JSON)
}

// =============================================================================
// Runners — gleipnir (native library), ruff + basedpyright (subprocess)
// =============================================================================

/// Run gleipnir guardrail checks on a file (native — no subprocess).
pub fn run_gleipnir(file_path: &Path) -> Vec<Issue> {
    let source = match std::fs::read(file_path) {
        Ok(bytes) => bytes,
        Err(_) => return Vec::new(),
    };
    let file_path_str = file_path.to_string_lossy();

    let violations = gleipnir_core::run_checks(&file_path_str, &source, None);

    violations
        .into_iter()
        .map(|v| Issue {
            tool: "gleipnir".into(),
            code: v.check_name,
            severity: match v.severity {
                gleipnir_core::Severity::Blocked => "blocked",
                gleipnir_core::Severity::Error => "error",
                gleipnir_core::Severity::Warning => "warning",
            }
            .into(),
            line: v.line,
            column: None,
            message: v.message,
            category: "structure".into(),
            fixable: false,
            signal: v.signal,
            direction: v.direction,
            canary: v.canary,
        })
        .collect()
}

/// Run ruff linter on a file using Saga's embedded config.
pub fn run_ruff(file_path: &Path) -> Vec<Issue> {
    let config = ruff_config();
    let mut cmd = Command::new("ruff");
    cmd.arg("check")
        .arg("--output-format=json")
        .arg("--config").arg(config.as_os_str())
        .arg(file_path.as_os_str());

    let output = match cmd.output() {
        Ok(out) => out,
        Err(_) => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.is_empty() {
        return Vec::new();
    }

    let items: Vec<serde_json::Value> = match serde_json::from_str(&stdout) {
        Ok(val) => val,
        Err(_) => return Vec::new(),
    };

    items
        .iter()
        .map(|item| {
            let code = item["code"].as_str().unwrap_or("").to_string();
            Issue {
                tool: "ruff".into(),
                category: categorize_ruff(&code).into(),
                code,
                severity: "warning".into(),
                line: item["location"]["row"].as_u64().unwrap_or(1) as usize,
                column: item["location"]["column"].as_u64().map(|c| c as usize),
                message: item["message"].as_str().unwrap_or("").into(),
                fixable: item.get("fix").is_some() && !item["fix"].is_null(),
                signal: String::new(),
                direction: String::new(),
                canary: String::new(),
            }
        })
        .collect()
}

/// Run basedpyright type checker on a file using Saga's embedded config.
pub fn run_basedpyright(file_path: &Path) -> Vec<Issue> {
    let config = pyright_config();
    let mut cmd = Command::new("basedpyright");
    cmd.arg("--outputjson")
        .arg("--project").arg(config.as_os_str())
        .arg(file_path.as_os_str());

    let output = match cmd.output() {
        Ok(out) => out,
        Err(_) => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.is_empty() {
        return Vec::new();
    }

    let data: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(val) => val,
        Err(_) => return Vec::new(),
    };

    let diags = match data["generalDiagnostics"].as_array() {
        Some(arr) => arr,
        None => return Vec::new(),
    };

    diags
        .iter()
        .map(|diag| {
            let severity = match diag["severity"].as_str().unwrap_or("error") {
                "error" => "error",
                "warning" => "warning",
                "information" => "info",
                _ => "error",
            };
            Issue {
                tool: "basedpyright".into(),
                code: diag["rule"].as_str().unwrap_or("type-error").into(),
                severity: severity.into(),
                line: diag["range"]["start"]["line"].as_u64().unwrap_or(0) as usize + 1,
                column: Some(
                    diag["range"]["start"]["character"].as_u64().unwrap_or(0) as usize + 1,
                ),
                message: diag["message"].as_str().unwrap_or("").into(),
                category: "type".into(),
                fixable: false,
                signal: String::new(),
                direction: String::new(),
                canary: String::new(),
            }
        })
        .collect()
}

/// Run all Python quality tools on a file.
pub fn run_python_tools(file_path: &Path) -> Vec<Issue> {
    let mut issues = Vec::new();
    issues.extend(run_gleipnir(file_path));
    issues.extend(run_ruff(file_path));
    issues.extend(run_basedpyright(file_path));
    issues
}

// =============================================================================
// Report generation
// =============================================================================

/// Generate a SanityReport for a file.
pub fn generate_report(file_path: &Path, project_root: Option<&Path>) -> SanityReport {
    let file_path = file_path.canonicalize().unwrap_or_else(|_| file_path.to_path_buf());

    let start = Instant::now();
    let issues = run_python_tools(&file_path);
    let elapsed_ms = start.elapsed().as_millis() as u64;

    let relative_path = project_root
        .and_then(|root| file_path.strip_prefix(root).ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.file_name().unwrap_or_default().to_string_lossy().to_string());

    let content_hash = hash_file(&file_path).unwrap_or_default();
    let total = issues.len();

    let mut by_tool: HashMap<String, usize> = HashMap::new();
    let mut by_severity: HashMap<String, usize> = HashMap::new();
    let mut by_category: HashMap<String, usize> = HashMap::new();
    for issue in &issues {
        *by_tool.entry(issue.tool.clone()).or_default() += 1;
        *by_severity.entry(issue.severity.clone()).or_default() += 1;
        *by_category.entry(issue.category.clone()).or_default() += 1;
    }

    SanityReport {
        file: file_path.to_string_lossy().to_string(),
        relative_path,
        content_hash,
        generated_at: chrono_now(),
        elapsed_ms,
        issues,
        total,
        by_tool,
        by_severity,
        by_category,
    }
}

/// Generate a SanityReport from content string (for stdin / hook usage).
/// Writes content to a temp file so external tools can process it.
pub fn generate_report_from_content(
    file_path: &Path,
    content: &str,
    project_root: Option<&Path>,
) -> SanityReport {
    let tmp_dir = saga_tmp();
    let _ = std::fs::create_dir_all(&tmp_dir);

    let file_name = file_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let tmp_file = tmp_dir.join(file_name.as_ref());

    if let Err(e) = std::fs::write(&tmp_file, content) {
        eprintln!("[saga] failed to write temp file: {}", e);
        return SanityReport {
            file: file_path.to_string_lossy().to_string(),
            ..Default::default()
        };
    }

    let start = Instant::now();
    let issues = run_python_tools(&tmp_file);
    let elapsed_ms = start.elapsed().as_millis() as u64;

    let _ = std::fs::remove_file(&tmp_file);

    let relative_path = project_root
        .and_then(|root| file_path.strip_prefix(root).ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| file_name.to_string());

    let content_hash = hash_content(content);
    let total = issues.len();

    let mut by_tool: HashMap<String, usize> = HashMap::new();
    let mut by_severity: HashMap<String, usize> = HashMap::new();
    let mut by_category: HashMap<String, usize> = HashMap::new();
    for issue in &issues {
        *by_tool.entry(issue.tool.clone()).or_default() += 1;
        *by_severity.entry(issue.severity.clone()).or_default() += 1;
        *by_category.entry(issue.category.clone()).or_default() += 1;
    }

    SanityReport {
        file: file_path.to_string_lossy().to_string(),
        relative_path,
        content_hash,
        generated_at: chrono_now(),
        elapsed_ms,
        issues,
        total,
        by_tool,
        by_severity,
        by_category,
    }
}

// =============================================================================
// Sidecar I/O
// =============================================================================

/// Get the .qa sidecar path for a source file.
pub fn qa_path(file_path: &Path) -> PathBuf {
    let name = file_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    file_path.parent().unwrap_or(Path::new(".")).join(format!(".{}.qa", name))
}

/// Reverse of qa_path: given `.foo.py.qa`, returns `foo.py` path in the same directory.
/// Returns None if the filename doesn't match the sidecar pattern (.*.qa).
pub fn source_path_from_qa(qa_path: &Path) -> Option<PathBuf> {
    let name = qa_path.file_name()?.to_string_lossy();
    // Pattern: .{source_name}.qa — minimum length is 5 (e.g. ".x.qa")
    if name.len() < 5 || !name.starts_with('.') || !name.ends_with(".qa") {
        return None;
    }
    // Strip leading '.' and trailing '.qa'
    let source_name = &name[1..name.len() - 3];
    if source_name.is_empty() {
        return None;
    }
    Some(qa_path.parent().unwrap_or(Path::new(".")).join(source_name))
}

/// Remove .qa sidecars whose source files no longer exist.
/// Returns list of removed sidecar paths.
pub fn remove_orphaned_sidecars(dir: &Path) -> Vec<PathBuf> {
    let qa_files = find_files(dir, &[], &|name| name.ends_with(".qa") && name.starts_with('.'));
    let mut removed = Vec::new();
    for qa_file in qa_files {
        if let Some(source) = source_path_from_qa(&qa_file) {
            if !source.exists() {
                if std::fs::remove_file(&qa_file).is_ok() {
                    removed.push(qa_file);
                }
            }
        }
    }
    removed
}

/// Write a SanityReport to its .qa sidecar.
pub fn save_sidecar(report: &SanityReport) -> std::io::Result<PathBuf> {
    let path = qa_path(Path::new(&report.file));
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(&path, json)?;
    Ok(path)
}

/// Load a SanityReport from a .qa sidecar.
pub fn load_sidecar(file_path: &Path) -> Option<SanityReport> {
    let path = qa_path(file_path);
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Load a SanityReport directly from a .qa file path.
pub fn load_qa_file(qa_file: &Path) -> Option<SanityReport> {
    let content = std::fs::read_to_string(qa_file).ok()?;
    serde_json::from_str(&content).ok()
}

// =============================================================================
// Directory walking
// =============================================================================

/// Directories always skipped during recursive file discovery.
const SKIP_DIRS: &[&str] = &[
    "__pycache__",
    "node_modules",
    ".venv",
    "venv",
];

/// Recursively collect files matching a predicate, skipping junk directories.
///
/// `extra_skip` allows callers to skip additional directory names.
pub fn walk_files(
    dir: &Path,
    extra_skip: &[&str],
    predicate: &dyn Fn(&str) -> bool,
    results: &mut Vec<PathBuf>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            if name.starts_with('.')
                || SKIP_DIRS.contains(&name.as_str())
                || extra_skip.contains(&name.as_str())
            {
                continue;
            }
            walk_files(&path, extra_skip, predicate, results);
        } else if predicate(&name) {
            results.push(path);
        }
    }
}

/// Collect files matching a predicate under `dir`, sorted.
pub fn find_files(dir: &Path, extra_skip: &[&str], predicate: &dyn Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk_files(dir, extra_skip, predicate, &mut files);
    files.sort();
    files
}

// =============================================================================
// Helpers
// =============================================================================

fn hash_content(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn hash_file(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let bytes_read = file.read(&mut buf).ok()?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buf[..bytes_read]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

fn chrono_now() -> String {
    // Simple ISO 8601 without external crate
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}Z", duration.as_secs())
}

fn categorize_ruff(code: &str) -> &'static str {
    // Try prefix match, longest first
    let prefixes: &[(&str, &str)] = &[
        ("PERF", "complexity"),
        ("ASYNC", "lint"),
        ("ANN", "type"),
        ("COM", "style"),
        ("CPY", "style"),
        ("DTZ", "lint"),
        ("T10", "lint"),
        ("T20", "lint"),
        ("EM", "style"),
        ("EXE", "lint"),
        ("FA", "type"),
        ("ISC", "style"),
        ("ICN", "style"),
        ("LOG", "lint"),
        ("INP", "lint"),
        ("PIE", "lint"),
        ("PYI", "type"),
        ("PT", "lint"),
        ("RSE", "lint"),
        ("RET", "lint"),
        ("SLF", "lint"),
        ("SIM", "lint"),
        ("TID", "style"),
        ("TCH", "type"),
        ("ARG", "lint"),
        ("PTH", "lint"),
        ("TD", "style"),
        ("FIX", "style"),
        ("ERA", "lint"),
        ("PD", "lint"),
        ("PGH", "lint"),
        ("PL", "lint"),
        ("TRY", "lint"),
        ("FLY", "lint"),
        ("NPY", "lint"),
        ("FURB", "lint"),
        ("RUF", "lint"),
        ("UP", "style"),
        ("YTT", "lint"),
        ("BLE", "lint"),
        ("FBT", "lint"),
        ("C4", "lint"),
        ("DJ", "lint"),
        ("SLOT", "lint"),
        ("INT", "lint"),
        ("E", "style"),
        ("W", "style"),
        ("F", "lint"),
        ("C", "complexity"),
        ("I", "style"),
        ("N", "style"),
        ("D", "style"),
        ("S", "lint"),
        ("B", "lint"),
        ("A", "lint"),
        ("G", "style"),
        ("Q", "style"),
    ];

    for (prefix, category) in prefixes {
        if code.starts_with(prefix) {
            return category;
        }
    }
    "lint"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_path_roundtrip() {
        let source = Path::new("/some/dir/foo.py");
        let qa = qa_path(source);
        let recovered = source_path_from_qa(&qa).unwrap();
        assert_eq!(recovered, source);
    }

    #[test]
    fn source_path_rejects_non_sidecar() {
        assert!(source_path_from_qa(Path::new("foo.py")).is_none());
        assert!(source_path_from_qa(Path::new(".qa")).is_none());
        assert!(source_path_from_qa(Path::new("regular.txt")).is_none());
    }

    #[test]
    fn source_path_from_hidden_qa() {
        let qa = Path::new("/dir/.foo.py.qa");
        let source = source_path_from_qa(qa).unwrap();
        assert_eq!(source, Path::new("/dir/foo.py"));
    }

    // =========================================================================
    // qa_path — additional edge cases
    // =========================================================================

    #[test]
    fn qa_path_file_in_root_directory() {
        let source = Path::new("/foo.py");
        let qa = qa_path(source);
        assert_eq!(qa, PathBuf::from("/.foo.py.qa"));
    }

    #[test]
    fn qa_path_multiple_extensions() {
        let source = Path::new("/dir/test.spec.py");
        let qa = qa_path(source);
        assert_eq!(qa, PathBuf::from("/dir/.test.spec.py.qa"));
        // Roundtrip: recover original from sidecar
        let recovered = source_path_from_qa(&qa).unwrap();
        assert_eq!(recovered, source);
    }

    #[test]
    fn qa_path_no_extension() {
        let source = Path::new("/dir/Makefile");
        let qa = qa_path(source);
        assert_eq!(qa, PathBuf::from("/dir/.Makefile.qa"));
        let recovered = source_path_from_qa(&qa).unwrap();
        assert_eq!(recovered, source);
    }

    // =========================================================================
    // categorize_ruff — prefix mapping table
    // =========================================================================

    #[test]
    fn categorize_ruff_style_e() {
        assert_eq!(categorize_ruff("E501"), "style");
    }

    #[test]
    fn categorize_ruff_lint_f() {
        assert_eq!(categorize_ruff("F401"), "lint");
    }

    #[test]
    fn categorize_ruff_lint_s() {
        assert_eq!(categorize_ruff("S701"), "lint");
    }

    #[test]
    fn categorize_ruff_style_i() {
        assert_eq!(categorize_ruff("I001"), "style");
    }

    #[test]
    fn categorize_ruff_complexity_c() {
        assert_eq!(categorize_ruff("C901"), "complexity");
    }

    #[test]
    fn categorize_ruff_complexity_perf() {
        assert_eq!(categorize_ruff("PERF401"), "complexity");
    }

    #[test]
    fn categorize_ruff_type_ann() {
        assert_eq!(categorize_ruff("ANN001"), "type");
    }

    #[test]
    fn categorize_ruff_style_up() {
        assert_eq!(categorize_ruff("UP001"), "style");
    }

    #[test]
    fn categorize_ruff_lint_b() {
        assert_eq!(categorize_ruff("B001"), "lint");
    }

    #[test]
    fn categorize_ruff_unknown_code_default() {
        assert_eq!(categorize_ruff("UNKNOWN_CODE"), "lint");
    }

    #[test]
    fn categorize_ruff_empty_string_default() {
        assert_eq!(categorize_ruff(""), "lint");
    }

    // =========================================================================
    // chrono_now — ISO format ending in Z
    // =========================================================================

    #[test]
    fn chrono_now_ends_with_z() {
        let ts = chrono_now();
        assert!(ts.ends_with('Z'), "timestamp should end with Z, got: {ts}");
    }

    // =========================================================================
    // hash_content — deterministic SHA-256
    // =========================================================================

    #[test]
    fn hash_content_deterministic() {
        let a = hash_content("hello world");
        let b = hash_content("hello world");
        assert_eq!(a, b);
    }

    #[test]
    fn hash_content_different_inputs() {
        let a = hash_content("hello");
        let b = hash_content("world");
        assert_ne!(a, b);
    }

    #[test]
    fn hash_content_empty_string() {
        let h = hash_content("");
        assert!(!h.is_empty(), "hash of empty string should not be empty");
        // SHA-256 of empty input is a well-known constant
        assert_eq!(h, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }
}

