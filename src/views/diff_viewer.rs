//! Diff review panel and Antigravity-style Changed Files Review box.

use crate::diff::{self, DiffStatus};
use crate::language::SupportedLanguage;
use crate::types::{CenterViewMode, Message, Role};
use crate::views::editor::update_editor_if_matching;
use freya::code_editor::CodeEditorData;
use freya::prelude::*;

/// Shared reactive state bundle for reviewing, accepting, and rejecting file diffs
/// from both the center diff viewer and the chat's end-of-conversation review box.
#[derive(Clone, Copy)]
pub struct DiffReviewState {
    pub pending_diffs: State<Vec<diff::PendingDiff>>,
    pub selected_diff_idx: State<usize>,
    pub inline_diff_idx: State<Option<usize>>,
    pub review_box_expanded: State<bool>,
    pub chat_scroll: ScrollController,
    pub center_view_mode: State<CenterViewMode>,
    pub messages: State<Vec<Message>>,
    pub editor: State<CodeEditorData>,
    pub current_language: State<SupportedLanguage>,
    pub file_name: State<String>,
}

impl DiffReviewState {
    /// Toggles the main accordion (unfold/fold the list of changed files in the chat box).
    pub fn toggle_review_box(mut self) {
        let cur = *self.review_box_expanded.read();
        *self.review_box_expanded.write() = !cur;
        if !cur {
            self.chat_scroll
                .scroll_to(ScrollPosition::End, Direction::Vertical);
        }
    }

    /// Opens the center diff viewer focused on the file at `idx`.
    pub fn open_diff_at(mut self, idx: usize) {
        if let Some(d) = self.pending_diffs.read().get(idx)
            && d.status == DiffStatus::Rejected
        {
            return;
        }
        *self.selected_diff_idx.write() = idx;
        *self.center_view_mode.write() = CenterViewMode::Diff;
    }

    /// Toggles the inline diff preview inside the chat review card for `idx`.
    pub fn toggle_inline_diff(mut self, idx: usize) {
        if let Some(d) = self.pending_diffs.read().get(idx)
            && d.status == DiffStatus::Rejected
        {
            return;
        }
        let current = *self.inline_diff_idx.read();
        if current == Some(idx) {
            *self.inline_diff_idx.write() = None;
        } else {
            *self.inline_diff_idx.write() = Some(idx);
            *self.selected_diff_idx.write() = idx;
        }
    }

    /// Accepts and writes the single pending diff at `idx` to disk.
    /// Rejected or already accepted files cannot be modified again.
    pub fn accept_diff(mut self, idx: usize) {
        let maybe_diff = {
            let mut list = self.pending_diffs.write();
            if let Some(d) = list.get_mut(idx) {
                if d.status != DiffStatus::Pending {
                    return;
                }
                if let Err(e) = diff::apply_diff_to_disk(d) {
                    self.messages.write().push(Message {
                        role: Role::AI,
                        content: e,
                    });
                    return;
                }
                d.status = DiffStatus::Accepted;
                Some((d.path.clone(), d.new_content.clone()))
            } else {
                None
            }
        };

        if let Some((path, new_content)) = maybe_diff {
            update_editor_if_matching(
                &path,
                &new_content,
                &self.file_name.read(),
                self.editor,
                self.current_language,
            );
        }

        let next_pending = self
            .pending_diffs
            .read()
            .iter()
            .position(|d| d.status == DiffStatus::Pending);
        if let Some(next_idx) = next_pending {
            *self.selected_diff_idx.write() = next_idx;
        } else {
            *self.center_view_mode.write() = CenterViewMode::Editor;
        }
    }

    /// Accepts and writes all remaining `Pending` diffs to disk.
    /// Rejected files remain rejected and cannot be recovered.
    pub fn accept_all(mut self) {
        let mut applied = Vec::new();
        let mut errors = Vec::new();
        {
            let mut list = self.pending_diffs.write();
            for d in list.iter_mut() {
                if d.status != DiffStatus::Pending {
                    continue;
                }
                if let Err(e) = diff::apply_diff_to_disk(d) {
                    errors.push(e);
                    continue;
                }
                d.status = DiffStatus::Accepted;
                applied.push((d.path.clone(), d.new_content.clone()));
            }
        }

        for err in errors {
            self.messages.write().push(Message {
                role: Role::AI,
                content: err,
            });
        }

        for (path, new_content) in applied {
            update_editor_if_matching(
                &path,
                &new_content,
                &self.file_name.read(),
                self.editor,
                self.current_language,
            );
        }

        *self.center_view_mode.write() = CenterViewMode::Editor;
    }

