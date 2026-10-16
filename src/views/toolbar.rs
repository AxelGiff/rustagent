//! Top application toolbar: title, theme preset selector, settings trigger, chat/terminal reset buttons.

use freya::prelude::*;
use crate::api;
use crate::theme::ThemePreset;

/// Builds the top application toolbar.
pub fn app_toolbar(
    current_theme: Writable<ThemePreset>,
    theme_state: Writable<Theme>,
    mut show_settings: Writable<bool>,
    available_models: Writable<Vec<String>>,
    models_loading: Writable<bool>,
    settings_endpoint_input: Readable<String>,
    settings_key_input: Readable<String>,
    mut on_clear_chat: impl FnMut() + 'static,
    mut on_reset_terminal: impl FnMut() + 'static,
    c: ColorsSheet,
) -> impl IntoElement {
    let active_preset = *current_theme.read();
    let theme_items = ThemePreset::ALL.iter().map({
        let current_theme = current_theme;
        let theme_state = theme_state;
        move |&preset| {
            let is_sel = preset == active_preset;
            let mut current_theme = current_theme.clone();
            let mut theme_state = theme_state.clone();
            MenuItem::new()
                .selected(is_sel)
                .on_press(move |_| {
                    *current_theme.write() = preset;
                    *theme_state.write() = preset.to_theme();
                })
                .child(format!("{} {}", preset.icon(), preset.name()))
        }
    });

    rect()
        .width(Size::fill())
        .height(Size::px(44.))
        .padding(8.)
        .background(c.surface_primary)
        .border(Border::new().fill(c.border).width(BorderWidth {
            top: 0.,
            right: 0.,
            bottom: 1.,
            left: 0.,
        }))
        .horizontal()
        .cross_align(Alignment::Center)
        .spacing(8.)
        .content(Content::Flex)
        .child(
            label()
                .text("Coding Assistant")
                .color(c.text_primary)
                .font_size(16.)
                .font_weight(FontWeight::BOLD),
        )
        .child(rect().width(Size::flex(1.)))
        .child(
            Select::new()
                .selected_item(format!("{} Thème", active_preset.icon()))
                .children(theme_items),
        )
        .child(
            Button::new()
                .background(c.surface_tertiary)
                .hover_background(c.tertiary)
                .border_fill(Color::TRANSPARENT)
                .color(c.text_secondary)
                .on_press({
                    let ep_val = settings_endpoint_input.read().clone();
                    let key_val = settings_key_input.read().clone();
                    let available_models = available_models;
                    let models_loading = models_loading;
                    let mut show_settings = show_settings;
                    move |_| {
                        *show_settings.write() = true;
                        if available_models.read().is_empty() {
                            let ep = ep_val.trim().to_string();
                            let k = key_val.trim().to_string();
                            let mut am = available_models.clone();
                            let mut ml = models_loading.clone();
                            *ml.write() = true;
                            spawn(async move {
                                if let Ok(m) = api::fetch_models(&ep, &k).await {
                                    *am.write() = m;
                                }
                                *ml.write() = false;
                            });
                        }
                    }
                })
                .child("Settings"),
        )
        .child(
            Button::new()
                .background(c.surface_tertiary)
                .hover_background(c.tertiary)
                .border_fill(Color::TRANSPARENT)
                .color(c.text_secondary)
                .on_press(move |_| on_clear_chat())
                .child("Clear Chat"),
        )
        .child(
            Button::new()
                .background(c.surface_tertiary)
                .hover_background(c.tertiary)
                .border_fill(Color::TRANSPARENT)
                .color(c.text_secondary)
                .on_press(move |_| on_reset_terminal())
                .child("Reset Terminal"),
        )
}

