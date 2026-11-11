//! File tree sidebar component.
//!
//! Uses an in-memory [`file_tree::FileTreeCache`] and lightweight keyed `rect()`
//! rows (instead of heavy stateful `Button` components per file) so expanding
//! directories is instantaneous and never freezes the UI thread.
//! Includes `+` / `✓` buttons to pin targets into the chat context and `●`
//! status dots (green for new files, orange for modified files) for pending diffs.

use crate::diff::{DiffStatus, PendingDiff};
use crate::file_tree::{self, FileTreeCache};
use crate::views::ui;
use freya::prelude::*;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Normalized path key (forward slashes, lowercase on Windows) for diff-badge matching.
fn normalize_path_key(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

/// Snapshot of files with active `Pending` diffs so the file tree can display
/// green (`●` new) or orange (`●` modified) status dots next to files and parent folders.
#[derive(Default)]
struct PendingDiffBadges {
    created_files: HashSet<String>,
    modified_files: HashSet<String>,
}

impl PendingDiffBadges {
    fn from_diffs(diffs: &[PendingDiff]) -> Self {
        let mut created_files = HashSet::new();
        let mut modified_files = HashSet::new();
        for d in diffs {
            if d.status != DiffStatus::Pending {
                continue;
            }
            let key = normalize_path_key(Path::new(&d.path));
            if d.old_content.is_empty() {
                created_files.insert(key);
            } else {
                modified_files.insert(key);
            }
        }
        Self {
            created_files,
            modified_files,
        }
    }

    /// Returns `Some(color)` if `file_path` has a pending diff (green if created, orange if modified).
    fn file_dot_color(&self, file_path: &Path) -> Option<Color> {
        let key = normalize_path_key(file_path);
        if self.created_files.contains(&key) {
            Some(Color::from_rgb(34, 197, 94))
        } else if self.modified_files.contains(&key) {
            Some(Color::from_rgb(245, 158, 11))
        } else {
            None
        }
    }

    /// Returns `Some(color)` if `dir_path` contains at least one pending diff inside it.
    fn dir_dot_color(&self, dir_path: &Path) -> Option<Color> {
        let prefix = format!("{}/", normalize_path_key(dir_path).trim_end_matches('/'));
        if self.modified_files.iter().any(|k| k.starts_with(&prefix)) {
            Some(Color::from_rgb(245, 158, 11))
        } else if self.created_files.iter().any(|k| k.starts_with(&prefix)) {
            Some(Color::from_rgb(34, 197, 94))
        } else {
            None
        }
    }
}

/// Toggles `path` inside the `context_paths` list.
fn toggle_context_path(mut context_paths: Writable<Vec<PathBuf>>, path: PathBuf) {
    let mut list = context_paths.write();
    if let Some(pos) = list.iter().position(|p| p == &path) {
        list.remove(pos);
    } else {
        list.push(path);
    }
}

/// Renders the compact `+` (or `✓` when already pinned) context action badge on a tree row.
fn render_context_toggle_badge(
    is_pinned: bool,
    colors: &ColorsSheet,
    on_press: impl Fn(Event<PressEventData>) + 'static,
) -> Element {
    let (bg, border_col, fg, symbol) = if is_pinned {
        (
            Color::from_argb(45, 16, 185, 129),
            Color::from_rgb(16, 185, 129),
            Color::from_rgb(74, 222, 128),
            "✓",
        )
    } else {
        (
            colors.surface_tertiary,
            colors.border,
            colors.text_secondary,
            "+",
        )
    };

    rect()
        .width(Size::px(20.))
        .height(Size::px(20.))
        .background(bg)
        .border(Border::new().fill(border_col).width(1.))
        .corner_radius(5.)
        .center()
        .on_press(on_press)
        .child(
            label()
                .text(symbol)
                .color(fg)
                .font_size(11.5)
                .font_weight(FontWeight::BOLD),
        )
        .into_element()
}

/// The left sidebar file-tree panel.
///
/// Shows the folders and files of the current working directory as an expandable
/// tree, backed by an in-memory cache so UI re-renders do not repeatedly call
/// `std::fs::read_dir` on the main thread.
pub fn file_tree_panel(
    current_dir: Writable<PathBuf>,
    expanded: Writable<HashSet<PathBuf>>,
    context_paths: Writable<Vec<PathBuf>>,
    pending_diffs: Readable<Vec<PendingDiff>>,
    colors: ColorsSheet,
    on_open_file: impl Fn(PathBuf) + Clone + 'static,
) -> impl IntoElement {
    let cache = use_hook(|| Rc::new(RefCell::new(FileTreeCache::default())));

    let dir = current_dir.read().clone();
    let expanded_snapshot = expanded.read().clone();
    let pinned_snapshot: HashSet<PathBuf> = context_paths.read().iter().cloned().collect();
    let diff_badges = PendingDiffBadges::from_diffs(&pending_diffs.read());
    cache.borrow_mut().sync_root(&dir);

    let c = colors;
    let rows = build_tree_rows(
        &dir,
        0,
        &expanded_snapshot,
        &pinned_snapshot,
        &diff_badges,
        expanded.clone(),
        context_paths.clone(),
        cache.clone(),
        &c,
        on_open_file,
    );

    let is_root_pinned = pinned_snapshot.contains(&dir);
    let pin_root = {
        let context_paths = context_paths;
        let root_path = dir.clone();
        move |_| {
            toggle_context_path(context_paths.clone(), root_path.clone());
        }
    };

    let go_parent_dir = {
        let mut current_dir = current_dir;
        let mut expanded = expanded.clone();
        let parent_opt = dir.parent().map(|p| p.to_path_buf());
        move |_| {
            if let Some(ref parent) = parent_opt
                && !parent.as_os_str().is_empty()
            {
                expanded.write().clear();
                *current_dir.write() = parent.clone();
            }
        }
    };

    let refresh_tree = {
        let cache = cache.clone();
        let mut expanded = expanded;
        move |_| {
            cache.borrow_mut().clear();
            let _ = expanded.write();
        }
    };

    rect()
        .expanded()
        .content(Content::Flex)
        .background(c.background)
        .child(
            ui::panel_header(&c, 38.)
                .border(Border::new().fill(c.border).width(BorderWidth {
                    top: 0.,
                    right: 1.,
                    bottom: 1.,
                    left: 0.,
                }))
                .child(
                    ui::row(6.)
                        .width(Size::flex(1.))
                        .child(
                            label()
                                .text(format!("📁 {}", file_tree::display_name(&dir)))
                                .color(c.text_primary)
                                .font_size(13.)
                                .font_weight(FontWeight::BOLD),
                        ),
                )
                .child(
                    rect()
                        .width(Size::px(22.))
                        .height(Size::px(20.))
                        .background(c.surface_tertiary)
                        .border(Border::new().fill(c.border).width(1.))
                        .corner_radius(5.)
                        .center()
                        .on_press(go_parent_dir)
                        .child(
                            label()
                                .text("⬆")
                                .color(c.text_secondary)
                                .font_size(11.)
                                .font_weight(FontWeight::BOLD),
                        ),
                )
                .child(render_context_toggle_badge(is_root_pinned, &c, pin_root))
                .child(
                    rect()
                        .width(Size::px(22.))
                        .height(Size::px(20.))
                        .background(c.surface_tertiary)
                        .border(Border::new().fill(c.border).width(1.))
                        .corner_radius(5.)
                        .center()
                        .on_press(refresh_tree)
                        .child(
                            label()
                                .text("↻")
                                .color(c.text_secondary)
                                .font_size(11.5)
                                .font_weight(FontWeight::BOLD),
                        ),
                ),
        )
        .child(
            rect()
                .width(Size::fill())
                .height(Size::flex(1.))
                .padding(6.)
                .child(
                    ScrollView::new().child(
                        rect()
                            .width(Size::fill())
                            .spacing(1.)
                            .children(rows),
                    ),
                ),
        )
}

/// Recursively build lightweight, keyed tree rows for a directory.
#[allow(clippy::too_many_arguments)]
fn build_tree_rows(
    dir: &Path,
    depth: usize,
    expanded_snapshot: &HashSet<PathBuf>,
    pinned_snapshot: &HashSet<PathBuf>,
    diff_badges: &PendingDiffBadges,
    expanded: Writable<HashSet<PathBuf>>,
    context_paths: Writable<Vec<PathBuf>>,
    cache: Rc<RefCell<FileTreeCache>>,
    colors: &ColorsSheet,
    on_open_file: impl Fn(PathBuf) + Clone + 'static,
) -> Vec<Element> {
    if depth >= file_tree::MAX_TREE_DEPTH {
        return Vec::new();
    }

    let listing = cache.borrow_mut().get_or_scan(dir);
    let mut rows = Vec::with_capacity(listing.entries.len() + 1);

    for entry in listing.entries {
        let indent = depth as f32 * 14.;
        let row_key = entry.path.to_string_lossy().to_string();
        let is_pinned = pinned_snapshot.contains(&entry.path);

        if entry.is_dir {
            let is_expanded = expanded_snapshot.contains(&entry.path);
            let is_heavy = file_tree::is_heavy_directory(&entry.name);
            let dir_dot = diff_badges.dir_dot_color(&entry.path);

            let toggle = {
                let mut expanded = expanded.clone();
                let cache = cache.clone();
                let path = entry.path.clone();
                move |_| {
                    let mut set = expanded.write();
                    if !set.remove(&path) {
                        cache.borrow_mut().invalidate(&path);
                        set.insert(path.clone());
                    }
                }
            };

            let toggle_ctx = {
                let context_paths = context_paths.clone();
                let path = entry.path.clone();
                move |_| {
                    toggle_context_path(context_paths.clone(), path.clone());
                }
            };

            let arrow = if is_expanded { "▾" } else { "▸" };
            let icon = if is_expanded { "📂" } else { "📁" };
            let text_col = if is_pinned {
                Color::from_rgb(74, 222, 128)
            } else if is_heavy {
                colors.text_secondary
            } else {
                colors.text_inverse
            };

            rows.push(
                rect()
                    .key(&row_key)
                    .width(Size::fill())
                    .padding(Gaps::new(2., 6., 2., indent + 4.))
                    .corner_radius(5.)
                    .background(if is_pinned {
                        Color::from_argb(24, 16, 185, 129)
                    } else if is_expanded {
                        Color::from_argb(18, 255, 255, 255)
                    } else {
                        Color::TRANSPARENT
                    })
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .content(Content::Flex)
                    .spacing(4.)
                    .child(
                        rect()
                            .width(Size::flex(1.))
                            .padding(Gaps::new(2., 4., 2., 0.))
                            .horizontal()
                            .cross_align(Alignment::Center)
                            .spacing(6.)
                            .on_press(toggle)
                            .child(
                                label()
                                    .text(arrow)
                                    .color(colors.text_secondary)
                                    .font_size(12.),
                            )
                            .child(
                                label()
                                    .text(format!("{} {}", icon, entry.name))
                                    .color(text_col)
                                    .font_size(12.5)
                                    .font_weight(if is_expanded || is_pinned {
                                        FontWeight::BOLD
                                    } else {
                                        FontWeight::NORMAL
                                    }),
                            )
                            .maybe_child(dir_dot.map(|dot_col| {
                                label()
                                    .text("●")
                                    .color(dot_col)
                                    .font_size(10.)
                                    .font_weight(FontWeight::BOLD)
                            })),
                    )
                    .child(render_context_toggle_badge(is_pinned, colors, toggle_ctx))
                    .into_element(),
            );

            if is_expanded {
                rows.extend(build_tree_rows(
                    &entry.path,
                    depth + 1,
                    expanded_snapshot,
                    pinned_snapshot,
                    diff_badges,
                    expanded.clone(),
                    context_paths.clone(),
                    cache.clone(),
                    colors,
                    on_open_file.clone(),
                ));
            }
        } else {
            let file_path = entry.path.clone();
            let file_dot = diff_badges.file_dot_color(&file_path);
            let on_open = on_open_file.clone();
            let open_click = move |_| {
                on_open(file_path.clone());
            };

            let toggle_ctx = {
                let context_paths = context_paths.clone();
                let path = entry.path.clone();
                move |_| {
                    toggle_context_path(context_paths.clone(), path.clone());
                }
            };

            let file_text_col = if is_pinned {
                Color::from_rgb(74, 222, 128)
            } else if let Some(dot_col) = file_dot {
                dot_col
            } else {
                colors.text_highlight
            };

            rows.push(
                rect()
                    .key(&row_key)
                    .width(Size::fill())
                    .padding(Gaps::new(1., 6., 1., indent + 18.))
                    .corner_radius(5.)
                    .background(if is_pinned {
                        Color::from_argb(24, 16, 185, 129)
                    } else {
                        Color::TRANSPARENT
                    })
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .content(Content::Flex)
                    .spacing(4.)
                    .child(
                        rect()
                            .width(Size::flex(1.))
                            .padding(Gaps::new(2., 4., 2., 0.))
                            .horizontal()
                            .cross_align(Alignment::Center)
                            .spacing(6.)
                            .on_press(open_click)
                            .child(
                                label()
                                    .text("📄")
                                    .color(colors.text_placeholder)
                                    .font_size(11.),
                            )
                            .child(
                                label()
                                    .text(entry.name)
                                    .color(file_text_col)
                                    .font_size(12.5)
                                    .font_weight(if is_pinned || file_dot.is_some() {
                                        FontWeight::BOLD
                                    } else {
                                        FontWeight::NORMAL
                                    }),
                            )
                            .maybe_child(file_dot.map(|dot_col| {
                                label()
                                    .text("●")
                                    .color(dot_col)
                                    .font_size(10.5)
                                    .font_weight(FontWeight::BOLD)
                            })),
                    )
                    .child(render_context_toggle_badge(is_pinned, colors, toggle_ctx))
                    .into_element(),
            );
        }
    }

    if listing.omitted_count > 0 {
        let indent = (depth + 1) as f32 * 14.;
        let omitted_key = format!("{}::__omitted__", dir.to_string_lossy());
        rows.push(
            rect()
                .key(&omitted_key)
                .width(Size::fill())
                .padding(Gaps::new(3., 6., 3., indent + 8.))
                .child(ui::muted(
                    format!(
                        "… +{} éléments masqués (dossier volumineux)",
                        listing.omitted_count
                    ),
                    colors,
                ))
                .into_element(),
        );
    }

    rows
}
