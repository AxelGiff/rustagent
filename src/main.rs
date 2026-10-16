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
    let editor_a11y_id = use_a11y();

    let startup_key_config = config::ApiKeyConfig::load();
    let startup_key_warning = startup_key_config.validate().err();

    let messages = use_state(|| {
        let mut initial = vec![Message {
            role: Role::AI,
            content: "Hello! I'm your coding assistant. Ask me to write code in any language (Python, JavaScript, TypeScript, HTML, CSS, C, C++, Java, Go, Rust) and I'll put it in the editor for you. You can then run it with the **Execute Code** button. Type **clear editor** to empty the code editor.".to_string(),
        }];
        if let Some(warning) = startup_key_warning {
            initial.push(Message {
                role: Role::AI,
                content: format!("⚠️ {}\n\nYou can set your API key via the **Settings** button in the toolbar, the `{}` environment variable, or the config file at `{}`.", warning, config::API_KEY_ENV, config::ApiKeyConfig::config_file_path().display()),
            });
        }
        initial
    });
    let input_value = use_state(String::new);
    let terminal_handle = use_state(spawn_terminal);

    let current_dir = use_state(|| std::env::current_dir().unwrap_or_default());
    let expanded_dirs = use_state(std::collections::HashSet::<std::path::PathBuf>::new);
    let pending_diffs = use_state(Vec::<diff::PendingDiff>::new);
    let center_view_mode = use_state(|| CenterViewMode::Editor);
    let is_generating = use_state(|| false);
    let is_spec_mode = use_state(|| false);
    let spec_selected_options = use_state(std::collections::HashMap::<String, String>::new);
    let spec_custom_input = use_state(String::new);

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
    let saved_cfg = config::ApiKeyConfig::load();
    let settings_key_input = use_state(|| saved_cfg.key);
    let settings_model_input = use_state(|| {
        saved_cfg
            .model
            .unwrap_or_else(|| agent::ALBERT_MODEL.to_string())
    });
    let settings_endpoint_input = use_state(|| {
        saved_cfg
            .endpoint
            .unwrap_or_else(|| agent::ALBERT_ENDPOINT.to_string())
    });
    let available_models = use_state(Vec::<String>::new);
    let models_loading = use_state(|| false);
    let settings_feedback = use_state(String::new);

    // Fetch models at startup if key is present
    let mut initial_models_loaded = use_state(|| false);
    if !*initial_models_loaded.read() {
        *initial_models_loaded.write() = true;
        let ep = settings_endpoint_input.read().trim().to_string();
        let k = settings_key_input.read().trim().to_string();
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

    let current_language = use_state(|| SupportedLanguage::Python);

    let editor = use_state(|| {
        let code = r#"def fibonacci(n):
    """Return the first n Fibonacci numbers."""
    if n <= 0:
        return []
    if n == 1:
        return [0]
    
    fib = [0, 1]
    for i in range(2, n):
        fib.append(fib[i - 1] + fib[i - 2])
    return fib

if __name__ == "__main__":
    result = fibonacci(10)
    print("First 10 Fibonacci numbers:", result)
"#;
        create_editor_data(code, Some(SupportedLanguage::Python))
    });

    let file_name = use_state(|| {
        flow::derive_file_name(&editor.read().rope.to_string(), *current_language.read())
    });

    let clear_chat = {
        let mut messages = messages;
        let mut spec_selected_options = spec_selected_options;
        let mut spec_custom_input = spec_custom_input;
        move || {
            messages.write().clear();
            spec_selected_options.write().clear();
            *spec_custom_input.write() = String::new();
            messages.write().push(Message {
                role: Role::AI,
                content: "Chat cleared. How can I help you?".to_string(),
            });
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
                key: key.clone(),
                model: if model.is_empty() { None } else { Some(model) },
                endpoint: if endpoint.is_empty() {
                    None
                } else {
                    Some(endpoint)
                },
                source: config::KeySource::ConfigFile,
            };
            match cfg.validate() {
                Ok(()) => match cfg.save() {
                    Ok(()) => {
                        *settings_feedback.write() = "Paramètres enregistrés.".to_string();
                        *show_settings.write() = false;
                    }
                    Err(e) => {
                        *settings_feedback.write() = format!("Save failed: {}", e);
                    }
                },
                Err(e) => {
                    *settings_feedback.write() = e;
                }
            }
        }
    };

    let close_settings = {
        let mut show_settings = show_settings;
        move |_| {
            *show_settings.write() = false;
        }
    };

    let accept_diff = {
        let mut pending_diffs = pending_diffs;
        let mut messages = messages;
        let mut center_view_mode = center_view_mode;
        move |idx: usize| {
            let diff_opt = {
                let mut diffs = pending_diffs.write();
                if idx < diffs.len() {
                    Some(diffs.remove(idx))
                } else if !diffs.is_empty() {
                    Some(diffs.remove(0))
                } else {
                    None
                }
            };
            if let Some(diff) = diff_opt {
                if let Err(e) = diff::apply_diff_to_disk(&diff) {
                    messages.write().push(Message {
                        role: Role::AI,
                        content: e,
                    });
                    return;
                }
                update_editor_if_matching(
                    &diff.path,
                    &diff.new_content,
                    &file_name.read(),
                    editor,
                    current_language,
                );
                messages.write().push(Message {
                    role: Role::AI,
                    content: format!("✅ Modifications validées et appliquées à `{}`.", diff.path),
                });
            }
            if pending_diffs.read().is_empty() {
                *center_view_mode.write() = CenterViewMode::Editor;
            }
        }
    };

    let accept_all_diffs = {
        let mut pending_diffs = pending_diffs;
        let mut messages = messages;
        let mut center_view_mode = center_view_mode;
        move || {
            let diffs = pending_diffs.write().drain(..).collect::<Vec<_>>();
            let mut count = 0;
            for diff in &diffs {
                if let Err(e) = diff::apply_diff_to_disk(diff) {
                    messages.write().push(Message {
                        role: Role::AI,
                        content: e,
                    });
                    continue;
                }
                count += 1;
                update_editor_if_matching(
                    &diff.path,
                    &diff.new_content,
                    &file_name.read(),
                    editor,
                    current_language,
                );
            }
            if count > 0 {
                messages.write().push(Message {
                    role: Role::AI,
                    content: format!(
                        "✅ {} modification(s) validée(s) et appliquées sur le disque.",
                        count
                    ),
                });
            }
            *center_view_mode.write() = CenterViewMode::Editor;
        }
    };

    let reject_diff = {
        let mut pending_diffs = pending_diffs;
        let mut messages = messages;
        let mut center_view_mode = center_view_mode;
        move |idx: usize| {
            let diff_opt = {
                let mut diffs = pending_diffs.write();
                if idx < diffs.len() {
                    Some(diffs.remove(idx))
                } else if !diffs.is_empty() {
                    Some(diffs.remove(0))
                } else {
                    None
                }
            };
            if let Some(diff) = diff_opt {
                update_editor_if_matching(
                    &diff.path,
                    &diff.old_content,
                    &file_name.read(),
                    editor,
                    current_language,
                );
                messages.write().push(Message {
                    role: Role::AI,
                    content: format!("❌ Modification pour `{}` rejetée.", diff.path),
                });
            }
            if pending_diffs.read().is_empty() {
                *center_view_mode.write() = CenterViewMode::Editor;
            }
        }
    };

    let reject_all_diffs = {
        let mut pending_diffs = pending_diffs;
        let mut messages = messages;
        let mut center_view_mode = center_view_mode;
        move || {
            let diffs = pending_diffs.write().drain(..).collect::<Vec<_>>();
            for diff in &diffs {
                update_editor_if_matching(
                    &diff.path,
                    &diff.old_content,
                    &file_name.read(),
                    editor,
                    current_language,
                );
            }
            let count = diffs.len();
            messages.write().push(Message {
                role: Role::AI,
                content: format!("❌ {} modification(s) en attente annulée(s).", count),
            });
            *center_view_mode.write() = CenterViewMode::Editor;
        }
    };

    let open_file = {
        let editor = std::rc::Rc::new(std::cell::RefCell::new(editor));
        let current_language = std::rc::Rc::new(std::cell::RefCell::new(current_language));
        let file_name = std::rc::Rc::new(std::cell::RefCell::new(file_name));
        let center_view_mode = std::rc::Rc::new(std::cell::RefCell::new(center_view_mode));
        move |path: std::path::PathBuf| {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                let lang = SupportedLanguage::from_extension(ext);
                if let Some(l) = lang {
                    *current_language.borrow_mut().write() = l;
                }
                *editor.borrow_mut().write() = create_editor_data(&content, lang);
                *file_name.borrow_mut().write() = path.to_string_lossy().to_string();
                *center_view_mode.borrow_mut().write() = CenterViewMode::Editor;
            }
        }
    };

    let send_text = {
        let mut messages = messages;
        let mut input_value = input_value;
        let mut editor = editor;
        let mut file_name = file_name;
        let mut is_generating = is_generating;
        move |user_message: String| {
            if !should_send_message(&user_message) {
                return;
            }

            messages.write().push(Message {
                role: Role::User,
                content: user_message.clone(),
            });

            let detected_language = SupportedLanguage::detect(&user_message);
            let wants_clear_editor = flow::wants_clear_editor(&user_message);
            let wants_code = flow::wants_code(&user_message);

            *input_value.write() = String::new();

            if wants_clear_editor {
                *editor.write() = create_editor_data("\n", Some(*current_language.read()));
                *file_name.write() = flow::derive_file_name("\n", *current_language.read());
                messages.write().push(Message {
                    role: Role::AI,
                    content: "The code editor has been cleared.".to_string(),
                });
                return;
            }

            api::reset_cancel();
            *is_generating.write() = true;

            let history_snapshot = messages.read().clone();
            let current_editor_file = file_name.read().clone();
            let current_editor_code = editor.read().to_string();
            let mut pending_diffs_stream = pending_diffs;
            let mut center_view_mode_stream = center_view_mode;
            let mut messages_stream = messages;
            let mut is_generating_stream = is_generating;
            let spec_mode = *is_spec_mode.read();

            spawn(async move {
                messages_stream.write().push(Message {
                    role: Role::AI,
                    content: String::new(),
                });
                let ai_msg_idx = messages_stream.read().len() - 1;

                let mut messages_for_chunk = messages_stream;
                let mut pending_diffs_for_cb = pending_diffs_stream;
                let mut center_view_for_cb = center_view_mode_stream;

                // Snapshot AVANT l'appel asynchrone : sinon le guard de lecture de
                // `pending_diffs_stream` reste vivant pendant tout le `.await` et le
                // callback `on_pending_diff` panique ("State already borrowed").
                let pending_diffs_snapshot = pending_diffs_stream.read().clone();

                let result = agent::run_agent_turn(
                    user_message,
                    spec_mode,
                    history_snapshot,
                    current_editor_code,
                    current_editor_file,
                    pending_diffs_snapshot,
                    wants_code,
                    detected_language,
                    |chunk| {
                        if let Some(msg) = messages_for_chunk.write().get_mut(ai_msg_idx) {
                            msg.content.push_str(chunk);
                        }
                    },
                    |diff| {
                        pending_diffs_for_cb.write().push(diff);
                        *center_view_for_cb.write() = CenterViewMode::Diff;
                    },
                )
                .await;

                *is_generating_stream.write() = false;

                match result {
                    Ok((final_response, new_diffs)) => {
                        if !new_diffs.is_empty() {
                            pending_diffs_stream.write().extend(new_diffs);
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
            });
        }
    };

    let execute_code = {
        let mut messages = messages;
        move || {
            let code_content = editor.read().rope.to_string();
            let language = *current_language.read();
            let Some(terminal_handle) = terminal_handle.read().clone() else {
                messages.write().push(Message {
                    role: Role::AI,
                    content: "Terminal is not available. Please reset the terminal.".to_string(),
                });
                return;
            };

            let temp_dir = std::env::temp_dir();
            let source_path = temp_dir.join(flow::derive_file_name(&code_content, language));
            if let Err(e) = std::fs::write(&source_path, &code_content) {
                messages.write().push(Message {
                    role: Role::AI,
                    content: format!("Failed to write code file: {}", e),
                });
                return;
            }

            let command = language.run_command(&source_path);
            let _ = terminal_handle.write(command.as_bytes());

            messages.write().push(Message {
                role: Role::AI,
                content: format!(
                    "Code execution started in the terminal ({}).",
                    flow::language_name(language)
                ),
            });
        }
    };

    let save_file = {
        let mut messages = messages;
        move || {
            let path_str = file_name.read().clone();
            let code_content = editor.read().rope.to_string();
            let path = std::path::Path::new(&path_str);
            if let Err(e) = std::fs::write(path, &code_content) {
                messages.write().push(Message {
                    role: Role::AI,
                    content: format!("⚠️ Erreur lors de la sauvegarde de `{}` : {}", path_str, e),
                });
            } else {
                messages.write().push(Message {
                    role: Role::AI,
                    content: format!("💾 Fichier `{}` sauvegardé sur le disque.", path_str),
                });
            }
        }
    };

    let project_name = current_dir
        .read()
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "rustagent".to_string());

    let current_file_display = {
        let f = file_name.read().clone();
        std::path::Path::new(&f)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or(f)
    };

    let is_diff_view =
        *center_view_mode.read() == CenterViewMode::Diff && !pending_diffs.read().is_empty();
    let center_top_panel = if is_diff_view {
        diff_viewer_panel(
            pending_diffs.into(),
            accept_diff,
            accept_all_diffs,
            reject_diff,
            reject_all_diffs,
            {
                let mut center_view_mode = center_view_mode;
                move || {
                    *center_view_mode.write() = CenterViewMode::Editor;
                }
            },
            c.clone(),
        )
        .into_element()
    } else {
        code_editor_panel(
            editor.into(),
            file_name.read().clone(),
            pending_diffs.read().len(),
            {
                let mut center_view_mode = center_view_mode;
                move || {
                    *center_view_mode.write() = CenterViewMode::Diff;
                }
            },
            save_file,
            execute_code,
            editor_a11y_id,
            c.clone(),
        )
        .into_element()
    };

    let chat = chat_panel(
        messages.into(),
        input_value.into(),
        is_generating.into(),
        is_spec_mode.into_writable(),
        spec_selected_options.into_writable(),
        spec_custom_input.into_writable(),
        settings_model_input.into_writable(),
        settings_endpoint_input.into(),
        settings_key_input.into(),
        available_models.into_writable(),
        models_loading.into_writable(),
        project_name,
        current_file_display,
        send_text,
        c.clone(),
    );

    let file_tree = file_tree_panel(
        current_dir.into(),
        expanded_dirs.into(),
        c.clone(),
        open_file.clone(),
    );

    let terminal = terminal_panel(terminal_handle.into_writable(), current_dir.into_writable());

    let toolbar = app_toolbar(
        current_theme.into_writable(),
        theme.into_writable(),
        show_settings.into_writable(),
        available_models.into_writable(),
        models_loading.into_writable(),
        settings_endpoint_input.into(),
        settings_key_input.into(),
        clear_chat,
        reset_terminal,
        c.clone(),
    );

    let main_workspace = main_workspace_layout(file_tree, center_top_panel, terminal, chat);

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
    fn empty_message_is_not_sent() {
        assert!(!should_send_message(""));
    }

    #[test]
    fn whitespace_only_message_is_not_sent() {
        assert!(!should_send_message("   "));
        assert!(!should_send_message("\t"));
        assert!(!should_send_message("\n"));
        assert!(!should_send_message(" \t \n "));
    }

    #[test]
    fn non_empty_message_is_sent() {
        assert!(should_send_message("hello"));
        assert!(should_send_message(" write a function "));
        assert!(should_send_message("clear editor"));
        assert!(should_send_message("a"));
    }

    #[test]
    fn message_with_leading_trailing_whitespace_is_sent() {
        assert!(should_send_message("  hello  "));
    }
}
