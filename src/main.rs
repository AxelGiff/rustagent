#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod api;
mod config;
mod file_tree;
mod flow;
mod platform;
mod theme;

use freya::{clipboard::Clipboard, code_editor::*, prelude::*, terminal::*, text_edit::TextEditor};
use futures_util::{FutureExt, StreamExt};
use rig::{
    agent::MultiTurnStreamItem,
    client::CompletionClient,
    providers::openai,
    streaming::{StreamedAssistantContent, StreamingChat},
};
use ropey::Rope;
use tokio::runtime::Builder;

// Albert API configuration (French government AI service)
const ALBERT_ENDPOINT: &str = "https://albert.api.etalab.gouv.fr/v1";
const ALBERT_MODEL: &str = "deepseek-v4-flash";

use api::{Message, Role};

/// A programming language supported by the editor. Each language knows its
/// tree-sitter grammar, highlights query, file extension and how to run it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum SupportedLanguage {
    Python,
    Rust,
    JavaScript,
    TypeScript,
    Html,
    Css,
    C,
    Cpp,
    Java,
    Go,
}

impl SupportedLanguage {
    /// The file extension (without the dot) used for the editor label and the
    /// temp file written before execution.
    fn extension(&self) -> &'static str {
        match self {
            SupportedLanguage::Python => "py",
            SupportedLanguage::Rust => "rs",
            SupportedLanguage::JavaScript => "js",
            SupportedLanguage::TypeScript => "ts",
            SupportedLanguage::Html => "html",
            SupportedLanguage::Css => "css",
            SupportedLanguage::C => "c",
            SupportedLanguage::Cpp => "cpp",
            SupportedLanguage::Java => "java",
            SupportedLanguage::Go => "go",
        }
    }

    /// Build the tree-sitter language + highlights query for the editor.
    fn editor_language(&self) -> EditorLanguage {
        match self {
            SupportedLanguage::Python => EditorLanguage::new(
                tree_sitter_python::LANGUAGE,
                tree_sitter_python::HIGHLIGHTS_QUERY,
            ),
            SupportedLanguage::Rust => EditorLanguage::new(
                tree_sitter_rust::LANGUAGE,
                tree_sitter_rust::HIGHLIGHTS_QUERY,
            ),
            SupportedLanguage::JavaScript => EditorLanguage::new(
                tree_sitter_javascript::LANGUAGE,
                tree_sitter_javascript::HIGHLIGHT_QUERY,
            ),
            SupportedLanguage::TypeScript => EditorLanguage::new(
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
            ),
            SupportedLanguage::Html => EditorLanguage::new(
                tree_sitter_html::LANGUAGE,
                tree_sitter_html::HIGHLIGHTS_QUERY,
            ),
            SupportedLanguage::Css => {
                EditorLanguage::new(tree_sitter_css::LANGUAGE, tree_sitter_css::HIGHLIGHTS_QUERY)
            }
            SupportedLanguage::C => {
                EditorLanguage::new(tree_sitter_c::LANGUAGE, tree_sitter_c::HIGHLIGHT_QUERY)
            }
            SupportedLanguage::Cpp => {
                EditorLanguage::new(tree_sitter_cpp::LANGUAGE, tree_sitter_cpp::HIGHLIGHT_QUERY)
            }
            SupportedLanguage::Java => EditorLanguage::new(
                tree_sitter_java::LANGUAGE,
                tree_sitter_java::HIGHLIGHTS_QUERY,
            ),
            SupportedLanguage::Go => {
                EditorLanguage::new(tree_sitter_go::LANGUAGE, tree_sitter_go::HIGHLIGHTS_QUERY)
            }
        }
    }

    /// The shell command used to run a file of this language. The `{file}`
    /// placeholder is replaced with the path to the temp file.
    fn run_command(&self, file: &std::path::Path) -> String {
        let file = file.display().to_string();
        let newline = if cfg!(windows) { "\r\n" } else { "\n" };
        // Use the platform temp directory for compiled binaries so the
        // command works on Windows, macOS, and Linux alike.
        let temp_dir = std::env::temp_dir();
        let bin = |name: &str| temp_dir.join(name).display().to_string();
        match self {
            SupportedLanguage::Python => {
                format!("{} {}{}", platform::python_command(), file, newline)
            }
            SupportedLanguage::Rust => {
                let out = bin("main_rs");
                format!(
                    "{} {} -o {} && {}{}",
                    platform::rust_compiler(),
                    file,
                    out,
                    out,
                    newline
                )
            }
            SupportedLanguage::JavaScript => {
                format!("{} {}{}", platform::node_runner(), file, newline)
            }
            SupportedLanguage::TypeScript => {
                format!("{} {}{}", platform::ts_runner(), file, newline)
            }
            SupportedLanguage::Html => platform::open_command(&file),
            SupportedLanguage::Css => format!("echo 'CSS is a stylesheet, nothing to run.'{}", newline),
            SupportedLanguage::C => {
                let out = bin("main_c");
                format!(
                    "{} {} -o {} && {}{}",
                    platform::c_compiler(),
                    file,
                    out,
                    out,
                    newline
                )
            }
            SupportedLanguage::Cpp => {
                let out = bin("main_cpp");
                format!(
                    "{} {} -o {} && {}{}",
                    platform::cpp_compiler(),
                    file,
                    out,
                    out,
                    newline
                )
            }
            SupportedLanguage::Java => {
                // Java requires the file name to match the public class name.
                format!(
                    "{} {} && {} Main{}",
                    platform::java_compiler(),
                    file,
                    platform::java_runtime(),
                    newline
                )
            }
            SupportedLanguage::Go => {
                format!("{} run {}{}", platform::go_runner(), file, newline)
            }
        }
    }

    /// Detect the language from a user message. Looks for common language
    /// names and aliases. Defaults to Python.
    fn detect(message: &str) -> SupportedLanguage {
        let lower = message.to_lowercase();
        let contains = |keywords: &[&str]| keywords.iter().any(|k| lower.contains(k));

        if contains(&["javascript", "js", "node"]) {
            SupportedLanguage::JavaScript
        } else if contains(&["typescript", "tsx", "ts "]) {
            SupportedLanguage::TypeScript
        } else if contains(&["html"]) {
            SupportedLanguage::Html
        } else if contains(&["css"]) {
            SupportedLanguage::Css
        } else if contains(&["c++", "cpp", "cplusplus"]) {
            SupportedLanguage::Cpp
        } else if contains(&["golang", "go "]) {
            SupportedLanguage::Go
        } else if contains(&["java"]) {
            SupportedLanguage::Java
        } else if contains(&["rust", "rs"]) {
            SupportedLanguage::Rust
        } else if contains(&["c "]) {
            SupportedLanguage::C
        } else {
            // Default to Python for anything else (including "python").
            SupportedLanguage::Python
        }
    }
}

