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
use std::process;

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
    /// Full path hardcoded. No LLM input needed.
    FixedFile(&'static str),

    /// Directory + fixed suffix. LLM provides a prefix via CLI arg.
    /// Constructed: `{dir}/{llm_prefix}{suffix}`
    DirectoryPrefix {
        dir: &'static str,
        suffix: &'static str,
    },

    /// Directory + fixed extension. LLM provides filename stem via CLI arg.
    /// Constructed: `{dir}/{llm_name}.{ext}`
    DirectoryName {
        dir: &'static str,
        ext: &'static str,
    },
}

/// Complete configuration for a writer binary.
pub struct WriterConfig {
    /// Binary name for --help and error messages.
    pub name: &'static str,
    /// Embedded schema validator.
    pub schema: &'static EmbeddedValidator,
    /// Absolute path to the .schema.json source file (for --help display).
    pub schema_source_path: &'static str,
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
}

impl WriteError {
    /// Format error as FAIL:<reason> with educational guidance.
    fn format(&self, config: &WriterConfig) -> String {
        match self {
            WriteError::StdinEmpty => format!(
                "FAIL:stdin is empty — pipe JSON data into this command.\n\
                 \n\
                 Usage:\n\
                 \x20 echo '<json>' | {name}\n\
                 \n\
                 For data with quotes or apostrophes, use heredoc:\n\
                 \x20 cat <<'RECORD' | {name}\n\
                 \x20 {{\"field\":\"value with 'quotes'\"}}\n\
                 \x20 RECORD",
                name = config.name
            ),
            WriteError::InvalidJson(detail) => format!(
                "FAIL:invalid JSON — {detail}\n\
                 \n\
                 If your data contains apostrophes or nested quotes, use heredoc:\n\
                 \x20 cat <<'RECORD' | {name}\n\
                 \x20 {{\"field\":\"value with 'quotes'\"}}\n\
                 \x20 RECORD",
                detail = detail,
                name = config.name
            ),
            WriteError::SchemaValidation(msg) => format!("FAIL:schema validation — {}", msg),
            WriteError::PathTraversal(input) => format!(
                "FAIL:path traversal blocked — filename must not contain \"..\" or \"/\"\n\
                 \n\
                 You provided: \"{input}\"\n\
                 Filenames must be plain stems (e.g., \"entry-123\"), no path separators.",
                input = input,
            ),
            WriteError::FileExists(path) => format!(
                "FAIL:output file already exists — refusing to overwrite.\n\
                 \n\
                 Path: {path}\n\
                 This writer creates new files only. Delete the existing file first if intentional.",
                path = path,
            ),
            WriteError::FileNotFound(path) => format!(
                "FAIL:output file does not exist — the dispatcher must create the file first.\n\
                 \n\
                 Expected: {path}\n\
                 Run: touch {path}",
                path = path,
            ),
            WriteError::DirectoryNotFound(path) => format!(
                "FAIL:output directory does not exist.\n\
                 \n\
                 Expected: {path}\n\
                 Run: mkdir -p {path}",
                path = path,
            ),
            WriteError::IoFailed(msg) => format!("FAIL:IO error — {}", msg),
            WriteError::BatchTooLarge { got, max } => format!(
                "FAIL:batch too large — got {got} records, max is {max}.\n\
                 Split the batch into smaller chunks.",
                got = got,
                max = max,
            ),
            WriteError::MissingArg(what) => format!(
                "FAIL:missing argument — {what} is required.\n\
                 \n\
                 Run: {name} --help",
                what = what,
                name = config.name,
            ),
        }
    }
}

// =============================================================================
// Path traversal protection
// =============================================================================

