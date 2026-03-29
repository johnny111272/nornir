//! Saga runner: quality tool execution and sidecar I/O.
//!
//! Capability crate — performs I/O (subprocess execution, filesystem, clock).
//! Pure types live in `saga_core`.

pub use saga_core::{Issue, SanityReport, qa_path, source_path_from_qa};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::Instant;

// =============================================================================
// Embedded configs — compiled into the binary, written to tmp at runtime
// =============================================================================

static RUFF_TOML: &str = include_str!("../ruff.toml");
static PYRIGHTCONFIG_JSON: &str = include_str!("../pyrightconfig.json");
static V2_PROJECTS_TOML: &str = include_str!("../v2_projects.toml");

/// Parsed list of project root paths that have opted in to v2 zone checks.
static V2_PROJECT_PATHS: LazyLock<Vec<String>> = LazyLock::new(|| {
    #[derive(serde::Deserialize)]
    struct ProjectEntry {
        path: String,
    }
    #[derive(serde::Deserialize)]
    struct V2Projects {
        #[serde(default)]
        projects: Vec<ProjectEntry>,
    }
    let parsed: V2Projects =
        toml::from_str(V2_PROJECTS_TOML).expect("v2_projects.toml parse error");
    parsed.projects.into_iter().map(|entry| entry.path).collect()
});

/// Check if a file belongs to a project that has opted in to v2 zone checks.
fn is_v2_project(file_path: &Path) -> bool {
    let path_str = file_path.to_string_lossy();
    V2_PROJECT_PATHS.iter().any(|prefix| path_str.starts_with(prefix.as_str()))
}

/// Version override for `--test v1` / `--test v2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionOverride {
    /// Use v2_projects.toml routing (default)
    Auto,
    /// Force v1 checks regardless of project
    ForceV1,
    /// Force v2 checks regardless of project
    ForceV2,
}

// Thread-local override for gleipnir version routing.
// Set via set_version_override(), read by run_gleipnir().
std::thread_local! {
    static VERSION_OVERRIDE: std::cell::Cell<VersionOverride> = const { std::cell::Cell::new(VersionOverride::Auto) };
}

/// Set the gleipnir version override. Affects all subsequent `run_gleipnir()` calls
/// on this thread.
pub fn set_version_override(version: VersionOverride) {
    VERSION_OVERRIDE.with(|cell| cell.set(version));
}

fn use_v2(file_path: &Path) -> bool {
    VERSION_OVERRIDE.with(|cell| match cell.get() {
        VersionOverride::Auto => is_v2_project(file_path),
        VersionOverride::ForceV1 => false,
        VersionOverride::ForceV2 => true,
    })
}

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

fn violation_to_issue(violation: gleipnir_core::Violation) -> Issue {
    Issue {
        tool: "gleipnir".into(),
        code: violation.check_name,
        severity: match violation.severity {
            gleipnir_core::Severity::Blocked => "blocked",
            gleipnir_core::Severity::Error => "error",
            gleipnir_core::Severity::Warning => "warning",
        }
        .into(),
        line: violation.line,
        column: None,
        message: violation.message,
        category: "structure".into(),
        fixable: false,
        signal: violation.signal,
        direction: violation.direction,
        canary: violation.canary,
    }
}

/// Run gleipnir guardrail checks on a Python file.
///
/// Routes to v2 zone checks for projects listed in v2_projects.toml,
/// otherwise uses v1 rules.
pub fn run_gleipnir(file_path: &Path) -> Vec<Issue> {
    let source = match std::fs::read(file_path) {
        Ok(bytes) => bytes,
        Err(_) => return Vec::new(),
    };
    let path_str = file_path.to_string_lossy();
    let violations = if use_v2(file_path) {
        gleipnir_core::run_checks_v2(&path_str, &source)
    } else {
        gleipnir_core::run_checks(&path_str, &source)
    };
    violations.into_iter().map(violation_to_issue).collect()
}

/// Run gleipnir Rust checks on a file.
pub fn run_gleipnir_rust(file_path: &Path) -> Vec<Issue> {
    let source = match std::fs::read(file_path) {
        Ok(bytes) => bytes,
        Err(_) => return Vec::new(),
    };
    gleipnir_core::run_checks_rust(&file_path.to_string_lossy(), &source)
        .into_iter()
        .map(violation_to_issue)
        .collect()
}