    /// Permanently rejects the pending diff at `idx`.
    /// Once rejected, a file cannot be accepted or restored.
    pub fn reject_diff(mut self, idx: usize) {
        let maybe_revert = {
            let mut list = self.pending_diffs.write();
            if let Some(d) = list.get_mut(idx) {
                if d.status != DiffStatus::Pending {
                    return;
                }
                d.status = DiffStatus::Rejected;
                Some((d.path.clone(), d.old_content.clone()))
            } else {
                None
            }
        };

        if *self.inline_diff_idx.read() == Some(idx) {
            *self.inline_diff_idx.write() = None;
        }

        if let Some((path, old_content)) = maybe_revert {
            update_editor_if_matching(
                &path,
                &old_content,
                &self.file_name.read(),
                self.editor,
                self.current_language,
            );
        }

        let next_pending = self
            .pending_diffs
            .read()
            .iter()
            .position(|d| d.status == DiffStatus::Pending);
        if let Some(next_idx) = next_pending {
            *self.selected_diff_idx.write() = next_idx;
        } else {
            *self.center_view_mode.write() = CenterViewMode::Editor;
        }
    }

    /// Permanently rejects all remaining `Pending` diffs.
    pub fn reject_all(mut self) {
        let mut reverted = Vec::new();
        {
            let mut list = self.pending_diffs.write();
            for d in list.iter_mut() {
                if d.status != DiffStatus::Pending {
                    continue;
                }
                d.status = DiffStatus::Rejected;
                reverted.push((d.path.clone(), d.old_content.clone()));
            }
        }

        *self.inline_diff_idx.write() = None;

        for (path, old_content) in reverted {
            update_editor_if_matching(
                &path,
                &old_content,
                &self.file_name.read(),
                self.editor,
                self.current_language,
            );
        }

        *self.center_view_mode.write() = CenterViewMode::Editor;
    }
}

/// Helper to build numbered diff lines (`old_ln`, `new_ln`, `DiffLine`) for unified diff display.
fn build_numbered_diff_rows(
    diff_lines: &[diff::DiffLine],
    max_rows: Option<usize>,
    colors: &ColorsSheet,
) -> Vec<Element> {
    let mut elements = Vec::new();
    let mut old_ln = 1usize;
    let mut new_ln = 1usize;
    let limit = max_rows.unwrap_or(usize::MAX);

    for (idx, d_line) in diff_lines.iter().enumerate() {
        if idx >= limit {
            let remaining = diff_lines.len().saturating_sub(limit);
            elements.push(
                rect()
                    .width(Size::fill())
                    .padding(Gaps::new(4., 10., 4., 10.))
                    .background(colors.surface_primary)
                    .child(
                        label()
                            .font_family("Jetbrains Mono")
                            .font_size(11.)
                            .text(format!(
                                "... +{} ligne(s) — cliquez sur « 🔍 Grand écran » pour tout voir",
                                remaining
                            ))
                            .color(colors.text_secondary),
                    )
                    .into_element(),
            );
            break;
        }

        match d_line {
            diff::DiffLine::Deleted(text) => {
                old_ln += 1;
                elements.push(
                    rect()
                        .width(Size::fill())
                        .padding(Gaps::new(2., 8., 2., 8.))
                        .background(Color::from_argb(45, 239, 68, 68))
                        .child(
                            label()
                                .font_family("Jetbrains Mono")
                                .font_size(12.)
                                .text(format!("{:>3}     - {}", old_ln - 1, text))
                                .color(Color::from_rgb(248, 113, 113)),
                        )
                        .into_element(),
                );
            }
            diff::DiffLine::Added(text) => {
                new_ln += 1;
                elements.push(
                    rect()
                        .width(Size::fill())
                        .padding(Gaps::new(2., 8., 2., 8.))
                        .background(Color::from_argb(45, 34, 197, 94))
                        .child(
                            label()
                                .font_family("Jetbrains Mono")
                                .font_size(12.)
                                .text(format!("    {:>3} + {}", new_ln - 1, text))
                                .color(Color::from_rgb(74, 222, 128)),
                        )
                        .into_element(),
                );
            }
            diff::DiffLine::Unchanged(text) => {
                old_ln += 1;
                new_ln += 1;
                elements.push(
                    rect()
                        .width(Size::fill())
                        .padding(Gaps::new(2., 8., 2., 8.))
                        .child(
                            label()
                                .font_family("Jetbrains Mono")
                                .font_size(12.)
                                .text(format!("{:>3} {:>3}   {}", old_ln - 1, new_ln - 1, text))
                                .color(colors.text_secondary),
                        )
                        .into_element(),
                );
            }
        }
    }

    elements
}

