//! Dispatcher utility: split_jsonl_batches
//! Splits a JSONL file into optimally-sized batch files for parallel agent dispatch.

use serde::Serialize;
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::process;

// =============================================================================
// Types
// =============================================================================

#[derive(Debug)]
struct Config {
    input: PathBuf,
    directory: String,
    min_batch: usize,
    max_batch: usize,
}

#[derive(Serialize)]
struct ManifestEntry {
    batch: usize,
    file: String,
    records: usize,
}

// =============================================================================
// Batch computation
// =============================================================================

/// Compute optimal batch sizes: fewest batches, all in [min, max] when possible.
///
/// Two strategies:
/// 1. If all batches can be >= min: distribute evenly (all valid, no tail).
/// 2. If not: fill (N-1) batches at min, one tail batch absorbs the remainder.
///    At most one batch (the tail) can be undersized — that's acceptable.
fn compute_batches(total: usize, min_batch: usize, max_batch: usize) -> Vec<usize> {
    if total == 0 {
        return vec![];
    }
    if total <= max_batch {
        return vec![total];
    }

    // Fewest batches where each batch <= max_batch
    let num_batches = (total + max_batch - 1) / max_batch;

    if num_batches * min_batch <= total {
        // Valid split: all batches can be >= min_batch. Distribute evenly.
        let base = total / num_batches;
        let remainder = total % num_batches;
        (0..num_batches)
            .map(|i| if i < remainder { base + 1 } else { base })
            .collect()
    } else {
        // Can't fill all batches to min. Fill (N-1) batches at min_batch,
        // one tail batch absorbs the remainder. Concentrates the deficit
        // in a single tail rather than spreading it across all batches.
        let main_count = num_batches - 1;
        let tail = total - main_count * min_batch;
        let mut batches = vec![min_batch; main_count];
        batches.push(tail);
        batches
    }
}

// =============================================================================
// Arg parsing
// =============================================================================

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut input = None;
    let mut directory = None;
    let mut min_batch = None;
    let mut max_batch = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--input" => {
                i += 1;
                input = Some(PathBuf::from(
                    args.get(i).ok_or("--input requires a value")?,
                ));
            }
            "--directory" => {
                i += 1;
                directory = Some(args.get(i).ok_or("--directory requires a value")?.clone());
            }
            "--min-batch" => {
                i += 1;
                let val = args.get(i).ok_or("--min-batch requires a value")?;
                min_batch = Some(val.parse::<usize>().map_err(|_| {
                    format!("--min-batch must be a positive integer, got '{val}'")
                })?);
            }
            "--max-batch" => {
                i += 1;
                let val = args.get(i).ok_or("--max-batch requires a value")?;
                max_batch = Some(val.parse::<usize>().map_err(|_| {
                    format!("--max-batch must be a positive integer, got '{val}'")
                })?);
            }
            other => {
                return Err(format!(
                    "unknown argument '{other}'\n  Run with --help for usage"
                ));
            }
        }
        i += 1;
    }

    let input = input.ok_or("--input is required")?;
    let directory = directory.ok_or("--directory is required")?;
    let min_batch = min_batch.ok_or("--min-batch is required")?;
    let max_batch = max_batch.ok_or("--max-batch is required")?;

    if min_batch == 0 {
        return Err("--min-batch must be at least 1".to_string());
    }
    if max_batch == 0 {
        return Err("--max-batch must be at least 1".to_string());
    }
    if min_batch > max_batch {
        return Err(format!(
            "--min-batch ({min_batch}) cannot be greater than --max-batch ({max_batch})"
        ));
    }

    // Path traversal protection on directory name
    if directory.contains("..") {
        return Err(
            "directory name must not contain \"..\" — path traversal blocked".to_string(),
        );
    }
    if directory.contains('/') || directory.contains('\\') {
        return Err(
            "directory name must not contain path separators — path traversal blocked".to_string(),
        );
    }
    if directory.contains('\0') {
        return Err("directory name must not contain null bytes".to_string());
    }
    if directory.is_empty() {
        return Err("directory name cannot be empty".to_string());
    }

    Ok(Config {
        input,
        directory,
        min_batch,
        max_batch,
    })
}

// =============================================================================
// Help
// =============================================================================

