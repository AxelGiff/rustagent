//! File tree sidebar component.

use crate::file_tree;
use freya::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The left sidebar file-tree panel.
///
/// It shows the folders and files of the terminal's current working directory
/// as an expandable tree, and follows the directory as it changes (e.g. when
/// the user runs `cd` in the terminal). The tree is rebuilt from scratch on
/// every render, so it always reflects the latest directory contents.
pub fn file_tree_panel(
    current_dir: Readable<PathBuf>,
    expanded: Writable<HashSet<PathBuf>>,
    colors: ColorsSheet,
    on_open_file: impl Fn(PathBuf) + Clone + 'static,
) -> impl IntoElement {
    let dir = current_dir.read().clone();
    let c = colors;
    let rows = build_tree_rows(&dir, 0, expanded.clone(), c.clone(), on_open_file);

    rect()
        .expanded()
        .content(Content::Flex)
        .background(c.background)
        .child(
            rect()
                .width(Size::fill())
                .height(Size::px(32.))
                .padding(8.)
                .background(c.surface_primary)
                .border(Border::new().fill(c.border).width(BorderWidth {
                    top: 0.,
                    right: 1.,
                    bottom: 1.,
                    left: 0.,
                }))
                .horizontal()
                .cross_align(Alignment::Center)
                .child(
                    label()
                        .text(file_tree::display_name(&dir))
                        .color(c.text_primary)
                        .font_size(13.)
                        .font_weight(FontWeight::BOLD),
                ),
        )
        .child(
            rect()
                .expanded()
                .padding(6.)
                .child(ScrollView::new().child(rect().width(Size::fill()).children(rows))),
        )
}

/// Recursively build the tree rows for a directory.
///
/// Each directory is rendered as a row with an expand/collapse toggle; when
/// expanded, its children are rendered beneath it with extra indentation.
/// Files are rendered as clickable buttons that open the file in the code editor.
pub fn build_tree_rows(
    dir: &Path,
    depth: usize,
    expanded: Writable<HashSet<PathBuf>>,
    colors: ColorsSheet,
    on_open_file: impl Fn(PathBuf) + Clone + 'static,
) -> Vec<Element> {
    let mut rows = Vec::new();
    for entry in file_tree::list_directory(dir) {
        let indent = depth as f32 * 14.;
        if entry.is_dir {
            let is_expanded = expanded.read().contains(&entry.path);
            let toggle = {
                let mut expanded = expanded.clone();
                let path = entry.path.clone();
                move |_| {
                    let already_open = expanded.peek().contains(&path);
                    if already_open {
                        expanded.write().remove(&path);
                    } else {
                        expanded.write().insert(path.clone());
                    }
                }
            };
            rows.push(
                rect()
                    .width(Size::fill())
                    .padding(Gaps::new(2., 4., 2., indent))
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(4.)
                    .child(
                        Button::new()
                            .background(Color::TRANSPARENT)
                            .hover_background(colors.surface_primary)
                            .border_fill(Color::TRANSPARENT)
                            .color(colors.text_secondary)
                            .on_press(toggle)
                            .child(if is_expanded { "▾" } else { "▸" }),
                    )
                    .child(label().text(entry.name.clone()).color(colors.text_inverse))
                    .into_element(),
            );
            if is_expanded {
                rows.extend(build_tree_rows(
                    &entry.path,
                    depth + 1,
                    expanded.clone(),
                    colors.clone(),
                    on_open_file.clone(),
                ));
            }
        } else {
            let file_path = entry.path.clone();
            let on_open = on_open_file.clone();
            let open_click = move |_| {
                on_open(file_path.clone());
            };
            rows.push(
                rect()
                    .width(Size::fill())
                    .padding(Gaps::new(1., 4., 1., indent + 6.))
                    .child(
                        Button::new()
                            .width(Size::fill())
                            .background(Color::TRANSPARENT)
                            .hover_background(colors.surface_primary)
                            .border_fill(Color::TRANSPARENT)
                            .color(colors.text_highlight)
                            .on_press(open_click)
                            .child(
                                rect()
                                    .horizontal()
                                    .cross_align(Alignment::Center)
                                    .spacing(6.)
                                    .child(label().text("•").color(colors.text_placeholder))
                                    .child(
                                        label()
                                            .text(entry.name.clone())
                                            .color(colors.text_highlight),
                                    ),
                            ),
                    )
                    .into_element(),
            );
        }
    }
    rows
}