/// Center panel diff viewer with a dropdown selector (`Select`) to switch between modified files.
pub fn diff_viewer_panel(
    diff_review: DiffReviewState,
    colors: ColorsSheet,
) -> impl IntoElement {
    let diffs = diff_review.pending_diffs.read().clone();
    if diffs.is_empty() {
        return rect()
            .expanded()
            .center()
            .background(colors.background)
            .child(
                label()
                    .text("Aucun diff à afficher.")
                    .color(colors.text_secondary),
            )
            .into_element();
    }

    let diff_count = diffs.len();
    let active_idx = (*diff_review.selected_diff_idx.read()).min(diff_count.saturating_sub(1));
    let diff = &diffs[active_idx];
    let diff_path = diff.path.clone();
    let active_short_name = std::path::Path::new(&diff_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| diff_path.clone());
    let diff_status = diff.status;
    let (added_lines, deleted_lines) = diff::diff_stats(diff);

    // Dropdown menu (`Select`) to pick any modified file with an arrow
    let file_menu_items = diffs.iter().enumerate().map(|(idx, item)| {
        let is_selected = idx == active_idx;
        let short_name = std::path::Path::new(&item.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| item.path.clone());
        let (add, del) = diff::diff_stats(item);
        let status_prefix = match item.status {
            DiffStatus::Pending => "",
            DiffStatus::Accepted => "✅ ",
            DiffStatus::Rejected => "❌ ",
        };
        let mut sel = diff_review.selected_diff_idx;
        MenuItem::new()
            .selected(is_selected)
            .on_press(move |_| {
                *sel.write() = idx;
            })
            .child(format!(
                "{}. {}{} (+{} -{})",
                idx + 1,
                status_prefix,
                short_name,
                add,
                del
            ))
    });

    let file_select = Select::new()
        .selected_item(format!(
            "📄 {} ({}/{}) ▾",
            active_short_name,
            active_idx + 1,
            diff_count
        ))
        .children(file_menu_items);

    let line_elements = build_numbered_diff_rows(&diff.diff_lines, None, &colors);
    let mut center_mode_back = diff_review.center_view_mode;

    rect()
        .expanded()
        .content(Content::Flex)
        .background(colors.background)
        .child(
            // Single clean header bar with dropdown selector (`Select`) + path + stats + Back button
            rect()
                .width(Size::fill())
                .height(Size::px(42.))
                .padding(Gaps::new(4., 12., 4., 12.))
                .background(colors.surface_primary)
                .border(Border::new().fill(colors.border).width(BorderWidth {
                    top: 0.,
                    right: 0.,
                    bottom: 1.,
                    left: 0.,
                }))
                .horizontal()
                .cross_align(Alignment::Center)
                .content(Content::Flex)
                .spacing(8.)
                .child(
                    rect()
                        .width(Size::flex(1.))
                        .horizontal()
                        .cross_align(Alignment::Center)
                        .spacing(8.)
                        .child(file_select)
                        .child(
                            rect()
                                .padding(Gaps::new(3., 8., 3., 8.))
                                .background(colors.surface_secondary)
                                .corner_radius(4.)
                                .border(Border::new().fill(colors.border).width(1.))
                                .child(
                                    label()
                                        .text(diff_path)
                                        .color(colors.text_highlight)
                                        .font_size(11.5),
                                ),
                        )
                        .child(
                            rect()
                                .padding(Gaps::new(2., 6., 2., 6.))
                                .background(Color::from_argb(35, 34, 197, 94))
                                .corner_radius(4.)
                                .child(
                                    label()
                                        .text(format!("+{}", added_lines))
                                        .color(Color::from_rgb(74, 222, 128))
                                        .font_size(11.5)
                                        .font_weight(FontWeight::BOLD),
                                ),
                        )
                        .child(
                            rect()
                                .padding(Gaps::new(2., 6., 2., 6.))
                                .background(Color::from_argb(35, 239, 68, 68))
                                .corner_radius(4.)
                                .child(
                                    label()
                                        .text(format!("-{}", deleted_lines))
                                        .color(Color::from_rgb(248, 113, 113))
                                        .font_size(11.5)
                                        .font_weight(FontWeight::BOLD),
                                ),
                        )
                        .child(match diff_status {
                            DiffStatus::Accepted => rect()
                                .padding(Gaps::new(3., 8., 3., 8.))
                                .background(Color::from_argb(40, 34, 197, 94))
                                .corner_radius(4.)
                                .child(
                                    label()
                                        .text("✅ Accepté")
                                        .color(Color::from_rgb(74, 222, 128))
                                        .font_size(11.5)
                                        .font_weight(FontWeight::BOLD),
                                )
                                .into_element(),
                            DiffStatus::Rejected => rect()
                                .padding(Gaps::new(3., 8., 3., 8.))
                                .background(Color::from_argb(40, 239, 68, 68))
                                .corner_radius(4.)
                                .child(
                                    label()
                                        .text("❌ Rejeté")
                                        .color(Color::from_rgb(248, 113, 113))
                                        .font_size(11.5)
                                        .font_weight(FontWeight::BOLD),
                                )
                                .into_element(),
                            DiffStatus::Pending => rect()
                                .width(Size::px(0.))
                                .height(Size::px(0.))
                                .into_element(),
                        }),
                )
                .child(
                    Button::new()
                        .background(colors.surface_tertiary)
                        .hover_background(colors.tertiary)
                        .border_fill(Color::TRANSPARENT)
                        .color(colors.text_primary)
                        .corner_radius(4.)
                        .on_press(move |_| {
                            *center_mode_back.write() = CenterViewMode::Editor;
                        })
                        .child("← Retour Éditeur"),
                ),
        )
        .child(
            // Scrollable diff area
            rect()
                .width(Size::fill())
                .height(Size::flex(1.))
                .padding(8.)
                .background(colors.surface_secondary)
                .child(
                    ScrollView::new()
                        .child(rect().width(Size::fill()).children(line_elements)),
                ),
        )
        .into_element()
}

