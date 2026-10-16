//! Chat panel, message bubbles, interactive spec questionnaire, and prompt input bar.

use freya::prelude::*;
use std::collections::HashMap;
use crate::api;
use crate::config;
use crate::types::{extract_spec_payload, Message, Role};

/// Builds the full chat sidebar panel (scrollable messages, interactive cards, input area).
pub fn chat_panel(
    messages: Readable<Vec<Message>>,
    input_value: Writable<String>,
    is_generating: Readable<bool>,
    is_spec_mode: Writable<bool>,
    spec_selected_options: Writable<HashMap<String, String>>,
    spec_custom_input: Writable<String>,
    settings_model_input: Writable<String>,
    settings_endpoint_input: Readable<String>,
    settings_key_input: Readable<String>,
    available_models: Writable<Vec<String>>,
    models_loading: Writable<bool>,
    project_name: String,
    current_file_display: String,
    on_send: impl FnMut(String) + Clone + 'static,
    c: ColorsSheet,
) -> impl IntoElement {
    let is_generating_val = *is_generating.read();
    let is_spec_val = *is_spec_mode.read();

    let mut on_send_input = on_send.clone();
    let on_submit_handler = move |text: String| {
        on_send_input(text);
    };

    let mut on_send_click = on_send.clone();
    let input_value_read = input_value.clone();
    let send_message_handler = move |_| {
        let text = input_value_read.read().clone();
        on_send_click(text);
    };

    // Chat scrollable area
    let chat_area = rect().width(Size::fill()).height(Size::flex(1.)).child(
        ScrollView::new().child(rect().width(Size::fill()).padding(16.).children(
            messages.read().iter().map(|msg| {
                let is_user = msg.role == Role::User;
                let bg_color = if is_user {
                    c.surface_inverse
                } else {
                    c.surface_inverse_secondary
                };
                let align = if is_user {
                    Alignment::End
                } else {
                    Alignment::Start
                };
                let text_align = if is_user {
                    TextAlign::End
                } else {
                    TextAlign::Start
                };
                let text_color = if is_user {
                    c.text_inverse
                } else {
                    c.text_highlight
                };

                let (clean_text, recommend_spec, spec_payload) = if is_user {
                    (msg.content.clone(), false, None)
                } else {
                    extract_spec_payload(&msg.content)
                };

                let mut bubble = rect()
                    .padding(12.)
                    .background(bg_color)
                    .corner_radius(16.)
                    .color(text_color)
                    .text_align(text_align)
                    .spacing(10.)
                    .child(if is_user {
                        SelectableText::new()
                            .span(msg.content.clone())
                            .color(text_color)
                            .into_element()
                    } else if clean_text.is_empty() && spec_payload.is_none() {
                        label()
                            .text("⏳ En attente de l'assistant...")
                            .color(c.text_placeholder)
                            .into_element()
                    } else if !clean_text.is_empty() {
                        MarkdownViewer::new(clean_text)
                            .color(text_color)
                            .into_element()
                    } else {
                        label()
                            .text("🛡️ Cadrage des spécifications :")
                            .color(text_color)
                            .into_element()
                    });

                // Bouton de redirection rapide vers le Mode Spec lorsque Quick Edit détecte une grosse fonctionnalité
                if recommend_spec {
                    let mut is_spec_mode_btn = is_spec_mode.clone();
                    let mut on_send_spec = on_send.clone();
                    bubble = bubble.child(
                        rect()
                            .width(Size::fill())
                            .padding(12.)
                            .background(c.surface_primary)
                            .corner_radius(10.)
                            .border(Border::new().fill(Color::from_rgb(16, 185, 129)).width(1.))
                            .spacing(8.)
                            .child(
                                label()
                                    .text("🛡️ Fonctionnalité complexe détectée — Spécification Security by Design recommandée")
                                    .color(c.text_primary)
                                    .font_size(12.),
                            )
                            .child(
                                Button::new()
                                    .background(Color::from_rgb(16, 185, 129))
                                    .hover_background(Color::from_rgb(5, 150, 105))
                                    .border_fill(Color::TRANSPARENT)
                                    .color(Color::WHITE)
                                    .on_press(move |_| {
                                        *is_spec_mode_btn.write() = true;
                                        on_send_spec(
                                            "Passe en Mode Spec et pose-moi les questions rapides de spécification (Security by Design) pour cadrer cette fonctionnalité avant de générer plan.md et le code.".to_string(),
                                        );
                                    })
                                    .child("🛡️ Passer en Mode Spec et spécifier maintenant"),
                            ),
                    );
                }

                // Carte interactive de questions en Mode Spec (choix rapides + champ libre "Autre")
                if let Some(payload) = spec_payload {
                    let questions_for_submit = payload.questions.clone();
                    let current_selections = spec_selected_options.read().clone();

                    let mut card = rect()
                        .width(Size::fill())
                        .padding(12.)
                        .background(c.surface_primary)
                        .corner_radius(12.)
                        .border(Border::new().fill(c.border).width(1.))
                        .spacing(10.)
                        .child(
                            label()
                                .text("🛡️ Spécification Rapide — Security by Design")
                                .color(c.text_primary)
                                .font_size(13.),
                        );

                    if let Some(summary) = &payload.context_summary {
                        if !summary.trim().is_empty() {
                            card = card.child(
                                label()
                                    .text(summary.clone())
                                    .color(c.text_secondary)
                                    .font_size(12.),
                            );
                        }
                    }

                    for (q_idx, q) in payload.questions.iter().enumerate() {
                        let q_key = if q.id.is_empty() {
                            format!("q_{}", q_idx + 1)
                        } else {
                            q.id.clone()
                        };
                        let selected_opt = current_selections.get(&q_key).cloned();

                        let mut q_block = rect()
                            .width(Size::fill())
                            .padding(10.)
                            .background(c.surface_secondary)
                            .corner_radius(8.)
                            .spacing(6.)
                            .child(
                                label()
                                    .text(format!("{}. {}", q_idx + 1, q.question))
                                    .color(c.text_primary)
                                    .font_size(12.),
                            );

                        if let Some(sec_note) = &q.security_note {
                            if !sec_note.trim().is_empty() {
                                q_block = q_block.child(
                                    label()
                                        .text(format!("🔒 {}", sec_note))
                                        .color(c.text_placeholder)
                                        .font_size(11.),
                                );
                            }
                        }

                        let mut opts_col = rect().width(Size::fill()).spacing(4.);
                        for opt in &q.options {
                            let is_chosen = selected_opt.as_deref() == Some(opt.as_str());
                            let opt_str = opt.clone();
                            let q_key_click = q_key.clone();
                            let mut spec_selected_options_btn = spec_selected_options.clone();
                            opts_col = opts_col.child(
                                Button::new()
                                    .background(if is_chosen {
                                        Color::from_rgb(16, 185, 129)
                                    } else {
                                        c.surface_tertiary
                                    })
                                    .hover_background(if is_chosen {
                                        Color::from_rgb(5, 150, 105)
                                    } else {
                                        c.tertiary
                                    })
                                    .border_fill(if is_chosen {
                                        Color::from_rgb(16, 185, 129)
                                    } else {
                                        c.border
                                    })
                                    .color(if is_chosen { Color::WHITE } else { c.text_primary })
                                    .on_press(move |_| {
                                        spec_selected_options_btn.write().insert(q_key_click.clone(), opt_str.clone());
                                    })
                                    .child(format!(
                                        "{} {}",
                                        if is_chosen { "☑" } else { "☐" },
                                        opt
                                    )),
                            );
                        }
                        q_block = q_block.child(opts_col);
                        card = card.child(q_block);
                    }

                    // Champ libre "Autre" pour permettre à l'utilisateur d'exprimer exactement ce qu'il veut
                    card = card.child(
                        rect()
                            .width(Size::fill())
                            .spacing(4.)
                            .child(
                                label()
                                    .text("✍️ Autre / Précisions libres (optionnel) :")
                                    .color(c.text_secondary)
                                    .font_size(11.),
                            )
                            .child(
                                Input::new(spec_custom_input.clone())
                                    .background(c.surface_tertiary)
                                    .focus_background(c.tertiary)
                                    .border_fill(c.border)
                                    .color(c.text_primary)
                                    .placeholder("Autre choix, contrainte spécifique ou instruction libre...")
                                    .width(Size::fill()),
                            ),
                    );

                    // Boutons d'action : Valider ou Fais au mieux
                    let mut on_send_submit = on_send.clone();
                    let mut on_send_auto = on_send.clone();
                    let mut spec_selected_options_submit = spec_selected_options.clone();
                    let mut spec_custom_input_submit = spec_custom_input.clone();

                    card = card.child(
                        rect()
                            .width(Size::fill())
                            .horizontal()
                            .spacing(8.)
                            .child(
                                Button::new()
                                    .background(Color::from_rgb(16, 185, 129))
                                    .hover_background(Color::from_rgb(5, 150, 105))
                                    .border_fill(Color::TRANSPARENT)
                                    .color(Color::WHITE)
                                    .on_press(move |_| {
                                        let selections = spec_selected_options_submit.read().clone();
                                        let custom = spec_custom_input_submit.read().trim().to_string();
                                        let mut lines = vec![
                                            "[RÉPONSES AUX SPÉCIFICATIONS] Voici mes choix de spécification (génère maintenant `plan.md` via `write_plan` puis implémente tous les fichiers) :".to_string(),
                                        ];
                                        for (idx, q) in questions_for_submit.iter().enumerate() {
                                            let key = if q.id.is_empty() {
                                                format!("q_{}", idx + 1)
                                            } else {
                                                q.id.clone()
                                            };
                                            let chosen = selections
                                                .get(&key)
                                                .cloned()
                                                .or_else(|| q.options.first().cloned())
                                                .unwrap_or_else(|| "Option recommandée".to_string());
                                            lines.push(format!("- **{}** : {}", q.question, chosen));
                                        }
                                        if !custom.is_empty() {
                                            lines.push(format!("- **Autre / Précisions utilisateur** : {}", custom));
                                        }
                                        spec_selected_options_submit.write().clear();
                                        *spec_custom_input_submit.write() = String::new();
                                        on_send_submit(lines.join("\n"));
                                    })
                                    .child("🚀 Valider et lancer l'implémentation"),
                            )
                            .child(
                                Button::new()
                                    .background(c.surface_tertiary)
                                    .hover_background(c.tertiary)
                                    .border_fill(c.border)
                                    .color(c.text_secondary)
                                    .on_press(move |_| {
                                        on_send_auto(
                                            "[RÉPONSES AUX SPÉCIFICATIONS] Fais au mieux avec les options Security by Design recommandées : génère `plan.md` avec `write_plan` et implémente immédiatement tous les fichiers.".to_string(),
                                        );
                                    })
                                    .child("⚡ Fais au mieux (défaut sécurisé)"),
                            ),
                    );

                    bubble = bubble.child(card);
                }

                rect()
                    .width(Size::fill())
                    .margin(8.)
                    .cross_align(align)
                    .child(bubble)
            }),
        )),
    );

    let active_model = settings_model_input.read().clone();
    let is_models_loading = *models_loading.read();
    let loaded_models = available_models.read().clone();

    let mut model_options: Vec<String> = if !loaded_models.is_empty() {
        loaded_models
    } else {
        vec![
            "deepseek-v4-flash".to_string(),
            "deepseek-v3".to_string(),
            "openweight-medium".to_string(),
            "albert-light".to_string(),
            "qwen-2.5-coder".to_string(),
        ]
    };
    if !active_model.is_empty() && !model_options.contains(&active_model) {
        model_options.insert(0, active_model.clone());
    }

    let model_items = model_options.into_iter().map({
        let active_model = active_model.clone();
        let settings_model_input = settings_model_input;
        move |m| {
            let is_selected = m == active_model;
            let m_choice = m.clone();
            let mut settings_model_input = settings_model_input.clone();
            MenuItem::new()
                .selected(is_selected)
                .on_press(move |_| {
                    *settings_model_input.write() = m_choice.clone();
                    let mut cfg = config::ApiKeyConfig::load();
                    cfg.model = Some(m_choice.clone());
                    let _ = cfg.save();
                })
                .child(m)
        }
    });

    let model_select = Select::new()
        .selected_item(format!("🤖 {}", if active_model.is_empty() { "Choisir modèle" } else { &active_model }))
        .children(model_items);

    let refresh_models_button = Button::new()
        .background(c.surface_tertiary)
        .hover_background(c.tertiary)
        .border_fill(Color::TRANSPARENT)
        .color(c.text_secondary)
        .on_press({
            let ep_val = settings_endpoint_input.read().clone();
            let key_val = settings_key_input.read().clone();
            let available_models = available_models;
            let models_loading = models_loading;
            move |_| {
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
        })
        .child(if is_models_loading { "⟳ ..." } else { "⟳" });

    let stop_button = Button::new()
        .background(Color::from_rgb(220, 38, 38))
        .hover_background(Color::from_rgb(185, 28, 28))
        .border_fill(Color::TRANSPARENT)
        .color(Color::WHITE)
        .on_press(|_| {
            api::request_cancel();
        })
        .child("⏹ Arrêter");

    let mode_toggle_button = {
        let mut is_spec_mode = is_spec_mode;
        Button::new()
            .background(if is_spec_val { Color::from_rgb(16, 185, 129) } else { c.surface_tertiary })
            .hover_background(if is_spec_val { Color::from_rgb(5, 150, 105) } else { c.tertiary })
            .border_fill(Color::TRANSPARENT)
            .color(if is_spec_val { Color::WHITE } else { c.text_primary })
            .on_press(move |_| {
                *is_spec_mode.write() = !is_spec_val;
            })
            .child(if is_spec_val { "🛡️ Mode Spec" } else { "⚡ Quick Edit" })
    };

    // Input area with text input and model selector bar
    let input_area = rect()
        .width(Size::fill())
        .height(Size::px(96.))
        .background(c.surface_primary)
        .border(Border::new().fill(c.border).width(BorderWidth {
            top: 1.,
            right: 0.,
            bottom: 0.,
            left: 0.,
        }))
        .padding(Gaps::new(8., 12., 8., 12.))
        .spacing(8.)
        .content(Content::Flex)
        .child(
            // Row 1: Chat input and send/stop button
            rect()
                .width(Size::fill())
                .height(Size::px(38.))
                .horizontal()
                .cross_align(Alignment::Center)
                .spacing(8.)
                .content(Content::Flex)
                .child(
                    Input::new(input_value)
                        .background(c.surface_tertiary)
                        .focus_background(c.tertiary)
                        .border_fill(Color::TRANSPARENT)
                        .color(c.text_secondary)
                        .placeholder(if is_spec_val {
                            "🛡️ Mode Spec : décrivez la fonctionnalité à spécifier (Security by Design)..."
                        } else {
                            "⚡ Quick Edit : modification rapide (ou redirection Spec si gros chantier)..."
                        })
                        .width(Size::flex(1.))
                        .on_submit(on_submit_handler),
                )
                .child(if is_generating_val {
                    stop_button.into_element()
                } else {
                    Button::new()
                        .background(c.surface_tertiary)
                        .hover_background(c.tertiary)
                        .border_fill(Color::TRANSPARENT)
                        .color(c.text_secondary)
                        .on_press(send_message_handler)
                        .child("Envoyer")
                        .into_element()
                }),
        )
        .child(
            // Row 2: Model select dropdown, refresh button, mode toggle and context label
            rect()
                .width(Size::fill())
                .height(Size::px(32.))
                .horizontal()
                .cross_align(Alignment::Center)
                .spacing(8.)
                .content(Content::Flex)
                .child(model_select)
                .child(refresh_models_button)
                .child(mode_toggle_button)
                .child(
                    rect()
                        .width(Size::flex(1.))
                        .horizontal()
                        .cross_align(Alignment::Center)
                        .main_align(Alignment::End)
                        .child(
                            label()
                                .text(format!("📁 {}  ·  📄 {}", project_name, current_file_display))
                                .color(c.text_placeholder)
                                .font_size(11.),
                        ),
                ),
        );

    rect()
        .expanded()
        .content(Content::Flex)
        .background(c.background)
        .child(chat_area)
        .child(input_area)
}

