//! Write engine for nornir enforcement output tools.
//!
//! Provides the shared infrastructure for all writer binaries:
//! - Format enforcement (jsonl append vs json write)
//! - Path enforcement with traversal protection
//! - Atomic writes with fsync
//! - Educational error messages for LLM consumers
//! - `--help` output showing all hardcoded configuration
//! - `--dump-schema` to print embedded JSON Schema for inspection

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use schema_core::EmbeddedValidator;

// =============================================================================
// Configuration types
// =============================================================================

/// Output format: line-delimited JSON or complete JSON document.
pub enum OutputFormat {
    /// Append one JSON line per write. File must already exist.
    Jsonl,
    /// Write a complete JSON document. File must NOT already exist.
    Json,
}

/// How many records per invocation.
pub enum WriteFrequency {
    /// Single JSON record from stdin.
    Record,
    /// Multiple JSONL lines from stdin, all-or-nothing validation.
    Batch,
}

/// Output path resolution strategy.
///
/// LLM-provided components are validated against path traversal before use.
pub enum OutputPath {
    /// Full path. No LLM input needed.
    FixedFile(PathBuf),

    /// Directory + fixed suffix. LLM provides a prefix via CLI arg.
    /// Constructed: `{dir}/{llm_prefix}{suffix}`
    DirectoryPrefix {
        dir: PathBuf,
        suffix: &'static str,
    },

    /// Directory + fixed extension. LLM provides filename stem via CLI arg.
    /// Constructed: `{dir}/{llm_name}.{ext}`
    DirectoryName {
        dir: PathBuf,
        ext: &'static str,
    },
}

/// Complete configuration for a writer binary.
pub struct WriterConfig {
    /// Binary name for --help and error messages.
    pub name: &'static str,
    /// Embedded schema validator.
    pub schema: &'static EmbeddedValidator,
    /// Path to the .schema.json source file (for --help display only).
    pub schema_source_path: String,
    /// Output format (jsonl or json).
    pub format: OutputFormat,
    /// Write frequency (record or batch).
    pub frequency: WriteFrequency,
    /// Output path strategy.
    pub output: OutputPath,
    /// Max records per batch (None = unlimited). Only used with Batch frequency.
    pub batch_size: Option<usize>,
}

// =============================================================================
// Error types (writer-specific, not NornirError)
// =============================================================================

/// Writer-specific errors with LLM-targeted messages.
#[derive(Debug)]
enum WriteError {
    StdinEmpty,
    InvalidJson(String),
    SchemaValidation(String),
    PathTraversal(String),
    FileExists(String),
    FileNotFound(String),
    DirectoryNotFound(String),
    IoFailed(String),
    BatchTooLarge { got: usize, max: usize },
    MissingArg(String),
    /// Pre-formatted batch line error (preserves exact original output format).
    BatchLine(String),
}

/// Heredoc usage hint shared by stdin-empty and invalid-json errors.
fn heredoc_hint(name: &str) -> String {
    format!(
        "For data with quotes or apostrophes, use heredoc:\n\
         \x20 {name} <<'RECORD'\n\
         \x20 {{\"field\":\"value with 'quotes'\"}}\n\
         \x20 RECORD",
        name = name,
    )
}