fn print_help() {
    let help = "\
split_jsonl_batches — Split a JSONL file into optimally-sized batches

USAGE:
  split_jsonl_batches --input FILE --directory DIR --min-batch N --max-batch M

ARGUMENTS:
  --input FILE       Path to input .jsonl file
  --directory DIR    Temp directory name (created under /tmp/)
  --min-batch N      Minimum records per batch
  --max-batch M      Maximum records per batch

OUTPUT:
  Writes batch files to /tmp/DIR/batch_001.jsonl, batch_002.jsonl, ...
  Outputs JSONL manifest to stdout (one line per batch):
    {\"batch\":1,\"file\":\"/tmp/DIR/batch_001.jsonl\",\"records\":50}

OPTIMIZATION:
  Minimizes number of batches while keeping all batch sizes in [min, max].
  Records are distributed as evenly as possible across batches.

EXIT CODES:
  0  success (manifest on stdout)
  1  failure (FAIL:<reason> on stdout)";
    eprintln!("{help}");
}

// =============================================================================
// Core logic
// =============================================================================

fn run(config: &Config) -> Result<(), String> {
    // 1. Read input file
    if !config.input.exists() {
        return Err(format!(
            "input file does not exist — {}",
            config.input.display()
        ));
    }

    let file =
        fs::File::open(&config.input).map_err(|e| format!("cannot read input file — {e}"))?;
    let reader = BufReader::new(file);

    // 2. Read and validate all JSON lines
    let mut lines: Vec<String> = Vec::new();
    for (i, line_result) in reader.lines().enumerate() {
        let line = line_result.map_err(|e| format!("cannot read line {} — {e}", i + 1))?;
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }
        // Validate JSON
        serde_json::from_str::<serde_json::Value>(&trimmed).map_err(|e| {
            let preview = if trimmed.len() > 80 {
                &trimmed[..80]
            } else {
                &trimmed
            };
            format!("line {} is not valid JSON — {e}\n  Content: {preview}", i + 1)
        })?;
        lines.push(trimmed);
    }

    let total = lines.len();
    if total == 0 {
        return Err("0 records in input file — nothing to split".to_string());
    }

    // 3. Compute batch sizes
    let batch_sizes = compute_batches(total, config.min_batch, config.max_batch);

    // 4. Create output directory
    let output_dir = PathBuf::from("/tmp").join(&config.directory);
    fs::create_dir_all(&output_dir)
        .map_err(|e| format!("cannot create directory /tmp/{} — {e}", config.directory))?;

    // 5. Write batch files
    let mut manifest: Vec<ManifestEntry> = Vec::with_capacity(batch_sizes.len());
    let mut offset = 0;

    for (i, &size) in batch_sizes.iter().enumerate() {
        let batch_num = i + 1;
        let filename = format!("batch_{:03}.jsonl", batch_num);
        let path = output_dir.join(&filename);

        let file = fs::File::create(&path)
            .map_err(|e| format!("cannot create batch file {} — {e}", path.display()))?;
        let mut writer = BufWriter::new(file);

        for line in &lines[offset..offset + size] {
            writeln!(writer, "{line}")
                .map_err(|e| format!("cannot write to {} — {e}", path.display()))?;
        }

        // Flush and fsync
        writer
            .flush()
            .map_err(|e| format!("cannot flush {} — {e}", path.display()))?;
        let inner = writer
            .into_inner()
            .map_err(|e| format!("cannot finalize {} — {e}", path.display()))?;
        inner
            .sync_all()
            .map_err(|e| format!("cannot fsync {} — {e}", path.display()))?;

        manifest.push(ManifestEntry {
            batch: batch_num,
            file: path.to_string_lossy().to_string(),
            records: size,
        });

        offset += size;
    }

    // 6. Output manifest to stdout
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for entry in &manifest {
        let json = serde_json::to_string(entry)
            .map_err(|e| format!("cannot serialize manifest entry — {e}"))?;
        writeln!(out, "{json}").map_err(|e| format!("cannot write manifest — {e}"))?;
    }

    Ok(())
}