/// Validate an LLM-provided filename component.
/// Rejects path traversal, separators, null bytes, hidden files, empty strings.
fn validate_filename_component(input: &str) -> Result<(), String> {
    if input.is_empty() {
        return Err("filename cannot be empty".to_string());
    }
    if input.contains("..") {
        return Err(format!("\"..\" forbidden in filename: \"{}\"", input));
    }
    if input.contains('/') || input.contains('\\') {
        return Err(format!(
            "path separators forbidden in filename: \"{}\"",
            input
        ));
    }
    if input.contains('\0') {
        return Err(format!("null byte in filename: \"{}\"", input));
    }
    if input.starts_with('.') {
        return Err(format!(
            "filename cannot start with \".\" (no hidden files): \"{}\"",
            input
        ));
    }
    Ok(())
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
        OutputPath::FixedFile(path) => Ok(PathBuf::from(path)),
        OutputPath::DirectoryPrefix { dir, suffix } => {
            let prefix = cli_arg.ok_or_else(|| {
                WriteError::MissingArg("filename prefix as first argument".to_string())
            })?;
            validate_filename_component(prefix)
                .map_err(|_| WriteError::PathTraversal(prefix.to_string()))?;
            Ok(PathBuf::from(dir).join(format!("{}{}", prefix, suffix)))
        }
        OutputPath::DirectoryName { dir, ext } => {
            let name = cli_arg.ok_or_else(|| {
                WriteError::MissingArg("filename stem as first argument".to_string())
            })?;
            validate_filename_component(name)
                .map_err(|_| WriteError::PathTraversal(name.to_string()))?;
            Ok(PathBuf::from(dir).join(format!("{}.{}", name, ext)))
        }
    }
}

// =============================================================================
// --help output
// =============================================================================

fn print_help(config: &WriterConfig) {
    let format_str = match config.format {
        OutputFormat::Jsonl => "jsonl (append)",
        OutputFormat::Json => "json (write new file)",
    };
    let freq_str = match config.frequency {
        WriteFrequency::Record => "record (one record per invocation)",
        WriteFrequency::Batch => "batch (multiple JSONL lines per invocation)",
    };
    let output_str = match &config.output {
        OutputPath::FixedFile(path) => format!("{}", path),
        OutputPath::DirectoryPrefix { dir, suffix } => {
            format!("{{dir}}/{{prefix}}{suffix}  (dir={dir})", suffix = suffix, dir = dir)
        }
        OutputPath::DirectoryName { dir, ext } => {
            format!("{{dir}}/{{name}}.{ext}  (dir={dir})", ext = ext, dir = dir)
        }
    };

    let needs_arg = !matches!(config.output, OutputPath::FixedFile(_));
    let arg_str = if needs_arg { " <name>" } else { "" };

    println!("{name} — Validated enforcement output tool\n", name = config.name);
    println!("HARDCODED CONFIGURATION:");
    println!("  Schema:      {} (embedded)", config.schema.schema_name());
    println!("  Schema path: {}", config.schema_source_path);
    println!("  Format:      {}", format_str);
    println!("  Output:      {}", output_str);
    println!("  Frequency:   {}", freq_str);
    if let Some(max) = config.batch_size {
        println!("  Max batch:   {} records", max);
    }

    println!();
    println!("USAGE:");
    println!("  echo '<json>' | {}{}", config.name, arg_str);
    println!();
    println!("  For data with quotes or apostrophes, use heredoc:");
    println!("  cat <<'RECORD' | {}{}", config.name, arg_str);
    println!("  {{\"uid\":\"...\",\"assessment\":\"...\"}}");
    println!("  RECORD");

    println!();
    println!("INSPECT:");
    println!("  {} --dump-schema    Print embedded JSON Schema to stdout", config.name);

    println!();
    println!("OUTPUT:");
    match config.frequency {
        WriteFrequency::Record => {
            println!("  OK              Record written successfully");
        }
        WriteFrequency::Batch => {
            println!("  OK:<count>      Records written successfully");
        }
    }
    println!("  FAIL:<reason>   Validation or write failed — see reason");

    println!();
    println!("EXIT CODES:");
    println!("  0  success");
    println!("  1  failure");
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
// Core run function
// =============================================================================

/// Main entry point for all writer binaries.
///
/// Reads config, parses args, reads stdin, validates, writes. Exits process.
pub fn run(config: &WriterConfig) -> ! {
    let args: Vec<String> = std::env::args().collect();

    // --help
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help(config);
        process::exit(0);
    }

    // --dump-schema: print embedded schema JSON to stdout
    if args.iter().any(|a| a == "--dump-schema") {
        println!("{}", config.schema.schema_json());
        process::exit(0);
    }

    // CLI arg (filename component) if needed
    let cli_arg = args.get(1).map(|s| s.as_str());

    // Resolve output path (includes traversal validation)
    let output_path = match resolve_output_path(config, cli_arg) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e.format(config));
            process::exit(1);
        }
    };

    // Read stdin
    let mut input = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut input) {
        eprintln!(
            "{}",
            WriteError::IoFailed(format!("reading stdin: {}", e)).format(config)
        );
        process::exit(1);
    }
    let input = input.trim();

    if input.is_empty() {
        eprintln!("{}", WriteError::StdinEmpty.format(config));
        process::exit(1);
    }

    // Parse and validate based on frequency
    match config.frequency {
        WriteFrequency::Record => {
            run_record(config, &output_path, input);
        }
        WriteFrequency::Batch => {
            run_batch(config, &output_path, input);
        }
    }
}