impl WriteError {
    /// Format error as FAIL:<reason> with educational guidance.
    fn format(&self, config: &WriterConfig) -> String {
        match self {
            WriteError::StdinEmpty => format!(
                "FAIL:stdin is empty — pipe JSON data into this command.\n\n\
                 Usage:\n\x20 echo '<json>' | {name}\n\n{}",
                heredoc_hint(config.name), name = config.name,
            ),
            WriteError::InvalidJson(detail) => format!(
                "FAIL:invalid JSON — {detail}\n\n{}",
                heredoc_hint(config.name), detail = detail,
            ),
            WriteError::SchemaValidation(msg) => format!("FAIL:schema validation — {}", msg),
            WriteError::PathTraversal(input) => format!(
                "FAIL:path traversal blocked — filename must not contain \"..\" or \"/\"\n\n\
                 You provided: \"{input}\"\n\
                 Filenames must be plain stems (e.g., \"entry-123\"), no path separators.",
            ),
            WriteError::FileExists(path) => format!(
                "FAIL:output file already exists — refusing to overwrite.\n\n\
                 Path: {path}\n\
                 This writer creates new files only. Delete the existing file first if intentional.",
            ),
            WriteError::FileNotFound(path) => format!(
                "FAIL:output file does not exist — the dispatcher must create the file first.\n\n\
                 Expected: {path}\n\
                 Run: touch {path}",
            ),
            WriteError::DirectoryNotFound(path) => format!(
                "FAIL:output directory does not exist.\n\n\
                 Expected: {path}\n\
                 Run: mkdir -p {path}",
            ),
            WriteError::IoFailed(msg) => format!("FAIL:IO error — {}", msg),
            WriteError::BatchTooLarge { got, max } => format!(
                "FAIL:batch too large — got {got} records, max is {max}.\n\
                 Split the batch into smaller chunks.",
            ),
            WriteError::MissingArg(what) => format!(
                "FAIL:missing argument — {what} is required.\n\n\
                 Run: {name} --help",
                name = config.name,
            ),
            WriteError::BatchLine(msg) => msg.clone(),
        }
    }
}

// =============================================================================
// Base path resolution
// =============================================================================

/// Resolve ~/ai as an absolute path from $HOME.
pub fn ai_home() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join("ai")
}

// =============================================================================
// Path traversal protection
// =============================================================================

/// Validate an LLM-provided filename component.
/// Delegates to path_core::validate_path_segment (shared across workspace).
fn validate_filename_component(input: &str) -> Result<(), String> {
    path_core::validate_path_segment(input)
}

// =============================================================================
// Output path resolution
// =============================================================================

/// Resolve the output path from config + optional CLI arg.
fn resolve_output_path(
    config: &WriterConfig,
    cli_arg: Option<&str>,
) -> Result<PathBuf, WriteError> {
    match &config.output {
        OutputPath::FixedFile(path) => Ok(path.clone()),
        OutputPath::DirectoryPrefix { dir, suffix } => {
            let prefix = cli_arg.ok_or_else(|| {
                WriteError::MissingArg("filename prefix as first argument".to_string())
            })?;
            validate_filename_component(prefix)
                .map_err(|_| WriteError::PathTraversal(prefix.to_string()))?;
            Ok(dir.join(format!("{}{}", prefix, suffix)))
        }
        OutputPath::DirectoryName { dir, ext } => {
            let name = cli_arg.ok_or_else(|| {
                WriteError::MissingArg("filename stem as first argument".to_string())
            })?;
            validate_filename_component(name)
                .map_err(|_| WriteError::PathTraversal(name.to_string()))?;
            Ok(dir.join(format!("{}.{}", name, ext)))
        }
    }
}

// =============================================================================
// --help output
// =============================================================================

fn format_help(config: &WriterConfig) -> String {
    let needs_arg = !matches!(config.output, OutputPath::FixedFile(_));
    let arg_str = if needs_arg { " <name>" } else { "" };

    let mut lines = Vec::new();
    lines.push(format!("{} — Validated enforcement output tool\n", config.name));
    format_help_config(config, &mut lines);

    lines.push(String::new());
    lines.push("USAGE:".into());
    lines.push(format!("  echo '<json>' | {}{}", config.name, arg_str));
    lines.push(String::new());
    lines.push("  For data with quotes or apostrophes, use heredoc:".into());
    lines.push(format!("  {}{} <<'RECORD'", config.name, arg_str));
    lines.push("  {\"uid\":\"...\",\"assessment\":\"...\"}".into());
    lines.push("  RECORD".into());

    lines.push(String::new());
    lines.push("INSPECT:".into());
    lines.push(format!("  {} --dump-schema    Print embedded JSON Schema to stdout", config.name));

    lines.push(String::new());
    format_help_output(config, &mut lines);

    lines.join("\n")
}

