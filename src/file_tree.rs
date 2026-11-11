//! File-tree helpers for the sidebar panel.
//!
//! This module contains the pure, platform-agnostic logic for scanning a
//! directory, bounding massive directories (such as `target/`, `node_modules/`,
//! or `AppData/`), and caching directory listings in memory so UI re-renders
//! never block the main thread with repeated synchronous disk I/O.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Hard cap on raw OS directory entries scanned per folder so a huge directory
/// (e.g. `target/debug/deps` with 5,000+ files) never stalls the UI thread.
pub const MAX_RAW_SCAN_ENTRIES: usize = 400;

/// Maximum number of sorted entries rendered in the UI tree for a single folder.
pub const MAX_DISPLAY_ENTRIES: usize = 100;

/// Maximum recursion depth for expanded folders to prevent deep/cyclic path hangs.
pub const MAX_TREE_DEPTH: usize = 10;

/// Known heavy build, dependency, or cache directories.
const HEAVY_DIRS: &[&str] = &[
    "target",
    "node_modules",
    ".git",
    ".gradle",
    "build",
    "dist",
    ".next",
    "__pycache__",
    ".venv",
    "venv",
    ".cargo",
    ".rustup",
    "AppData",
];

/// Returns `true` if a directory name is a known heavy build/cache directory.
pub fn is_heavy_directory(name: &str) -> bool {
    HEAVY_DIRS
        .iter()
        .any(|d| d.eq_ignore_ascii_case(name))
}

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

/// Bounded result of scanning a directory for the UI tree.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DirectoryListing {
    /// Entries to display (sorted: directories first, then files).
    pub entries: Vec<FileEntry>,
    /// Number of additional entries omitted because the directory exceeded limits.
    pub omitted_count: usize,
}

/// In-memory cache of directory listings so Freya re-renders (chat streaming,
/// input typing, hover states) do not hit the disk with `std::fs::read_dir`.
#[derive(Default)]
pub struct FileTreeCache {
    root: PathBuf,
    entries_by_dir: HashMap<PathBuf, DirectoryListing>,
}

impl FileTreeCache {
    /// Ensures the cache is tied to `current_root`, clearing cached subtrees if the root changed.
    pub fn sync_root(&mut self, current_root: &Path) {
        if self.root != current_root {
            self.root = current_root.to_path_buf();
            self.entries_by_dir.clear();
        }
    }

    /// Invalidates a single directory so its next lookup re-reads from disk.
    pub fn invalidate(&mut self, dir: &Path) {
        self.entries_by_dir.remove(dir);
    }

    /// Clears all cached directory listings.
    pub fn clear(&mut self) {
        self.entries_by_dir.clear();
    }

    /// Returns the cached [`DirectoryListing`] for `dir`, scanning the disk at most once.
    pub fn get_or_scan(&mut self, dir: &Path) -> DirectoryListing {
        if let Some(cached) = self.entries_by_dir.get(dir) {
            return cached.clone();
        }
        let listing = list_directory_bounded(dir, MAX_DISPLAY_ENTRIES);
        self.entries_by_dir.insert(dir.to_path_buf(), listing.clone());
        listing
    }
}

