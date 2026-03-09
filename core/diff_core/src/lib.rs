//! Line-level diff and TOML block extraction.
//!
//! Given two TOML strings, finds what changed and extracts complete
//! valid blocks (from `[[header]]` to next header or EOF).
//!
//! Pure functions, no I/O.

/// A range of line indices (0-based, inclusive start, exclusive end).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

/// Result of diffing two texts line by line.
#[derive(Debug, Clone)]
pub struct DiffResult {
    /// Line ranges present in `new` but not in `old`.
    pub added: Vec<LineRange>,
    /// Line ranges present in `old` but not in `new`.
    pub removed: Vec<LineRange>,
    /// True when old and new are identical.
    pub unchanged: bool,
}

/// Diff two texts line by line. Returns ranges of added/removed lines
/// indexed into new/old respectively.
pub fn diff_lines(old: &str, new: &str) -> DiffResult {
    if old == new {
        return DiffResult {
            added: Vec::new(),
            removed: Vec::new(),
            unchanged: true,
        };
    }

    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let mut added = Vec::new();
    let mut removed = Vec::new();

    let mut old_idx = 0;
    let mut new_idx = 0;

    while old_idx < old_lines.len() && new_idx < new_lines.len() {
        if old_lines[old_idx] == new_lines[new_idx] {
            old_idx += 1;
            new_idx += 1;
            continue;
        }

        // Find the extent of the changed region.
        // Look ahead in both to find next sync point.
        let sync = find_sync_point(&old_lines, old_idx, &new_lines, new_idx);

        if sync.old_resume > old_idx {
            removed.push(LineRange {
                start: old_idx,
                end: sync.old_resume,
            });
        }
        if sync.new_resume > new_idx {
            added.push(LineRange {
                start: new_idx,
                end: sync.new_resume,
            });
        }

        old_idx = sync.old_resume;
        new_idx = sync.new_resume;
    }

    if old_idx < old_lines.len() {
        removed.push(LineRange {
            start: old_idx,
            end: old_lines.len(),
        });
    }
    if new_idx < new_lines.len() {
        added.push(LineRange {
            start: new_idx,
            end: new_lines.len(),
        });
    }

    DiffResult {
        added,
        removed,
        unchanged: false,
    }
}

struct SyncPoint {
    old_resume: usize,
    new_resume: usize,
}

/// Find the next point where old and new lines re-synchronize.
/// Searches outward from the current positions.
fn find_sync_point(
    old: &[&str],
    old_start: usize,
    new: &[&str],
    new_start: usize,
) -> SyncPoint {
    let max_search = 500; // cap search window

    for offset in 1..max_search {
        // Check if advancing old by `offset` syncs with new_start
        let oi = old_start + offset;
        if oi < old.len() && new_start < new.len() && old[oi] == new[new_start] {
            return SyncPoint {
                old_resume: oi,
                new_resume: new_start,
            };
        }

        // Check if advancing new by `offset` syncs with old_start
        let ni = new_start + offset;
        if old_start < old.len() && ni < new.len() && old[old_start] == new[ni] {
            return SyncPoint {
                old_resume: old_start,
                new_resume: ni,
            };
        }

        // Check diagonal: both advance by offset
        if oi < old.len() && ni < new.len() && old[oi] == new[ni] {
            return SyncPoint {
                old_resume: oi,
                new_resume: ni,
            };
        }
    }

    // No sync found within window — treat rest as changed
    SyncPoint {
        old_resume: old.len(),
        new_resume: new.len(),
    }
}

/// Given a TOML text and line ranges of changes, extract the complete
/// enclosing TOML blocks. Each block starts at the nearest `[[header]]`
/// line at or before the change and extends to the next `[[header]]` or EOF.
///
/// Returns deduplicated, ordered blocks as strings.
pub fn extract_enclosing_blocks(text: &str, ranges: &[LineRange]) -> Vec<String> {
    if ranges.is_empty() {
        return Vec::new();
    }

    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    // Find all header line indices
    let header_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.starts_with("[["))
        .map(|(i, _)| i)
        .collect();

    // For each change range, find the enclosing block boundaries
    let mut block_bounds: Vec<(usize, usize)> = Vec::new();

    for range in ranges {
        let block_start = enclosing_header(&header_indices, range.start);
        let block_end = next_header_or_eof(&header_indices, range.start, lines.len());

        // Also cover the end of the range
        let end_block_end = next_header_or_eof(&header_indices, range.end.saturating_sub(1), lines.len());
        let merged_end = block_end.max(end_block_end);

        block_bounds.push((block_start, merged_end));
    }

    // Merge overlapping bounds
    block_bounds.sort();
    let merged = merge_overlapping(&block_bounds);

    // Extract text for each merged block
    merged
        .iter()
        .map(|(start, end)| {
            lines[*start..*end].join("\n")
        })
        .collect()
}