/// Returns `true` if a chat message should be sent to the LLM.
///
/// Empty or whitespace-only messages are ignored. This is the single source
/// of truth for the empty-check shared by both the Send button and the Enter
/// key, so both triggers behave identically.
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
    )
}

/// Spawn a fresh terminal and return its handle (or None on failure).
/// The shell and environment are chosen per-platform via the `platform` module.
fn spawn_terminal() -> Option<TerminalHandle> {
    let mut cmd = CommandBuilder::new(platform::terminal_shell());
    // Pass shell arguments that make the shell report its current working
    // directory via OSC 7, so the file-tree sidebar can follow the terminal.
    for arg in platform::terminal_shell_args() {
        cmd.arg(arg);
    }
    for (key, value) in platform::terminal_env() {
        cmd.env(key, value);
    }
    TerminalHandle::new(TerminalId::new(), cmd, None).ok()
}

/// Builds the code editor pane. The editor state is lifted up into `app()` so
/// that the chat and terminal panels can read/write the same content.
fn code_editor_panel(editor: Writable<CodeEditorData>, file_name: String) -> impl IntoElement {
    let a11y_id = use_a11y();
    let c = use_theme().read().colors.clone();

    rect()
        .expanded()
        .content(Content::Flex)
        .background(c.background)
        .child(
            rect()
                .width(Size::fill())
                .height(Size::px(32.))
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
                .child(
                    label()
                        .text(file_name)
                        .color(c.text_primary)
                        .font_size(13.)
                        .font_weight(FontWeight::BOLD),
                ),
        )
        .child(
            rect()
                .expanded()
                .padding(Gaps::new(6., 6., 6., 0.))
                .child(CodeEditor::new(editor, a11y_id).background(c.surface_secondary)),
        )
}

