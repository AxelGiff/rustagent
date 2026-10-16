use similar::{Algorithm, ChangeTag, TextDiff};
use crate::myers;

#[derive(Debug, Clone, PartialEq)]
pub enum DiffLine {
    Unchanged(String),
    Added(String),
    Deleted(String),
}

/// A pending modification proposed by the AI assistant that awaits explicit user validation.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingDiff {
    pub id: String,
    pub path: String,
    pub old_content: String,
    pub new_content: String,
    pub diff_lines: Vec<DiffLine>,
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