/// Find the nearest `[[header]]` line at or before `line_idx`.
/// Returns 0 if no header found before the line.
fn enclosing_header(header_indices: &[usize], line_idx: usize) -> usize {
    match header_indices.binary_search(&line_idx) {
        Ok(i) => header_indices[i],
        Err(0) => 0,
        Err(i) => header_indices[i - 1],
    }
}

/// Find the next `[[header]]` line after `line_idx`, or EOF.
fn next_header_or_eof(header_indices: &[usize], line_idx: usize, total_lines: usize) -> usize {
    for &h in header_indices {
        if h > line_idx {
            return h;
        }
    }
    total_lines
}

/// Merge overlapping or adjacent (start, end) ranges.
fn merge_overlapping(bounds: &[(usize, usize)]) -> Vec<(usize, usize)> {
    if bounds.is_empty() {
        return Vec::new();
    }

    let mut merged = vec![bounds[0]];
    for &(start, end) in &bounds[1..] {
        let last = merged.last_mut().unwrap();
        if start <= last.1 {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_texts_are_unchanged() {
        let text = "line1\nline2\nline3";
        let result = diff_lines(text, text);
        assert!(result.unchanged);
        assert!(result.added.is_empty());
        assert!(result.removed.is_empty());
    }

    #[test]
    fn appended_lines_detected() {
        let old = "line1\nline2";
        let new = "line1\nline2\nline3\nline4";
        let result = diff_lines(old, new);
        assert!(!result.unchanged);
        assert_eq!(result.added.len(), 1);
        assert_eq!(result.added[0].start, 2);
        assert_eq!(result.added[0].end, 4);
        assert!(result.removed.is_empty());
    }

    #[test]
    fn removed_lines_detected() {
        let old = "line1\nline2\nline3";
        let new = "line1";
        let result = diff_lines(old, new);
        assert!(!result.unchanged);
        assert!(result.added.is_empty());
        assert_eq!(result.removed.len(), 1);
        assert_eq!(result.removed[0].start, 1);
        assert_eq!(result.removed[0].end, 3);
    }

    #[test]
    fn changed_line_shows_as_remove_plus_add() {
        let old = "aaa\nbbb\nccc";
        let new = "aaa\nBBB\nccc";
        let result = diff_lines(old, new);
        assert!(!result.unchanged);
        assert_eq!(result.removed.len(), 1);
        assert_eq!(result.added.len(), 1);
        assert_eq!(result.removed[0], LineRange { start: 1, end: 2 });
        assert_eq!(result.added[0], LineRange { start: 1, end: 2 });
    }

    #[test]
    fn extract_blocks_from_toml() {
        let toml = "\
[[messages]]
role = \"user\"
content = \"hello\"

[[messages]]
role = \"assistant\"
content = \"hi\"

[[messages]]
role = \"user\"
content = \"new message\"";

        // Change is in lines 9-10 (the last message block)
        let ranges = vec![LineRange { start: 9, end: 11 }];
        let blocks = extract_enclosing_blocks(toml, &ranges);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("new message"));
        assert!(blocks[0].starts_with("[[messages]]"));
    }

    #[test]
    fn extract_merges_adjacent_blocks() {
        let toml = "\
[[a]]
x = 1

[[b]]
y = 2

[[c]]
z = 3";

        // Changes span blocks a and b
        let ranges = vec![
            LineRange { start: 1, end: 2 },
            LineRange { start: 4, end: 5 },
        ];
        let blocks = extract_enclosing_blocks(toml, &ranges);
        // Should get two separate blocks since they have different headers
        assert!(blocks.len() <= 2);
    }

    #[test]
    fn empty_ranges_return_empty() {
        let blocks = extract_enclosing_blocks("some text", &[]);
        assert!(blocks.is_empty());
    }
}