/// Scans a directory with hard safety bounds on both raw OS iteration and returned items.
///
/// Symlinked directories / Windows NTFS junctions (`is_symlink()`) are treated as non-expandable
/// entries to avoid cyclic directory loops (such as `AppData\Local\Application Data`).
pub fn list_directory_bounded(path: &Path, max_display: usize) -> DirectoryListing {
    let mut entries = Vec::new();
    let mut extra_raw = 0usize;

    if let Ok(read_dir) = std::fs::read_dir(path) {
        for (idx, entry) in read_dir.flatten().enumerate() {
            if idx >= MAX_RAW_SCAN_ENTRIES {
                extra_raw += 1;
                // Stop iterating entirely if a directory is gigantic (> 2000 files)
                if extra_raw >= 1600 {
                    break;
                }
                continue;
            }
            let is_dir = entry
                .file_type()
                .map(|t| t.is_dir() && !t.is_symlink())
                .unwrap_or(false);
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

    let total_scanned = entries.len();
    let omitted_from_display = total_scanned.saturating_sub(max_display);
    if entries.len() > max_display {
        entries.truncate(max_display);
    }

    DirectoryListing {
        entries,
        omitted_count: omitted_from_display + extra_raw,
    }
}

/// List the immediate children of a directory.
///
/// Entries are sorted with directories first, then files, each ordered
/// alphabetically (case-insensitively). Unreadable or missing directories
/// yield an empty list rather than an error, so the UI never panics.
pub fn list_directory(path: &Path) -> Vec<FileEntry> {
    list_directory_bounded(path, MAX_RAW_SCAN_ENTRIES).entries
}

/// The display name for a directory, falling back to the full path when the
/// name is empty (e.g. the filesystem root).
pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

// ============================================================================
// `@` MENTION AUTOCOMPLETE & RESOLUTION
// ============================================================================

/// Maximum number of workspace entries indexed for `@` mention search.
const MAX_MENTION_INDEX_ENTRIES: usize = 500;

/// Maximum directory depth traversed when indexing workspace entries for `@`.
const MAX_MENTION_INDEX_DEPTH: usize = 5;

/// An indexed file or folder entry used for `@` mention autocomplete in the chat.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceEntry {
    /// Short file or folder name (e.g. `chat.rs` or `views`).
    pub name: String,
    /// Relative forward-slash path from the workspace root (e.g. `src/views/chat.rs`).
    pub rel_path: String,
    /// Full path on disk.
    pub full_path: PathBuf,
    /// Whether this entry is a directory.
    pub is_dir: bool,
}

/// Cached flat index of workspace files and folders for instant `@` autocomplete.
#[derive(Default)]
pub struct WorkspaceMentionIndex {
    root: PathBuf,
    entries: Vec<WorkspaceEntry>,
}

impl WorkspaceMentionIndex {
    /// Ensures the index is built for `current_root`, rescanning only if `current_root` changed.
    pub fn sync_root(&mut self, current_root: &Path) {
        if self.root != current_root || self.entries.is_empty() {
            self.root = current_root.to_path_buf();
            self.entries = collect_workspace_entries(current_root);
        }
    }

    /// Forces a rescan of the workspace index.
    pub fn refresh(&mut self) {
        if !self.root.as_os_str().is_empty() {
            self.entries = collect_workspace_entries(&self.root);
        }
    }

    /// Filters indexed entries matching the `@query` string, up to `limit` results.
    pub fn filter_suggestions(&self, query: &str, limit: usize) -> Vec<WorkspaceEntry> {
        let q = query.trim().to_lowercase().replace('\\', "/");
        if q.is_empty() {
            return self.entries.iter().take(limit).cloned().collect();
        }

        let mut scored: Vec<(u8, &WorkspaceEntry)> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let name_low = entry.name.to_lowercase();
                let rel_low = entry.rel_path.to_lowercase();
                if name_low == q || rel_low == q {
                    Some((0, entry))
                } else if name_low.starts_with(&q) {
                    Some((1, entry))
                } else if rel_low.starts_with(&q) {
                    Some((2, entry))
                } else if name_low.contains(&q) {
                    Some((3, entry))
                } else if rel_low.contains(&q) {
                    Some((4, entry))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|(sa, a), (sb, b)| {
            sa.cmp(sb)
                .then_with(|| a.rel_path.len().cmp(&b.rel_path.len()))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        scored
            .into_iter()
            .take(limit)
            .map(|(_, e)| e.clone())
            .collect()
    }
}

/// Collects a bounded list of workspace files and directories for `@` mentions,
/// skipping deep traversal inside heavy build/cache directories (`target`, `node_modules`, `.git`, etc.).
pub fn collect_workspace_entries(root: &Path) -> Vec<WorkspaceEntry> {
    let mut results = Vec::new();
    collect_recursive(root, root, 0, &mut results);
    results
}

fn collect_recursive(
    root: &Path,
    current: &Path,
    depth: usize,
    out: &mut Vec<WorkspaceEntry>,
) {
    if depth >= MAX_MENTION_INDEX_DEPTH || out.len() >= MAX_MENTION_INDEX_ENTRIES {
        return;
    }

    let listing = list_directory_bounded(current, 80);
    let mut subdirs_to_visit = Vec::new();

    for entry in listing.entries {
        if out.len() >= MAX_MENTION_INDEX_ENTRIES {
            break;
        }
        if entry.name.starts_with('.') && entry.name != ".env" && entry.name != ".gitignore" {
            continue;
        }
        let rel_path = entry
            .path
            .strip_prefix(root)
            .unwrap_or(&entry.path)
            .to_string_lossy()
            .replace('\\', "/");

        let is_heavy = entry.is_dir && is_heavy_directory(&entry.name);
        out.push(WorkspaceEntry {
            name: entry.name,
            rel_path,
            full_path: entry.path.clone(),
            is_dir: entry.is_dir,
        });

        if entry.is_dir && !is_heavy {
            subdirs_to_visit.push(entry.path);
        }
    }

    for sub in subdirs_to_visit {
        if out.len() >= MAX_MENTION_INDEX_ENTRIES {
            break;
        }
        collect_recursive(root, &sub, depth + 1, out);
    }
}

/// Detects an active `@query` token at the end of the chat input string.
///
/// Returns `Some((at_byte_index, query_slice))` when the user is currently typing
/// an `@mention` (e.g. `"Modifie @src/v"` -> `Some((8, "src/v"))`, `"@"` -> `Some((0, ""))`).
/// Returns `None` if the input ends with whitespace or has no active `@` token.
pub fn extract_active_at_query(input: &str) -> Option<(usize, &str)> {
    if input.is_empty() || input.ends_with(char::is_whitespace) {
        return None;
    }
    let last_word_start = input
        .rfind(char::is_whitespace)
        .map(|i| i + 1)
        .unwrap_or(0);
    let token = &input[last_word_start..];
    if let Some(query) = token.strip_prefix('@') {
        Some((last_word_start, query))
    } else {
        None
    }
}

/// Replaces the active `@query` at the end of `input` with `@<rel_path> `.
pub fn replace_active_at_query(input: &str, rel_path: &str) -> String {
    if let Some((at_idx, _)) = extract_active_at_query(input) {
        format!("{}@{} ", &input[..at_idx], rel_path)
    } else if input.is_empty() || input.ends_with(char::is_whitespace) {
        format!("{}@{} ", input, rel_path)
    } else {
        format!("{} @{} ", input, rel_path)
    }
}

/// Extracts all `@path` or `@filename` mentions from a prompt message and resolves them
/// against `root` (or the workspace index) so typing `@src/main.rs` or `@chat.rs` directly
/// pins those files/folders for the agent turn.
pub fn resolve_at_mentions_in_text(text: &str, root: &Path) -> Vec<PathBuf> {
    let mut resolved = Vec::new();
    let mut index_cache: Option<Vec<WorkspaceEntry>> = None;

    for raw_word in text.split_whitespace() {
        let Some(after_at) = raw_word.strip_prefix('@') else {
            continue;
        };
        let cleaned = after_at.trim_end_matches([',', '.', ';', ':', '!', '?', ')', ']', '"', '\'']);
        if cleaned.is_empty() {
            continue;
        }

        // 1. Direct relative path inside `root`
        let candidate_in_root = root.join(cleaned);
        if candidate_in_root.exists() {
            if !resolved.contains(&candidate_in_root) {
                resolved.push(candidate_in_root);
            }
            continue;
        }

        // 2. Direct path as-is (absolute or relative to process cwd)
        let candidate_direct = PathBuf::from(cleaned);
        if candidate_direct.exists() {
            if !resolved.contains(&candidate_direct) {
                resolved.push(candidate_direct);
            }
            continue;
        }

        // 3. Match by file/folder name or relative path suffix inside workspace index
        let entries = index_cache.get_or_insert_with(|| collect_workspace_entries(root));
        let cleaned_low = cleaned.to_lowercase().replace('\\', "/");
        if let Some(found) = entries.iter().find(|e| {
            e.rel_path.eq_ignore_ascii_case(&cleaned_low)
                || e.name.eq_ignore_ascii_case(&cleaned_low)
                || e.rel_path.to_lowercase().ends_with(&format!("/{}", cleaned_low))
        }) && !resolved.contains(&found.full_path)
        {
            resolved.push(found.full_path.clone());
        }
    }

    resolved
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
    fn list_directory_bounded_truncates_and_counts_omitted() {
        let dir =
            std::env::temp_dir().join(format!("rustagent_bounded_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..10 {
            std::fs::write(dir.join(format!("file_{:02}.txt", i)), "x").unwrap();
        }

        let listing = list_directory_bounded(&dir, 4);
        assert_eq!(listing.entries.len(), 4);
        assert_eq!(listing.omitted_count, 6);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn display_name_uses_last_component() {
        assert_eq!(display_name(Path::new("/home/user/projects")), "projects");
        assert_eq!(display_name(Path::new("/")), "/");
    }

    #[test]
    fn extract_and_replace_at_query_works() {
        assert_eq!(extract_active_at_query("@"), Some((0, "")));
        assert_eq!(
            extract_active_at_query("Modifie @src/views"),
            Some((8, "src/views"))
        );
        assert_eq!(extract_active_at_query("Modifie @src/views "), None);
        assert_eq!(extract_active_at_query("Bonjour tout le monde"), None);

        let replaced = replace_active_at_query("Modifie @ch", "src/views/chat.rs");
        assert_eq!(replaced, "Modifie @src/views/chat.rs ");
    }
}
