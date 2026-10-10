#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod api;
mod config;
mod diff;
mod file_tree;
mod flow;
mod mcp;
mod myers;
mod platform;
mod theme;
mod tools;

use freya::{clipboard::Clipboard, code_editor::*, prelude::*, terminal::*};
use futures_util::FutureExt;
use ropey::Rope;
use tokio::runtime::Builder;

// Albert API configuration (French government AI service)
const ALBERT_ENDPOINT: &str = "https://albert.api.etalab.gouv.fr/v1";
const ALBERT_MODEL: &str = "deepseek-v4-flash";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    AI,
    User,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

/// A programming language supported by the editor. Each language knows its
/// tree-sitter grammar, highlights query, file extension and how to run it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SupportedLanguage {
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
    /// Detects supported language from a file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "rs" => Some(SupportedLanguage::Rust),
            "py" => Some(SupportedLanguage::Python),
            "js" | "mjs" | "cjs" => Some(SupportedLanguage::JavaScript),
            "ts" | "tsx" => Some(SupportedLanguage::TypeScript),
            "html" | "htm" => Some(SupportedLanguage::Html),
            "css" => Some(SupportedLanguage::Css),
            "c" | "h" => Some(SupportedLanguage::C),
            "cpp" | "cc" | "cxx" | "hpp" => Some(SupportedLanguage::Cpp),
            "java" => Some(SupportedLanguage::Java),
            "go" => Some(SupportedLanguage::Go),
            _ => None,
        }
    }

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
        // Use the platform temp directory for compiled binaries so the
        // command works on Windows, macOS, and Linux alike.
        let temp_dir = std::env::temp_dir();
        let bin = |name: &str| temp_dir.join(name).display().to_string();
        match self {
            SupportedLanguage::Python => {
                format!("{} {}\n", platform::python_command(), file)
            }
            SupportedLanguage::Rust => {
                let out = bin("main_rs");
                format!(
                    "{} {} -o {} && {}\n",
                    platform::rust_compiler(),
                    file,
                    out,
                    out
                )
            }
            SupportedLanguage::JavaScript => {
                format!("{} {}\n", platform::node_runner(), file)
            }
            SupportedLanguage::TypeScript => {
                format!("{} {}\n", platform::ts_runner(), file)
            }
            SupportedLanguage::Html => platform::open_command(&file),
            SupportedLanguage::Css => "echo 'CSS is a stylesheet, nothing to run.'\n".to_string(),
            SupportedLanguage::C => {
                let out = bin("main_c");
                format!(
                    "{} {} -o {} && {}\n",
                    platform::c_compiler(),
                    file,
                    out,
                    out
                )
            }
            SupportedLanguage::Cpp => {
                let out = bin("main_cpp");
                format!(
                    "{} {} -o {} && {}\n",
                    platform::cpp_compiler(),
                    file,
                    out,
                    out
                )
            }
            SupportedLanguage::Java => {
                // Java requires the file name to match the public class name.
                format!(
                    "{} {} && {} Main\n",
                    platform::java_compiler(),
                    file,
                    platform::java_runtime()
                )
            }
            SupportedLanguage::Go => {
                format!("{} run {}\n", platform::go_runner(), file)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CenterViewMode {
    Editor,
    Diff,
}

/// Builds the code editor pane. The editor state is lifted up into `app()` so
/// that the chat and terminal panels can read/write the same content.
fn code_editor_panel(
    editor: Writable<CodeEditorData>,
    file_name: String,
    has_pending_diffs: usize,
    mut on_view_diff: impl FnMut() + 'static,
    save_button: impl IntoElement,
    execute_button: impl IntoElement,
    a11y_id: AccessibilityId,
    c: ColorsSheet,
) -> impl IntoElement {
    rect()
        .expanded()
        .content(Content::Flex)
        .background(c.background)
        .child(
            rect()
                .width(Size::fill())
                .height(Size::px(36.))
                .padding(Gaps::new(4., 8., 4., 8.))
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
                    rect()
                        .expanded()
                        .horizontal()
                        .cross_align(Alignment::Center)
                        .spacing(8.)
                        .child(
                            label()
                                .text(file_name)
                                .color(c.text_primary)
                                .font_size(13.)
                                .font_weight(FontWeight::BOLD),
                        )
                        .child(if has_pending_diffs > 0 {
                            Button::new()
                                .background(Color::from_rgb(234, 88, 12))
                                .hover_background(Color::from_rgb(194, 65, 12))
                                .border_fill(Color::TRANSPARENT)
                                .color(Color::WHITE)
                                .on_press(move |_| on_view_diff())
                                .child(format!("📝 Diff en attente ({})", has_pending_diffs))
                                .into_element()
                        } else {
                            rect().width(Size::px(0.)).height(Size::px(0.)).into_element()
                        }),
                )
                .child(
                    rect()
                        .horizontal()
                        .cross_align(Alignment::Center)
                        .spacing(6.)
                        .child(save_button)
                        .child(execute_button),
                ),
        )
        .child(
            rect()
                .expanded()
                .padding(Gaps::new(6., 6., 6., 0.))
                .child(CodeEditor::new(editor, a11y_id).background(c.surface_secondary)),
        )
}

/// A dedicated diff preview panel for user review before accepting or rejecting file modifications.
fn diff_viewer_panel(
    pending_diffs: Writable<Vec<diff::PendingDiff>>,
    mut on_accept: impl FnMut(usize) + 'static,
    mut on_reject: impl FnMut(usize) + 'static,
    mut on_reject_all: impl FnMut() + 'static,
    mut on_switch_to_editor: impl FnMut() + 'static,
    colors: ColorsSheet,
) -> impl IntoElement {
    let diffs = pending_diffs.read().clone();
    if diffs.is_empty() {
        return rect()
            .expanded()
            .center()
            .background(colors.background)
            .child(label().text("Aucun diff en attente.").color(colors.text_secondary))
            .into_element();
    }

    let diff = &diffs[0];
    let diff_path = diff.path.clone();
    let diff_count = diffs.len();

    let mut added_lines = 0;
    let mut deleted_lines = 0;
    for l in &diff.diff_lines {
        match l {
            diff::DiffLine::Added(_) => added_lines += 1,
            diff::DiffLine::Deleted(_) => deleted_lines += 1,
            diff::DiffLine::Unchanged(_) => {}
        }
    }

    let accept_handler = move |_| {
        on_accept(0);
    };

    let reject_handler = move |_| {
        on_reject(0);
    };

    let reject_all_handler = move |_| {
        on_reject_all();
    };

    let switch_handler = move |_| {
        on_switch_to_editor();
    };

    let mut line_elements = Vec::new();
    for d_line in &diff.diff_lines {
        match d_line {
            diff::DiffLine::Deleted(text) => {
                line_elements.push(
                    rect()
                        .width(Size::fill())
                        .padding(Gaps::new(2., 10., 2., 10.))
                        .background(Color::from_argb(45, 239, 68, 68))
                        .child(
                            label()
                                .font_family("Jetbrains Mono")
                                .font_size(12.5)
                                .text(format!("- {}", text))
                                .color(Color::from_rgb(248, 113, 113)),
                        )
                        .into_element(),
                );
            }
            diff::DiffLine::Added(text) => {
                line_elements.push(
                    rect()
                        .width(Size::fill())
                        .padding(Gaps::new(2., 10., 2., 10.))
                        .background(Color::from_argb(45, 34, 197, 94))
                        .child(
                            label()
                                .font_family("Jetbrains Mono")
                                .font_size(12.5)
                                .text(format!("+ {}", text))
                                .color(Color::from_rgb(74, 222, 128)),
                        )
                        .into_element(),
                );
            }
            diff::DiffLine::Unchanged(text) => {
                line_elements.push(
                    rect()
                        .width(Size::fill())
                        .padding(Gaps::new(2., 10., 2., 10.))
                        .child(
                            label()
                                .font_family("Jetbrains Mono")
                                .font_size(12.5)
                                .text(format!("  {}", text))
                                .color(colors.text_secondary),
                        )
                        .into_element(),
                );
            }
        }
    }

    rect()
        .expanded()
        .content(Content::Flex)
        .background(colors.background)
        .child(
            // Row 1: File navigation and stats bar
            rect()
                .width(Size::fill())
                .height(Size::px(38.))
                .padding(Gaps::new(4., 12., 4., 12.))
                .background(colors.surface_primary)
                .border(Border::new().fill(colors.border).width(BorderWidth {
                    top: 0.,
                    right: 0.,
                    bottom: 1.,
                    left: 0.,
                }))
                .horizontal()
                .cross_align(Alignment::Center)
                .child(
                    rect()
                        .expanded()
                        .horizontal()
                        .cross_align(Alignment::Center)
                        .spacing(8.)
                        .child(
                            label()
                                .text("📝 Revue :")
                                .color(colors.text_primary)
                                .font_size(13.)
                                .font_weight(FontWeight::BOLD),
                        )
                        .child(
                            rect()
                                .padding(Gaps::new(2., 8., 2., 8.))
                                .background(colors.surface_secondary)
                                .corner_radius(4.)
                                .border(Border::new().fill(colors.border).width(1.))
                                .child(
                                    label()
                                        .text(diff_path)
                                        .color(colors.text_highlight)
                                        .font_size(12.)
                                        .font_weight(FontWeight::BOLD),
                                ),
                        )
                        .child(
                            rect()
                                .padding(Gaps::new(2., 6., 2., 6.))
                                .background(Color::from_argb(35, 34, 197, 94))
                                .corner_radius(4.)
                                .child(
                                    label()
                                        .text(format!("+{}", added_lines))
                                        .color(Color::from_rgb(74, 222, 128))
                                        .font_size(12.)
                                        .font_weight(FontWeight::BOLD),
                                ),
                        )
                        .child(
                            rect()
                                .padding(Gaps::new(2., 6., 2., 6.))
                                .background(Color::from_argb(35, 239, 68, 68))
                                .corner_radius(4.)
                                .child(
                                    label()
                                        .text(format!("-{}", deleted_lines))
                                        .color(Color::from_rgb(248, 113, 113))
                                        .font_size(12.)
                                        .font_weight(FontWeight::BOLD),
                                ),
                        )
                        .child(if diff_count > 1 {
                            label()
                                .text(format!("(1/{} en attente)", diff_count))
                                .color(colors.text_secondary)
                                .font_size(12.)
                                .into_element()
                        } else {
                            rect().width(Size::px(0.)).height(Size::px(0.)).into_element()
                        }),
                )
                .child(
                    Button::new()
                        .background(colors.surface_tertiary)
                        .hover_background(colors.tertiary)
                        .border_fill(Color::TRANSPARENT)
                        .color(colors.text_primary)
                        .corner_radius(4.)
                        .on_press(switch_handler)
                        .child("← Retour Éditeur"),
                ),
        )
        .child(
            // Row 2: Dedicated prominent action banner (buttons are on the left, immediately visible)
            rect()
                .width(Size::fill())
                .height(Size::px(48.))
                .padding(Gaps::new(6., 16., 6., 16.))
                .background(colors.surface_secondary)
                .border(Border::new().fill(colors.border).width(BorderWidth {
                    top: 0.,
                    right: 0.,
                    bottom: 1.,
                    left: 0.,
                }))
                .horizontal()
                .cross_align(Alignment::Center)
                .spacing(12.)
                .child(
                    Button::new()
                        .background(Color::from_rgb(22, 163, 74))
                        .hover_background(Color::from_rgb(21, 128, 61))
                        .border_fill(Color::TRANSPARENT)
                        .color(Color::WHITE)
                        .corner_radius(6.)
                        .on_press(accept_handler)
                        .child("✓ Accepter la modification"),
                )
                .child(
                    Button::new()
                        .background(Color::from_rgb(220, 38, 38))
                        .hover_background(Color::from_rgb(185, 28, 28))
                        .border_fill(Color::TRANSPARENT)
                        .color(Color::WHITE)
                        .corner_radius(6.)
                        .on_press(reject_handler)
                        .child("✗ Rejeter la modification"),
                )
                .child(if diff_count > 1 {
                    Button::new()
                        .background(Color::from_rgb(127, 29, 29))
                        .hover_background(Color::from_rgb(153, 27, 27))
                        .border_fill(Color::TRANSPARENT)
                        .color(Color::WHITE)
                        .corner_radius(6.)
                        .on_press(reject_all_handler)
                        .child("✗ Tout rejeter")
                        .into_element()
                } else {
                    rect().width(Size::px(0.)).height(Size::px(0.)).into_element()
                })
                .child(
                    label()
                        .text("— Cliquez pour confirmer ou refuser l'écriture")
                        .color(colors.text_secondary)
                        .font_size(11.5),
                ),
        )
        .child(
            rect()
                .expanded()
                .padding(8.)
                .background(colors.surface_secondary)
                .child(
                    ScrollView::new().child(
                        rect().width(Size::fill()).children(line_elements),
                    ),
                ),
        )
        .child(
            rect()
                .width(Size::fill())
                .height(Size::px(32.))
                .padding(Gaps::new(4., 16., 4., 16.))
                .background(colors.surface_primary)
                .border(Border::new().fill(colors.border).width(BorderWidth {
                    top: 1.,
                    right: 0.,
                    bottom: 0.,
                    left: 0.,
                }))
                .horizontal()
                .cross_align(Alignment::Center)
                .child(
                    label()
                        .text("💡 Aucun fichier n'est modifié sur votre disque tant que vous ne cliquez pas sur 'Accepter la modification'.")
                        .color(colors.text_secondary)
                        .font_size(11.5),
                ),
        )
        .into_element()
}

/// Helper function to create a clean CodeEditorData without stale AST nodes from previous files.
fn create_editor_data(content: &str, language: Option<SupportedLanguage>) -> CodeEditorData {
    let rope = Rope::from_str(content);
    let editor_lang = language.map(|l| l.editor_language());
    let mut data = CodeEditorData::new(rope, editor_lang);
    data.set_theme(EditorSyntaxTheme::dark());
    data.parse();
    data.measure(14., "Jetbrains Mono");
    data
}

fn app() -> impl IntoElement {
    // Load the external theme (colors) from theme.json and provide it to the
    // whole component tree. This is the app's "stylesheet": the single source
    // of truth for the color palette. `c` is a shorthand for the color sheet.
    let theme = use_provide_theme(theme::load_theme);
    let c = theme.read().colors.clone();
    let editor_a11y_id = use_a11y();

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
    let pending_diffs = use_state(Vec::<diff::PendingDiff>::new);
    let center_view_mode = use_state(|| CenterViewMode::Editor);

    // Whether the settings panel is open.
    let show_settings = use_state(|| false);
    // The settings fields being edited in the settings panel.
    let saved_cfg = config::ApiKeyConfig::load();
    let settings_key_input = use_state(|| saved_cfg.key);
    let settings_model_input = use_state(|| saved_cfg.model.unwrap_or_else(|| ALBERT_MODEL.to_string()));
    let settings_endpoint_input = use_state(|| saved_cfg.endpoint.unwrap_or_else(|| ALBERT_ENDPOINT.to_string()));
    let available_models = use_state(Vec::<String>::new);
    let models_loading = use_state(|| false);
    // Feedback message shown in the settings panel after saving.
    let settings_feedback = use_state(String::new);

    // Automatically fetch models from endpoint at startup if API key is present
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
                if let Ok(m) = api::fetch_models(&ep, &k).await {
                    if !m.is_empty() {
                        *available_models.write() = m;
                    }
                }
                *models_loading.write() = false;
            }
        });
    }

    // The currently selected language. Defaults to Python.
    let current_language = use_state(|| SupportedLanguage::Python);

    // Shared editor state, lifted up so the chat and terminal panels can read
    // and write the exact same content that is displayed in the editor.
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

    // The file name shown in the editor header. This is shared state so it can
    // be updated whenever code is inserted (or the editor is cleared) and read
    // by `code_editor_panel`. It is derived from the script content so the
    // title reflects what was actually written, falling back to `main.<ext>`
    // conscience no meaningful name can be derived.
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
        move |_| {
            // Kill the current terminal (if any) and spawn a fresh one
            *terminal_handle.write() = spawn_terminal();
        }
    };

    // Refresh models from API endpoint
    let refresh_models = {
        let available_models = available_models;
        let models_loading = models_loading;
        let settings_feedback = settings_feedback;
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

    // Save the API key, model, and endpoint entered in the settings panel to the config file.
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
                endpoint: if endpoint.is_empty() { None } else { Some(endpoint) },
                source: config::KeySource::ConfigFile,
            };
            match cfg.validate() {
                Ok(()) => match cfg.save() {
                    Ok(()) => {
                        *settings_feedback.write() = "Paramètres enregistrés.".to_string();
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

    let accept_diff = {
        let mut pending_diffs = pending_diffs;
        let mut editor = editor;
        let mut current_language = current_language;
        let file_name = file_name;
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
                if let Err(e) = std::fs::write(&diff.path, &diff.new_content) {
                    messages.write().push(Message {
                        role: Role::AI,
                        content: format!("⚠️ Impossible d'écrire le fichier '{}': {}", diff.path, e),
                    });
                    return;
                }
                myers::log_myers_diff(&diff.path, &diff.old_content, &diff.new_content);

                let current_open = file_name.read().clone();
                if current_open == diff.path || current_open.ends_with(&diff.path) || diff.path.ends_with(&current_open) {
                    let ext = std::path::Path::new(&diff.path).extension().and_then(|e| e.to_str()).unwrap_or("");
                    let lang = SupportedLanguage::from_extension(ext);
                    if let Some(l) = lang {
                        *current_language.write() = l;
                    }
                    *editor.write() = create_editor_data(&diff.new_content, lang);
                }

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

    let reject_diff = {
        let mut pending_diffs = pending_diffs;
        let mut messages = messages;
        let mut center_view_mode = center_view_mode;
        let mut editor = editor;
        let mut current_language = current_language;
        let file_name = file_name;
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
                let current_open = file_name.read().clone();
                if current_open == diff.path || current_open.ends_with(&diff.path) || diff.path.ends_with(&current_open) {
                    let ext = std::path::Path::new(&diff.path).extension().and_then(|e| e.to_str()).unwrap_or("");
                    let lang = SupportedLanguage::from_extension(ext);
                    if let Some(l) = lang {
                        *current_language.write() = l;
                    }
                    *editor.write() = create_editor_data(&diff.old_content, lang);
                }

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
        let mut editor = editor;
        let mut current_language = current_language;
        let file_name = file_name;
        move || {
            let diffs = pending_diffs.write().drain(..).collect::<Vec<_>>();
            let current_open = file_name.read().clone();
            for diff in &diffs {
                if current_open == diff.path || current_open.ends_with(&diff.path) || diff.path.ends_with(&current_open) {
                    let ext = std::path::Path::new(&diff.path).extension().and_then(|e| e.to_str()).unwrap_or("");
                    let lang = SupportedLanguage::from_extension(ext);
                    if let Some(l) = lang {
                        *current_language.write() = l;
                    }
                    *editor.write() = create_editor_data(&diff.old_content, lang);
                }
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

    // Shared chat send logic. Both the Send button and the Enter key route
    // through this same code path so they behave identically.
    let send_text = {
        let mut messages = messages;
        let mut input_value = input_value;
        let mut editor = editor;
        let mut current_language = current_language;
        let mut file_name = file_name;
        let pending_diffs = pending_diffs;
        let center_view_mode = center_view_mode;
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

            // Check if the user wants to clear the editor.
            let wants_clear_editor = flow::wants_clear_editor(&user_message);
            let wants_code = flow::wants_code(&user_message);

            // Clear input
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

            let history_snapshot = messages.read().clone();
            let mut pending_diffs_stream = pending_diffs;
            let mut center_view_mode_stream = center_view_mode;
            let mut messages_stream = messages;

            spawn(async move {
                let api_key_config = config::ApiKeyConfig::load();
                if let Err(msg) = api_key_config.validate() {
                    messages_stream.write().push(Message {
                        role: Role::AI,
                        content: format!("⚠️ {}\n\nPlease configure your API key and try again.", msg),
                    });
                    return;
                }

                let endpoint_to_use = api_key_config.endpoint.clone().unwrap_or_else(|| ALBERT_ENDPOINT.to_string());
                let model_to_use = api_key_config.model.clone().unwrap_or_else(|| ALBERT_MODEL.to_string());

                // 1. Préparation de la bulle de chat pour le streaming
                messages_stream.write().push(Message {
                    role: Role::AI,
                    content: String::new(),
                });
                let ai_msg_idx = messages_stream.read().len() - 1;

                // 2. Exécution avec outils natifs, streaming, et diff callback
                let endpoint = endpoint_to_use;
                let api_key = api_key_config.key.clone();
                let model = model_to_use;
                let user_msg = user_message.clone();

                let result = api::prompt_with_retry(|| {
                    let endpoint = endpoint.clone();
                    let api_key = api_key.clone();
                    let model = model.clone();
                    let user_msg = user_msg.clone();
                    let history = history_snapshot.clone();
                    let mut messages_stream = messages_stream;
                    let mut pending_diffs_stream = pending_diffs_stream;
                    let mut center_view_mode_stream = center_view_mode_stream;

                    async move {
                        println!("[DEBUG] Prompt envoyé : {}", user_msg);
                        api::run_agent_loop_stream(
                            &endpoint,
                            &api_key,
                            &model,
                            "Tu es un assistant de programmation expert. Tu as accès aux outils natifs du projet (read_file, write_file, edit_file, list_directory, search_files, run_command). \
                             IMPORTANT : Ne liste et n'explore JAMAIS les répertoires `target/`, `.git/` ou `node_modules/`. \
                             Privilégie `list_directory` sur `src/` ou à la racine. \
                             Lorsque tu proposes d'éditer ou créer un fichier, un diff est automatiquement généré pour validation utilisateur avant écriture sur disque. \
                             Sois précis, concis et efficace.",
                            &history,
                            &user_msg,
                            |chunk| {
                                if let Some(msg) = messages_stream.write().get_mut(ai_msg_idx) {
                                    msg.content.push_str(chunk);
                                }
                            },
                            |diff| {
                                pending_diffs_stream.write().push(diff);
                                *center_view_mode_stream.write() = CenterViewMode::Diff;
                            },
                        )
                        .await
                    }
                })
                .await;

                match result {
                    Ok(response) => {
                        let final_response = if !pending_diffs_stream.read().is_empty() {
                            // An agent tool (write_file/edit_file) already generated a pending diff!
                            // Keep the response text and avoid generating duplicate diffs.
                            response
                        } else {
                            match flow::decide_editor_action(
                                &response,
                                wants_code,
                                detected_language,
                            ) {
                                flow::EditorAction::Insert { language, code } => {
                                    let target_file = flow::find_target_file(&user_message, &file_name.read());
                                    let display_name = target_file.clone().unwrap_or_else(|| file_name.read().clone());
                                    if let Some(ref path) = target_file {
                                        let old_code = std::fs::read_to_string(path).unwrap_or_default();
                                        let diff_lines = diff::computed_diff(&old_code, &code);
                                        let pd = diff::PendingDiff {
                                            id: format!(
                                                "diff_{}",
                                                std::time::SystemTime::now()
                                                    .duration_since(std::time::UNIX_EPOCH)
                                                    .unwrap_or_default()
                                                    .as_millis()
                                            ),
                                            path: path.clone(),
                                            old_content: old_code,
                                            new_content: code.clone(),
                                            diff_lines,
                                        };
                                        pending_diffs_stream.write().push(pd);
                                        *center_view_mode_stream.write() = CenterViewMode::Diff;
                                        flow::insertion_confirmation(language)
                                    } else {
                                        *current_language.write() = language;
                                        *editor.write() = create_editor_data(&code, Some(language));
                                        *file_name.write() = display_name;
                                        flow::insertion_confirmation(language)
                                    }
                                }
                                flow::EditorAction::ShowResponse => response,
                            }
                        };

                        if let Some(msg) = messages_stream.write().get_mut(ai_msg_idx) {
                            msg.content = final_response;
                        }
                    }
                    Err((_category, message)) => {
                        if let Some(msg) = messages_stream.write().get_mut(ai_msg_idx) {
                            msg.content = format!("⚠️ **Erreur lors de l'appel :**\n\n{}", message);
                        }
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

    // Save the current editor buffer to disk.
    let save_file = {
        let mut messages = messages;
        let file_name = file_name;
        let editor = editor;
        move |_| {
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
                            } else if msg.content.trim().is_empty() {
                                label()
                                    .text("⏳ En attente de l'assistant...")
                                    .color(c.text_placeholder)
                                    .into_element()
                            } else {
                                MarkdownViewer::new(msg.content.clone())
                                    .color(text_color)
                                    .into_element()
                            }),
                    )
            }),
        )),
    );

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
        let mut settings_model_input = settings_model_input;
        move |m| {
            let is_selected = m == active_model;
            let m_choice = m.clone();
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
                let mut available_models = available_models;
                let mut models_loading = models_loading;
                *models_loading.write() = true;
                spawn(async move {
                    if let Ok(m) = api::fetch_models(&ep, &k).await {
                        *available_models.write() = m;
                    }
                    *models_loading.write() = false;
                });
            }
        })
        .child(if is_models_loading { "⟳ ..." } else { "⟳" });

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
            // Row 1: Chat input and send button
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
                        .placeholder("Poser une question ou demander une action...")
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
                        .child("Envoyer"),
                ),
        )
        .child(
            // Row 2: Model select dropdown, refresh button and context label
            rect()
                .width(Size::fill())
                .height(Size::px(32.))
                .horizontal()
                .cross_align(Alignment::Center)
                .spacing(8.)
                .content(Content::Flex)
                .child(model_select)
                .child(refresh_models_button)
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

    let chat_panel = rect()
        .expanded()
        .content(Content::Flex)
        .background(c.background)
        .child(chat_area)
        .child(input_area);

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
            Button::new()
                .background(c.surface_tertiary)
                .hover_background(c.tertiary)
                .border_fill(Color::TRANSPARENT)
                .color(c.text_secondary)
                .on_press({
                    let mut show_settings = show_settings;
                    let available_models = available_models;
                    let models_loading = models_loading;
                    let ep_val = settings_endpoint_input.read().clone();
                    let key_val = settings_key_input.read().clone();
                    move |_| {
                        *show_settings.write() = true;
                        if available_models.read().is_empty() {
                            let ep = ep_val.trim().to_string();
                            let k = key_val.trim().to_string();
                            let mut available_models = available_models;
                            let mut models_loading = models_loading;
                            *models_loading.write() = true;
                            spawn(async move {
                                if let Ok(m) = api::fetch_models(&ep, &k).await {
                                    *available_models.write() = m;
                                }
                                *models_loading.write() = false;
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
        );

    // Save button for code editor header
    let save_button = Button::new()
        .background(c.surface_tertiary)
        .hover_background(c.tertiary)
        .border_fill(Color::TRANSPARENT)
        .color(c.text_secondary)
        .on_press(save_file)
        .child("💾 Sauvegarder");

    // Execute button for code editor header
    let execute_button = Button::new()
        .background(c.surface_tertiary)
        .hover_background(c.tertiary)
        .border_fill(Color::TRANSPARENT)
        .color(c.text_secondary)
        .on_press(execute_code)
        .child("Execute Code");

    let show_settings_val = *show_settings.read();

    let is_diff_view = *center_view_mode.read() == CenterViewMode::Diff && !pending_diffs.read().is_empty();
    let center_top_panel = if is_diff_view {
        diff_viewer_panel(
            pending_diffs.into(),
            accept_diff,
            reject_diff,
            reject_all_diffs,
            {
                let mut center_view_mode = center_view_mode.clone();
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
                let mut center_view_mode = center_view_mode.clone();
                move || {
                    *center_view_mode.write() = CenterViewMode::Diff;
                }
            },
            save_button,
            execute_button,
            editor_a11y_id,
            c.clone(),
        )
        .into_element()
    };

    let main_workspace = ResizableContainer::new()
        .direction(Direction::Horizontal)
        .panel(
            ResizablePanel::new(PanelSize::percent(18.)).child(
                file_tree_panel(
                    current_dir.into(),
                    expanded_dirs.into(),
                    c.clone(),
                    open_file.clone(),
                ),
            ),
        )
        .panel(
            ResizablePanel::new(PanelSize::percent(52.)).child(
                ResizableContainer::new()
                    .direction(Direction::Vertical)
                    .panel(ResizablePanel::new(PanelSize::percent(55.)).child(center_top_panel))
                    .panel(
                        ResizablePanel::new(PanelSize::percent(45.)).child(
                            terminal_panel(
                                terminal_handle.into_writable(),
                                current_dir.into_writable(),
                            ),
                        ),
                    ),
            ),
        )
        .panel(
            ResizablePanel::new(PanelSize::percent(30.)).child(
                rect()
                    .expanded()
                    .content(Content::Flex)
                    .child(chat_panel),
            ),
        );

    if show_settings_val {
        rect()
            .expanded()
            .background(c.background)
            .content(Content::Flex)
            .child(
                settings_page_view(
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
                ),
            )
    } else {
        rect()
            .expanded()
            .background(c.background)
            .content(Content::Flex)
            .child(toolbar)
            .child(rect().expanded().child(main_workspace))
    }
}

/// A dedicated, accessible full-page settings view.
fn settings_page_view<H0, H1, H2>(
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
                                .text("⚙ Paramètres & Configuration IA")
                                .color(c.text_primary)
                                .font_size(15.)
                                .font_weight(FontWeight::BOLD),
                        ),
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
                            .padding(Gaps::new(24., 32., 24., 32.))
                            .spacing(16.)
                            .content(Content::Flex)
                            .child(
                                // Card 1: API Key
                                rect()
                                    .width(Size::fill())
                                    .padding(18.)
                                    .background(c.surface_primary)
                                    .corner_radius(8.)
                                    .border(Border::new().fill(c.border).width(1.))
                                    .spacing(8.)
                                    .child(
                                        label()
                                            .text("Clé API (Albert / OpenAI)")
                                            .color(c.text_primary)
                                            .font_size(14.)
                                            .font_weight(FontWeight::BOLD),
                                    )
                                    .child(
                                        label()
                                            .text("Votre clé est masquée pour la sécurité et conservée dans la configuration locale.")
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
                                // Card 2: Endpoint URL
                                rect()
                                    .width(Size::fill())
                                    .padding(18.)
                                    .background(c.surface_primary)
                                    .corner_radius(8.)
                                    .border(Border::new().fill(c.border).width(1.))
                                    .spacing(8.)
                                    .child(
                                        label()
                                            .text("URL Endpoint API")
                                            .color(c.text_primary)
                                            .font_size(14.)
                                            .font_weight(FontWeight::BOLD),
                                    )
                                    .child(
                                        label()
                                            .text("Adresse de base du service compatible OpenAI /v1.")
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
                                // Card 3: Models selection
                                rect()
                                    .width(Size::fill())
                                    .padding(18.)
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
                                                            .text("Modèle par défaut & Modèles de l'endpoint")
                                                            .color(c.text_primary)
                                                            .font_size(14.)
                                                            .font_weight(FontWeight::BOLD),
                                                    )
                                                    .child(
                                                        label()
                                                            .text("Sélectionnez un modèle ci-dessous ou saisissez son identifiant.")
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
                                                    .child(if is_loading { "⟳ Chargement..." } else { "⟳ Rafraîchir les modèles" }),
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
                                                rect()
                                                    .width(Size::fill())
                                                    .height(Size::fill())
                                                    .center()
                                                    .child(
                                                        label()
                                                            .text(if is_loading { "Récupération des modèles en cours..." } else { "Aucun modèle chargé. Cliquez sur 'Rafraîchir les modèles' ci-dessus." })
                                                            .color(c.text_placeholder)
                                                            .font_size(12.),
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
                                                            move |m| {
                                                                let m_clone = m.clone();
                                                                let is_active = m == current_selected;
                                                                let bg = if is_active {
                                                                    c.surface_tertiary
                                                                } else {
                                                                    Color::TRANSPARENT
                                                                };
                                                                let mut model_input_press = model_input.clone();
                                                                let m_selected = m.clone();
                                                                rect()
                                                                    .width(Size::fill())
                                                                    .padding(8.)
                                                                    .background(bg)
                                                                    .corner_radius(4.)
                                                                    .horizontal()
                                                                    .cross_align(Alignment::Center)
                                                                    .on_press(move |_| {
                                                                        *model_input_press.write() = m_selected.clone();
                                                                    })
                                                                    .child(
                                                                        label()
                                                                            .text(if is_active { format!("✓ {}", m_clone) } else { format!("  {}", m_clone) })
                                                                            .color(if is_active { c.text_primary } else { c.text_secondary })
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
                                                "Fichier de configuration : {}",
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

/// The left sidebar file-tree panel.
///
/// It shows the folders and files of the terminal's current working directory
/// as an expandable tree, and follows the directory as it changes (e.g. when
/// the user runs `cd` in the terminal). The tree is rebuilt from scratch on
/// every render, so it always reflects the latest directory contents.
fn file_tree_panel(
    current_dir: Readable<std::path::PathBuf>,
    expanded: Writable<std::collections::HashSet<std::path::PathBuf>>,
    colors: ColorsSheet,
    on_open_file: impl Fn(std::path::PathBuf) + Clone + 'static,
) -> impl IntoElement {
    let dir = current_dir.read().clone();
    let c = colors;
    let rows = build_tree_rows(&dir, 0, expanded.clone(), c.clone(), on_open_file);

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
/// Files are rendered as clickable buttons that open the file in the code editor.
fn build_tree_rows(
    dir: &std::path::Path,
    depth: usize,
    expanded: Writable<std::collections::HashSet<std::path::PathBuf>>,
    colors: ColorsSheet,
    on_open_file: impl Fn(std::path::PathBuf) + Clone + 'static,
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
                rows.extend(build_tree_rows(&entry.path, depth + 1, expanded.clone(), colors.clone(), on_open_file.clone()));
            }
        } else {
            let file_path = entry.path.clone();
            let on_open = on_open_file.clone();
            let open_click = move |_| {
                on_open(file_path.clone());
            };
            rows.push(
                rect()
                    .width(Size::fill())
                    .padding(Gaps::new(1., 4., 1., indent + 6.))
                    .child(
                        Button::new()
                            .width(Size::fill())
                            .background(Color::TRANSPARENT)
                            .hover_background(colors.surface_primary)
                            .border_fill(Color::TRANSPARENT)
                            .color(colors.text_highlight)
                            .on_press(open_click)
                            .child(
                                rect()
                                    .horizontal()
                                    .cross_align(Alignment::Center)
                                    .spacing(6.)
                                    .child(label().text("•").color(colors.text_placeholder))
                                    .child(label().text(entry.name.clone()).color(colors.text_highlight)),
                            ),
                    )
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
            let terminal_handle = handle_for_future.read().clone();
            let Some(terminal_handle) = terminal_handle else {
                return;
            };
            loop {
                futures_util::select! {
                    _ = terminal_handle.closed().fuse() => {
                        let _ = handle_for_future.write().take();
                        break;
                    }
                    _ = terminal_handle.clipboard_changed().fuse() => {
                        if let Some(text) = terminal_handle.clipboard_content() {
                            let _ = Clipboard::set(text);
                        }
                    }
                    _ = terminal_handle.output_received().fuse() => {
                        // The shell reports its working directory via OSC 7 on
                        // every prompt. Whenever new output arrives, check the
                        // reported directory and update the sidebar if it
                        // changed (e.g. after the user runs `cd`).
                        if let Some(cwd) = terminal_handle.cwd()
                            && *current_dir_for_future.read() != cwd
                        {
                            *current_dir_for_future.write() = cwd;
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
