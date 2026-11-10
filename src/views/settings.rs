//! Application settings view page component for Albert API configuration.

use crate::agent;
use crate::config;
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
            *feedback.write() = "🏛️ Configuration Albert par défaut restaurée (deepseek-v4-flash).".to_string();
        }
    };

    rect()
        .expanded()
        .background(c.background)
        .content(Content::Flex)
        .child(
            // Top Navigation Bar
            rect()
                .width(Size::fill())
                .height(Size::px(48.))
                .padding(Gaps::new(6., 20., 6., 20.))
                .background(c.surface_primary)
                .border(Border::new().fill(c.border).width(BorderWidth {
                    top: 0.,
                    right: 0.,
                    bottom: 1.,
                    left: 0.,
                }))
                .horizontal()
                .cross_align(Alignment::Center)
                .content(Content::Flex)
                .spacing(14.)
                .child(
                    Button::new()
                        .background(c.surface_tertiary)
                        .hover_background(c.tertiary)
                        .border_fill(Color::TRANSPARENT)
                        .color(c.text_primary)
                        .on_press(on_close.clone())
                        .child("← Retour au workspace"),
                )
                .child(
                    rect()
                        .width(Size::flex(1.))
                        .horizontal()
                        .cross_align(Alignment::Center)
                        .child(
                            label()
                                .text("⚙ Paramètres & Configuration Albert API")
                                .color(c.text_primary)
                                .font_size(15.)
                                .font_weight(FontWeight::BOLD),
                        ),
                )
                .child(
                    Button::new()
                        .background(c.surface_tertiary)
                        .hover_background(c.tertiary)
                        .border_fill(Color::TRANSPARENT)
                        .color(c.text_primary)
                        .on_press(reset_albert_defaults)
                        .child("🏛️ Valeurs par défaut Albert"),
                )
                .child(
                    Button::new()
                        .background(Color::from_rgb(22, 163, 74))
                        .hover_background(Color::from_rgb(21, 128, 61))
                        .border_fill(Color::TRANSPARENT)
                        .color(Color::WHITE)
                        .on_press(on_save.clone())
                        .child("💾 Enregistrer"),
                ),
        )
        .child(
            // Full-Page Scrollable Content Body
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
                                rect()
                                    .width(Size::fill())
                                    .padding(16.)
                                    .background(c.surface_primary)
                                    .corner_radius(8.)
                                    .border(Border::new().fill(c.border).width(1.))
                                    .spacing(8.)
                                    .child(
                                        label()
                                            .text("Clé API Albert (Étalab)")
                                            .color(c.text_primary)
                                            .font_size(14.)
                                            .font_weight(FontWeight::BOLD),
                                    )
                                    .child(
                                        label()
                                            .text("Clé d'authentification pour accéder aux modèles de la plateforme Albert API.")
                                            .color(c.text_secondary)
                                            .font_size(12.),
                                    )
                                    .child(
                                        Input::new(key_input)
                                            .mode(InputMode::Hidden('•'))
                                            .background(c.surface_secondary)
                                            .focus_background(c.surface_tertiary)
                                            .border_fill(Color::TRANSPARENT)
                                            .color(c.text_inverse)
                                            .placeholder("sk-...")
                                            .width(Size::fill()),
                                    ),
                            )
                            .child(
                                // Card: Endpoint URL
                                rect()
                                    .width(Size::fill())
                                    .padding(16.)
                                    .background(c.surface_primary)
                                    .corner_radius(8.)
                                    .border(Border::new().fill(c.border).width(1.))
                                    .spacing(8.)
                                    .child(
                                        label()
                                            .text("URL Endpoint Albert API")
                                            .color(c.text_primary)
                                            .font_size(14.)
                                            .font_weight(FontWeight::BOLD),
                                    )
                                    .child(
                                        label()
                                            .text("Endpoint officiel Albert : https://albert.api.etalab.gouv.fr/v1")
                                            .color(c.text_secondary)
                                            .font_size(12.),
                                    )
                                    .child(
                                        Input::new(endpoint_input)
                                            .background(c.surface_secondary)
                                            .focus_background(c.surface_tertiary)
                                            .border_fill(Color::TRANSPARENT)
                                            .color(c.text_inverse)
                                            .placeholder("https://albert.api.etalab.gouv.fr/v1")
                                            .width(Size::fill()),
                                    ),
                            )
                            .child(
                                // Card: Models selection
                                rect()
                                    .width(Size::fill())
                                    .padding(16.)
                                    .background(c.surface_primary)
                                    .corner_radius(8.)
                                    .border(Border::new().fill(c.border).width(1.))
                                    .spacing(10.)
                                    .content(Content::Flex)
                                    .child(
                                        rect()
                                            .width(Size::fill())
                                            .horizontal()
                                            .cross_align(Alignment::Center)
                                            .child(
                                                rect()
                                                    .width(Size::flex(1.))
                                                    .spacing(2.)
                                                    .child(
                                                        label()
                                                            .text("Modèle Albert sélectionné")
                                                            .color(c.text_primary)
                                                            .font_size(14.)
                                                            .font_weight(FontWeight::BOLD),
                                                    )
                                                    .child(
                                                        label()
                                                            .text("Sélectionnez un modèle Albert ci-dessous ou saisissez son identifiant.")
                                                            .color(c.text_secondary)
                                                            .font_size(12.),
                                                    ),
                                            )
                                            .child(
                                                Button::new()
                                                    .background(c.surface_tertiary)
                                                    .hover_background(c.tertiary)
                                                    .border_fill(Color::TRANSPARENT)
                                                    .color(c.text_primary)
                                                    .on_press(on_refresh_models)
                                                    .child(if is_loading {
                                                        "⟳ Chargement..."
                                                    } else {
                                                        "⟳ Rafraîchir les modèles"
                                                    }),
                                            ),
                                    )
                                    .child(
                                        Input::new(model_input.clone())
                                            .background(c.surface_secondary)
                                            .focus_background(c.surface_tertiary)
                                            .border_fill(Color::TRANSPARENT)
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
                                            .padding(6.)
                                            .child(if models.is_empty() {
                                                let suggestions = config::ALBERT_MODELS;
                                                ScrollView::new().child(
                                                    rect()
                                                        .width(Size::fill())
                                                        .spacing(4.)
                                                        .children(suggestions.iter().map({
                                                            let model_input = model_input.clone();
                                                            let c = c.clone();
                                                            let cur = current_selected.clone();
                                                            move |&m| {
                                                                let is_active = m == cur;
                                                                let mut model_input_press = model_input.clone();
                                                                let m_str = m.to_string();
                                                                rect()
                                                                    .width(Size::fill())
                                                                    .padding(6.)
                                                                    .background(if is_active {
                                                                        c.surface_tertiary
                                                                    } else {
                                                                        Color::TRANSPARENT
                                                                    })
                                                                    .corner_radius(4.)
                                                                    .horizontal()
                                                                    .cross_align(Alignment::Center)
                                                                    .on_press(move |_| {
                                                                        *model_input_press.write() = m_str.clone();
                                                                    })
                                                                    .child(
                                                                        label()
                                                                            .text(format!(
                                                                                "{} 🏛️ {}",
                                                                                if is_active { "✓" } else { " " },
                                                                                m
                                                                            ))
                                                                            .color(if is_active {
                                                                                c.text_primary
                                                                            } else {
                                                                                c.text_secondary
                                                                            })
                                                                            .font_size(12.),
                                                                    )
                                                            }
                                                        })),
                                                )
                                                .into_element()
                                            } else {
                                                ScrollView::new().child(
                                                    rect()
                                                        .width(Size::fill())
                                                        .spacing(4.)
                                                        .children(models.into_iter().map({
                                                            let model_input = model_input.clone();
                                                            let c = c.clone();
                                                            let cur = current_selected.clone();
                                                            move |m| {
                                                                let m_clone = m.clone();
                                                                let is_active = m == cur;
                                                                let bg = if is_active {
                                                                    c.surface_tertiary
                                                                } else {
                                                                    Color::TRANSPARENT
                                                                };
                                                                let mut model_input_press = model_input.clone();
                                                                let m_selected = m.clone();
                                                                rect()
                                                                    .width(Size::fill())
                                                                    .padding(6.)
                                                                    .background(bg)
                                                                    .corner_radius(4.)
                                                                    .horizontal()
                                                                    .cross_align(Alignment::Center)
                                                                    .on_press(move |_| {
                                                                        *model_input_press.write() = m_selected.clone();
                                                                    })
                                                                    .child(
                                                                        label()
                                                                            .text(format!(
                                                                                "{} 🤖 {}",
                                                                                if is_active { "✓" } else { " " },
                                                                                m_clone
                                                                            ))
                                                                            .color(if is_active {
                                                                                c.text_primary
                                                                            } else {
                                                                                c.text_secondary
                                                                            })
                                                                            .font_size(12.),
                                                                    )
                                                            }
                                                        })),
                                                )
                                                .into_element()
                                            }),
                                    ),
                            )
                            .child(
                                // Feedback / Status & Config file location
                                rect()
                                    .width(Size::fill())
                                    .spacing(6.)
                                    .child(
                                        label()
                                            .text(format!(
                                                "Fichier de configuration locale : {}",
                                                config::ApiKeyConfig::config_file_path().display()
                                            ))
                                            .color(c.text_placeholder)
                                            .font_size(11.),
                                    )
                                    .child(if !feedback.read().is_empty() {
                                        label()
                                            .text(feedback.read().clone())
                                            .color(c.warning)
                                            .font_size(12.)
                                            .into_element()
                                    } else {
                                        rect().width(Size::px(0.)).height(Size::px(0.)).into_element()
                                    }),
                            )
                            .child(
                                // Bottom action buttons
                                rect()
                                    .width(Size::fill())
                                    .horizontal()
                                    .cross_align(Alignment::Center)
                                    .spacing(12.)
                                    .child(
                                        Button::new()
                                            .background(Color::from_rgb(22, 163, 74))
                                            .hover_background(Color::from_rgb(21, 128, 61))
                                            .border_fill(Color::TRANSPARENT)
                                            .color(Color::WHITE)
                                            .on_press(on_save)
                                            .child("💾 Enregistrer"),
                                    )
                                    .child(
                                        Button::new()
                                            .background(c.surface_tertiary)
                                            .hover_background(c.tertiary)
                                            .border_fill(Color::TRANSPARENT)
                                            .color(c.text_primary)
                                            .on_press(on_close)
                                            .child("← Retour"),
                                    ),
                            ),
                    ),
                ),
        )
}