/// Format the HARDCODED CONFIGURATION section of --help output.
fn format_help_config(config: &WriterConfig, lines: &mut Vec<String>) {
    let format_str = match config.format {
        OutputFormat::Jsonl => "jsonl (append)",
        OutputFormat::Json => "json (write new file)",
    };
    let freq_str = match config.frequency {
        WriteFrequency::Record => "record (one record per invocation)",
        WriteFrequency::Batch => "batch (multiple JSONL lines per invocation)",
    };
    let output_str = match &config.output {
        OutputPath::FixedFile(path) => format!("{}", path.display()),
        OutputPath::DirectoryPrefix { dir, suffix } => {
            format!("{{dir}}/{{prefix}}{suffix}  (dir={dir})", suffix = suffix, dir = dir.display())
        }
        OutputPath::DirectoryName { dir, ext } => {
            format!("{{dir}}/{{name}}.{ext}  (dir={dir})", ext = ext, dir = dir.display())
        }
    };

    lines.push("HARDCODED CONFIGURATION:".into());
    lines.push(format!("  Schema:      {} (embedded)", config.schema.schema_name()));
    lines.push(format!("  Schema path: {}", config.schema_source_path));
    lines.push(format!("  Format:      {}", format_str));
    lines.push(format!("  Output:      {}", output_str));
    lines.push(format!("  Frequency:   {}", freq_str));
    if let Some(max) = config.batch_size {
        lines.push(format!("  Max batch:   {} records", max));
    }
}

/// Format the OUTPUT and EXIT CODES sections of --help output.
fn format_help_output(config: &WriterConfig, lines: &mut Vec<String>) {
    lines.push("OUTPUT:".into());
    match config.frequency {
        WriteFrequency::Record => lines.push("  OK              Record written successfully".into()),
        WriteFrequency::Batch => lines.push("  OK:<count>      Records written successfully".into()),
    }
    lines.push("  FAIL:<reason>   Validation or write failed — see reason".into());
    lines.push(String::new());
    lines.push("EXIT CODES:".into());
    lines.push("  0  success".into());
    lines.push("  1  failure".into());
}

// =============================================================================
// Atomic write helpers
// =============================================================================

/// Append a line to an existing file with fsync.
fn append_line(path: &Path, line: &str) -> Result<(), WriteError> {
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|e| WriteError::IoFailed(format!("{}: {}", path.display(), e)))?;
    writeln!(file, "{}", line)
        .map_err(|e| WriteError::IoFailed(format!("write to {}: {}", path.display(), e)))?;
    file.sync_all()
        .map_err(|e| WriteError::IoFailed(format!("fsync {}: {}", path.display(), e)))?;
    Ok(())
}

/// Write a complete file atomically (write temp, fsync, rename).
fn write_atomic(path: &Path, content: &str) -> Result<(), WriteError> {
    let parent = path
        .parent()
        .ok_or_else(|| WriteError::IoFailed("cannot determine parent directory".to_string()))?;

    let tmp_path = parent.join(format!(
        ".{}.tmp",
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "output".to_string())
    ));

    // Write to temp file
    let mut file = File::create(&tmp_path)
        .map_err(|e| WriteError::IoFailed(format!("create {}: {}", tmp_path.display(), e)))?;
    file.write_all(content.as_bytes())
        .map_err(|e| WriteError::IoFailed(format!("write {}: {}", tmp_path.display(), e)))?;
    file.sync_all()
        .map_err(|e| WriteError::IoFailed(format!("fsync {}: {}", tmp_path.display(), e)))?;
    drop(file);

    // Atomic rename
    fs::rename(&tmp_path, path).map_err(|e| {
        // Clean up temp file on rename failure
        let _ = fs::remove_file(&tmp_path);
        WriteError::IoFailed(format!(
            "rename {} -> {}: {}",
            tmp_path.display(),
            path.display(),
            e
        ))
    })?;

    Ok(())
}

// =============================================================================
// Public utilities
// =============================================================================

/// Write a file atomically (temp file + fsync + rename).
///
/// For use by non-writer binaries that need atomic writes without the full
/// writer pipeline (schema validation, stdin parsing, path resolution).
pub fn write_file_atomic(path: &Path, content: &str) -> Result<(), String> {
    write_atomic(path, content).map_err(|e| match e {
        WriteError::IoFailed(msg) => msg,
        other => format!("{other:?}"),
    })
}