/// Handle single-record writes.
fn run_record(config: &WriterConfig, output_path: &Path, input: &str) -> ! {
    // Validate JSON + schema
    let result = config.schema.validate(input);
    let result = match result {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "{}",
                WriteError::InvalidJson(e.to_string()).format(config)
            );
            process::exit(1);
        }
    };

    if !result.valid {
        eprintln!(
            "{}",
            WriteError::SchemaValidation(result.message).format(config)
        );
        process::exit(1);
    }

    // Re-serialize to compact JSON (normalized)
    let data = result.data.unwrap();
    let compact = serde_json::to_string(&data).unwrap();

    // Write based on format
    match config.format {
        OutputFormat::Jsonl => {
            // File must exist
            if !output_path.exists() {
                eprintln!(
                    "{}",
                    WriteError::FileNotFound(output_path.display().to_string()).format(config)
                );
                process::exit(1);
            }
            if let Err(e) = append_line(output_path, &compact) {
                eprintln!("{}", e.format(config));
                process::exit(1);
            }
        }
        OutputFormat::Json => {
            // File must NOT exist
            if output_path.exists() {
                eprintln!(
                    "{}",
                    WriteError::FileExists(output_path.display().to_string()).format(config)
                );
                process::exit(1);
            }
            // Parent directory must exist
            if let Some(parent) = output_path.parent() {
                if !parent.exists() {
                    eprintln!(
                        "{}",
                        WriteError::DirectoryNotFound(parent.display().to_string()).format(config)
                    );
                    process::exit(1);
                }
            }
            // Pretty-print for json files
            let pretty = serde_json::to_string_pretty(&data).unwrap();
            if let Err(e) = write_atomic(output_path, &pretty) {
                eprintln!("{}", e.format(config));
                process::exit(1);
            }
        }
    }

    println!("OK");
    process::exit(0);
}

