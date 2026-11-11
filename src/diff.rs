use crate::myers;
use similar::{Algorithm, ChangeTag, TextDiff};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq)]
pub enum DiffLine {
    Unchanged(String),
    Added(String),
    Deleted(String),
}

/// Review state of a modified file in the conversation's review box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffStatus {
    #[default]
    Pending,
    Accepted,
    Rejected,
}

/// A modification proposed by the AI assistant that can be reviewed, accepted, or rejected.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingDiff {
    pub id: String,
    pub path: String,
    pub old_content: String,
    pub new_content: String,
    pub diff_lines: Vec<DiffLine>,
    pub status: DiffStatus,
}

/// A contiguous or near-contiguous block of modifications (Hunk) within a [`PendingDiff`].
#[derive(Debug, Clone, PartialEq)]
pub struct DiffHunk {
    /// 0-based hunk index within the file.
    pub index: usize,
    /// Range of indices `[start..end)` in `diff.diff_lines` covered by this hunk.
    pub line_range: Range<usize>,
    /// 1-based starting line number in the old file.
    pub old_start_line: usize,
    /// 1-based starting line number in the new file.
    pub new_start_line: usize,
    /// Number of added lines in this hunk.
    pub added: usize,
    /// Number of deleted lines in this hunk.
    pub deleted: usize,
}

/// A single row in a Side-by-Side (Split) diff view: `(old_line_num, old_text, is_deleted)` and `(new_line_num, new_text, is_added)`.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitDiffRow {
    pub left_line: Option<usize>,
    pub left_text: String,
    pub left_deleted: bool,
    pub right_line: Option<usize>,
    pub right_text: String,
    pub right_added: bool,
}

/// Compute diff using Myers' algorithm (via `similar` crate configured with `Algorithm::Myers`).
pub fn computed_diff(old_text: &str, new_text: &str) -> Vec<DiffLine> {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Myers)
        .diff_lines(old_text, new_text);

    let mut changes = Vec::new();

    for change in diff.iter_all_changes() {
        let line = change.value().trim_end_matches(['\n', '\r']).to_string();
        match change.tag() {
            ChangeTag::Delete => changes.push(DiffLine::Deleted(line)),
            ChangeTag::Insert => changes.push(DiffLine::Added(line)),
            ChangeTag::Equal => changes.push(DiffLine::Unchanged(line)),
        }
    }
    changes
}

/// Groups `diff_lines` into discrete [`DiffHunk`]s separated by at least `context_gap` (e.g. 3) unchanged lines.
pub fn extract_hunks(diff_lines: &[DiffLine]) -> Vec<DiffHunk> {
    const GAP_THRESHOLD: usize = 3;
    let mut hunks: Vec<DiffHunk> = Vec::new();

    let mut old_ln = 1usize;
    let mut new_ln = 1usize;

    // Precompute `(old_ln_at_i, new_ln_at_i)` for each index in `diff_lines`.
    let mut line_coords = Vec::with_capacity(diff_lines.len());
    for line in diff_lines {
        line_coords.push((old_ln, new_ln));
        match line {
            DiffLine::Deleted(_) => old_ln += 1,
            DiffLine::Added(_) => new_ln += 1,
            DiffLine::Unchanged(_) => {
                old_ln += 1;
                new_ln += 1;
            }
        }
    }

    let mut i = 0;
    while i < diff_lines.len() {
        if matches!(diff_lines[i], DiffLine::Unchanged(_)) {
            i += 1;
            continue;
        }

        let start = i;
        let (old_start_line, new_start_line) = line_coords[start];
        let mut end = i + 1;
        let mut unchanged_run = 0usize;

        let mut scan = i + 1;
        while scan < diff_lines.len() {
            if matches!(diff_lines[scan], DiffLine::Unchanged(_)) {
                unchanged_run += 1;
                if unchanged_run >= GAP_THRESHOLD {
                    break;
                }
            } else {
                unchanged_run = 0;
                end = scan + 1;
            }
            scan += 1;
        }

        let mut added = 0usize;
        let mut deleted = 0usize;
        for item in &diff_lines[start..end] {
            match item {
                DiffLine::Added(_) => added += 1,
                DiffLine::Deleted(_) => deleted += 1,
                DiffLine::Unchanged(_) => {}
            }
        }

        let index = hunks.len();
        hunks.push(DiffHunk {
            index,
            line_range: start..end,
            old_start_line,
            new_start_line,
            added,
            deleted,
        });

        i = end;
    }

    hunks
}

