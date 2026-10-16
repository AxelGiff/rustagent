//! Confirmation bar shown when the assistant wants to run a shell command.

use crate::tools::{PendingCommand, resolve_command, set_auto_approve};
use freya::prelude::*;

/// A bottom bar asking the user to approve or reject a shell command proposed
/// by the assistant. It is shown while a request is pending and hides itself
/// once the request is resolved (driven by [`crate::tools::subscribe`]).
pub fn command_confirmation_bar(command: PendingCommand, colors: ColorsSheet) -> impl IntoElement {
    let accept_id = command.id.clone();
    let reject_id = command.id.clone();
    let always_id = command.id.clone();

    let on_accept = move |_| {
        let id = accept_id.clone();
        spawn(async move {
            let _ = resolve_command(&id, true).await;
        });
    };
    let on_reject = move |_| {
        let id = reject_id.clone();
        spawn(async move {
            let _ = resolve_command(&id, false).await;
        });
    };
    let on_always = move |_| {
        let id = always_id.clone();
        set_auto_approve(true);
        spawn(async move {
            let _ = resolve_command(&id, true).await;
        });
    };

    let mut command_box = rect()
        .width(Size::fill())
        .padding(Gaps::new(8., 12., 8., 12.))
        .background(colors.surface_tertiary)
        .corner_radius(6.)
        .content(Content::Flex)
        .spacing(4.)
        .child(
            label()
                .font_family("Jetbrains Mono")
                .font_size(12.5)
                .text(format!("$ {}", command.command))
                .color(colors.text_highlight),
        );

    if let Some(cwd) = command.cwd.clone() {
        command_box = command_box.child(
            label()
                .font_family("Jetbrains Mono")
                .font_size(11.)
                .text(format!("cwd: {}", cwd))
                .color(colors.text_secondary),
        );
    }

    rect()
        .width(Size::fill())
        .padding(Gaps::new(10., 14., 10., 14.))
        .background(colors.surface_primary)
        .border(Border::new().fill(colors.warning).width(BorderWidth {
            top: 2.,
            right: 0.,
            bottom: 0.,
            left: 0.,
        }))
        .content(Content::Flex)
        .spacing(8.)
        .child(
            label()
                .text("⚠️ L'assistant demande à exécuter une commande — confirmation requise")
                .color(colors.text_primary)
                .font_size(13.)
                .font_weight(FontWeight::BOLD),
        )
        .child(command_box)
        .child(
            rect()
                .horizontal()
                .spacing(8.)
                .cross_align(Alignment::Center)
                .child(
                    Button::new()
                        .background(colors.success)
                        .hover_background(Color::from_rgb(5, 150, 105))
                        .border_fill(Color::TRANSPARENT)
                        .color(Color::from_rgb(255, 255, 255))
                        .on_press(on_accept)
                        .child("✓ Exécuter"),
                )
                .child(
                    Button::new()
                        .background(colors.error)
                        .hover_background(Color::from_rgb(185, 28, 28))
                        .border_fill(Color::TRANSPARENT)
                        .color(Color::from_rgb(255, 255, 255))
                        .on_press(on_reject)
                        .child("✗ Refuser"),
                )
                .child(
                    Button::new()
                        .background(colors.surface_tertiary)
                        .hover_background(colors.tertiary)
                        .border_fill(Color::TRANSPARENT)
                        .color(colors.text_secondary)
                        .on_press(on_always)
                        .child("Toujours autoriser (session)"),
                ),
        )
}
