//! Dispatcher utility: split_jsonl_batches
//! Splits a JSONL file into optimally-sized batch files for parallel agent dispatch.

use clap::Parser;
use serde::Serialize;
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::process;

// =============================================================================
// Types
// =============================================================================

/// Split a JSONL file into optimally-sized batches for parallel agent dispatch.
#[derive(Debug, Parser)]
#[command(name = "split_jsonl_batches")]
struct Args {
    /// Path to input .jsonl file
    #[arg(long)]
    input: PathBuf,

    /// Temp directory name (created under /tmp/)
    #[arg(long)]
    directory: String,

    /// Minimum records per batch
    #[arg(long)]
    min_batch: usize,

    /// Maximum records per batch
    #[arg(long)]
    max_batch: usize,
}

fn validate_args(args: &Args) -> Result<(), String> {
    if args.min_batch == 0 {
        return Err("--min-batch must be at least 1".into());
    }
    if args.max_batch == 0 {
        return Err("--max-batch must be at least 1".into());
    }
    if args.min_batch > args.max_batch {
        return Err(format!(
            "--min-batch ({}) cannot be greater than --max-batch ({})",
            args.min_batch, args.max_batch
        ));
    }
    validate_directory_name(&args.directory)?;
    Ok(())
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

/// Delegates to path_core::validate_path_segment (shared across workspace).
fn validate_directory_name(directory: &str) -> Result<(), String> {
    path_core::validate_path_segment(directory)
}

// =============================================================================
// Core logic
// =============================================================================

fn read_validated_lines(input: &std::path::Path) -> Result<Vec<String>, String> {
    if !input.exists() {
        return Err(format!("input file does not exist — {}", input.display()));
    }

    let file = fs::File::open(input).map_err(|e| format!("cannot read input file — {e}"))?;
    let reader = BufReader::new(file);

    let mut lines: Vec<String> = Vec::new();
    for (i, line_result) in reader.lines().enumerate() {
        let line = line_result.map_err(|e| format!("cannot read line {} — {e}", i + 1))?;
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }
        serde_json::from_str::<serde_json::Value>(&trimmed).map_err(|e| {
            let preview = if trimmed.len() > 80 { &trimmed[..80] } else { &trimmed };
            format!("line {} is not valid JSON — {e}\n  Content: {preview}", i + 1)
        })?;
        lines.push(trimmed);
    }

    if lines.is_empty() {
        return Err("0 records in input file — nothing to split".to_string());
    }
    Ok(lines)
}

fn write_batch(lines: &[String], path: &std::path::Path) -> Result<(), String> {
    let file = fs::File::create(path)
        .map_err(|e| format!("cannot create batch file {} — {e}", path.display()))?;
    let mut writer = BufWriter::new(file);

    for line in lines {
        writeln!(writer, "{line}")
            .map_err(|e| format!("cannot write to {} — {e}", path.display()))?;
    }

    writer.flush().map_err(|e| format!("cannot flush {} — {e}", path.display()))?;
    let inner = writer.into_inner().map_err(|e| format!("cannot finalize {} — {e}", path.display()))?;
    inner.sync_all().map_err(|e| format!("cannot fsync {} — {e}", path.display()))?;
    Ok(())
}

fn run(args: &Args) -> Result<(), String> {
    let lines = read_validated_lines(&args.input)?;
    let batch_sizes = compute_batches(lines.len(), args.min_batch, args.max_batch);

    let output_dir = PathBuf::from("/tmp").join(&args.directory);
    fs::create_dir_all(&output_dir)
        .map_err(|e| format!("cannot create directory /tmp/{} — {e}", args.directory))?;

    let mut manifest: Vec<ManifestEntry> = Vec::with_capacity(batch_sizes.len());
    let mut offset = 0;

    for (i, &size) in batch_sizes.iter().enumerate() {
        let batch_num = i + 1;
        let path = output_dir.join(format!("batch_{:03}.jsonl", batch_num));
        write_batch(&lines[offset..offset + size], &path)?;
        manifest.push(ManifestEntry {
            batch: batch_num,
            file: path.to_string_lossy().to_string(),
            records: size,
        });
        offset += size;
    }

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
    let args = Args::parse();

    if let Err(e) = validate_args(&args) {
        eprintln!("FAIL:{e}");
        process::exit(1);
    }

    match run(&args) {
        Ok(()) => process::exit(0),
        Err(e) => {
            eprintln!("FAIL:{e}");
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

    // -- arg parsing (clap + validate_args) --

    fn parse_and_validate(items: &[&str]) -> Result<Args, String> {
        let args = Args::try_parse_from(items).map_err(|e| e.to_string())?;
        validate_args(&args)?;
        Ok(args)
    }

    #[test]
    fn test_parse_valid_args() {
        let args = parse_and_validate(&[
            "split_jsonl_batches",
            "--input", "/path/to/file.jsonl",
            "--directory", "f3c67b",
            "--min-batch", "35",
            "--max-batch", "50",
        ]).unwrap();
        assert_eq!(args.input, PathBuf::from("/path/to/file.jsonl"));
        assert_eq!(args.directory, "f3c67b");
        assert_eq!(args.min_batch, 35);
        assert_eq!(args.max_batch, 50);
    }

    #[test]
    fn test_parse_missing_input() {
        let result = parse_and_validate(&[
            "split_jsonl_batches",
            "--directory", "abc", "--min-batch", "10", "--max-batch", "20",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_min_greater_than_max() {
        let err = parse_and_validate(&[
            "split_jsonl_batches",
            "--input", "/f.jsonl",
            "--directory", "abc",
            "--min-batch", "60",
            "--max-batch", "50",
        ]).unwrap_err();
        assert!(err.contains("cannot be greater than"));
    }

    #[test]
    fn test_parse_path_traversal_dotdot() {
        let err = parse_and_validate(&[
            "split_jsonl_batches",
            "--input", "/f.jsonl",
            "--directory", "../etc",
            "--min-batch", "10",
            "--max-batch", "20",
        ]).unwrap_err();
        assert!(err.contains("forbidden"));
    }

    #[test]
    fn test_parse_path_traversal_slash() {
        let err = parse_and_validate(&[
            "split_jsonl_batches",
            "--input", "/f.jsonl",
            "--directory", "foo/bar",
            "--min-batch", "10",
            "--max-batch", "20",
        ]).unwrap_err();
        assert!(err.contains("forbidden"));
    }

    #[test]
    fn test_parse_zero_batch_size() {
        let err = parse_and_validate(&[
            "split_jsonl_batches",
            "--input", "/f.jsonl",
            "--directory", "abc",
            "--min-batch", "0",
            "--max-batch", "50",
        ]).unwrap_err();
        assert!(err.contains("at least 1"));
    }
}