/// Pairs `diff_lines[range]` into side-by-side [`SplitDiffRow`]s for the Split Diff view.
pub fn build_split_rows_for_slice(
    diff_lines: &[DiffLine],
    range: Range<usize>,
    mut old_ln: usize,
    mut new_ln: usize,
) -> (Vec<SplitDiffRow>, usize, usize) {
    let mut rows = Vec::new();
    let slice = &diff_lines[range];
    let mut i = 0;

    while i < slice.len() {
        match &slice[i] {
            DiffLine::Unchanged(text) => {
                rows.push(SplitDiffRow {
                    left_line: Some(old_ln),
                    left_text: text.clone(),
                    left_deleted: false,
                    right_line: Some(new_ln),
                    right_text: text.clone(),
                    right_added: false,
                });
                old_ln += 1;
                new_ln += 1;
                i += 1;
            }
            DiffLine::Deleted(_) | DiffLine::Added(_) => {
                let mut dels = Vec::new();
                let mut adds = Vec::new();
                while i < slice.len() && !matches!(slice[i], DiffLine::Unchanged(_)) {
                    match &slice[i] {
                        DiffLine::Deleted(t) => {
                            dels.push((old_ln, t.clone()));
                            old_ln += 1;
                        }
                        DiffLine::Added(t) => {
                            adds.push((new_ln, t.clone()));
                            new_ln += 1;
                        }
                        DiffLine::Unchanged(_) => unreachable!(),
                    }
                    i += 1;
                }
                let pair_len = dels.len().max(adds.len());
                for p in 0..pair_len {
                    let (l_num, l_txt, l_del) = if let Some((ln, txt)) = dels.get(p) {
                        (Some(*ln), txt.clone(), true)
                    } else {
                        (None, String::new(), false)
                    };
                    let (r_num, r_txt, r_add) = if let Some((ln, txt)) = adds.get(p) {
                        (Some(*ln), txt.clone(), true)
                    } else {
                        (None, String::new(), false)
                    };
                    rows.push(SplitDiffRow {
                        left_line: l_num,
                        left_text: l_txt,
                        left_deleted: l_del,
                        right_line: r_num,
                        right_text: r_txt,
                        right_added: r_add,
                    });
                }
            }
        }
    }

    (rows, old_ln, new_ln)
}

/// Applies a single [`DiffHunk`] (`hunk_idx`) to disk and updates `diff.old_content` + `diff.diff_lines`.
/// If all hunks have been applied, marks `diff.status = DiffStatus::Accepted`.
pub fn accept_hunk_to_disk(diff: &mut PendingDiff, hunk_idx: usize) -> Result<(), String> {
    let hunks = extract_hunks(&diff.diff_lines);
    let Some(hunk) = hunks.get(hunk_idx) else {
        return Ok(());
    };
    let range = hunk.line_range.clone();

    let mut updated_baseline_lines = Vec::new();
    for (idx, line) in diff.diff_lines.iter().enumerate() {
        let in_hunk = range.contains(&idx);
        match line {
            DiffLine::Unchanged(s) => updated_baseline_lines.push(s.as_str()),
            DiffLine::Added(s) if in_hunk => updated_baseline_lines.push(s.as_str()),
            DiffLine::Deleted(s) if !in_hunk => updated_baseline_lines.push(s.as_str()),
            _ => {}
        }
    }

    let mut new_baseline = updated_baseline_lines.join("\n");
    if diff.new_content.ends_with('\n') && !new_baseline.is_empty() {
        new_baseline.push('\n');
    }

    if let Some(parent) = std::path::Path::new(&diff.path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&diff.path, &new_baseline)
        .map_err(|e| format!("⚠️ Impossible d'écrire le fichier '{}': {}", diff.path, e))?;
    myers::log_myers_diff(&diff.path, &diff.old_content, &new_baseline);

    diff.old_content = new_baseline;
    diff.diff_lines = computed_diff(&diff.old_content, &diff.new_content);
    if diff_stats(diff) == (0, 0) {
        diff.status = DiffStatus::Accepted;
    }
    Ok(())
}

/// Rejects a single [`DiffHunk`] (`hunk_idx`) by reverting its changes in `diff.new_content`
/// and recomputing `diff.diff_lines`. If no changes remain, updates `diff.status`.
pub fn reject_hunk(diff: &mut PendingDiff, hunk_idx: usize) {
    let hunks = extract_hunks(&diff.diff_lines);
    let Some(hunk) = hunks.get(hunk_idx) else {
        return;
    };
    let range = hunk.line_range.clone();

    let mut updated_new_lines = Vec::new();
    for (idx, line) in diff.diff_lines.iter().enumerate() {
        let in_hunk = range.contains(&idx);
        match line {
            DiffLine::Unchanged(s) => updated_new_lines.push(s.as_str()),
            DiffLine::Deleted(s) if in_hunk => updated_new_lines.push(s.as_str()),
            DiffLine::Added(s) if !in_hunk => updated_new_lines.push(s.as_str()),
            _ => {}
        }
    }

    let mut updated_new = updated_new_lines.join("\n");
    if diff.old_content.ends_with('\n') && !updated_new.is_empty() {
        updated_new.push('\n');
    }

    diff.new_content = updated_new;
    diff.diff_lines = computed_diff(&diff.old_content, &diff.new_content);
    if diff_stats(diff) == (0, 0) {
        diff.status = DiffStatus::Rejected;
    }
}