/// Run ruff linter on a file using Saga's embedded config.
pub fn run_ruff(file_path: &Path) -> Vec<Issue> {
    let config = ruff_config();
    let output = match Command::new("ruff")
        .arg("check")
        .arg("--output-format=json")
        .arg("--config")
        .arg(config.as_os_str())
        .arg(file_path.as_os_str())
        .output()
    {
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
    let output = match Command::new("basedpyright")
        .arg("--outputjson")
        .arg("--project")
        .arg(config.as_os_str())
        .arg(file_path.as_os_str())
        .output()
    {
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

/// Run all Rust quality tools on a file.
pub fn run_rust_tools(file_path: &Path) -> Vec<Issue> {
    run_gleipnir_rust(file_path)
}

/// Run gleipnir Svelte/TypeScript checks on a file.
pub fn run_gleipnir_svelte(file_path: &Path) -> Vec<Issue> {
    let source = match std::fs::read(file_path) {
        Ok(bytes) => bytes,
        Err(_) => return Vec::new(),
    };
    gleipnir_core::run_checks_svelte(&file_path.to_string_lossy(), &source)
        .into_iter()
        .map(violation_to_issue)
        .collect()
}

/// Run all Svelte quality tools on a file.
pub fn run_svelte_tools(file_path: &Path) -> Vec<Issue> {
    run_gleipnir_svelte(file_path)
}

// =============================================================================
// Report generation
// =============================================================================

/// Generate a SanityReport for a file.
pub fn generate_report(file_path: &Path, project_root: Option<&Path>) -> SanityReport {
    let file_path = file_path
        .canonicalize()
        .unwrap_or_else(|_| file_path.to_path_buf());

    let start = Instant::now();
    let issues = match file_path.extension().and_then(|e| e.to_str()) {
        Some("rs") => run_rust_tools(&file_path),
        Some("svelte") => run_svelte_tools(&file_path),
        _ => run_python_tools(&file_path),
    };
    let elapsed_ms = start.elapsed().as_millis() as u64;

    let relative_path = project_root
        .and_then(|root| file_path.strip_prefix(root).ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| tail_path(&file_path, 3));

    let content_hash = hash_file(&file_path).unwrap_or_default();
    let total = issues.len();
    let (by_tool, by_severity, by_category) = summarize_issues(&issues);

    SanityReport {
        file: file_path.to_string_lossy().to_string(),
        relative_path,
        content_hash,
        generated_at: timestamp_now(),
        elapsed_ms,
        issues,
        total,
        by_tool,
        by_severity,
        by_category,
    }
}

/// Generate a SanityReport from content string (for stdin / hook usage).
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

    if std::fs::write(&tmp_file, content).is_err() {
        return SanityReport {
            file: file_path.to_string_lossy().to_string(),
            ..Default::default()
        };
    }

    let start = Instant::now();
    let issues = match file_path.extension().and_then(|e| e.to_str()) {
        Some("rs") => run_rust_tools(&tmp_file),
        Some("svelte") => run_svelte_tools(&tmp_file),
        _ => run_python_tools(&tmp_file),
    };
    let elapsed_ms = start.elapsed().as_millis() as u64;

    let _ = std::fs::remove_file(&tmp_file);

    let relative_path = project_root
        .and_then(|root| file_path.strip_prefix(root).ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| tail_path(file_path, 3));

    let content_hash = hash_content(content);
    let total = issues.len();
    let (by_tool, by_severity, by_category) = summarize_issues(&issues);

    SanityReport {
        file: file_path.to_string_lossy().to_string(),
        relative_path,
        content_hash,
        generated_at: timestamp_now(),
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

/// Write a SanityReport to its .qa sidecar.
pub fn save_sidecar(report: &SanityReport) -> std::io::Result<PathBuf> {
    let path = qa_path(Path::new(&report.file));
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(&path, json)?;
    Ok(path)
}

/// Load a SanityReport from a .qa sidecar (given the source file path).
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

/// Remove .qa sidecars whose source files no longer exist.
pub fn remove_orphaned_sidecars(search_dir: &Path) -> Vec<PathBuf> {
    let qa_files = find_files(search_dir, &[], &|name| {
        name.ends_with(".qa") && name.starts_with('.')
    });
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

// =============================================================================
// Directory walking
// =============================================================================

/// Recursively collect files matching a predicate, skipping junk directories.
pub fn walk_files(
    search_dir: &Path,
    extra_skip: &[&str],
    predicate: &dyn Fn(&str) -> bool,
    results: &mut Vec<PathBuf>,
) {
    let entries = match std::fs::read_dir(search_dir) {
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

/// Collect files matching a predicate under a directory, sorted.
pub fn find_files(
    search_dir: &Path,
    extra_skip: &[&str],
    predicate: &dyn Fn(&str) -> bool,
) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk_files(search_dir, extra_skip, predicate, &mut files);
    files.sort();
    files
}

// =============================================================================
// Internal helpers
// =============================================================================

/// Directories always skipped during recursive file discovery.
const SKIP_DIRS: &[&str] = &["__pycache__", "node_modules", ".venv", "venv", "target"];

/// Last N path components as a string. Disambiguates `lib.rs` across crates.
fn tail_path(path: &Path, depth: usize) -> String {
    let components: Vec<_> = path.components().collect();
    let start = components.len().saturating_sub(depth);
    components[start..]
        .iter()
        .collect::<PathBuf>()
        .to_string_lossy()
        .to_string()
}

fn hash_content(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn summarize_issues(
    issues: &[Issue],
) -> (
    HashMap<String, usize>,
    HashMap<String, usize>,
    HashMap<String, usize>,
) {
    let mut by_tool: HashMap<&str, usize> = HashMap::new();
    let mut by_severity: HashMap<&str, usize> = HashMap::new();
    let mut by_category: HashMap<&str, usize> = HashMap::new();
    for issue in issues {
        *by_tool.entry(&issue.tool).or_default() += 1;
        *by_severity.entry(&issue.severity).or_default() += 1;
        *by_category.entry(&issue.category).or_default() += 1;
    }
    let to_owned = |map: HashMap<&str, usize>| -> HashMap<String, usize> {
        map.into_iter().map(|(key, count)| (key.to_string(), count)).collect()
    };
    (to_owned(by_tool), to_owned(by_severity), to_owned(by_category))
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

fn timestamp_now() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}Z", duration.as_secs())
}

/// Ruff code → category mapping table.
const RUFF_CATEGORIES: &[(&str, &str)] = &[
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

fn categorize_ruff(code: &str) -> &'static str {
    for (prefix, category) in RUFF_CATEGORIES {
        if code.starts_with(prefix) {
            return category;
        }
    }
    "lint"
}
