//! File-tree helpers for the sidebar panel.
//!
//! This module contains the pure, platform-agnostic logic for scanning a
//! directory and describing its contents. The rendering itself lives in
//! `main.rs` (`file_tree_panel`), but the data model and the directory scan
//! are kept here so they can be unit-tested in isolation.

use std::path::{Path, PathBuf};

/// A single entry (file or directory) inside a directory listing.
#[derive(Clone, Debug, PartialEq)]
pub struct FileEntry {
    /// The entry's file name (the last path component).
    pub name: String,
    /// The full path to the entry.
    pub path: PathBuf,
    /// Whether this entry is a directory (as opposed to a regular file).
    pub is_dir: bool,
}

/// List the immediate children of a directory.
///
/// Entries are sorted with directories first, then files, each ordered
/// alphabetically (case-insensitively). Unreadable or missing directories
/// yield an empty list rather than an error, so the UI never panics.
pub fn list_directory(path: &Path) -> Vec<FileEntry> {
    let mut entries = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(path) {
        for entry in read_dir.flatten() {
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            entries.push(FileEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                path: entry.path(),
                is_dir,
            });
        }
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

/// The display name for a directory, falling back to the full path when the
/// name is empty (e.g. the filesystem root).
pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_directory_sorts_directories_first_then_files() {
        let dir = std::env::temp_dir().join(format!("rustagent_tree_test_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("zeta_dir")).unwrap();
        std::fs::create_dir_all(dir.join("alpha_dir")).unwrap();
        std::fs::write(dir.join("beta_file.txt"), "x").unwrap();
        std::fs::write(dir.join("alpha_file.txt"), "x").unwrap();

        let entries = list_directory(&dir);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();

        // Directories first (alphabetical), then files (alphabetical).
        assert_eq!(
            names,
            vec!["alpha_dir", "zeta_dir", "alpha_file.txt", "beta_file.txt"]
        );
        assert!(entries[0].is_dir);
        assert!(entries[1].is_dir);
        assert!(!entries[2].is_dir);
        assert!(!entries[3].is_dir);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn list_directory_missing_path_is_empty() {
        let missing = std::env::temp_dir().join("does_not_exist_rustagent_xyz");
        assert!(list_directory(&missing).is_empty());
    }

    #[test]
    fn display_name_uses_last_component() {
        assert_eq!(display_name(Path::new("/home/user/projects")), "projects");
        assert_eq!(display_name(Path::new("/")), "/");
    }
}
