#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod agent;
mod api;
mod config;
mod diff;
mod file_tree;
mod flow;
mod language;
mod mcp;
mod myers;
mod platform;
mod session;
mod theme;
mod tools;
mod types;
mod views;

use freya::prelude::*;
pub use language::SupportedLanguage;
use tokio::runtime::Builder;
pub use types::*;
use views::*;

/// Returns `true` if a chat message should be sent to the LLM.
fn should_send_message(message: &str) -> bool {
    !message.trim().is_empty()
}

fn main() {
    let rt = Builder::new_multi_thread().enable_all().build().unwrap();
    let _rt = rt.enter();
    launch(
        LaunchConfig::new().with_window(
            WindowConfig::new(app)
                .with_title("RustAgent")
                .with_size(1400., 700.),
        ),
    );
}

fn app() -> impl IntoElement {
    let current_theme = use_state(|| theme::ThemePreset::ObsidianDark);
    let theme = use_provide_theme(theme::load_theme);
    let c = theme.read().colors.clone();

    let messages = use_state(|| {
        let startup_key_config = config::ApiKeyConfig::load();
        let startup_key_warning = startup_key_config.validate().err();
        let mut initial = Vec::new();
        if let Some(warning) = startup_key_warning {
            initial.push(Message {
                role: Role::AI,
                content: format!("⚠️ {}\n\nConfigurez votre clé API dans les Paramètres ou via la variable `{}`.", warning, config::API_KEY_ENV),
            });
        }
        initial
    });
    let input_value = use_state(String::new);
    let terminal_handle = use_state(spawn_terminal);

    let current_dir = use_state(|| {
        let d = std::env::current_dir().unwrap_or_default();
        crate::session::set_active_project_root(Some(d.clone()));
        d
    });
    let expanded_dirs = use_state(std::collections::HashSet::<std::path::PathBuf>::new);
    let context_paths = use_state(Vec::<std::path::PathBuf>::new);
    let pending_diffs = use_state(Vec::<diff::PendingDiff>::new);
    let selected_diff_idx = use_state(|| 0usize);
    let inline_diff_idx = use_state(|| None::<usize>);
    let review_box_expanded = use_state(|| false);
    let split_diff_view = use_state(|| false);
    let companion_mode = use_state(|| true);
    let show_sidebar = use_state(|| true);
    let show_dashboard = use_state(|| true);
    let chat_scroll = use_scroll_controller(|| ScrollConfig {
        default_vertical_position: ScrollPosition::End,
        ..Default::default()
    });
    let user_scrolled_up = use_state(|| false);
    let center_view_mode = use_state(|| CenterViewMode::Editor);
    let is_generating = use_state(|| false);
    let is_spec_mode = use_state(|| false);
    let spec_selected_options = use_state(std::collections::HashMap::<String, String>::new);
    let spec_custom_input = use_state(String::new);
    let audit_running = use_state(|| false);
    let audit_result = use_state(|| None::<String>);

    // Command approval: the assistant's shell commands must be confirmed by the
    // user. The agent loop blocks asynchronously on a channel that the
    // confirmation bar's buttons resolve.
    let pending_command = use_state(|| None::<tools::PendingCommand>);
    use_future(move || {
        let mut pending_command = pending_command;
        async move {
            let mut rx = tools::subscribe();
            while rx.changed().await.is_ok() {
                let value = rx.borrow().clone();
                *pending_command.write() = value;
            }
        }
    });

    let show_settings = use_state(|| false);
    let saved_cfg = use_hook(config::ApiKeyConfig::load);
    let settings_key_input = use_state(|| saved_cfg.key.clone());
    let settings_model_input = use_state(|| {
        saved_cfg
            .model
            .clone()
            .unwrap_or_else(|| agent::ALBERT_MODEL.to_string())
    });
    let settings_endpoint_input = use_state(|| {
        saved_cfg
            .endpoint
            .clone()
            .unwrap_or_else(|| agent::ALBERT_ENDPOINT.to_string())
    });
    let available_models = use_state(Vec::<String>::new);
    let models_loading = use_state(|| false);
    let settings_feedback = use_state(String::new);

    // Fetch models at startup if key is present
    let mut initial_models_loaded = use_state(|| false);
    if !*initial_models_loaded.read() {
        *initial_models_loaded.write() = true;
        let ep = saved_cfg
            .endpoint
            .clone()
            .unwrap_or_else(|| agent::ALBERT_ENDPOINT.to_string());
        let k = saved_cfg.key.clone();
        let mut available_models = available_models;
        let mut models_loading = models_loading;
        spawn(async move {
            if !k.is_empty() {
                *models_loading.write() = true;
                if let Ok(m) = api::fetch_models(&ep, &k).await
                    && !m.is_empty()
                {
                    *available_models.write() = m;
                }
                *models_loading.write() = false;
            }
        });
    }

    let current_language = use_state(|| SupportedLanguage::Rust);
    let editor = use_state(|| create_editor_data("", None));
    let file_name = use_state(String::new);

    let open_project = {
        let mut current_dir = current_dir;
        let mut expanded_dirs = expanded_dirs;
        let terminal_handle = terminal_handle;
        move || {
            let init_dir = current_dir.read().clone();
            let terminal_opt = terminal_handle.read().clone();
            spawn(async move {
                let picked =
                    tokio::task::spawn_blocking(move || platform::pick_folder_dialog(Some(&init_dir)))
                        .await
                        .ok()
                        .flatten();
                if let Some(chosen) = picked {
                    expanded_dirs.write().clear();
                    *current_dir.write() = chosen.clone();
                    crate::session::set_active_project_root(Some(chosen.clone()));
                    if let Some(term) = terminal_opt {
                        let cmd = platform::cd_command(&chosen);
                        let _ = term.write(cmd.as_bytes());
                    }
                }
            });
        }
    };

    let clear_chat = {
        let mut messages = messages;
        let mut pending_diffs = pending_diffs;
        let mut selected_diff_idx = selected_diff_idx;
        let mut inline_diff_idx = inline_diff_idx;
        let mut spec_selected_options = spec_selected_options;
        let mut spec_custom_input = spec_custom_input;
        let mut context_paths = context_paths;
        let mut is_generating = is_generating;
        move || {
            api::request_cancel();
            *is_generating.write() = false;
            messages.write().clear();
            pending_diffs.write().clear();
            context_paths.write().clear();
            *selected_diff_idx.write() = 0;
            *inline_diff_idx.write() = None;
            spec_selected_options.write().clear();
            *spec_custom_input.write() = String::new();
        }
    };

    let reset_terminal = {
        let mut terminal_handle = terminal_handle;
        move || {
            *terminal_handle.write() = spawn_terminal();
        }
    };

    let refresh_models = {
        let endpoint_val = settings_endpoint_input.read().clone();
        let key_val = settings_key_input.read().clone();
        move |_| {
            let ep = endpoint_val.trim().to_string();
            let k = key_val.trim().to_string();
            let mut available_models = available_models;
            let mut models_loading = models_loading;
            let mut settings_feedback = settings_feedback;
            *models_loading.write() = true;
            spawn(async move {
                match api::fetch_models(&ep, &k).await {
                    Ok(models) => {
                        *available_models.write() = models;
                        *settings_feedback.write() = String::new();
                    }
                    Err(e) => {
                        *settings_feedback.write() = format!("Erreur modèles: {}", e);
                    }
                }
                *models_loading.write() = false;
            });
        }
    };

    let save_api_key = {
        let mut settings_feedback = settings_feedback;
        let mut show_settings = show_settings;
        move |_| {
            let key = settings_key_input.read().trim().to_string();
            let model = settings_model_input.read().trim().to_string();
            let endpoint = settings_endpoint_input.read().trim().to_string();
            let cfg = config::ApiKeyConfig {
                key,
                model: if model.is_empty() { None } else { Some(model) },
                endpoint: if endpoint.is_empty() { None } else { Some(endpoint) },
                source: config::KeySource::ConfigFile,
            };
            let val_res = cfg.validate();
            if let Err(e) = cfg.save() {
                *settings_feedback.write() = format!("Échec d'enregistrement: {}", e);
            } else if let Err(warn) = val_res {
                *settings_feedback.write() = format!("Enregistré avec avertissement: {}", warn);
            } else {
                *settings_feedback.write() = "Paramètres enregistrés avec succès.".to_string();
                *show_settings.write() = false;
            }
        }
    };

    let close_settings = {
        let mut show_settings = show_settings;
        move |_| {
            *show_settings.write() = false;
        }
    };

    let diff_review = DiffReviewState {
        pending_diffs,
        selected_diff_idx,
        inline_diff_idx,
        review_box_expanded,
        split_diff_view,
        chat_scroll,
        center_view_mode,
        messages,
        editor,
        current_language,
        file_name,
        user_scrolled_up,
    };

    let open_file = {
        let file_name = file_name;
        move |path: std::path::PathBuf| {
            let mut file_name = file_name;
            *file_name.write() = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            crate::platform::open_in_vscode(&path, None);
        }
    };

    let send_text = {
        let (messages, input_value, editor, file_name, is_generating, chat_scroll, context_paths, user_scrolled_up) =
            (messages, input_value, editor, file_name, is_generating, chat_scroll, context_paths, user_scrolled_up);
        move |user_message: String| {
            let (mut messages, mut input_value, mut editor, mut file_name, mut is_generating, mut chat_scroll, mut context_paths, mut user_scrolled_up) =
                (messages, input_value, editor, file_name, is_generating, chat_scroll, context_paths, user_scrolled_up);
            if !should_send_message(&user_message) {
                return;
            }

            let root_snapshot = current_dir.read().clone();
            let at_resolved = file_tree::resolve_at_mentions_in_text(&user_message, &root_snapshot);
            if !at_resolved.is_empty() {
                let mut ctx = context_paths.write();
                for p in at_resolved {
                    if !ctx.contains(&p) {
                        ctx.push(p);
                    }
                }
            }

            let pinned_snapshot = context_paths.read().clone();
            let display_user_msg = if pinned_snapshot.is_empty() {
                user_message.clone()
            } else {
                let targets = pinned_snapshot
                    .iter()
                    .map(|p| {
                        let name = p
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| p.to_string_lossy().to_string());
                        if p.is_dir() {
                            format!("📁 {}", name)
                        } else {
                            format!("📄 {}", name)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" · ");
                format!("🎯 {}\n{}", targets, user_message)
            };

            messages.write().push(Message {
                role: Role::User,
                content: display_user_msg,
            });
            *user_scrolled_up.write() = false;
            chat_scroll.scroll_to(ScrollPosition::End, Direction::Vertical);

            let detected_language = SupportedLanguage::detect(&user_message);
            let wants_clear_editor = flow::wants_clear_editor(&user_message);
            let wants_code = flow::wants_code(&user_message);

            *input_value.write() = String::new();

            if wants_clear_editor {
                *editor.write() = create_editor_data("", None);
                *file_name.write() = String::new();
                messages.write().push(Message {
                    role: Role::AI,
                    content: "L'éditeur a été vidé.".to_string(),
                });
                chat_scroll.scroll_to(ScrollPosition::End, Direction::Vertical);
                return;
            }

            api::reset_cancel();
            *is_generating.write() = true;

            let history_snapshot = messages.read().clone();
            let current_editor_file = file_name.read().clone();
            let current_editor_code = editor.read().to_string();
            let mut current_dir_stream = current_dir;
            let mut expanded_dirs_stream = expanded_dirs;
            let terminal_for_sync = terminal_handle.read().clone();
            if let Some(detected_proj) = tools::detect_project_dir(&user_message, &[])
                && detected_proj != root_snapshot
            {
                expanded_dirs_stream.write().clear();
                *current_dir_stream.write() = detected_proj.clone();
                crate::session::set_active_project_root(Some(detected_proj.clone()));
                if let Some(ref term) = terminal_for_sync {
                    let cmd = platform::cd_command(&detected_proj);
                    let _ = term.write(cmd.as_bytes());
                }
            }

            let (mut pending_diffs_stream, mut center_view_mode_stream, mut messages_stream, mut is_generating_stream, mut chat_scroll_stream, mut user_scrolled_stream) =
                (pending_diffs, center_view_mode, messages, is_generating, chat_scroll, user_scrolled_up);
            let spec_mode = *is_spec_mode.read();

            spawn(async move {
                messages_stream.write().push(Message {
                    role: Role::AI,
                    content: String::new(),
                });
                *user_scrolled_stream.write() = false;
                chat_scroll_stream.scroll_to(ScrollPosition::End, Direction::Vertical);
                let ai_msg_idx = messages_stream.read().len() - 1;

                let (mut messages_for_chunk, mut pending_diffs_for_cb, mut center_view_for_cb, mut chat_scroll_for_cb, mut chat_scroll_for_chunk, user_scrolled_for_chunk, mut chunk_cnt) =
                    (messages_stream, pending_diffs_stream, center_view_mode_stream, chat_scroll_stream, chat_scroll_stream, user_scrolled_stream, 0usize);

                // Snapshot AVANT l'appel asynchrone : sinon le guard de lecture de
                // `pending_diffs_stream` reste vivant pendant tout le `.await` et le
                // callback `on_pending_diff` panique ("State already borrowed").
                let pending_diffs_snapshot = pending_diffs_stream.read().clone();

                crate::session::set_active_project_root(Some(root_snapshot.clone()));
                let result = agent::run_agent_turn(
                    user_message,
                    spec_mode,
                    history_snapshot,
                    current_editor_code,
                    current_editor_file,
                    root_snapshot,
                    pinned_snapshot,
                    pending_diffs_snapshot,
                    wants_code,
                    detected_language,
                    |chunk| {
                        if let Some(msg) = messages_for_chunk.write().get_mut(ai_msg_idx) {
                            msg.content.push_str(chunk);
                        }
                        chunk_cnt += 1;
                        if (chunk_cnt % 5 == 0 || chunk.contains('\n')) && !*user_scrolled_for_chunk.read() {
                            chat_scroll_for_chunk.scroll_to(ScrollPosition::End, Direction::Vertical);
                        }
                    },
                    |diff| {
                        let diff_path = std::path::PathBuf::from(&diff.path);
                        let cur_root = current_dir_stream.peek().clone();
                        if !diff_path.starts_with(&cur_root)
                            && let Some(new_root) = tools::infer_project_root_from_path(&diff_path)
                            && new_root != cur_root
                        {
                            expanded_dirs_stream.write().clear();
                            *current_dir_stream.write() = new_root.clone();
                            if let Some(ref term) = terminal_for_sync {
                                let cmd = platform::cd_command(&new_root);
                                let _ = term.write(cmd.as_bytes());
                            }
                        }
                        diff::upsert_diff(&mut pending_diffs_for_cb.write(), diff);
                        *center_view_for_cb.write() = CenterViewMode::Diff;
                        chat_scroll_for_cb.scroll_to(ScrollPosition::End, Direction::Vertical);
                    },
                )
                .await;

                *is_generating_stream.write() = false;

                match result {
                    Ok((final_response, new_diffs)) => {
                        if !new_diffs.is_empty() {
                            for d in new_diffs {
                                diff::upsert_diff(&mut pending_diffs_stream.write(), d);
                            }
                            *center_view_mode_stream.write() = CenterViewMode::Diff;
                        }
                        if let Some(msg) = messages_stream.write().get_mut(ai_msg_idx) {
                            msg.content = final_response;
                        }
                    }
                    Err(friendly) => {
                        if let Some(msg) = messages_stream.write().get_mut(ai_msg_idx) {
                            if msg.content.trim().is_empty() {
                                msg.content =
                                    format!("⚠️ **Erreur lors de l'appel :**\n\n{}", friendly);
                            } else {
                                msg.content.push_str(&format!(
                                    "\n\n⚠️ **Erreur lors de l'appel :**\n\n{}",
                                    friendly
                                ));
                            }
                        }
                    }
                }
                if !*user_scrolled_stream.read() {
                    chat_scroll_stream.scroll_to(ScrollPosition::End, Direction::Vertical);
                }
            });
        }
    };

    let project_name = current_dir
        .read()
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "rustagent".to_string());

    let current_file_display = {
        let f = file_name.read().clone();
        if f.trim().is_empty() {
            "Aucun fichier ouvert".to_string()
        } else {
            std::path::Path::new(&f)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or(f)
        }
    };

    let is_diff_view =
        *center_view_mode.read() == CenterViewMode::Diff && !pending_diffs.read().is_empty();
    let center_top_panel = if is_diff_view {
        diff_viewer_panel(diff_review, c.clone()).into_element()
    } else {
        code_editor_panel(
            current_dir.into(),
            audit_running.into_writable(),
            audit_result.into_writable(),
            pending_diffs
                .read()
                .iter()
                .filter(|d| d.status != diff::DiffStatus::Rejected)
                .count(),
            {
                let mut center_view_mode = center_view_mode;
                move || {
                    *center_view_mode.write() = CenterViewMode::Diff;
                }
            },
            send_text.clone(),
            show_dashboard.into_writable(),
            messages.into(),
            context_paths.into(),
            clear_chat.clone(),
            c.clone(),
        )
        .into_element()
    };

    let chat = chat_panel(
        messages.into(),
        input_value.into(),
        is_generating.into_writable(),
        is_spec_mode.into_writable(),
        spec_selected_options.into_writable(),
        spec_custom_input.into_writable(),
        current_dir.into(),
        context_paths.into_writable(),
        settings_model_input.into_writable(),
        settings_endpoint_input.into(),
        settings_key_input.into(),
        available_models.into_writable(),
        models_loading.into_writable(),
        project_name,
        current_file_display,
        diff_review,
        send_text,
        c.clone(),
    );

    let file_tree = file_tree_panel(
        current_dir.into_writable(),
        expanded_dirs.into(),
        context_paths.into_writable(),
        pending_diffs.into(),
        c.clone(),
        open_file.clone(),
        show_sidebar.into_writable(),
    );

    let terminal = terminal_panel(terminal_handle.into_writable(), current_dir.into_writable());

    let toolbar = app_toolbar(
        current_theme.into_writable(),
        theme.into_writable(),
        show_settings.into_writable(),
        companion_mode.into_writable(),
        show_sidebar.into_writable(),
        show_dashboard.into_writable(),
        available_models.into_writable(),
        models_loading.into_writable(),
        settings_endpoint_input.into(),
        settings_key_input.into(),
        open_project,
        clear_chat,
        reset_terminal,
        c.clone(),
    );

    let main_workspace = main_workspace_layout(
        file_tree,
        center_top_panel,
        terminal,
        chat,
        *companion_mode.read(),
        *show_sidebar.read(),
        *show_dashboard.read(),
    );

    let mut root = rect()
        .expanded()
        .background(c.background)
        .content(Content::Flex);

    if *show_settings.read() {
        root = root.child(settings_page_view(
            c.clone(),
            settings_key_input.into(),
            settings_model_input.into(),
            settings_endpoint_input.into(),
            available_models.into(),
            models_loading.into(),
            settings_feedback.into(),
            refresh_models,
            save_api_key,
            close_settings,
        ));
    } else {
        root = root
            .child(toolbar)
            .child(rect().expanded().child(main_workspace));
    }

    // Command confirmation bar, spanning the whole window.
    if let Some(command) = pending_command.read().clone() {
        root = root.child(command_confirmation_bar(command, c.clone()));
    }

    root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_send_message() {
        assert!(!should_send_message(""));
        for s in &["   ", "\t", "\n", " \t \n "] { assert!(!should_send_message(s)); }
        for s in &["hello", " write a function ", "clear editor", "a", "  hello  "] { assert!(should_send_message(s)); }
    }
}
