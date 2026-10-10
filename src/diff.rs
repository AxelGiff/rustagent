use similar::{Algorithm, ChangeTag, TextDiff};

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
}