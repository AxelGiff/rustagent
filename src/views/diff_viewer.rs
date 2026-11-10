//! Diff review panel and Antigravity-style Changed Files Review box.

use crate::diff::{self, DiffStatus};
use crate::language::SupportedLanguage;
use crate::types::{CenterViewMode, Message, Role};
use crate::views::editor::update_editor_if_matching;
use crate::views::ui::{self, Tone};
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
                } else {
                    d.status = DiffStatus::Accepted;
                    applied.push((d.path.clone(), d.new_content.clone()));
                }
            }
        }

        for (path, new_content) in &applied {
            update_editor_if_matching(
                path,
                new_content,
                &self.file_name.read(),
                self.editor,
                self.current_language,
            );
        }

        for err in errors {
            self.messages.write().push(Message {
                role: Role::AI,
                content: err,
            });
        }

        *self.center_view_mode.write() = CenterViewMode::Editor;
    }

    /// Permanently rejects a `Pending` diff.
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

/// Renders a single numbered unified diff line (`Deleted`, `Added`, or `Unchanged`).
fn render_diff_line(
    ln: usize,
    prefix: &str,
    text: &str,
    bg: Color,
    fg: Color,
    ln_col: Color,
) -> Element {
    rect()
        .width(Size::fill())
        .horizontal()
        .background(bg)
        .padding(Gaps::new(2., 8., 2., 8.))
        .child(
            rect()
                .width(Size::px(38.))
                .child(ui::mono(format!("{:>3} ", ln), 11.5, ln_col)),
        )
        .child(ui::mono(format!("{} {}", prefix, text), 12., fg))
        .into_element()
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
                    .child(ui::mono(
                        format!(
                            "... +{} ligne(s) — cliquez sur « 🔍 Grand écran » pour tout voir",
                            remaining
                        ),
                        11.,
                        colors.text_secondary,
                    ))
                    .into_element(),
            );
            break;
        }

        match d_line {
            diff::DiffLine::Deleted(text) => {
                let ln = old_ln;
                old_ln += 1;
                elements.push(render_diff_line(
                    ln,
                    "-",
                    text,
                    Color::from_argb(45, 239, 68, 68),
                    Color::from_rgb(252, 165, 165),
                    Color::from_rgb(185, 80, 80),
                ));
            }
            diff::DiffLine::Added(text) => {
                let ln = new_ln;
                new_ln += 1;
                elements.push(render_diff_line(
                    ln,
                    "+",
                    text,
                    Color::from_argb(45, 34, 197, 94),
                    Color::from_rgb(134, 239, 172),
                    Color::from_rgb(80, 185, 100),
                ));
            }
            diff::DiffLine::Unchanged(text) => {
                let ln = new_ln;
                old_ln += 1;
                new_ln += 1;
                elements.push(render_diff_line(
                    ln,
                    " ",
                    text,
                    Color::TRANSPARENT,
                    colors.text_secondary,
                    colors.text_placeholder,
                ));
            }
        }
    }

    elements
}