/// Truncate a file and write new content, preserving the original inode.
///
/// Unlike `write_file_atomic` (which renames a temp file and changes the inode),
/// this opens the existing file with truncation. Consumers holding open file
/// descriptors (e.g., watchers using BufReader) retain a valid fd and can detect
/// the size change to re-seek.
pub fn write_truncate_fsync(path: &Path, content: &str) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    file.write_all(content.as_bytes())
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    file.sync_all()
        .map_err(|e| format!("fsync {}: {e}", path.display()))?;
    Ok(())
}

/// Append one line to a file with fsync. Creates the file if it doesn't exist.
///
/// A trailing newline is added automatically.
/// Used for JSONL append across the workspace.
pub fn append_line_fsync(path: &Path, line: &str) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    writeln!(file, "{}", line)
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    file.sync_all()
        .map_err(|e| format!("fsync {}: {e}", path.display()))?;
    Ok(())
}

// =============================================================================
// Core run function
// =============================================================================

/// Main entry point for all writer binaries.
///
/// Reads config, parses args, reads stdin, validates, writes.
/// Returns `Ok(message)` on success or `Err(message)` on failure.
/// The caller is responsible for printing and calling `process::exit`.
///
pub fn run(config: &WriterConfig) -> Result<String, String> {
    let args: Vec<String> = std::env::args().collect();

    // --help
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(format_help(config));
    }

    // --dump-schema: return embedded schema JSON
    if args.iter().any(|a| a == "--dump-schema") {
        return Ok(config.schema.schema_json().to_string());
    }

    // CLI arg (filename component) if needed
    let cli_arg = args.get(1).map(|s| s.as_str());

    // Resolve output path (includes traversal validation)
    let output_path = resolve_output_path(config, cli_arg)
        .map_err(|e| e.format(config))?;

    // Read stdin
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| WriteError::IoFailed(format!("reading stdin: {}", e)).format(config))?;
    let input = input.trim();

    if input.is_empty() {
        return Err(WriteError::StdinEmpty.format(config));
    }

    // Parse and validate based on frequency
    let result = match config.frequency {
        WriteFrequency::Record => run_record(config, &output_path, input),
        WriteFrequency::Batch => run_batch(config, &output_path, input),
    };

    result.map_err(|e| e.format(config))
}

/// Handle single-record writes.
fn run_record(config: &WriterConfig, output_path: &Path, input: &str) -> Result<String, WriteError> {
    // Validate JSON + schema
    let result = config
        .schema
        .validate(input)
        .map_err(|e| WriteError::InvalidJson(e.to_string()))?;

    if !result.valid {
        return Err(WriteError::SchemaValidation(result.message));
    }

    // Re-serialize to compact JSON (normalized)
    let data = result.data.ok_or(WriteError::IoFailed(
        "schema validation returned no data".to_string(),
    ))?;
    let compact = serde_json::to_string(&data)
        .map_err(|e| WriteError::IoFailed(format!("re-serialization: {e}")))?;

    // Write based on format
    match config.format {
        OutputFormat::Jsonl => {
            // File must exist
            if !output_path.exists() {
                return Err(WriteError::FileNotFound(
                    output_path.display().to_string(),
                ));
            }
            append_line(output_path, &compact)?;
        }
        OutputFormat::Json => {
            // File must NOT exist
            if output_path.exists() {
                return Err(WriteError::FileExists(
                    output_path.display().to_string(),
                ));
            }
            // Parent directory must exist
            if let Some(parent) = output_path.parent() {
                if !parent.exists() {
                    return Err(WriteError::DirectoryNotFound(
                        parent.display().to_string(),
                    ));
                }
            }
            // Pretty-print for json files
            let pretty = serde_json::to_string_pretty(&data)
                .map_err(|e| WriteError::IoFailed(format!("re-serialization: {e}")))?;
            write_atomic(output_path, &pretty)?;
        }
    }

    Ok("OK".to_string())
}

fn validate_batch_lines(config: &WriterConfig, lines: &[&str]) -> Result<Vec<String>, WriteError> {
    let mut validated: Vec<String> = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let line_num = i + 1;
        let result = config.schema.validate(line).map_err(|e| {
            WriteError::BatchLine(format!(
                "FAIL:line {} — {}",
                line_num,
                WriteError::InvalidJson(e.to_string()).format(config)
            ))
        })?;

        if !result.valid {
            return Err(WriteError::BatchLine(format!(
                "FAIL:line {} — schema validation — {}",
                line_num, result.message
            )));
        }

        let data = result.data.ok_or_else(|| {
            WriteError::BatchLine(format!("FAIL:line {} — no data after validation", line_num))
        })?;
        let compact = serde_json::to_string(&data).map_err(|e| {
            WriteError::BatchLine(format!("FAIL:line {} — re-serialization: {e}", line_num))
        })?;
        validated.push(compact);
    }
    Ok(validated)
}