fn app() -> impl IntoElement {
    // Load the external theme (colors) from theme.json and provide it to the
    // whole component tree. This is the app's "stylesheet": the single source
    // of truth for the color palette. `c` is a shorthand for the color sheet.
    let theme = use_provide_theme(theme::load_theme);
    let c = theme.read().colors.clone();

    // Check the API key configuration at startup so the user is informed
    // immediately if it is missing or invalid, rather than failing silently
    // on the first message.
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

    // The current working directory reported by the terminal (via OSC 7). This
    // is shared state: `terminal_panel` updates it whenever the terminal's
    // path changes, and `file_tree_panel` reads it to render the sidebar tree.
    let current_dir = use_state(|| std::env::current_dir().unwrap_or_default());
    // The set of directories the user has expanded in the file-tree sidebar,
    // keyed by their absolute path.
    let expanded_dirs = use_state(std::collections::HashSet::<std::path::PathBuf>::new);

    // Whether the settings panel is open.
    let show_settings = use_state(|| false);
    // The API key being edited in the settings panel.
    let settings_key_input = use_state(|| config::ApiKeyConfig::load().key);
    // Feedback message shown in the settings panel after saving.
    let settings_feedback = use_state(String::new);

    // The currently selected language. Defaults to Python.
    let current_language = use_state(|| SupportedLanguage::Python);

    // Shared editor state, lifted up so the chat and terminal panels can read
    // and write the exact same content that is displayed in the editor.
    let editor = use_state(|| {
        let rope = Rope::from_str(
            r#"def fibonacci(n):
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
"#,
        );
        let language = SupportedLanguage::Python.editor_language();
        let mut editor = CodeEditorData::new(rope, language);
        editor.set_theme(EditorSyntaxTheme::dark());
        editor.parse();
        editor.measure(14., "Jetbrains Mono");
        editor
    });

    // The file name shown in the editor header. This is shared state so it can
    // be updated whenever code is inserted (or the editor is cleared) and read
    // by `code_editor_panel`. It is derived from the script content so the
    // title reflects what was actually written, falling back to `main.<ext>`
    // when no meaningful name can be derived.
    let file_name = use_state(|| {
        flow::derive_file_name(&editor.read().rope.to_string(), *current_language.read())
    });

    // Toolbar actions
    let clear_chat = {
        let mut messages = messages;
        move |_| {
            messages.write().clear();
            messages.write().push(Message {
                role: Role::AI,
                content: "Chat cleared. How can I help you?".to_string(),
            });
        }
    };

    let reset_terminal = {
        let mut terminal_handle = terminal_handle;
        let mut current_dir = current_dir;
        move |_| {
            // Unmount the current terminal widget first so Freya remounts a fresh one
            *terminal_handle.write() = None;
            *current_dir.write() = std::env::current_dir().unwrap_or_default();
            let mut terminal_handle = terminal_handle.clone();
            spawn(async move {
                tokio::task::yield_now().await;
                *terminal_handle.write() = spawn_terminal();
            });
        }
    };

    // Save the API key entered in the settings panel to the config file.
    let save_api_key = {
        let mut settings_feedback = settings_feedback;
        let mut show_settings = show_settings;
        move |_| {
            let key = settings_key_input.read().trim().to_string();
            let cfg = config::ApiKeyConfig {
                key: key.clone(),
                source: config::KeySource::ConfigFile,
            };
            match cfg.validate() {
                Ok(()) => match cfg.save() {
                    Ok(()) => {
                        *settings_feedback.write() = "API key saved to config file.".to_string();
                        // Close the panel after a successful save.
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

    // Close the settings panel.
    let close_settings = {
        let mut show_settings = show_settings;
        move |_| {
            *show_settings.write() = false;
        }
    };

    // Shared chat send logic. Both the Send button and the Enter key route
    // through this same code path so they behave identically. It takes the raw
    // message text, ignores empty/whitespace-only messages, and dispatches it
    // (clear editor locally, or call the AI).
    let send_text = {
        let mut messages = messages;
        let mut input_value = input_value;
        let mut editor = editor;
        let mut current_language = current_language;
        let mut file_name = file_name;
        move |user_message: String| {
            if !should_send_message(&user_message) {
                return;
            }

            // Add user message
            messages.write().push(Message {
                role: Role::User,
                content: user_message.clone(),
            });

            // Detect the language the user is asking about.
            let detected_language = SupportedLanguage::detect(&user_message);

            // Check if the user wants to clear the editor. This is handled
            // locally (no AI call needed).
            let wants_clear_editor = flow::wants_clear_editor(&user_message);

            // Check if user wants to generate code. This is intentionally broad:
            // it triggers when the user asks to write/generate/create/make code,
            // a function, a program, a script, etc.
            let wants_code = flow::wants_code(&user_message);

            // Clear input
            *input_value.write() = String::new();

            // Handle "clear editor" locally without calling the AI.
            if wants_clear_editor {
                // Re-setting the language invalidates the cached tree-sitter
                // tree. This is essential: if we cleared the rope while the old
                // tree (built from the previous content) was still cached, the
                // next parse would try to read bytes that no longer exist and
                // panic. We also seed the rope with a single newline so it is
                // never empty, which keeps the highlighter happy.
                editor
                    .write()
                    .set_language(current_language.read().editor_language());
                editor.write().set("\n");
                editor.write().set_selection((0, 0));
                editor.write().parse();
                editor.write().measure(14., "Jetbrains Mono");
                // The editor is now empty, so the title falls back to the
                // conventional `main.<ext>` for the current language.
                *file_name.write() = flow::derive_file_name("\n", *current_language.read());
                messages.write().push(Message {
                    role: Role::AI,
                    content: "The code editor has been cleared.".to_string(),
                });
                return;
            }

            // Add AI response using streaming via rig-core with the Albert endpoint
            spawn(async move {
                // Load the Albert API key from env var or config file.
                let api_key_config = config::ApiKeyConfig::load();

                // If no key is configured, inform the user instead of silently
                // failing on the first request.
                if let Err(msg) = api_key_config.validate() {
                    messages.write().push(Message {
                        role: Role::AI,
                        content: format!(
                            "⚠️ {}\n\nPlease configure your API key and try again.",
                            msg
                        ),
                    });
                    return;
                }

                // Build an OpenAI-compatible Completions client pointed at the Albert endpoint
                let client = openai::CompletionsClient::builder()
                    .api_key(&api_key_config.key)
                    .base_url(ALBERT_ENDPOINT)
                    .build();

                match client {
                    Ok(client) => {
                        let agent = client.agent(ALBERT_MODEL).build();

                        // Build conversation history from messages prior to this prompt
                        let prior_messages = {
                            let msgs = messages.read();
                            if msgs.len() > 1 {
                                msgs[..msgs.len() - 1].to_vec()
                            } else {
                                Vec::new()
                            }
                        };
                        let chat_history = api::build_chat_history(
                            &prior_messages,
                            api::MAX_HISTORY_MESSAGES,
                        );

                        // Pre-push an empty AI message that will receive tokens incrementally as they stream in
                        let ai_msg_index = {
                            let mut msgs = messages.write();
                            msgs.push(Message {
                                role: Role::AI,
                                content: String::new(),
                            });
                            msgs.len() - 1
                        };

                        let mut messages_stream = messages.clone();
                        let result = api::prompt_stream_with_retry(
                            || {
                                let agent = agent.clone();
                                let user_message = user_message.clone();
                                let chat_history = chat_history.clone();
                                async move {
                                    let stream = agent.stream_chat(user_message, chat_history).await;
                                    let mapped = stream.map(|item| match item {
                                        Ok(MultiTurnStreamItem::StreamAssistantItem(
                                            StreamedAssistantContent::Text(text),
                                        )) => Ok(text.to_string()),
                                        Ok(_) => Ok(String::new()),
                                        Err(e) => Err(e.to_string()),
                                    });
                                    Ok(Box::pin(mapped))
                                }
                            },
                            |chunk| {
                                let mut msgs = messages_stream.write();
                                if let Some(msg) = msgs.get_mut(ai_msg_index) {
                                    msg.content.push_str(chunk);
                                }
                            },
                            || false,
                        )
                        .await;

                        match result {
                            Ok(response) => {
                                // Decide what to do with the complete response once streaming finishes:
                                // inject code into the editor, or keep the raw streaming text in chat.
                                match flow::decide_editor_action(
                                    &response,
                                    wants_code,
                                    detected_language,
                                ) {
                                    flow::EditorAction::Insert { language, code } => {
                                        // Switch the editor to the detected language.
                                        *current_language.write() = language;
                                        editor.write().set_language(language.editor_language());
                                        // Push the generated code into the shared editor state.
                                        editor.write().set(&code);
                                        editor.write().set_selection((0, 0));
                                        editor.write().parse();
                                        editor.write().measure(14., "Jetbrains Mono");
                                        // Update the editor header title to reflect the script that was written.
                                        *file_name.write() =
                                            flow::derive_file_name(&code, language);
                                        // Update the AI chat message content to the insertion confirmation.
                                        let mut msgs = messages.write();
                                        if let Some(msg) = msgs.get_mut(ai_msg_index) {
                                            msg.content = flow::insertion_confirmation(language);
                                        }
                                    }
                                    flow::EditorAction::ShowResponse => {
                                        // The stream has already populated the complete response.
                                    }
                                }
                            }
                            Err((_category, full_err_or_partial)) => {
                                let mut msgs = messages.write();
                                if let Some(msg) = msgs.get_mut(ai_msg_index) {
                                    if msg.content.is_empty() {
                                        msg.content = format!("⚠️ {}", full_err_or_partial);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        messages.write().push(Message {
                            role: Role::AI,
                            content: format!("Failed to build client: {}", e),
                        });
                    }
                }
            });
        }
    };

    // Send button handler: reads the current input field and sends it.
    let send_message = {
        let mut send_text = send_text;
        move |_| {
            let text = input_value.read().clone();
            send_text(text);
        }
    };

    // Enter key handler: the Input component calls `on_submit` with the
    // committed text when Enter is pressed, so we send it directly.
    let on_submit = {
        let mut send_text = send_text;
        move |text: String| {
            send_text(text);
        }
    };

    // Execute the code currently in the editor inside the terminal.
    let execute_code = {
        let mut messages = messages;
        move |_| {
            // Read the live content straight from the shared editor state.
            let code_content = editor.read().rope.to_string();
            let language = *current_language.read();
            let Some(terminal_handle) = terminal_handle.read().clone() else {
                messages.write().push(Message {
                    role: Role::AI,
                    content: "Terminal is not available. Please reset the terminal.".to_string(),
                });
                return;
            };

            // Write the code to a temp file so it can be run. The file name
            // matches the editor title (derived from the script content) so
            // the title stays consistent with what is actually executed.
            let temp_dir = std::env::temp_dir();
            let source_path = temp_dir.join(flow::derive_file_name(&code_content, language));
            if let Err(e) = std::fs::write(&source_path, &code_content) {
                messages.write().push(Message {
                    role: Role::AI,
                    content: format!("Failed to write code file: {}", e),
                });
                return;
            }

            // Run the file inside the terminal using the language's command.
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

    // Chat area
    let chat_area = rect().width(Size::fill()).height(Size::flex(1.)).child(
        ScrollView::new().child(rect().width(Size::fill()).padding(16.).children(
            messages.read().iter().map(|msg| {
                let is_user = msg.role == Role::User;
                // OBSIDIAN THEME COLORS
                let bg_color = if is_user {
                    c.surface_inverse // Obsidian blue-gray for user messages
                } else {
                    c.surface_inverse_secondary // Deep obsidian background for AI messages
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
                    c.text_inverse // Light text for user messages
                } else {
                    c.text_highlight // Subtle text for AI messages
                };

                rect()
                    .width(Size::fill())
                    .margin(8.)
                    .cross_align(align)
                    .child(
                        rect()
                            .padding(12.)
                            .background(bg_color)
                            .corner_radius(16.)
                            .color(text_color)
                            .text_align(text_align)
                            .child(if is_user {
                                SelectableText::new()
                                    .span(msg.content.clone())
                                    .color(text_color)
                                    .into_element()
                            } else {
                                rect()
                                    .width(Size::fill())
                                    .children(
                                        flow::parse_markdown_segments(&msg.content)
                                            .into_iter()
                                            .map(|seg| match seg {
                                                flow::MarkdownSegment::Text(text) => {
                                                    MarkdownViewer::new(text)
                                                        .color(text_color)
                                                        .into_element()
                                                }
                                                flow::MarkdownSegment::CodeBlock {
                                                    language,
                                                    code,
                                                } => {
                                                    rect()
                                                        .width(Size::fill())
                                                        .margin(4.)
                                                        .corner_radius(8.)
                                                        .background(c.surface_primary)
                                                        .border(
                                                            Border::new()
                                                                .fill(c.border)
                                                                .width(BorderWidth {
                                                                    top: 1.,
                                                                    right: 1.,
                                                                    bottom: 1.,
                                                                    left: 1.,
                                                                }),
                                                        )
                                                        .child(
                                                            rect()
                                                                .width(Size::fill())
                                                                .height(Size::px(32.))
                                                                .padding(4.)
                                                                .background(c.surface_secondary)
                                                                .content(Content::Flex)
                                                                .horizontal()
                                                                .cross_align(Alignment::Center)
                                                                .main_align(Alignment::SpaceBetween)
                                                                .child(
                                                                    label()
                                                                        .text(if language.is_empty() {
                                                                            "CODE".to_string()
                                                                        } else {
                                                                            language.to_uppercase()
                                                                        })
                                                                        .font_size(11.)
                                                                        .font_weight(FontWeight::BOLD)
                                                                        .color(c.text_secondary),
                                                                )
                                                                .child(
                                                                    Button::new()
                                                                        .background(c.surface_tertiary)
                                                                        .hover_background(c.tertiary)
                                                                        .border_fill(Color::TRANSPARENT)
                                                                        .color(c.text_secondary)
                                                                        .on_press({
                                                                            let code_to_copy = code.clone();
                                                                            move |_| {
                                                                                let _ = Clipboard::set(code_to_copy.clone());
                                                                            }
                                                                        })
                                                                        .child(
                                                                            rect()
                                                                                .horizontal()
                                                                                .cross_align(Alignment::Center)
                                                                                .spacing(4.)
                                                                                .child(label().text("📋").font_size(11.))
                                                                                .child(label().text("Copy Code").font_size(11.)),
                                                                        ),
                                                                ),
                                                        )
                                                        .child(
                                                            rect()
                                                                .width(Size::fill())
                                                                .padding(8.)
                                                                .child(
                                                                    SelectableText::new()
                                                                        .span(code)
                                                                        .color(c.text_primary)
                                                                        .font_family("Jetbrains Mono")
                                                                        .into_element(),
                                                                ),
                                                        )
                                                        .into_element()
                                                }
                                            }),
                                    )
                                    .into_element()
                            }),
                    )
            }),
        )),
    );

    // Input area
    let input_area = rect()
        .width(Size::fill())
        .height(Size::px(60.))
        .padding(12.)
        .child(
            rect()
                .horizontal()
                .expanded()
                .cross_align(Alignment::Center)
                .spacing(8.)
                .content(Content::Flex)
                .child(
                    Input::new(input_value)
                        .background(c.surface_tertiary)
                        .focus_background(c.tertiary)
                        .border_fill(Color::TRANSPARENT)
                        .color(c.text_secondary)
                        .placeholder("Type your message...")
                        .width(Size::flex(1.))
                        .on_submit(on_submit),
                )
                .child(
                    Button::new()
                        .background(c.surface_tertiary)
                        .hover_background(c.tertiary)
                        .border_fill(Color::TRANSPARENT)
                        .color(c.text_secondary)
                        .on_press(send_message)
                        .child("Send"),
                ),
        );

    let chat_panel = rect()
        .expanded()
        .content(Content::Flex)
        .background(c.background)
        .child(chat_area)
        .child(input_area);

    // Execute button for code (styled with a green play triangle icon)
    let execute_button = Button::new()
        .background(c.surface_tertiary)
        .hover_background(c.tertiary)
        .border_fill(Color::TRANSPARENT)
        .color(c.text_secondary)
        .on_press(execute_code)
        .child(
            rect()
                .horizontal()
                .cross_align(Alignment::Center)
                .spacing(6.)
                .child(
                    label()
                        .text("▶")
                        .font_size(14.)
                        .font_weight(FontWeight::BOLD)
                        .color(Color::from_rgb(34, 197, 94)),
                )
                .child(
                    label()
                        .text("Execute Code")
                        .font_size(13.)
                        .color(c.text_secondary),
                ),
        );

    // Toolbar
    let toolbar = rect()
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
        .content(Content::Flex)
        .horizontal()
        .cross_align(Alignment::Center)
        .main_align(Alignment::SpaceBetween)
        .child(
            label()
                .text("Coding Assistant")
                .color(c.text_primary)
                .font_size(16.)
                .font_weight(FontWeight::BOLD),
        )
        .child(
            rect()
                .horizontal()
                .cross_align(Alignment::Center)
                .spacing(8.)
                .child(
                    Button::new()
                        .background(c.surface_tertiary)
                        .hover_background(c.tertiary)
                        .border_fill(Color::TRANSPARENT)
                        .color(c.text_secondary)
                        .on_press({
                            let mut show_settings = show_settings;
                            move |_| {
                                *show_settings.write() = true;
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
                        .on_press(clear_chat)
                        .child("Clear Chat"),
                )
                .child(
                    Button::new()
                        .background(c.surface_tertiary)
                        .hover_background(c.tertiary)
                        .border_fill(Color::TRANSPARENT)
                        .color(c.text_secondary)
                        .on_press(reset_terminal)
                        .child("Reset Terminal"),
                )
                .child(execute_button),
        );

    rect()
        .expanded()
        .background(c.background)
        .content(Content::Flex)
        .child(toolbar)
        .child(
            rect()
                .expanded()
                .child(
                    ResizableContainer::new()
                        .direction(Direction::Horizontal)
                        .panel(
                            ResizablePanel::new(PanelSize::percent(20.)).child(
                                file_tree_panel(current_dir.into(), expanded_dirs.into()),
                            ),
                        )
                        .panel(
                            ResizablePanel::new(PanelSize::percent(30.)).child(chat_panel),
                        )
                        .panel(
                            ResizablePanel::new(PanelSize::percent(50.)).child(
                                // The editor and terminal are stacked vertically
                                // (editor on top, terminal below) so the terminal
                                // sits directly beneath the code editor, while the
                                // chat panel keeps the full height of the window.
                                ResizableContainer::new()
                                    .direction(Direction::Vertical)
                                    .panel(ResizablePanel::new(PanelSize::percent(50.)).child(
                                        code_editor_panel(editor.into(), file_name.read().clone()),
                                    ))
                                    .panel(
                                        ResizablePanel::new(PanelSize::percent(50.)).child(
                                            terminal_panel(
                                                terminal_handle.into_writable(),
                                                current_dir.into_writable(),
                                            ),
                                        ),
                                    ),
                            ),
                        ),
                )
               .child(if *show_settings.read() {
                    settings_panel(
                        c.clone(),
                        settings_key_input.into(),
                        settings_feedback.into(),
                        save_api_key,
                        close_settings,
                    )
                    .into_element()
                } else {
                    rect()
                        .layer(Layer::Overlay)
                        .width(Size::px(0.))
                        .height(Size::px(0.))
                        .into_element()
                }),
        )
}

/// A modal settings panel for configuring the Albert API key.
fn settings_panel<H1, H2>(
    c: ColorsSheet,
    key_input: Writable<String>,
    feedback: Writable<String>,
    on_save: H1,
    on_close: H2,
) -> impl IntoElement
where
    H1: Into<EventHandler<Event<PressEventData>>>,
    H2: Into<EventHandler<Event<PressEventData>>>,
{
    rect()
        .layer(Layer::Overlay)
        .position(Position::new_absolute().top(0.).left(0.))
        .width(Size::fill())
        .height(Size::fill())
        .background(c.overlay)
        .content(Content::Flex)
        .cross_align(Alignment::Center)
        .main_align(Alignment::Center)
        .child(
            rect()
                .width(Size::px(480.))
                .padding(24.)
                .background(c.surface_primary)
                .corner_radius(12.)
                .shadow(Shadow::new().x(0.).y(4.).blur(20.).color(c.shadow))
                .spacing(12.)

                .child(
                    label()
                        .text("Settings")
                        .color(c.text_primary)
                        .font_size(18.)
                        .font_weight(FontWeight::BOLD),
                )
                .child(
                    label()
                        .text("Albert API Key")
                        .color(c.text_secondary)
                        .font_size(13.),
                )
                .child(
                    Input::new(key_input)
                        .background(c.surface_secondary)
                        .focus_background(c.surface_tertiary)
                        .border_fill(Color::TRANSPARENT)
                        .color(c.text_inverse)
                        .placeholder("sk-...")
                        .width(Size::fill()),
                )
                .child(
                    label()
                        .text(format!(
                            "Saved to: {}",
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
                    rect()
                        .width(Size::px(0.))
                        .height(Size::px(0.))
                        .into_element()
                })
                .child(
                    rect()
                        .horizontal()
                        .cross_align(Alignment::Center)
                        .spacing(8.)
                        .child(
                            Button::new()
                                .background(c.surface_tertiary)
                                .hover_background(c.tertiary)
                                .border_fill(Color::TRANSPARENT)
                                .color(c.text_inverse)
                                .on_press(on_save)
                                .child("Save"),
                        )
                        .child(
                            Button::new()
                                .background(c.surface_tertiary)
                                .hover_background(c.tertiary)
                                .border_fill(Color::TRANSPARENT)
                                .color(c.text_inverse)
                                .on_press(on_close)
                                .child("Close"),
                        ),
                ),
        )
}

/// The left sidebar file-tree panel.
///
/// It shows the folders and files of the terminal's current working directory
/// as an expandable tree, and follows the directory as it changes (e.g. when
/// the user runs `cd` in the terminal). The tree is rebuilt from scratch on
/// every render, so it always reflects the latest directory contents.
fn file_tree_panel(
    current_dir: Readable<std::path::PathBuf>,
    expanded: Writable<std::collections::HashSet<std::path::PathBuf>>,
) -> impl IntoElement {
    let dir = current_dir.read().clone();
    let c = use_theme().read().colors.clone();
    let rows = build_tree_rows(&dir, 0, expanded.clone(), c.clone());

    rect()
        .expanded()
        .content(Content::Flex)
        .background(c.background)
        .child(
            rect()
                .width(Size::fill())
                .height(Size::px(32.))
                .padding(8.)
                .background(c.surface_primary)
                .border(Border::new().fill(c.border).width(BorderWidth {
                    top: 0.,
                    right: 1.,
                    bottom: 1.,
                    left: 0.,
                }))
                .horizontal()
                .cross_align(Alignment::Center)
                .child(
                    label()
                        .text(file_tree::display_name(&dir))
                        .color(c.text_primary)
                        .font_size(13.)
                        .font_weight(FontWeight::BOLD),
                ),
        )
        .child(
            rect()
                .expanded()
                .padding(6.)
                .child(ScrollView::new().child(rect().width(Size::fill()).children(rows))),
        )
}

/// Recursively build the tree rows for a directory.
///
/// Each directory is rendered as a row with an expand/collapse toggle; when
/// expanded, its children are rendered beneath it with extra indentation.
/// Files are rendered as plain rows. The `expanded` set tracks which
/// directories are currently open, keyed by absolute path.
fn build_tree_rows(
    dir: &std::path::Path,
    depth: usize,
    expanded: Writable<std::collections::HashSet<std::path::PathBuf>>,
    colors: ColorsSheet,
) -> Vec<Element> {
    let mut rows = Vec::new();
    for entry in file_tree::list_directory(dir) {
        let indent = depth as f32 * 14.;
        if entry.is_dir {
            let is_expanded = expanded.read().contains(&entry.path);
            let toggle = {
                let mut expanded = expanded.clone();
                let path = entry.path.clone();
                move |_| {
                    if expanded.read().contains(&path) {
                        expanded.write().remove(&path);
                    } else {
                        expanded.write().insert(path.clone());
                    }
                }
            };
            rows.push(
                rect()
                    .width(Size::fill())
                    .padding(Gaps::new(2., 4., 2., indent))
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(4.)
                    .child(
                        Button::new()
                            .background(Color::TRANSPARENT)
                            .hover_background(colors.surface_primary)
                            .border_fill(Color::TRANSPARENT)
                            .color(colors.text_secondary)
                            .on_press(toggle)
                            .child(if is_expanded { "▾" } else { "▸" }),
                    )
                    .child(label().text(entry.name.clone()).color(colors.text_inverse))
                    .into_element(),
            );
            if is_expanded {
                rows.extend(build_tree_rows(&entry.path, depth + 1, expanded.clone(), colors.clone()));
            }
        } else {
            rows.push(
                rect()
                    .width(Size::fill())
                    .padding(Gaps::new(2., 4., 2., indent + 14.))
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(4.)
                    .child(label().text("•").color(colors.text_placeholder))
                    .child(label().text(entry.name.clone()).color(colors.text_highlight))
                    .into_element(),
            );
        }
    }
    rows
}

fn terminal_panel(
    handle: Writable<Option<TerminalHandle>>,
    current_dir: Writable<std::path::PathBuf>,
) -> impl IntoElement {
    let handle_for_future = handle.clone();
    let current_dir_for_future = current_dir.clone();
    use_future(move || {
        let mut handle_for_future = handle_for_future.clone();
        let mut current_dir_for_future = current_dir_for_future.clone();
        async move {
            loop {
                let current_handle = handle_for_future.read().clone();
                let Some(terminal_handle) = current_handle else {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                };

                if let Some(cwd) = terminal_handle.cwd() {
                    if *current_dir_for_future.read() != cwd {
                        *current_dir_for_future.write() = cwd;
                    }
                }

                let mut closed = terminal_handle.closed().fuse();
                let mut clipboard = terminal_handle.clipboard_changed().fuse();
                let mut output = terminal_handle.output_received().fuse();

                loop {
                    let active_handle = handle_for_future.read().clone();
                    let is_same = match (&active_handle, &terminal_handle) {
                        (Some(h1), h2) => h1.id() == h2.id(),
                        _ => false,
                    };
                    if !is_same {
                        break;
                    }

                    let timer = tokio::time::sleep(std::time::Duration::from_millis(300)).fuse();
                    futures_util::pin_mut!(timer);

                    futures_util::select! {
                        _ = &mut timer => {
                            if let Some(cwd) = terminal_handle.cwd()
                                && *current_dir_for_future.read() != cwd
                            {
                                *current_dir_for_future.write() = cwd;
                            }
                        }
                        _ = &mut closed => {
                            let active_handle = handle_for_future.read().clone();
                            let is_same = match (&active_handle, &terminal_handle) {
                                (Some(h1), h2) => h1.id() == h2.id(),
                                _ => false,
                            };
                            if is_same {
                                *handle_for_future.write() = spawn_terminal();
                            }
                            break;
                        }
                        _ = &mut clipboard => {
                            if let Some(text) = terminal_handle.clipboard_content() {
                                let _ = Clipboard::set(text);
                            }
                            clipboard = terminal_handle.clipboard_changed().fuse();
                        }
                        _ = &mut output => {
                            if let Some(cwd) = terminal_handle.cwd()
                                && *current_dir_for_future.read() != cwd
                            {
                                *current_dir_for_future.write() = cwd;
                            }
                            output = terminal_handle.output_received().fuse();
                        }
                    }
                }
            }
        }
    });

    let a11y_id = use_a11y();
    let c = use_theme().read().colors.clone();
    let focus = use_focus(a11y_id);
    let mut dimensions = use_state(|| (0.0, 0.0));
    let mut click_origin = use_state(|| None::<(usize, usize)>);

    let handle_for_side_effect = handle.clone();
    use_side_effect(move || {
        let focused = *Platform::get().is_app_focused.read() && focus().is_focused();
        if let Some(handle) = handle_for_side_effect.read().clone() {
            handle.focus_changed(focused);
        }
    });

    rect()
        .expanded()
        .center()
        .background(c.background)
        .color(c.text_primary)
        .child(if let Some(handle) = handle.read().clone() {
            rect()
                .child(
                    Terminal::new(handle.clone())
                        .on_measured(move |(char_width, line_height)| {
                            dimensions.set((char_width, line_height));
                        })
                        .on_mouse_down({
                            let handle = handle.clone();
                            move |e: Event<MouseEventData>| {
                                a11y_id.request_focus();
                                let (char_width, line_height) = dimensions();
                                let col = (e.element_location.x / char_width as f64) as f32;
                                let row = (e.element_location.y / line_height as f64) as f32;
                                click_origin.set(Some((row as usize, col as usize)));
                                let button = match e.button {
                                    Some(MouseButton::Middle) => TerminalMouseButton::Middle,
                                    Some(MouseButton::Right) => TerminalMouseButton::Right,
                                    _ => TerminalMouseButton::Left,
                                };
                                let selection_type = match EventsCombos::pressed(e.element_location)
                                {
                                    PressEventType::Double => SelectionType::Semantic,
                                    PressEventType::Triple => SelectionType::Lines,
                                    _ => SelectionType::Simple,
                                };
                                handle.mouse_down(row, col, button, selection_type);
                            }
                        })
                        .on_mouse_move({
                            let handle = handle.clone();
                            move |e: Event<MouseEventData>| {
                                let (char_width, line_height) = dimensions();
                                let col = (e.element_location.x / char_width as f64) as f32;
                                let row = (e.element_location.y / line_height as f64) as f32;
                                handle.mouse_move(row, col);
                            }
                        })
                        .on_mouse_up({
                            let handle = handle.clone();
                            move |e: Event<MouseEventData>| {
                                let (char_width, line_height) = dimensions();
                                let col = (e.element_location.x / char_width as f64) as f32;
                                let row = (e.element_location.y / line_height as f64) as f32;
                                let button = match e.button {
                                    Some(MouseButton::Middle) => TerminalMouseButton::Middle,
                                    Some(MouseButton::Right) => TerminalMouseButton::Right,
                                    _ => TerminalMouseButton::Left,
                                };
                                handle.mouse_up(row, col, button);
                                let origin = click_origin();
                                click_origin.set(None);
                                if button == TerminalMouseButton::Left
                                    && origin == Some((row as usize, col as usize))
                                    && let Some(url) = handle.hyperlink_at(row, col)
                                {
                                    let _ = open::that(url);
                                }
                            }
                        })
                        .on_global_pointer_press({
                            let handle = handle.clone();
                            move |_: Event<PointerEventData>| {
                                handle.release();
                            }
                        })
                        .on_wheel({
                            let handle = handle.clone();
                            move |e: Event<WheelEventData>| {
                                let (char_width, line_height) = dimensions();
                                let (mouse_x, mouse_y) = e.element_location.to_tuple();
                                let col = (mouse_x / char_width as f64) as f32;
                                let row = (mouse_y / line_height as f64) as f32;
                                handle.wheel(e.delta_y, row, col);
                            }
                        })
                        .a11y_id(a11y_id)
                        .a11y_role(AccessibilityRole::Terminal)
                        .a11y_auto_focus(true)
                        .on_key_up({
                            let handle = handle.clone();
                            move |e: Event<KeyboardEventData>| {
                                if e.key == Key::Named(NamedKey::Shift) {
                                    handle.shift_pressed(false);
                                }
                            }
                        })
                        .on_key_down(move |e: Event<KeyboardEventData>| {
                            let ctrl_shift =
                                e.modifiers.contains(Modifiers::CONTROL | Modifiers::SHIFT);

                            match &e.key {
                                Key::Character(ch)
                                    if ctrl_shift && ch.eq_ignore_ascii_case("c") =>
                                {
                                    if let Some(text) = handle.get_selected_text() {
                                        let _ = Clipboard::set(text);
                                    }
                                }
                                Key::Character(ch)
                                    if ctrl_shift && ch.eq_ignore_ascii_case("v") =>
                                {
                                    if let Ok(text) = Clipboard::get() {
                                        let _ = handle.paste(&text);
                                    }
                                }
                                _ => {
                                    let _ = handle.write_key(&e.key, e.modifiers);
                                }
                            }
                        }),
                )
                .expanded()
                .background(c.surface_secondary)
                .padding(6.)
                .into_element()
        } else {
            "Terminal exited".into_element()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // should_send_message()
    //
    // This is the shared empty-check used by both the Send button and the
    // Enter key, so it must reject empty/whitespace-only messages and accept
    // anything else.
    // ------------------------------------------------------------------

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
        // Whitespace around real content is fine; only all-whitespace is ignored.
        assert!(should_send_message("  hello  "));
    }

    // ------------------------------------------------------------------
    // SupportedLanguage::detect()
    // ------------------------------------------------------------------

    #[test]
    fn detect_defaults_to_python() {
        // Anything that doesn't match a known keyword falls back to Python.
        assert_eq!(
            SupportedLanguage::detect("hello there"),
            SupportedLanguage::Python
        );
        assert_eq!(SupportedLanguage::detect(""), SupportedLanguage::Python);
        assert_eq!(
            SupportedLanguage::detect("write a program"),
            SupportedLanguage::Python
        );
    }

    #[test]
    fn detect_python_explicitly() {
        assert_eq!(
            SupportedLanguage::detect("write python code"),
            SupportedLanguage::Python
        );
        assert_eq!(
            SupportedLanguage::detect("a python script"),
            SupportedLanguage::Python
        );
    }

    #[test]
    fn detect_javascript() {
        assert_eq!(
            SupportedLanguage::detect("write javascript"),
            SupportedLanguage::JavaScript
        );
        assert_eq!(
            SupportedLanguage::detect("a js function"),
            SupportedLanguage::JavaScript
        );
        assert_eq!(
            SupportedLanguage::detect("node script"),
            SupportedLanguage::JavaScript
        );
    }

    #[test]
    fn detect_typescript() {
        assert_eq!(
            SupportedLanguage::detect("write typescript"),
            SupportedLanguage::TypeScript
        );
        assert_eq!(
            SupportedLanguage::detect("ts code"),
            SupportedLanguage::TypeScript
        );
    }

    #[test]
    fn detect_html_and_css() {
        assert_eq!(
            SupportedLanguage::detect("make an html page"),
            SupportedLanguage::Html
        );
        assert_eq!(
            SupportedLanguage::detect("style with css"),
            SupportedLanguage::Css
        );
    }

    #[test]
    fn detect_c_family() {
        assert_eq!(
            SupportedLanguage::detect("write c++ code"),
            SupportedLanguage::Cpp
        );
        assert_eq!(
            SupportedLanguage::detect("cpp program"),
            SupportedLanguage::Cpp
        );
        assert_eq!(
            SupportedLanguage::detect("cplusplus"),
            SupportedLanguage::Cpp
        );
        assert_eq!(
            SupportedLanguage::detect("write c code"),
            SupportedLanguage::C
        );
    }

    #[test]
    fn detect_go_java_rust() {
        assert_eq!(
            SupportedLanguage::detect("golang program"),
            SupportedLanguage::Go
        );
        assert_eq!(SupportedLanguage::detect("go code"), SupportedLanguage::Go);
        assert_eq!(
            SupportedLanguage::detect("java class"),
            SupportedLanguage::Java
        );
        assert_eq!(
            SupportedLanguage::detect("rust code"),
            SupportedLanguage::Rust
        );
        assert_eq!(
            SupportedLanguage::detect("rs module"),
            SupportedLanguage::Rust
        );
    }

    #[test]
    fn detect_is_case_insensitive() {
        assert_eq!(
            SupportedLanguage::detect("WRITE JAVASCRIPT"),
            SupportedLanguage::JavaScript
        );
        assert_eq!(SupportedLanguage::detect("Rust"), SupportedLanguage::Rust);
    }

    // ------------------------------------------------------------------
    // SupportedLanguage::extension() / flow::derive_file_name()
    // ------------------------------------------------------------------

    #[test]
    fn extensions_are_correct() {
        assert_eq!(SupportedLanguage::Python.extension(), "py");
        assert_eq!(SupportedLanguage::Rust.extension(), "rs");
        assert_eq!(SupportedLanguage::JavaScript.extension(), "js");
        assert_eq!(SupportedLanguage::TypeScript.extension(), "ts");
        assert_eq!(SupportedLanguage::Html.extension(), "html");
        assert_eq!(SupportedLanguage::Css.extension(), "css");
        assert_eq!(SupportedLanguage::C.extension(), "c");
        assert_eq!(SupportedLanguage::Cpp.extension(), "cpp");
        assert_eq!(SupportedLanguage::Java.extension(), "java");
        assert_eq!(SupportedLanguage::Go.extension(), "go");
    }

    #[test]
    fn file_names_use_extension() {
        // With no meaningful name derivable from the content, the file name
        // falls back to the conventional `main.<ext>`.
        assert_eq!(
            flow::derive_file_name("\n", SupportedLanguage::Python),
            "main.py"
        );
        assert_eq!(
            flow::derive_file_name("\n", SupportedLanguage::Rust),
            "main.rs"
        );
        assert_eq!(
            flow::derive_file_name("\n", SupportedLanguage::JavaScript),
            "main.js"
        );
        assert_eq!(
            flow::derive_file_name("\n", SupportedLanguage::Go),
            "main.go"
        );
    }

    /// Java's header label must be `Main.java` so it matches the temp file
    /// written before execution (the compiler requires the public class name
    /// to match the file name).
    #[test]
    fn java_file_name_is_capital_main() {
        assert_eq!(
            flow::derive_file_name("public class Foo {}\n", SupportedLanguage::Java),
            "Main.java"
        );
    }

    // ------------------------------------------------------------------
    // SupportedLanguage::run_command()
    // ------------------------------------------------------------------

    #[test]
    fn run_command_python_uses_python_interpreter() {
        let cmd = SupportedLanguage::Python.run_command(std::path::Path::new("/tmp/main.py"));
        assert!(cmd.contains(platform::python_command()));
        assert!(cmd.contains("/tmp/main.py"));
    }

    #[test]
    fn run_command_rust_compiles_then_runs() {
        let cmd = SupportedLanguage::Rust.run_command(std::path::Path::new("/tmp/main.rs"));
        assert!(cmd.contains(platform::rust_compiler()));
        assert!(cmd.contains("-o"));
        assert!(cmd.contains("&&"));
    }

    #[test]
    fn run_command_javascript_uses_node() {
        let cmd = SupportedLanguage::JavaScript.run_command(std::path::Path::new("/tmp/main.js"));
        assert!(cmd.contains(platform::node_runner()));
        assert!(cmd.contains("/tmp/main.js"));
    }

    #[test]
    fn run_command_typescript_uses_ts_runner() {
        let cmd = SupportedLanguage::TypeScript.run_command(std::path::Path::new("/tmp/main.ts"));
        assert!(cmd.contains(platform::ts_runner()));
    }

    #[test]
    fn run_command_css_is_noop_message() {
        let cmd = SupportedLanguage::Css.run_command(std::path::Path::new("/tmp/main.css"));
        assert!(cmd.contains("nothing to run"));
    }

    #[test]
    fn run_command_go_uses_go_run() {
        let cmd = SupportedLanguage::Go.run_command(std::path::Path::new("/tmp/main.go"));
        assert!(cmd.contains(platform::go_runner()));
        assert!(cmd.contains("run"));
    }

    #[test]
    fn run_command_java_compiles_and_runs_main() {
        let cmd = SupportedLanguage::Java.run_command(std::path::Path::new("/tmp/main.java"));
        assert!(cmd.contains(platform::java_compiler()));
        assert!(cmd.contains(platform::java_runtime()));
        assert!(cmd.contains("Main"));
    }

    #[test]
    fn run_command_c_and_cpp_compile_then_run() {
        let c_cmd = SupportedLanguage::C.run_command(std::path::Path::new("/tmp/main.c"));
        assert!(c_cmd.contains(platform::c_compiler()));
        assert!(c_cmd.contains("-o"));

        let cpp_cmd = SupportedLanguage::Cpp.run_command(std::path::Path::new("/tmp/main.cpp"));
        assert!(cpp_cmd.contains(platform::cpp_compiler()));
        assert!(cpp_cmd.contains("-o"));
    }
}
