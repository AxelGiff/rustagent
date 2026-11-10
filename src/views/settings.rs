//! Application settings view page component for Albert API configuration.

use crate::agent;
use crate::config;
use crate::views::ui::{self, Tone};
use freya::prelude::*;

/// A dedicated, accessible full-page settings view for Albert API.
#[allow(clippy::too_many_arguments)]
pub fn settings_page_view<H0, H1, H2>(
    c: ColorsSheet,
    key_input: Writable<String>,
    model_input: Writable<String>,
    endpoint_input: Writable<String>,
    available_models: Readable<Vec<String>>,
    models_loading: Readable<bool>,
    feedback: Writable<String>,
    on_refresh_models: H0,
    on_save: H1,
    on_close: H2,
) -> impl IntoElement
where
    H0: Into<EventHandler<Event<PressEventData>>>,
    H1: Into<EventHandler<Event<PressEventData>>> + Clone,
    H2: Into<EventHandler<Event<PressEventData>>> + Clone,
{
    let models = available_models.read().clone();
    let is_loading = *models_loading.read();
    let current_selected = model_input.read().clone();

    let reset_albert_defaults = {
        let mut endpoint_input = endpoint_input.clone();
        let mut model_input = model_input.clone();
        let mut feedback = feedback.clone();
        move |_| {
            *endpoint_input.write() = agent::ALBERT_ENDPOINT.to_string();
            *model_input.write() = agent::ALBERT_MODEL.to_string();
            *feedback.write() =
                "🏛️ Configuration Albert par défaut restaurée (deepseek-v4-flash).".to_string();
        }
    };

    let model_list: Vec<String> = if models.is_empty() {
        config::ALBERT_MODELS
            .iter()
            .map(|&s| s.to_string())
            .collect()
    } else {
        models
    };

    let model_rows = model_list.into_iter().map({
        let model_input = model_input.clone();
        let c = c.clone();
        let cur = current_selected.clone();
        move |m| {
            let is_active = m == cur;
            let mut model_input_press = model_input.clone();
            let m_selected = m.clone();
            rect()
                .width(Size::fill())
                .padding(Gaps::new(6., 10., 6., 10.))
                .background(if is_active {
                    c.surface_tertiary
                } else {
                    Color::TRANSPARENT
                })
                .corner_radius(6.)
                .horizontal()
                .cross_align(Alignment::Center)
                .on_press(move |_| {
                    *model_input_press.write() = m_selected.clone();
                })
                .child(
                    label()
                        .text(format!("{} 🤖 {}", if is_active { "✓" } else { " " }, m))
                        .color(if is_active {
                            c.text_primary
                        } else {
                            c.text_secondary
                        })
                        .font_size(12.),
                )
        }
    });

    rect()
        .expanded()
        .background(c.background)
        .content(Content::Flex)
        .child(
            ui::panel_header(&c, 48.)
                .padding(Gaps::new(6., 20., 6., 20.))
                .spacing(12.)
                .child(ui::btn_sm(
                    "← Retour au workspace",
                    Tone::Neutral,
                    &c,
                    on_close.clone(),
                ))
                .child(
                    ui::row(8.)
                        .width(Size::flex(1.))
                        .child(ui::title("⚙ Paramètres & Configuration Albert API", &c).font_size(15.)),
                )
                .child(ui::btn_sm(
                    "🏛️ Valeurs par défaut Albert",
                    Tone::Neutral,
                    &c,
                    reset_albert_defaults,
                ))
                .child(ui::btn_sm("💾 Enregistrer", Tone::Success, &c, on_save.clone())),
        )
        .child(
            rect()
                .expanded()
                .content(Content::Flex)
                .child(
                    ScrollView::new().child(
                        rect()
                            .width(Size::fill())
                            .padding(Gaps::new(20., 32., 24., 32.))
                            .spacing(14.)
                            .content(Content::Flex)
                            .child(
                                // Card: Albert API Key
                                ui::card(&c)
                                    .child(ui::title("Clé API Albert (Étalab)", &c))
                                    .child(ui::subtitle(
                                        "Clé d'authentification pour accéder aux modèles de la plateforme Albert API.",
                                        &c,
                                    ))
                                    .child(
                                        Input::new(key_input)
                                            .mode(InputMode::Hidden('•'))
                                            .background(c.surface_secondary)
                                            .focus_background(c.surface_tertiary)
                                            .border_fill(c.border)
                                            .color(c.text_inverse)
                                            .placeholder("sk-...")
                                            .width(Size::fill()),
                                    ),
                            )
                            .child(
                                // Card: Endpoint URL
                                ui::card(&c)
                                    .child(ui::title("URL Endpoint Albert API", &c))
                                    .child(ui::subtitle(
                                        "Endpoint officiel Albert : https://albert.api.etalab.gouv.fr/v1",
                                        &c,
                                    ))
                                    .child(
                                        Input::new(endpoint_input)
                                            .background(c.surface_secondary)
                                            .focus_background(c.surface_tertiary)
                                            .border_fill(c.border)
                                            .color(c.text_inverse)
                                            .placeholder("https://albert.api.etalab.gouv.fr/v1")
                                            .width(Size::fill()),
                                    ),
                            )
                            .child(
                                // Card: Models selection
                                ui::card(&c)
                                    .spacing(10.)
                                    .content(Content::Flex)
                                    .child(
                                        ui::row_fill(8.)
                                            .child(
                                                rect()
                                                    .width(Size::flex(1.))
                                                    .spacing(2.)
                                                    .child(ui::title("Modèle Albert sélectionné", &c))
                                                    .child(ui::subtitle(
                                                        "Sélectionnez un modèle Albert ci-dessous ou saisissez son identifiant.",
                                                        &c,
                                                    )),
                                            )
                                            .child(ui::btn_sm(
                                                if is_loading {
                                                    "⟳ Chargement..."
                                                } else {
                                                    "⟳ Rafraîchir les modèles"
                                                },
                                                Tone::Neutral,
                                                &c,
                                                on_refresh_models,
                                            )),
                                    )
                                    .child(
                                        Input::new(model_input)
                                            .background(c.surface_secondary)
                                            .focus_background(c.surface_tertiary)
                                            .border_fill(c.border)
                                            .color(c.text_inverse)
                                            .placeholder("deepseek-v4-flash")
                                            .width(Size::fill()),
                                    )
                                    .child(
                                        rect()
                                            .width(Size::fill())
                                            .height(Size::px(180.))
                                            .background(c.surface_secondary)
                                            .corner_radius(6.)
                                            .border(Border::new().fill(c.border).width(1.))
                                            .padding(6.)
                                            .child(
                                                ScrollView::new().child(
                                                    ui::col(4.).children(model_rows),
                                                ),
                                            ),
                                    ),
                            )
                            .child(
                                // Feedback / Status & Config file location
                                ui::col(6.)
                                    .child(ui::muted(
                                        format!(
                                            "Fichier de configuration locale : {}",
                                            config::ApiKeyConfig::config_file_path().display()
                                        ),
                                        &c,
                                    ))
                                    .maybe_child((!feedback.read().is_empty()).then(|| {
                                        label()
                                            .text(feedback.read().clone())
                                            .color(c.warning)
                                            .font_size(12.)
                                    })),
                            )
                            .child(
                                ui::row(12.)
                                    .child(ui::btn("💾 Enregistrer", Tone::Success, &c, on_save))
                                    .child(ui::btn("← Retour", Tone::Neutral, &c, on_close)),
                            ),
                    ),
                ),
        )
}