/// Renders the status badge (`✅ Accepté` or `❌ Rejeté`, or empty when `Pending`).
fn render_status_tag(status: DiffStatus, c: &ColorsSheet) -> Element {
    match status {
        DiffStatus::Pending => ui::empty(),
        DiffStatus::Accepted => ui::tag("✅ Accepté", Tone::Success, c).into_element(),
        DiffStatus::Rejected => ui::tag("❌ Rejeté", Tone::Danger, c).into_element(),
    }
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
            .child(ui::subtitle("Aucun diff à afficher.", &colors))
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
    let c_menu = colors.clone();
    let file_menu_items = diffs.iter().enumerate().map(move |(idx, item)| {
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
        ui::menu_item(
            format!(
                "{}. {}{} (+{} -{})",
                idx + 1,
                status_prefix,
                short_name,
                add,
                del
            ),
            is_selected,
            &c_menu,
            move |_| {
                *sel.write() = idx;
            },
        )
    });

    let file_select = ui::select(
        format!(
            "📄 {} ({}/{})",
            active_short_name,
            active_idx + 1,
            diff_count
        ),
        file_menu_items,
        &colors,
    );

    let line_elements = build_numbered_diff_rows(&diff.diff_lines, None, &colors);
    let mut center_mode_back = diff_review.center_view_mode;

    rect()
        .expanded()
        .content(Content::Flex)
        .background(colors.background)
        .child(
            ui::panel_header(&colors, 46.)
                .child(
                    ui::row(8.)
                        .width(Size::flex(1.))
                        .child(file_select)
                        .child(ui::tag(diff_path, Tone::Subtle, &colors))
                        .child(ui::diff_stats(added_lines, deleted_lines, &colors))
                        .child(render_status_tag(diff_status, &colors)),
                )
                .child(ui::btn_sm(
                    "← Retour Éditeur",
                    Tone::Neutral,
                    &colors,
                    move |_| {
                        *center_mode_back.write() = CenterViewMode::Editor;
                    },
                )),
        )
        .child(
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
        return ui::empty();
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
                let is_selected = idx == active_idx && !is_rejected;
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

                ui::item_card(&colors, is_selected)
                    .child(
                        // Line 1: New/Modified tag + Status + Filename + (+A -D)
                        ui::row_fill(6.)
                            .child(if is_new_file {
                                ui::tag("NOUVEAU", Tone::Success, &colors)
                            } else {
                                ui::tag("MODIFIÉ", Tone::Warning, &colors)
                            })
                            .child(render_status_tag(item_status, &colors))
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
                            .child(ui::diff_stats(added, deleted, &colors)),
                    )
                    .child(ui::muted(full_path, &colors))
                    .maybe_child((!is_rejected).then(|| {
                        // Line 3: Action buttons (hidden completely once a file is Rejected)
                        ui::row_fill(6.)
                            .child(ui::btn_sm(
                                if is_expanded {
                                    "▾ Masquer le diff"
                                } else {
                                    "▸ Voir le diff ici"
                                },
                                Tone::Subtle,
                                &colors,
                                move |_| diff_review.toggle_inline_diff(idx),
                            ))
                            .child(ui::btn_sm(
                                "🔍 Grand écran",
                                Tone::Subtle,
                                &colors,
                                move |_| diff_review.open_diff_at(idx),
                            ))
                            .maybe_child((item_status == DiffStatus::Pending).then(|| {
                                ui::btn_sm("✓ Accepter", Tone::Success, &colors, move |_| {
                                    diff_review.accept_diff(idx)
                                })
                            }))
                            .maybe_child((item_status == DiffStatus::Pending).then(|| {
                                ui::btn_sm("✗ Rejeter", Tone::Danger, &colors, move |_| {
                                    diff_review.reject_diff(idx)
                                })
                            }))
                    }))
                    .maybe_child(is_expanded.then(|| {
                        rect()
                            .width(Size::fill())
                            .padding(Gaps::new(6., 4., 4., 4.))
                            .background(colors.background)
                            .corner_radius(6.)
                            .border(Border::new().fill(colors.border).width(1.))
                            .children(inline_diff_rows)
                    }))
                    .into_element()
            })
            .collect()
    } else {
        Vec::new()
    };

    ui::accent_card(&colors, colors.primary)
        .child(
            // Header Row 1: Clickable Arrow Button (`▸` / `▾`) + Stats + Summary Status
            ui::row_fill(8.)
                .child(ui::btn_sm(
                    format!(
                        "{} 🗂️ Revue des fichiers modifiés ({})",
                        if box_expanded { "▾" } else { "▸" },
                        total_files
                    ),
                    Tone::Subtle,
                    &colors,
                    move |_| diff_review.toggle_review_box(),
                ))
                .child(ui::diff_stats(total_added, total_deleted, &colors))
                .child(if pending_count == 0 && accepted_count == total_files {
                    ui::tag("✅ Tout accepté", Tone::Success, &colors).into_element()
                } else if pending_count == 0 && rejected_count == total_files {
                    ui::tag("❌ Tout rejeté", Tone::Danger, &colors).into_element()
                } else if pending_count == 0 {
                    ui::tag(
                        format!("{}/{} accepté(s)", accepted_count, total_files),
                        Tone::Neutral,
                        &colors,
                    )
                    .into_element()
                } else {
                    ui::empty()
                }),
        )
        .maybe_child((pending_count > 0 || accepted_count > 0).then(|| {
            // Header Row 2: Global Action Buttons
            ui::row_fill(8.)
                .child(ui::btn_sm(
                    "🔍 Ouvrir dans l'éditeur",
                    Tone::Subtle,
                    &colors,
                    move |_| diff_review.open_diff_at(active_idx),
                ))
                .maybe_child((pending_count > 0).then(|| {
                    ui::btn_sm(
                        format!("✓ Tout accepter ({})", pending_count),
                        Tone::Success,
                        &colors,
                        move |_| diff_review.accept_all(),
                    )
                }))
                .maybe_child((pending_count > 0).then(|| {
                    ui::btn_sm("✗ Tout rejeter", Tone::Danger, &colors, move |_| {
                        diff_review.reject_all()
                    })
                }))
        }))
        .children(file_cards)
        .into_element()
}
