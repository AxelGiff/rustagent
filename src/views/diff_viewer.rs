//! Diff review panel component.

use freya::prelude::*;
use crate::diff;

/// A dedicated diff preview panel for user review before accepting or rejecting file modifications.
pub fn diff_viewer_panel(
    pending_diffs: Writable<Vec<diff::PendingDiff>>,
    mut on_accept: impl FnMut(usize) + 'static,
    mut on_accept_all: impl FnMut() + 'static,
    mut on_reject: impl FnMut(usize) + 'static,
    mut on_reject_all: impl FnMut() + 'static,
    mut on_switch_to_editor: impl FnMut() + 'static,
    colors: ColorsSheet,
) -> impl IntoElement {
    let diffs = pending_diffs.read().clone();
    if diffs.is_empty() {
        return rect()
            .expanded()
            .center()
            .background(colors.background)
            .child(label().text("Aucun diff en attente.").color(colors.text_secondary))
            .into_element();
    }

    let diff = &diffs[0];
    let diff_path = diff.path.clone();
    let diff_count = diffs.len();

    let mut added_lines = 0;
    let mut deleted_lines = 0;
    for l in &diff.diff_lines {
        match l {
            diff::DiffLine::Added(_) => added_lines += 1,
            diff::DiffLine::Deleted(_) => deleted_lines += 1,
            diff::DiffLine::Unchanged(_) => {}
        }
    }

    let accept_handler = move |_| {
        on_accept(0);
    };

    let accept_all_handler = move |_| {
        on_accept_all();
    };

    let reject_handler = move |_| {
        on_reject(0);
    };

    let reject_all_handler = move |_| {
        on_reject_all();
    };

    let switch_handler = move |_| {
        on_switch_to_editor();
    };

    let mut line_elements = Vec::new();
    for d_line in &diff.diff_lines {
        match d_line {
            diff::DiffLine::Deleted(text) => {
                line_elements.push(
                    rect()
                        .width(Size::fill())
                        .padding(Gaps::new(2., 10., 2., 10.))
                        .background(Color::from_argb(45, 239, 68, 68))
                        .child(
                            label()
                                .font_family("Jetbrains Mono")
                                .font_size(12.5)
                                .text(format!("- {}", text))
                                .color(Color::from_rgb(248, 113, 113)),
                        )
                        .into_element(),
                );
            }
            diff::DiffLine::Added(text) => {
                line_elements.push(
                    rect()
                        .width(Size::fill())
                        .padding(Gaps::new(2., 10., 2., 10.))
                        .background(Color::from_argb(45, 34, 197, 94))
                        .child(
                            label()
                                .font_family("Jetbrains Mono")
                                .font_size(12.5)
                                .text(format!("+ {}", text))
                                .color(Color::from_rgb(74, 222, 128)),
                        )
                        .into_element(),
                );
            }
            diff::DiffLine::Unchanged(text) => {
                line_elements.push(
                    rect()
                        .width(Size::fill())
                        .padding(Gaps::new(2., 10., 2., 10.))
                        .child(
                            label()
                                .font_family("Jetbrains Mono")
                                .font_size(12.5)
                                .text(format!("  {}", text))
                                .color(colors.text_secondary),
                        )
                        .into_element(),
                );
            }
        }
    }

    rect()
        .expanded()
        .content(Content::Flex)
        .background(colors.background)
        .child(
            // Row 1: File navigation and stats bar
            rect()
                .width(Size::fill())
                .height(Size::px(38.))
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
                .child(
                    rect()
                        .expanded()
                        .horizontal()
                        .cross_align(Alignment::Center)
                        .spacing(8.)
                        .child(
                            label()
                                .text("📝 Revue :")
                                .color(colors.text_primary)
                                .font_size(13.)
                                .font_weight(FontWeight::BOLD),
                        )
                        .child(
                            rect()
                                .padding(Gaps::new(2., 8., 2., 8.))
                                .background(colors.surface_secondary)
                                .corner_radius(4.)
                                .border(Border::new().fill(colors.border).width(1.))
                                .child(
                                    label()
                                        .text(diff_path)
                                        .color(colors.text_highlight)
                                        .font_size(12.)
                                        .font_weight(FontWeight::BOLD),
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
                                        .font_size(12.)
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
                                        .font_size(12.)
                                        .font_weight(FontWeight::BOLD),
                                ),
                        )
                        .child(if diff_count > 1 {
                            label()
                                .text(format!("(1/{} en attente)", diff_count))
                                .color(colors.text_secondary)
                                .font_size(12.)
                                .into_element()
                        } else {
                            rect().width(Size::px(0.)).height(Size::px(0.)).into_element()
                        }),
                )
                .child(
                    Button::new()
                        .background(colors.surface_tertiary)
                        .hover_background(colors.tertiary)
                        .border_fill(Color::TRANSPARENT)
                        .color(colors.text_primary)
                        .corner_radius(4.)
                        .on_press(switch_handler)
                        .child("← Retour Éditeur"),
                ),
        )
        .child(
            // Row 2: Dedicated prominent action banner
            rect()
                .width(Size::fill())
                .height(Size::px(48.))
                .padding(Gaps::new(6., 16., 6., 16.))
                .background(colors.surface_secondary)
                .border(Border::new().fill(colors.border).width(BorderWidth {
                    top: 0.,
                    right: 0.,
                    bottom: 1.,
                    left: 0.,
                }))
                .horizontal()
                .cross_align(Alignment::Center)
                .spacing(12.)
                .child(
                    Button::new()
                        .background(Color::from_rgb(22, 163, 74))
                        .hover_background(Color::from_rgb(21, 128, 61))
                        .border_fill(Color::TRANSPARENT)
                        .color(Color::WHITE)
                        .corner_radius(6.)
                        .on_press(accept_handler)
                        .child("✓ Accepter ce fichier"),
                )
                .child(if diff_count > 1 {
                    Button::new()
                        .background(Color::from_rgb(16, 185, 129))
                        .hover_background(Color::from_rgb(5, 150, 105))
                        .border_fill(Color::TRANSPARENT)
                        .color(Color::WHITE)
                        .corner_radius(6.)
                        .on_press(accept_all_handler)
                        .child(format!("✓ Tout accepter ({})", diff_count))
                        .into_element()
                } else {
                    rect().width(Size::px(0.)).height(Size::px(0.)).into_element()
                })
                .child(
                    Button::new()
                        .background(Color::from_rgb(220, 38, 38))
                        .hover_background(Color::from_rgb(185, 28, 28))
                        .border_fill(Color::TRANSPARENT)
                        .color(Color::WHITE)
                        .corner_radius(6.)
                        .on_press(reject_handler)
                        .child("✗ Rejeter ce fichier"),
                )
                .child(if diff_count > 1 {
                    Button::new()
                        .background(Color::from_rgb(127, 29, 29))
                        .hover_background(Color::from_rgb(153, 27, 27))
                        .border_fill(Color::TRANSPARENT)
                        .color(Color::WHITE)
                        .corner_radius(6.)
                        .on_press(reject_all_handler)
                        .child("✗ Tout rejeter")
                        .into_element()
                } else {
                    rect().width(Size::px(0.)).height(Size::px(0.)).into_element()
                })
                .child(
                    label()
                        .text("— Cliquez pour confirmer ou refuser l'écriture")
                        .color(colors.text_secondary)
                        .font_size(11.5),
                ),
        )
        .child(
            rect()
                .expanded()
                .padding(8.)
                .background(colors.surface_secondary)
                .child(
                    ScrollView::new().child(
                        rect().width(Size::fill()).children(line_elements),
                    ),
                ),
        )
        .into_element()
}

