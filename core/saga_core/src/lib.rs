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
// Config paths
// =============================================================================

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()))
}

fn saga_root() -> PathBuf {
    home_dir().join(".ai/phoenix/quality/saga")
}

fn gleipnir_python() -> PathBuf {
    home_dir().join(".ai/gleipnir/.venv/bin/python")
}

// =============================================================================
// Runners — shell out to external tools, parse JSON output
// =============================================================================

/// Run gleipnir guardrail checks on a file.
pub fn run_gleipnir(file_path: &Path) -> Vec<Issue> {
    let python = gleipnir_python();
    if !python.exists() {
        return Vec::new();
    }

    let output = Command::new(python.as_os_str())
        .args(["-m", "gleipnir.runners.file_gleipnir"])
        .arg(file_path.as_os_str())
        .arg("--json")
        .output();

    let output = match output {
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
        .map(|item| Issue {
            tool: "gleipnir".into(),
            code: item["check_name"].as_str().unwrap_or("guardrail").into(),
            severity: item["severity"].as_str().unwrap_or("warning").into(),
            line: item["line"].as_u64().unwrap_or(1) as usize,
            column: None,
            message: item["message"].as_str().unwrap_or("").into(),
            category: "structure".into(),
            fixable: false,
            signal: item["signal"].as_str().unwrap_or("").into(),
            direction: item["direction"].as_str().unwrap_or("").into(),
            canary: item["canary"].as_str().unwrap_or("").into(),
        })
        .collect()
}

/// Run ruff linter on a file using Saga's global config.
pub fn run_ruff(file_path: &Path) -> Vec<Issue> {
    let config = saga_root().join("ruff.toml");
    let mut cmd = Command::new("ruff");
    cmd.arg("check")
        .arg("--output-format=json")
        .arg(file_path.as_os_str());

    if config.exists() {
        cmd.arg("--config").arg(config.as_os_str());
    }

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

/// Run basedpyright type checker on a file using Saga's global config.
pub fn run_basedpyright(file_path: &Path) -> Vec<Issue> {
    let config = saga_root().join("pyrightconfig.json");
    let mut cmd = Command::new("basedpyright");
    cmd.arg("--outputjson")
        .arg(file_path.as_os_str());

    if config.exists() {
        cmd.arg("--project").arg(config.as_os_str());
    }

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
    let tmp_dir = std::env::temp_dir().join("saga");
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