// =============================================================================
// Entry point
// =============================================================================

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        process::exit(0);
    }

    let config = match parse_args(&args[1..]) {
        Ok(c) => c,
        Err(e) => {
            println!("FAIL:{e}");
            process::exit(1);
        }
    };

    match run(&config) {
        Ok(()) => process::exit(0),
        Err(e) => {
            println!("FAIL:{e}");
            process::exit(1);
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- compute_batches: valid splits (all batches >= min) --

    #[test]
    fn test_exact_division() {
        // 100 records, [25, 50] → 2 batches of 50
        let batches = compute_batches(100, 25, 50);
        assert_eq!(batches, vec![50, 50]);
    }

    #[test]
    fn test_even_distribution() {
        // 387 records, [35, 50] → 8 batches, even distribution
        // 8*35=280 <= 387, valid split. 387/8 = 48 r 3 → 3×49 + 5×48
        let batches = compute_batches(387, 35, 50);
        assert_eq!(batches.len(), 8);
        assert_eq!(batches.iter().sum::<usize>(), 387);
        assert_eq!(batches[0], 49);
        assert_eq!(batches[2], 49);
        assert_eq!(batches[3], 48);
        assert!(batches.iter().all(|&s| s >= 35 && s <= 50));
    }

    #[test]
    fn test_even_split_exact() {
        // 200 records, [40, 50] → 4 batches of 50
        let batches = compute_batches(200, 40, 50);
        assert_eq!(batches, vec![50, 50, 50, 50]);
    }

    #[test]
    fn test_large_input() {
        // 10000 records, [90, 100] → 100 batches of 100
        let batches = compute_batches(10000, 90, 100);
        assert_eq!(batches.len(), 100);
        assert!(batches.iter().all(|&s| s == 100));
    }

    // -- compute_batches: tail batch (can't fill all to min) --

    #[test]
    fn test_tail_batch_78_in_40_50() {
        // 78 records, [40, 50] → ceil(78/50)=2, 2*40=80 > 78
        // Tail strategy: [40, 38]. One undersized tail.
        let batches = compute_batches(78, 40, 50);
        assert_eq!(batches, vec![40, 38]);
        assert_eq!(batches.iter().sum::<usize>(), 78);
    }

    #[test]
    fn test_tail_batch_158_in_40_50() {
        // 158 records, [40, 50] → ceil(158/50)=4, 4*40=160 > 158
        // Tail strategy: [40, 40, 40, 38]. One undersized tail.
        let batches = compute_batches(158, 40, 50);
        assert_eq!(batches, vec![40, 40, 40, 38]);
        assert_eq!(batches.iter().sum::<usize>(), 158);
    }

    #[test]
    fn test_tail_batch_89_in_45_50() {
        // 89 records, [45, 50] → ceil(89/50)=2, 2*45=90 > 89
        // Tail strategy: [45, 44]. One undersized tail.
        let batches = compute_batches(89, 45, 50);
        assert_eq!(batches, vec![45, 44]);
        assert_eq!(batches.iter().sum::<usize>(), 89);
    }

    // -- compute_batches: edge cases --

    #[test]
    fn test_single_batch_under_max() {
        // 30 records, [35, 50] → single batch (30 < max)
        let batches = compute_batches(30, 35, 50);
        assert_eq!(batches, vec![30]);
    }

    #[test]
    fn test_single_batch_exact_max() {
        let batches = compute_batches(50, 35, 50);
        assert_eq!(batches, vec![50]);
    }

    #[test]
    fn test_zero_records() {
        let batches = compute_batches(0, 35, 50);
        assert!(batches.is_empty());
    }

    #[test]
    fn test_one_record() {
        let batches = compute_batches(1, 35, 50);
        assert_eq!(batches, vec![1]);
    }

    // -- parse_args --

    #[test]
    fn test_parse_valid_args() {
        let args: Vec<String> = vec![
            "--input",
            "/path/to/file.jsonl",
            "--directory",
            "f3c67b",
            "--min-batch",
            "35",
            "--max-batch",
            "50",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        let config = parse_args(&args).unwrap();
        assert_eq!(config.input, PathBuf::from("/path/to/file.jsonl"));
        assert_eq!(config.directory, "f3c67b");
        assert_eq!(config.min_batch, 35);
        assert_eq!(config.max_batch, 50);
    }

    #[test]
    fn test_parse_missing_input() {
        let args: Vec<String> = vec!["--directory", "abc", "--min-batch", "10", "--max-batch", "20"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_min_greater_than_max() {
        let args: Vec<String> = vec![
            "--input",
            "/f.jsonl",
            "--directory",
            "abc",
            "--min-batch",
            "60",
            "--max-batch",
            "50",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let err = parse_args(&args).unwrap_err();
        assert!(err.contains("cannot be greater than"));
    }

    #[test]
    fn test_parse_path_traversal_dotdot() {
        let args: Vec<String> = vec![
            "--input",
            "/f.jsonl",
            "--directory",
            "../etc",
            "--min-batch",
            "10",
            "--max-batch",
            "20",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let err = parse_args(&args).unwrap_err();
        assert!(err.contains("path traversal"));
    }

    #[test]
    fn test_parse_path_traversal_slash() {
        let args: Vec<String> = vec![
            "--input",
            "/f.jsonl",
            "--directory",
            "foo/bar",
            "--min-batch",
            "10",
            "--max-batch",
            "20",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let err = parse_args(&args).unwrap_err();
        assert!(err.contains("path traversal"));
    }

    #[test]
    fn test_parse_zero_batch_size() {
        let args: Vec<String> = vec![
            "--input",
            "/f.jsonl",
            "--directory",
            "abc",
            "--min-batch",
            "0",
            "--max-batch",
            "50",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let err = parse_args(&args).unwrap_err();
        assert!(err.contains("at least 1"));
    }
}