/// Applies a pending diff by writing the new content to disk and logging the Myers audit trail.
pub fn apply_diff_to_disk(diff: &PendingDiff) -> Result<(), String> {
    if let Some(parent) = std::path::Path::new(&diff.path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&diff.path, &diff.new_content)
        .map_err(|e| format!("⚠️ Impossible d'écrire le fichier '{}': {}", diff.path, e))?;
    myers::log_myers_diff(&diff.path, &diff.old_content, &diff.new_content);
    Ok(())
}

/// Checks whether an open editor buffer corresponds to the modified diff file path.
pub fn matches_open_editor(diff_path: &str, open_file: &str) -> bool {
    open_file == diff_path || open_file.ends_with(diff_path) || diff_path.ends_with(open_file)
}

/// Takes a pending diff by index or the first available diff if index is out of bounds.
pub fn take_diff(diffs: &mut Vec<PendingDiff>, idx: usize) -> Option<PendingDiff> {
    if idx < diffs.len() {
        Some(diffs.remove(idx))
    } else if !diffs.is_empty() {
        Some(diffs.remove(0))
    } else {
        None
    }
}

/// Returns `(added_lines, deleted_lines)` for a [`PendingDiff`].
pub fn diff_stats(diff: &PendingDiff) -> (usize, usize) {
    let mut added = 0;
    let mut deleted = 0;
    for l in &diff.diff_lines {
        match l {
            DiffLine::Added(_) => added += 1,
            DiffLine::Deleted(_) => deleted += 1,
            DiffLine::Unchanged(_) => {}
        }
    }
    (added, deleted)
}

/// Inserts a new [`PendingDiff`] or merges it into an existing entry for the same file path,
/// preserving the original `old_content` baseline and recomputing the diff against the latest `new_content`.
pub fn upsert_diff(diffs: &mut Vec<PendingDiff>, new_diff: PendingDiff) {
    let norm_new = new_diff.path.replace('\\', "/");
    if let Some(existing) = diffs
        .iter_mut()
        .find(|d| d.path.replace('\\', "/") == norm_new)
    {
        if existing.status == DiffStatus::Accepted {
            existing.old_content = new_diff.old_content;
        }
        existing.new_content = new_diff.new_content;
        existing.diff_lines = computed_diff(&existing.old_content, &existing.new_content);
        existing.status = DiffStatus::Pending;
    } else {
        diffs.push(new_diff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_computed_diff_myers() {
        let old_text = "fn main() {\n    println!(\"hello\");\n}";
        let new_text = "fn main() {\n    println!(\"hello world\");\n}";
        let diffs = computed_diff(old_text, new_text);
        assert!(diffs.iter().any(|d| matches!(d, DiffLine::Added(_))));
        assert!(diffs.iter().any(|d| matches!(d, DiffLine::Deleted(_))));
    }

    #[test]
    fn test_extract_hunks_and_reject_hunk() {
        let old_text = "line1\nold_a\nline3\nline4\nline5\nline6\nold_b\nline8";
        let new_text = "line1\nnew_a\nline3\nline4\nline5\nline6\nnew_b\nline8";
        let diff_lines = computed_diff(old_text, new_text);
        let hunks = extract_hunks(&diff_lines);
        assert_eq!(hunks.len(), 2);

        let mut pending = PendingDiff {
            id: "d1".to_string(),
            path: "test.txt".to_string(),
            old_content: old_text.to_string(),
            new_content: new_text.to_string(),
            diff_lines,
            status: DiffStatus::Pending,
        };
        reject_hunk(&mut pending, 0);
        let remaining = extract_hunks(&pending.diff_lines);
        assert_eq!(remaining.len(), 1);
        assert!(pending.new_content.contains("old_a"));
        assert!(pending.new_content.contains("new_b"));
    }

    #[test]
    fn test_matches_open_editor() {
        assert!(matches_open_editor("src/main.rs", "src/main.rs"));
        assert!(matches_open_editor("C:/app/src/main.rs", "src/main.rs"));
        assert!(matches_open_editor("src/main.rs", "C:/app/src/main.rs"));
        assert!(!matches_open_editor("src/main.rs", "src/lib.rs"));
    }
}
