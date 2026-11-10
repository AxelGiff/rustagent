use crate::myers;
use similar::{Algorithm, ChangeTag, TextDiff};

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
    fn test_matches_open_editor() {
        assert!(matches_open_editor("src/main.rs", "src/main.rs"));
        assert!(matches_open_editor("C:/app/src/main.rs", "src/main.rs"));
        assert!(matches_open_editor("src/main.rs", "C:/app/src/main.rs"));
        assert!(!matches_open_editor("src/main.rs", "src/lib.rs"));
    }
}