/// Antigravity-style collapsible "Changed Files Review Box" rendered at the end of the conversation.
/// Clicking the arrow button (`▸` / `▾`) unfolds or folds the list of modified files.
pub fn changed_files_review_box(
    diff_review: DiffReviewState,
    colors: ColorsSheet,
) -> Element {
    let diffs = diff_review.pending_diffs.read().clone();
    if diffs.is_empty() {
        return rect()
            .width(Size::px(0.))
            .height(Size::px(0.))
            .into_element();
    }

    let total_files = diffs.len();
    let mut total_added = 0usize;
    let mut total_deleted = 0usize;
    let mut pending_count = 0usize;
    let mut accepted_count = 0usize;
    let mut rejected_count = 0usize;
    for d in &diffs {
        let (a, del) = diff::diff_stats(d);
        total_added += a;
        total_deleted += del;
        match d.status {
            DiffStatus::Pending => pending_count += 1,
            DiffStatus::Accepted => accepted_count += 1,
            DiffStatus::Rejected => rejected_count += 1,
        }
    }

    let active_idx = (*diff_review.selected_diff_idx.read()).min(total_files.saturating_sub(1));
    let inline_idx = *diff_review.inline_diff_idx.read();
    let box_expanded = *diff_review.review_box_expanded.read();

    let file_cards: Vec<Element> = if box_expanded {
        diffs
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let item_status = item.status;
                let is_rejected = item_status == DiffStatus::Rejected;
                let is_expanded = !is_rejected && inline_idx == Some(idx);
                let is_selected = idx == active_idx;
                let (added, deleted) = diff::diff_stats(item);
                let is_new_file = item.old_content.trim().is_empty();
                let short_name = std::path::Path::new(&item.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| item.path.clone());
                let full_path = item.path.clone();

                let inline_diff_rows = if is_expanded {
                    build_numbered_diff_rows(&item.diff_lines, Some(80), &colors)
                } else {
                    Vec::new()
                };

                let status_badge = match item_status {
                    DiffStatus::Pending => rect()
                        .width(Size::px(0.))
                        .height(Size::px(0.))
                        .into_element(),
                    DiffStatus::Accepted => rect()
                        .padding(Gaps::new(2., 6., 2., 6.))
                        .background(Color::from_argb(40, 34, 197, 94))
                        .corner_radius(4.)
                        .child(
                            label()
                                .text("✅ Accepté")
                                .font_size(10.)
                                .font_weight(FontWeight::BOLD)
                                .color(Color::from_rgb(74, 222, 128)),
                        )
                        .into_element(),
                    DiffStatus::Rejected => rect()
                        .padding(Gaps::new(2., 6., 2., 6.))
                        .background(Color::from_argb(40, 239, 68, 68))
                        .corner_radius(4.)
                        .child(
                            label()
                                .text("❌ Rejeté")
                                .font_size(10.)
                                .font_weight(FontWeight::BOLD)
                                .color(Color::from_rgb(248, 113, 113)),
                        )
                        .into_element(),
                };

                rect()
                    .width(Size::fill())
                    .background(colors.surface_primary)
                    .corner_radius(6.)
                    .border(
                        Border::new()
                            .fill(if is_selected && !is_rejected {
                                colors.primary
                            } else {
                                colors.border
                            })
                            .width(1.),
                    )
                    .padding(Gaps::new(6., 10., 6., 10.))
                    .spacing(4.)
                    .child(
                        // Line 1: Badges + File Name + (+A -D)
                        rect()
                            .width(Size::fill())
                            .horizontal()
                            .cross_align(Alignment::Center)
                            .spacing(6.)
                            .child(
                                rect()
                                    .padding(Gaps::new(2., 5., 2., 5.))
                                    .background(if is_new_file {
                                        Color::from_argb(40, 34, 197, 94)
                                    } else {
                                        Color::from_argb(40, 234, 179, 8)
                                    })
                                    .corner_radius(4.)
                                    .child(
                                        label()
                                            .text(if is_new_file { "NOUVEAU" } else { "MODIFIÉ" })
                                            .font_size(9.5)
                                            .font_weight(FontWeight::BOLD)
                                            .color(if is_new_file {
                                                Color::from_rgb(74, 222, 128)
                                            } else {
                                                Color::from_rgb(250, 204, 21)
                                            }),
                                    ),
                            )
                            .child(status_badge)
                            .child(
                                label()
                                    .text(short_name)
                                    .font_size(12.5)
                                    .font_weight(FontWeight::BOLD)
                                    .color(if is_rejected {
                                        colors.text_secondary
                                    } else {
                                        colors.text_primary
                                    }),
                            )
                            .child(
                                rect()
                                    .padding(Gaps::new(1., 5., 1., 5.))
                                    .background(Color::from_argb(35, 34, 197, 94))
                                    .corner_radius(4.)
                                    .child(
                                        label()
                                            .text(format!("+{}", added))
                                            .font_size(11.)
                                            .font_weight(FontWeight::BOLD)
                                            .color(Color::from_rgb(74, 222, 128)),
                                    ),
                            )
                            .child(
                                rect()
                                    .padding(Gaps::new(1., 5., 1., 5.))
                                    .background(Color::from_argb(35, 239, 68, 68))
                                    .corner_radius(4.)
                                    .child(
                                        label()
                                            .text(format!("-{}", deleted))
                                            .font_size(11.)
                                            .font_weight(FontWeight::BOLD)
                                            .color(Color::from_rgb(248, 113, 113)),
                                    ),
                            ),
                    )
                    .child(
                        // Line 2: Full relative path
                        label()
                            .text(full_path)
                            .font_size(10.5)
                            .color(colors.text_secondary),
                    )
                    .child(if !is_rejected {
                        // Line 3: Action buttons (hidden completely if file is Rejected!)
                        rect()
                            .width(Size::fill())
                            .horizontal()
                            .cross_align(Alignment::Center)
                            .spacing(6.)
                            .child(
                                Button::new()
                                    .background(colors.surface_secondary)
                                    .hover_background(colors.surface_tertiary)
                                    .border_fill(colors.border)
                                    .corner_radius(4.)
                                    .padding(Gaps::new(3., 8., 3., 8.))
                                    .on_press(move |_| diff_review.toggle_inline_diff(idx))
                                    .child(
                                        label()
                                            .text(if is_expanded {
                                                "▾ Masquer le diff"
                                            } else {
                                                "▸ Voir le diff ici"
                                            })
                                            .font_size(11.)
                                            .color(colors.text_primary),
                                    ),
                            )
                            .child(
                                Button::new()
                                    .background(colors.surface_secondary)
                                    .hover_background(colors.surface_tertiary)
                                    .border_fill(colors.border)
                                    .corner_radius(4.)
                                    .padding(Gaps::new(3., 8., 3., 8.))
                                    .on_press(move |_| diff_review.open_diff_at(idx))
                                    .child(
                                        label()
                                            .text("🔍 Grand écran")
                                            .font_size(11.)
                                            .color(colors.text_highlight),
                                    ),
                            )
                            .child(if item_status == DiffStatus::Pending {
                                Button::new()
                                    .background(Color::from_rgb(22, 163, 74))
                                    .hover_background(Color::from_rgb(21, 128, 61))
                                    .border_fill(Color::TRANSPARENT)
                                    .corner_radius(4.)
                                    .padding(Gaps::new(3., 8., 3., 8.))
                                    .on_press(move |_| diff_review.accept_diff(idx))
                                    .child(
                                        label()
                                            .text("✓ Accepter")
                                            .font_size(11.)
                                            .font_weight(FontWeight::BOLD)
                                            .color(Color::WHITE),
                                    )
                                    .into_element()
                            } else {
                                rect().width(Size::px(0.)).height(Size::px(0.)).into_element()
                            })
                            .child(if item_status == DiffStatus::Pending {
                                Button::new()
                                    .background(Color::from_rgb(220, 38, 38))
                                    .hover_background(Color::from_rgb(185, 28, 28))
                                    .border_fill(Color::TRANSPARENT)
                                    .corner_radius(4.)
                                    .padding(Gaps::new(3., 8., 3., 8.))
                                    .on_press(move |_| diff_review.reject_diff(idx))
                                    .child(
                                        label()
                                            .text("✗ Rejeter")
                                            .font_size(11.)
                                            .font_weight(FontWeight::BOLD)
                                            .color(Color::WHITE),
                                    )
                                    .into_element()
                            } else {
                                rect().width(Size::px(0.)).height(Size::px(0.)).into_element()
                            })
                            .into_element()
                    } else {
                        rect().width(Size::px(0.)).height(Size::px(0.)).into_element()
                    })
                    .child(if is_expanded {
                        rect()
                            .width(Size::fill())
                            .padding(Gaps::new(6., 4., 4., 4.))
                            .background(colors.background)
                            .corner_radius(4.)
                            .border(Border::new().fill(colors.border).width(1.))
                            .children(inline_diff_rows)
                            .into_element()
                    } else {
                        rect()
                            .width(Size::px(0.))
                            .height(Size::px(0.))
                            .into_element()
                    })
                    .into_element()
            })
            .collect()
    } else {
        Vec::new()
    };

    rect()
        .width(Size::fill())
        .padding(12.)
        .background(colors.surface_secondary)
        .corner_radius(8.)
        .border(Border::new().fill(colors.primary).width(1.5))
        .spacing(8.)
        .child(
            // Header Row 1: Clickable Arrow Button (`▸` / `▾`) to unfold/fold the file list + Stats + Status
            rect()
                .width(Size::fill())
                .horizontal()
                .cross_align(Alignment::Center)
                .spacing(8.)
                .child(
                    Button::new()
                        .background(colors.surface_primary)
                        .hover_background(colors.surface_tertiary)
                        .border_fill(colors.border)
                        .corner_radius(5.)
                        .padding(Gaps::new(4., 10., 4., 10.))
                        .on_press(move |_| diff_review.toggle_review_box())
                        .child(
                            label()
                                .text(format!(
                                    "{} 🗂️ Revue des fichiers modifiés ({})",
                                    if box_expanded { "▾" } else { "▸" },
                                    total_files
                                ))
                                .font_size(12.5)
                                .font_weight(FontWeight::BOLD)
                                .color(colors.text_primary),
                        ),
                )
                .child(
                    rect()
                        .padding(Gaps::new(2., 6., 2., 6.))
                        .background(Color::from_argb(35, 34, 197, 94))
                        .corner_radius(4.)
                        .child(
                            label()
                                .text(format!("+{}", total_added))
                                .font_size(11.5)
                                .font_weight(FontWeight::BOLD)
                                .color(Color::from_rgb(74, 222, 128)),
                        ),
                )
                .child(
                    rect()
                        .padding(Gaps::new(2., 6., 2., 6.))
                        .background(Color::from_argb(35, 239, 68, 68))
                        .corner_radius(4.)
                        .child(
                            label()
                                .text(format!("-{}", total_deleted))
                                .font_size(11.5)
                                .font_weight(FontWeight::BOLD)
                                .color(Color::from_rgb(248, 113, 113)),
                        ),
                )
                .child(if pending_count == 0 && accepted_count == total_files {
                    label()
                        .text("✅ Tout accepté")
                        .font_size(11.5)
                        .color(Color::from_rgb(74, 222, 128))
                        .into_element()
                } else if pending_count == 0 && rejected_count == total_files {
                    label()
                        .text("❌ Tout rejeté")
                        .font_size(11.5)
                        .color(Color::from_rgb(248, 113, 113))
                        .into_element()
                } else if pending_count == 0 {
                    label()
                        .text(format!("{}/{} accepté(s)", accepted_count, total_files))
                        .font_size(11.5)
                        .color(colors.text_secondary)
                        .into_element()
                } else {
                    rect().width(Size::px(0.)).height(Size::px(0.)).into_element()
                }),
        )
        .child(if pending_count > 0 || accepted_count > 0 {
            // Header Row 2: Global Action Buttons (only shown while there are pending or accepted files)
            rect()
                .width(Size::fill())
                .horizontal()
                .cross_align(Alignment::Center)
                .spacing(8.)
                .child(
                    Button::new()
                        .background(colors.surface_primary)
                        .hover_background(colors.surface_tertiary)
                        .border_fill(colors.border)
                        .corner_radius(5.)
                        .padding(Gaps::new(4., 10., 4., 10.))
                        .on_press(move |_| diff_review.open_diff_at(active_idx))
                        .child(
                            label()
                                .text("🔍 Ouvrir dans l'éditeur")
                                .font_size(11.5)
                                .color(colors.text_highlight),
                        ),
                )
                .child(if pending_count > 0 {
                    Button::new()
                        .background(Color::from_rgb(22, 163, 74))
                        .hover_background(Color::from_rgb(21, 128, 61))
                        .border_fill(Color::TRANSPARENT)
                        .corner_radius(5.)
                        .padding(Gaps::new(4., 10., 4., 10.))
                        .on_press(move |_| diff_review.accept_all())
                        .child(
                            label()
                                .text(format!("✓ Tout accepter ({})", pending_count))
                                .font_size(11.5)
                                .font_weight(FontWeight::BOLD)
                                .color(Color::WHITE),
                        )
                        .into_element()
                } else {
                    rect().width(Size::px(0.)).height(Size::px(0.)).into_element()
                })
                .child(if pending_count > 0 {
                    Button::new()
                        .background(Color::from_rgb(220, 38, 38))
                        .hover_background(Color::from_rgb(185, 28, 28))
                        .border_fill(Color::TRANSPARENT)
                        .corner_radius(5.)
                        .padding(Gaps::new(4., 10., 4., 10.))
                        .on_press(move |_| diff_review.reject_all())
                        .child(
                            label()
                                .text("✗ Tout rejeter")
                                .font_size(11.5)
                                .font_weight(FontWeight::BOLD)
                                .color(Color::WHITE),
                        )
                        .into_element()
                } else {
                    rect().width(Size::px(0.)).height(Size::px(0.)).into_element()
                })
                .into_element()
        } else {
            rect().width(Size::px(0.)).height(Size::px(0.)).into_element()
        })
        .children(file_cards)
        .into_element()
}
