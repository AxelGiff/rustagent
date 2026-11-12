//! Chat panel, message bubbles, interactive spec questionnaire, and prompt input bar.

use crate::api;
use crate::config;
use crate::file_tree::{self, WorkspaceMentionIndex};
use crate::types::{Message, Role, extract_spec_payload};
use crate::views::diff_viewer::{DiffReviewState, changed_files_review_box};
use crate::views::ui::{self, Tone};
use freya::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

/// Builds the full chat sidebar panel (scrollable messages, interactive cards, input area).
#[allow(clippy::too_many_arguments)] // a UI panel receives one handle per piece of state
pub fn chat_panel(
    messages: Readable<Vec<Message>>,
    input_value: Writable<String>,
    is_generating: Writable<bool>,
    is_spec_mode: Writable<bool>,
    spec_selected_options: Writable<HashMap<String, String>>,
    spec_custom_input: Writable<String>,
    current_dir: Readable<PathBuf>,
    context_paths: Writable<Vec<PathBuf>>,
    settings_model_input: Writable<String>,
    _settings_endpoint_input: Readable<String>,
    _settings_key_input: Readable<String>,
    available_models: Writable<Vec<String>>,
    models_loading: Writable<bool>,
    project_name: String,
    current_file_display: String,
    diff_review: DiffReviewState,
    on_send: impl FnMut(String) + Clone + 'static,
    c: ColorsSheet,
) -> impl IntoElement {
    let mention_index = use_hook(|| Rc::new(RefCell::new(WorkspaceMentionIndex::default())));
    let workspace_root = current_dir.read().clone();
    mention_index.borrow_mut().sync_root(&workspace_root);

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

    let on_send_starter = on_send.clone();
    let has_no_messages = messages.read().is_empty();

    // Chat scrollable area
    let mut user_scrolled_wheel = diff_review.user_scrolled_up;
    let chat_area = rect()
        .width(Size::fill())
        .height(Size::flex(1.))
        .on_wheel(move |e: Event<WheelEventData>| {
            if e.delta_y > 0.0 {
                *user_scrolled_wheel.write() = true;
            }
        })
        .child(
            if has_no_messages {
                rect()
                    .expanded()
                    .center()
                    .padding(16.)
                    .child(
                        ui::col(14.)
                            .width(Size::fill())
                            .cross_align(Alignment::Center)
                            .child(
                                ui::col(4.)
                                    .cross_align(Alignment::Center)
                                    .child(ui::mono("RustAgent", 18., c.text_primary))
                                    .child(ui::subtitle("Architecture, sécurité et refactoring assistés", &c)),
                            )
                            .child(
                                ui::col(6.)
                                    .width(Size::fill())
                                    .child({
                                        let mut s = on_send_starter.clone();
                                        starter_row(&c, "shield", "1. Audit OWASP (Security by Design)", "Injection, secrets, gestion des erreurs", move || {
                                            s("Audite la sécurité du projet selon les principes Security by Design (OWASP, validation des entrées, contrôle des erreurs, gestion des secrets). Rédige ton rapport d'audit directement dans la conversation (sans créer de fichier).".to_string())
                                        })
                                    })
                                    .child({
                                        let mut s = on_send_starter.clone();
                                        starter_row(&c, "cpu", "2. Mémoire & ownership", "Fuites, concurrence, lifecycles", move || {
                                            s("Vérifie la gestion mémoire, les fuites potentielles, la concurrence et les règles d'ownership dans le projet.".to_string())
                                        })
                                    })
                                    .child({
                                        let mut s = on_send_starter.clone();
                                        starter_row(&c, "file", "3. Cadrer une feature (plan.md)", "Spécification guidée par questions", move || {
                                            s("Passe en Mode Spec et pose-moi les questions rapides pour formaliser un cadrage d'architecture plan.md pour ce projet.".to_string())
                                        })
                                    })
                                    .child({
                                        let mut s = on_send_starter.clone();
                                        starter_row(&c, "blocks", "4. Refactoring modulaire", "Découpage, typage et séparation", move || {
                                            s("Analyse la structure des fichiers du projet et propose un refactoring propre, modulaire et typé sans casser l'existant.".to_string())
                                        })
                                    }),
                            ),
                    )
                    .into_element()
                } else {
                    ScrollView::new_controlled(diff_review.chat_scroll).child({
                    rect()
                        .width(Size::fill())
                        .padding(16.)
                        .children(
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
                    .corner_radius(14.)
                    .border(Border::new().fill(c.border).width(1.))
                    .shadow(
                        Shadow::new()
                            .x(0.)
                            .y(2.)
                            .blur(6.)
                            .color(Color::from_argb(35, 0, 0, 0)),
                    )
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
                            .text("En attente de réponse...")
                            .color(c.text_placeholder)
                            .into_element()
                    } else if !clean_text.is_empty() {
                        MarkdownViewer::new(clean_text)
                            .color(text_color)
                            .into_element()
                    } else {
                        label()
                            .text("Spécifications requises :")
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
                            .corner_radius(8.)
                            .border(Border::new().fill(c.border).width(1.))
                            .spacing(8.)
                            .child(
                                label()
                                    .text("Spécification préalable recommandée pour cette fonctionnalité")
                                    .color(c.text_primary)
                                    .font_size(12.),
                            )
                            .child(
                                Button::new()
                                    .cursor_icon(CursorIcon::Pointer)
                                    .background(c.surface_tertiary)
                                    .hover_background(c.tertiary)
                                    .border_fill(c.border)
                                    .color(c.text_primary)
                                    .on_press(move |_| {
                                        *is_spec_mode_btn.write() = true;
                                        on_send_spec(
                                            "Passe en Mode Spec et pose-moi les questions rapides de spécification pour cadrer cette fonctionnalité avant de générer plan.md et le code.".to_string(),
                                        );
                                    })
                                    .child("Passer en Mode Spec"),
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
                                .text("◈ Spécification Rapide — Security by Design")
                                .color(c.text_primary)
                                .font_size(13.),
                        );

                    if let Some(summary) = &payload.context_summary
                        && !summary.trim().is_empty() {
                            card = card.child(
                                label()
                                    .text(summary.clone())
                                    .color(c.text_secondary)
                                    .font_size(12.),
                            );
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

                        if let Some(sec_note) = &q.security_note
                            && !sec_note.trim().is_empty() {
                                q_block = q_block.child(
                                    label()
                                        .text(format!("🔒 {}", sec_note))
                                        .color(c.text_placeholder)
                                        .font_size(11.),
                                );
                            }

                        let mut opts_col = rect().width(Size::fill()).spacing(4.);
                        for opt in &q.options {
                            let is_chosen = selected_opt.as_deref() == Some(opt.as_str());
                            let opt_str = opt.clone();
                            let q_key_click = q_key.clone();
                            let mut spec_selected_options_btn = spec_selected_options.clone();
                            opts_col = opts_col.child(
                                Button::new()
                                    .cursor_icon(CursorIcon::Pointer)
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
                                    .cursor_icon(CursorIcon::Pointer)
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
                                    .cursor_icon(CursorIcon::Pointer)
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
            })
            ).child(
                rect()
                    .width(Size::fill())
                    .margin(8.)
                    .child(changed_files_review_box(diff_review, c.clone())),
            ).into_element()
        })
            .into_element()
        }
    );

    let active_model = settings_model_input.read().clone();
    let is_models_loading = *models_loading.read();
    let loaded_models = available_models.read().clone();

    let mut model_options: Vec<String> = if !loaded_models.is_empty() {
        loaded_models
    } else {
        config::ALBERT_MODELS
            .iter()
            .map(|&s| s.to_string())
            .collect()
    };
    if !active_model.is_empty() && !model_options.contains(&active_model) {
        model_options.insert(0, active_model.clone());
    }

    let c_menu = c.clone();
    let model_items = model_options.into_iter().map({
        let active_model = active_model.clone();
        let settings_model_input = settings_model_input;
        move |m| {
            let is_selected = m == active_model;
            let m_choice = m.clone();
            let mut settings_model_input = settings_model_input.clone();
            ui::menu_item(format!("🤖 {}", m), is_selected, &c_menu, move |_| {
                *settings_model_input.write() = m_choice.clone();
                let mut cfg = config::ApiKeyConfig::load();
                cfg.model = Some(m_choice.clone());
                let _ = cfg.save();
            })
        }
    });

    let model_select = ui::select(
        if active_model.is_empty() {
            "modèle: auto".to_string()
        } else {
            active_model.clone()
        },
        model_items,
        &c,
    );

    let refresh_models_button = ui::btn_sm(
        if is_models_loading { "⟳ ..." } else { "⟳" },
        Tone::Neutral,
        &c,
        {
            let available_models = available_models;
            let models_loading = models_loading;
            move |_| {
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
        },
    );

    let mut is_generating_stop = is_generating;
    let stop_button = ui::btn_sm("Arrêter", Tone::Danger, &c, move |_| {
        api::request_cancel();
        *is_generating_stop.write() = false;
    });

    let mode_segmented_control = {
        let is_spec_val = is_spec_val;
        rect()
            .horizontal()
            .cross_align(Alignment::Center)
            .background(c.surface_secondary)
            .corner_radius(4.)
            .border(Border::new().fill(c.border).width(1.))
            .padding(Gaps::new(2., 2., 2., 2.))
            .spacing(2.)
            .child(
                rect()
                    .padding(Gaps::new(3., 6., 3., 6.))
                    .background(if !is_spec_val { c.surface_tertiary } else { Color::TRANSPARENT })
                    .corner_radius(3.)
                    .on_pointer_enter(|_| Cursor::set(CursorIcon::Pointer))
                    .on_pointer_leave(|_| Cursor::set(CursorIcon::default()))
                    .on_press({
                        let mut is_spec_mode = is_spec_mode.clone();
                        move |_| { *is_spec_mode.write() = false; }
                    })
                    .child(
                        label()
                            .text("Edit")
                            .color(if !is_spec_val { c.text_primary } else { c.text_secondary })
                            .font_size(11.5)
                            .font_weight(if !is_spec_val { FontWeight::BOLD } else { FontWeight::NORMAL }),
                    ),
            )
            .child(
                rect()
                    .padding(Gaps::new(3., 6., 3., 6.))
                    .background(if is_spec_val { c.primary } else { Color::TRANSPARENT })
                    .corner_radius(3.)
                    .on_pointer_enter(|_| Cursor::set(CursorIcon::Pointer))
                    .on_pointer_leave(|_| Cursor::set(CursorIcon::default()))
                    .on_press({
                        let mut is_spec_mode = is_spec_mode.clone();
                        move |_| { *is_spec_mode.write() = true; }
                    })
                    .child(
                        label()
                            .text("Spec")
                            .color(if is_spec_val { Color::WHITE } else { c.text_secondary })
                            .font_size(11.5)
                            .font_weight(if is_spec_val { FontWeight::BOLD } else { FontWeight::NORMAL }),
                    ),
            )
    };

    let pinned_list = context_paths.read().clone();
    let has_pinned = !pinned_list.is_empty();
    let pinned_chips_row = if has_pinned {
        let chips = pinned_list.iter().map({
            let context_paths = context_paths.clone();
            let c = c.clone();
            move |p| {
                let path_clone = p.clone();
                let mut context_paths = context_paths.clone();
                let is_dir = p.is_dir();
                let icon = if is_dir { "📁" } else { "📄" };
                let short_name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| p.to_string_lossy().to_string());
                rect()
                    .padding(Gaps::new(3., 8., 3., 8.))
                    .background(Color::from_argb(38, 16, 185, 129))
                    .border(Border::new().fill(Color::from_rgb(16, 185, 129)).width(1.))
                    .corner_radius(6.)
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(5.)
                    .on_pointer_enter(|_| Cursor::set(CursorIcon::Pointer))
                    .on_pointer_leave(|_| Cursor::set(CursorIcon::default()))
                    .on_press(move |_| {
                        context_paths.write().retain(|item| item != &path_clone);
                    })
                    .child(
                        label()
                            .text(format!("{} {}", icon, short_name))
                            .color(Color::from_rgb(74, 222, 128))
                            .font_size(11.)
                            .font_weight(FontWeight::BOLD),
                    )
                    .child(
                        label()
                            .text("×")
                            .color(c.text_secondary)
                            .font_size(11.5)
                            .font_weight(FontWeight::BOLD),
                    )
                    .into_element()
            }
        });

        let clear_all_pinned = {
            let mut context_paths = context_paths.clone();
            move |_| {
                context_paths.write().clear();
            }
        };

        rect()
            .key("pinned_chips_slot")
            .width(Size::fill())
            .height(Size::px(26.))
            .child(
                ui::row_fill(6.)
                    .height(Size::px(26.))
                    .child(
                        label()
                            .text("🎯 Cibles :")
                            .color(c.text_secondary)
                            .font_size(11.)
                            .font_weight(FontWeight::BOLD),
                    )
                    .child(
                        ScrollView::new()
                            .direction(Direction::Horizontal)
                            .width(Size::flex(1.))
                            .height(Size::px(26.))
                            .child(ui::row(6.).children(chips)),
                    )
                    .maybe_child((pinned_list.len() > 1).then(|| {
                        rect()
                            .padding(Gaps::new(2., 6., 2., 6.))
                            .background(c.surface_tertiary)
                            .corner_radius(4.)
                            .on_pointer_enter(|_| Cursor::set(CursorIcon::Pointer))
                            .on_pointer_leave(|_| Cursor::set(CursorIcon::default()))
                            .on_press(clear_all_pinned)
                            .child(
                                label()
                                    .text("Tout retirer")
                                    .color(c.text_secondary)
                                    .font_size(10.5),
                            )
                    })),
            )
    } else {
        rect()
            .key("pinned_chips_slot")
            .width(Size::px(0.))
            .height(Size::px(0.))
    };

    // Menu d'autocomplétion `@` lorsque l'utilisateur tape `@` ou `@recherche` dans l'input
    // Toujours monté dans un slot fixe `.key("at_mention_slot")` pour que l'Input ne perde jamais le focus !
    let current_input_text = input_value.read().clone();
    let at_mention_popup = if let Some((_, query)) =
        file_tree::extract_active_at_query(&current_input_text)
    {
        let suggestions = mention_index.borrow().filter_suggestions(query, 7);
        let refresh_index = {
            let mention_index = mention_index.clone();
            let mut input_value = input_value.clone();
            move |_| {
                mention_index.borrow_mut().refresh();
                let _ = input_value.write();
            }
        };

        let items = suggestions.into_iter().map({
            let input_value = input_value.clone();
            let context_paths = context_paths.clone();
            let c = c.clone();
            move |entry| {
                let mut input_value = input_value.clone();
                let mut context_paths = context_paths.clone();
                let rel_path = entry.rel_path.clone();
                let full_path = entry.full_path.clone();
                let icon = if entry.is_dir { "📁" } else { "📄" };
                let row_key = format!("at_sug_{}", entry.rel_path);

                rect()
                    .key(&row_key)
                    .width(Size::fill())
                    .padding(Gaps::new(5., 10., 5., 10.))
                    .background(c.surface_secondary)
                    .corner_radius(6.)
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .content(Content::Flex)
                    .spacing(8.)
                    .on_pointer_enter(|_| Cursor::set(CursorIcon::Pointer))
                    .on_pointer_leave(|_| Cursor::set(CursorIcon::default()))
                    .on_press(move |_| {
                        let cur = input_value.read().clone();
                        let updated = file_tree::replace_active_at_query(&cur, &rel_path);
                        *input_value.write() = updated;
                        let mut pinned = context_paths.write();
                        if !pinned.contains(&full_path) {
                            pinned.push(full_path.clone());
                        }
                    })
                    .child(
                        label()
                            .text(format!("{} {}", icon, entry.name))
                            .color(if entry.is_dir {
                                Color::from_rgb(74, 222, 128)
                            } else if entry.is_recent {
                                Color::from_rgb(147, 197, 253)
                            } else {
                                c.text_primary
                            })
                            .font_size(12.)
                            .font_weight(FontWeight::BOLD),
                    )
                    .maybe_child(entry.is_recent.then(|| {
                        ui::tag("💙 VS Code", Tone::Primary, &c)
                    }))
                    .child(ui::spacer())
                    .child(ui::muted(entry.rel_path, &c))
                    .into_element()
            }
        });

        rect()
            .key("at_mention_slot")
            .width(Size::fill())
            .padding(Gaps::new(8., 12., 8., 12.))
            .background(c.surface_primary)
            .border(Border::new().fill(c.primary).width(BorderWidth {
                top: 1.5,
                right: 0.,
                bottom: 0.,
                left: 0.,
            }))
            .spacing(6.)
            .content(Content::Flex)
            .child(
                ui::row_fill(6.)
                    .child(
                        label()
                            .text(if query.is_empty() {
                                "🎯 Cibler un fichier ou dossier (@) — cliquez pour ajouter au contexte :".to_string()
                            } else {
                                format!("🎯 Recherche `@{}` — cliquez pour cibler :", query)
                            })
                            .color(c.primary)
                            .font_size(11.5)
                            .font_weight(FontWeight::BOLD),
                    )
                    .child(ui::spacer())
                    .child(
                        rect()
                            .padding(Gaps::new(2., 6., 2., 6.))
                            .background(c.surface_tertiary)
                            .corner_radius(4.)
                            .on_pointer_enter(|_| Cursor::set(CursorIcon::Pointer))
                            .on_pointer_leave(|_| Cursor::set(CursorIcon::default()))
                            .on_press(refresh_index)
                            .child(
                                label()
                                    .text("↻ Actualiser")
                                    .color(c.text_secondary)
                                    .font_size(10.5),
                            ),
                    ),
            )
            .child(ui::col(4.).children(items))
    } else {
        rect()
            .key("at_mention_slot")
            .width(Size::px(0.))
            .height(Size::px(0.))
    };

    // Input area with text input and model selector bar
    let _ = (&project_name, &current_file_display);
    let mut ctx_input = input_value.clone();
    let input_area = rect()
        .key("chat_input_area")
        .width(Size::fill())
        .height(Size::px(if has_pinned { 138. } else { 104. }))
        .background(c.surface_primary)
        .border(Border::new().fill(c.border).width(BorderWidth {
            top: 1.,
            right: 0.,
            bottom: 0.,
            left: 0.,
        }))
        .padding(Gaps::new(8., 12., 8., 12.))
        .spacing(6.)
        .content(Content::Flex)
        .child(pinned_chips_row)
        .child(
            // Row 1: Chat input and send/stop button
            ui::row_fill(8.)
                .key("chat_input_row_1")
                .height(Size::px(40.))
                .child(
                    Input::new(input_value)
                        .key("main_chat_prompt_input")
                        .background(c.surface_tertiary)
                        .focus_background(c.tertiary)
                        .border_fill(c.border)
                        .color(c.text_secondary)
                        .placeholder(if is_spec_val {
                            "Décrivez une fonctionnalité à spécifier (tapez @ pour cibler)..."
                        } else {
                            "Posez une question ou décrivez une modification (tapez @ pour cibler)..."
                        })
                        .width(Size::flex(1.))
                        .on_submit(on_submit_handler),
                )
                .child(if is_generating_val {
                    let mut stop_row = ui::row(6.).child(stop_button);
                    if *diff_review.user_scrolled_up.read() {
                        let mut user_scrolled_btn = diff_review.user_scrolled_up;
                        let mut chat_scroll_btn = diff_review.chat_scroll;
                        stop_row = stop_row.child(
                            ui::btn_sm("↓ Suivre", Tone::Primary, &c, move |_| {
                                *user_scrolled_btn.write() = false;
                                chat_scroll_btn.scroll_to(ScrollPosition::End, Direction::Vertical);
                            })
                        );
                    }
                    stop_row.into_element()
                } else {
                    ui::btn_sm("Envoyer", Tone::Primary, &c, send_message_handler).into_element()
                }),
        )
        .child(
            // Row 2: Model select dropdown, refresh button, mode toggle and context label
            ui::row_fill(8.)
                .key("chat_input_row_2")
                .height(Size::px(38.))
                .child(model_select)
                .child(refresh_models_button)
                .child(mode_segmented_control)
                .child(
                    ui::row(4.)
                        .width(Size::flex(1.))
                        .main_align(Alignment::End)
                        .child(ui::btn_sm(
                            if has_pinned {
                                format!("+ @ ({})", pinned_list.len())
                            } else {
                                "+ @".to_string()
                            },
                            Tone::Subtle,
                            &c,
                            move |_| {
                                let mut cur = ctx_input.read().clone();
                                if !cur.ends_with('@') {
                                    cur.push('@');
                                }
                                *ctx_input.write() = cur;
                            },
                        )),
                ),
        );

    rect()
        .expanded()
        .content(Content::Flex)
        .background(c.background)
        .child(chat_area.key("chat_messages_area"))
        .child(at_mention_popup)
        .child(input_area)
}

/// Compact starter row (icon, title, one-line hint) adapting responsively to any panel width.
fn starter_row(
    c: &ColorsSheet,
    icon: &'static str,
    title: &'static str,
    hint: &'static str,
    mut on_press: impl FnMut() + 'static,
) -> Rect {
    ui::clickable(
        rect()
            .width(Size::fill())
            .padding(Gaps::new(8., 12., 8., 12.))
            .background(c.surface_primary)
            .corner_radius(6.)
            .border(Border::new().fill(c.border).width(1.))
            .horizontal()
            .cross_align(Alignment::Center)
            .content(Content::Flex)
            .spacing(12.)
            .on_press(move |_| on_press())
            .child(ui::icon(icon, 15., c.primary))
            .child(
                ui::col(2.)
                    .width(Size::flex(1.))
                    .child(ui::title(title, c).font_size(12.))
                    .child(ui::subtitle(hint, c).font_size(10.5)),
            ),
    )
}