/// Handle batch writes (multiple JSONL lines, all-or-nothing).
fn run_batch(config: &WriterConfig, output_path: &Path, input: &str) -> ! {
    let lines: Vec<&str> = input.lines().filter(|l| !l.trim().is_empty()).collect();

    if lines.is_empty() {
        eprintln!("{}", WriteError::StdinEmpty.format(config));
        process::exit(1);
    }

    // Check batch size limit
    if let Some(max) = config.batch_size {
        if lines.len() > max {
            eprintln!(
                "{}",
                WriteError::BatchTooLarge {
                    got: lines.len(),
                    max,
                }
                .format(config)
            );
            process::exit(1);
        }
    }

    // Validate ALL records before writing ANY
    let mut validated: Vec<String> = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let result = config.schema.validate(line);
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                eprintln!(
                    "FAIL:line {} — {}",
                    i + 1,
                    WriteError::InvalidJson(e.to_string()).format(config)
                );
                process::exit(1);
            }
        };

        if !result.valid {
            eprintln!(
                "FAIL:line {} — schema validation — {}",
                i + 1,
                result.message
            );
            process::exit(1);
        }

        let data = result.data.unwrap();
        validated.push(serde_json::to_string(&data).unwrap());
    }

    // File must exist for jsonl append
    match config.format {
        OutputFormat::Jsonl => {
            if !output_path.exists() {
                eprintln!(
                    "{}",
                    WriteError::FileNotFound(output_path.display().to_string()).format(config)
                );
                process::exit(1);
            }

            // Append all validated records
            let mut file = match OpenOptions::new().append(true).open(output_path) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!(
                        "{}",
                        WriteError::IoFailed(format!("{}: {}", output_path.display(), e))
                            .format(config)
                    );
                    process::exit(1);
                }
            };

            for line in &validated {
                if let Err(e) = writeln!(file, "{}", line) {
                    eprintln!(
                        "{}",
                        WriteError::IoFailed(format!("write: {}", e)).format(config)
                    );
                    process::exit(1);
                }
            }

            if let Err(e) = file.sync_all() {
                eprintln!(
                    "{}",
                    WriteError::IoFailed(format!("fsync: {}", e)).format(config)
                );
                process::exit(1);
            }
        }
        OutputFormat::Json => {
            // Batch mode with Json format is unusual but handle it:
            // write a JSON array
            if output_path.exists() {
                eprintln!(
                    "{}",
                    WriteError::FileExists(output_path.display().to_string()).format(config)
                );
                process::exit(1);
            }
            if let Some(parent) = output_path.parent() {
                if !parent.exists() {
                    eprintln!(
                        "{}",
                        WriteError::DirectoryNotFound(parent.display().to_string()).format(config)
                    );
                    process::exit(1);
                }
            }
            let values: Vec<serde_json::Value> = validated
                .iter()
                .map(|s| serde_json::from_str(s).unwrap())
                .collect();
            let pretty = serde_json::to_string_pretty(&values).unwrap();
            if let Err(e) = write_atomic(output_path, &pretty) {
                eprintln!("{}", e.format(config));
                process::exit(1);
            }
        }
    }

    println!("OK:{}", validated.len());
    process::exit(0);
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
            schema_source_path: "test",
            format: OutputFormat::Jsonl,
            frequency: WriteFrequency::Record,
            output: OutputPath::FixedFile("/tmp/test.jsonl"),
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
            schema_source_path: "test",
            format: OutputFormat::Json,
            frequency: WriteFrequency::Record,
            output: OutputPath::DirectoryName {
                dir: "/tmp/out",
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
            schema_source_path: "test",
            format: OutputFormat::Jsonl,
            frequency: WriteFrequency::Batch,
            output: OutputPath::DirectoryPrefix {
                dir: "/tmp/out",
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
            schema_source_path: "test",
            format: OutputFormat::Json,
            frequency: WriteFrequency::Record,
            output: OutputPath::DirectoryName {
                dir: "/tmp/out",
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
            schema_source_path: "test",
            format: OutputFormat::Json,
            frequency: WriteFrequency::Record,
            output: OutputPath::DirectoryName {
                dir: "/tmp/out",
                ext: "json",
            },
            batch_size: None,
        };
        assert!(resolve_output_path(&config, None).is_err());
    }

    // Dummy validator for path resolution tests (schema content doesn't matter)
    static DUMMY_SCHEMA: &str = r#"{"type": "object"}"#;
    static DUMMY_VALIDATOR: EmbeddedValidator =
        EmbeddedValidator::new(DUMMY_SCHEMA, "test-schema");
}
