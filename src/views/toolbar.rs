//! Top application toolbar: title, theme preset selector, settings trigger, chat/terminal reset buttons.

use crate::api;
use crate::config;
use crate::theme::ThemePreset;
use crate::views::ui::{self, Tone};
use freya::prelude::*;

/// Builds the top application toolbar.
#[allow(clippy::too_many_arguments)] // a UI panel receives one handle per piece of state
pub fn app_toolbar(
    current_theme: Writable<ThemePreset>,
    theme_state: Writable<Theme>,
    show_settings: Writable<bool>,
    companion_mode: Writable<bool>,
    show_sidebar: Writable<bool>,
    show_dashboard: Writable<bool>,
    available_models: Writable<Vec<String>>,
    models_loading: Writable<bool>,
    _settings_endpoint_input: Readable<String>,
    _settings_key_input: Readable<String>,
    mut on_open_project: impl FnMut() + 'static,
    mut on_clear_chat: impl FnMut() + 'static,
    mut on_reset_terminal: impl FnMut() + 'static,
    c: ColorsSheet,
) -> impl IntoElement {
    let active_preset = *current_theme.read();
    let c_menu = c.clone();
    let theme_items = ThemePreset::ALL.iter().map({
        let current_theme = current_theme;
        let theme_state = theme_state;
        move |&preset| {
            let is_sel = preset == active_preset;
            let mut current_theme = current_theme.clone();
            let mut theme_state = theme_state.clone();
            ui::menu_item(
                format!("{} {}", preset.icon(), preset.name()),
                is_sel,
                &c_menu,
                move |_| {
                    *current_theme.write() = preset;
                    *theme_state.write() = preset.to_theme();
                },
            )
        }
    });

    let is_comp = *companion_mode.read();
    let mut comp_toggle = companion_mode;
    let is_sidebar_open = *show_sidebar.read();
    let mut sidebar_toggle = show_sidebar;
    let is_dashboard_open = *show_dashboard.read();
    let mut dashboard_toggle = show_dashboard;

    ui::panel_header(&c, 46.)
        .child(
            ui::row(8.)
                .child(ui::title("RustAgent", &c).font_size(15.))
                .maybe_child((!is_sidebar_open).then(|| {
                    ui::btn_sm(
                        "▶ Explorateur",
                        Tone::Primary,
                        &c,
                        move |_| {
                            *sidebar_toggle.write() = true;
                        },
                    )
                }))
                .maybe_child((!is_dashboard_open).then(|| {
                    ui::btn_sm(
                        "🎛 Dashboard",
                        Tone::Primary,
                        &c,
                        move |_| {
                            *dashboard_toggle.write() = true;
                        },
                    )
                }))
                .child(ui::btn_sm(
                    if is_comp { "Mode Compagnon" } else { "Mode Classique" },
                    Tone::Subtle,
                    &c,
                    move |_| {
                        *comp_toggle.write() = !is_comp;
                    },
                ))
                .child(ui::btn_sm(
                    "Ouvrir un dossier",
                    Tone::Subtle,
                    &c,
                    move |_| on_open_project(),
                )),
        )
        .child(ui::spacer())
        .child(ui::select(
            format!("{} Thème", active_preset.icon()),
            theme_items,
            &c,
        ))
        .child(ui::btn_sm(
            "Paramètres",
            Tone::Neutral,
            &c,
            {
                let available_models = available_models;
                let models_loading = models_loading;
                let mut show_settings = show_settings;
                move |_| {
                    *show_settings.write() = true;
                    if available_models.read().is_empty() {
                        let cfg = config::ApiKeyConfig::load();
                        let ep = cfg
                            .endpoint
                            .unwrap_or_else(|| crate::agent::ALBERT_ENDPOINT.to_string());
                        let k = cfg.key;
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
            },
        ))
        .child(ui::btn_sm(
            "Effacer le chat",
            Tone::Neutral,
            &c,
            move |_| on_clear_chat(),
        ))
        .maybe_child((!is_comp).then(|| {
            ui::btn_sm(
                "Reset Terminal",
                Tone::Neutral,
                &c,
                move |_| on_reset_terminal(),
            )
        }))
}