/// Handle batch writes (multiple JSONL lines, all-or-nothing).
fn run_batch(config: &WriterConfig, output_path: &Path, input: &str) -> Result<String, WriteError> {
    let lines: Vec<&str> = input.lines().filter(|l| !l.trim().is_empty()).collect();

    if lines.is_empty() {
        return Err(WriteError::StdinEmpty);
    }

    if let Some(max) = config.batch_size {
        if lines.len() > max {
            return Err(WriteError::BatchTooLarge {
                got: lines.len(),
                max,
            });
        }
    }

    let validated = validate_batch_lines(config, &lines)?;

    // File must exist for jsonl append
    match config.format {
        OutputFormat::Jsonl => {
            if !output_path.exists() {
                return Err(WriteError::FileNotFound(
                    output_path.display().to_string(),
                ));
            }

            // Append all validated records
            let mut file = OpenOptions::new()
                .append(true)
                .open(output_path)
                .map_err(|e| {
                    WriteError::IoFailed(format!("{}: {}", output_path.display(), e))
                })?;

            for line in &validated {
                writeln!(file, "{}", line).map_err(|e| {
                    WriteError::IoFailed(format!("write: {}", e))
                })?;
            }

            file.sync_all().map_err(|e| {
                WriteError::IoFailed(format!("fsync: {}", e))
            })?;
        }
        OutputFormat::Json => {
            // Batch mode with Json format is unusual but handle it:
            // write a JSON array
            if output_path.exists() {
                return Err(WriteError::FileExists(
                    output_path.display().to_string(),
                ));
            }
            if let Some(parent) = output_path.parent() {
                if !parent.exists() {
                    return Err(WriteError::DirectoryNotFound(
                        parent.display().to_string(),
                    ));
                }
            }
            let values: Vec<serde_json::Value> = validated
                .iter()
                .map(|s| serde_json::from_str(s))
                .collect::<Result<_, _>>()
                .map_err(|e| WriteError::IoFailed(format!("batch re-parse: {e}")))?;
            let pretty = serde_json::to_string_pretty(&values)
                .map_err(|e| WriteError::IoFailed(format!("batch re-serialization: {e}")))?;
            write_atomic(output_path, &pretty)?;
        }
    }

    Ok(format!("OK:{}", validated.len()))
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_filename_clean() {
        assert!(validate_filename_component("entry-123").is_ok());
        assert!(validate_filename_component("interview-42").is_ok());
        assert!(validate_filename_component("my_file_name").is_ok());
    }

    #[test]
    fn test_validate_filename_traversal() {
        assert!(validate_filename_component("../etc/passwd").is_err());
        assert!(validate_filename_component("foo/../bar").is_err());
        assert!(validate_filename_component("..").is_err());
    }

    #[test]
    fn test_validate_filename_separators() {
        assert!(validate_filename_component("foo/bar").is_err());
        assert!(validate_filename_component("foo\\bar").is_err());
    }

    #[test]
    fn test_validate_filename_hidden() {
        assert!(validate_filename_component(".hidden").is_err());
        assert!(validate_filename_component(".").is_err());
    }

    #[test]
    fn test_validate_filename_empty() {
        assert!(validate_filename_component("").is_err());
    }

    #[test]
    fn test_validate_filename_null() {
        assert!(validate_filename_component("foo\0bar").is_err());
    }

    #[test]
    fn test_resolve_fixed_path() {
        let config = WriterConfig {
            name: "test",
            schema: &DUMMY_VALIDATOR,
            schema_source_path: "test".into(),
            format: OutputFormat::Jsonl,
            frequency: WriteFrequency::Record,
            output: OutputPath::FixedFile(PathBuf::from("/tmp/test.jsonl")),
            batch_size: None,
        };
        let path = resolve_output_path(&config, None).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/test.jsonl"));
    }

    #[test]
    fn test_resolve_directory_name() {
        let config = WriterConfig {
            name: "test",
            schema: &DUMMY_VALIDATOR,
            schema_source_path: "test".into(),
            format: OutputFormat::Json,
            frequency: WriteFrequency::Record,
            output: OutputPath::DirectoryName {
                dir: PathBuf::from("/tmp/out"),
                ext: "json",
            },
            batch_size: None,
        };
        let path = resolve_output_path(&config, Some("entry-123")).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/out/entry-123.json"));
    }

    #[test]
    fn test_resolve_directory_prefix() {
        let config = WriterConfig {
            name: "test",
            schema: &DUMMY_VALIDATOR,
            schema_source_path: "test".into(),
            format: OutputFormat::Jsonl,
            frequency: WriteFrequency::Batch,
            output: OutputPath::DirectoryPrefix {
                dir: PathBuf::from("/tmp/out"),
                suffix: ".summaries.jsonl",
            },
            batch_size: None,
        };
        let path = resolve_output_path(&config, Some("interview-42")).unwrap();
        assert_eq!(
            path,
            PathBuf::from("/tmp/out/interview-42.summaries.jsonl")
        );
    }

    #[test]
    fn test_resolve_directory_name_traversal_blocked() {
        let config = WriterConfig {
            name: "test",
            schema: &DUMMY_VALIDATOR,
            schema_source_path: "test".into(),
            format: OutputFormat::Json,
            frequency: WriteFrequency::Record,
            output: OutputPath::DirectoryName {
                dir: PathBuf::from("/tmp/out"),
                ext: "json",
            },
            batch_size: None,
        };
        assert!(resolve_output_path(&config, Some("../../../etc/passwd")).is_err());
        assert!(resolve_output_path(&config, Some("foo/bar")).is_err());
        assert!(resolve_output_path(&config, Some(".hidden")).is_err());
    }

    #[test]
    fn test_resolve_missing_arg() {
        let config = WriterConfig {
            name: "test",
            schema: &DUMMY_VALIDATOR,
            schema_source_path: "test".into(),
            format: OutputFormat::Json,
            frequency: WriteFrequency::Record,
            output: OutputPath::DirectoryName {
                dir: PathBuf::from("/tmp/out"),
                ext: "json",
            },
            batch_size: None,
        };
        assert!(resolve_output_path(&config, None).is_err());
    }

    // =========================================================================
    // append_line_fsync
    // =========================================================================

    #[test]
    fn append_line_fsync_creates_and_appends() {
        let dir = std::env::temp_dir().join(format!("we_test_append_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.jsonl");

        append_line_fsync(&path, r#"{"a":1}"#).unwrap();
        append_line_fsync(&path, r#"{"b":2}"#).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], r#"{"a":1}"#);
        assert_eq!(lines[1], r#"{"b":2}"#);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // =========================================================================
    // write_truncate_fsync
    // =========================================================================

    #[test]
    fn write_truncate_fsync_replaces_content() {
        let dir = std::env::temp_dir().join(format!("we_test_trunc_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.jsonl");

        // Write initial content
        append_line_fsync(&path, "line1").unwrap();
        append_line_fsync(&path, "line2").unwrap();
        append_line_fsync(&path, "line3").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 3);

        // Truncate to single line
        write_truncate_fsync(&path, "only_this\n").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "only_this\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn write_truncate_fsync_preserves_inode() {
        use std::os::unix::fs::MetadataExt;

        let dir = std::env::temp_dir().join(format!("we_test_inode_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.jsonl");

        append_line_fsync(&path, "original").unwrap();
        let ino_before = std::fs::metadata(&path).unwrap().ino();

        write_truncate_fsync(&path, "replaced\n").unwrap();
        let ino_after = std::fs::metadata(&path).unwrap().ino();

        assert_eq!(ino_before, ino_after);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // Dummy validator for path resolution tests (schema content doesn't matter)
    static DUMMY_SCHEMA: &str = r#"{"type": "object"}"#;
    static DUMMY_VALIDATOR: EmbeddedValidator =
        EmbeddedValidator::new(DUMMY_SCHEMA, "test-schema");
}
