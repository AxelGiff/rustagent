//! Code editor view panel component with multi-file tabs and close (`✕`) buttons.

use crate::language::SupportedLanguage;
use crate::views::ui::{self, Tone};
use freya::code_editor::*;
use freya::prelude::*;
use std::path::Path;

/// Opens `path_str` from disk into `editor`, `current_language`, `file_name`, and `open_tabs`.
pub fn load_file_into_editor(
    path: &Path,
    mut editor: Writable<CodeEditorData>,
    mut current_language: Writable<SupportedLanguage>,
    mut file_name: Writable<String>,
    mut open_tabs: Writable<Vec<String>>,
) {
    if let Ok(content) = std::fs::read_to_string(path) {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let lang = SupportedLanguage::from_extension(ext);
        if let Some(l) = lang {
            *current_language.write() = l;
        }
        *editor.write() = create_editor_data(&content, lang);
        let path_str = path.to_string_lossy().to_string();
        {
            let mut tabs = open_tabs.write();
            if !tabs.contains(&path_str) {
                tabs.push(path_str.clone());
            }
        }
        *file_name.write() = path_str;
    }
}

/// Builds the code editor pane. When no file is opened (`file_name.is_empty()`),
/// displays a styled RustAgent empty-state card indicating that no diff or file is active.
#[allow(clippy::too_many_arguments)] // a UI panel receives one handle per piece of state
pub fn code_editor_panel(
    editor: Writable<CodeEditorData>,
    current_language: Writable<SupportedLanguage>,
    file_name: Writable<String>,
    open_tabs: Writable<Vec<String>>,
    has_diffs_count: usize,
    mut on_view_diff: impl FnMut() + 'static,
    mut on_save: impl FnMut() + 'static,
    mut on_execute: impl FnMut() + 'static,
    a11y_id: AccessibilityId,
    c: ColorsSheet,
) -> impl IntoElement {
    let active_file = file_name.read().clone();
    if active_file.trim().is_empty() {
        return rect()
            .expanded()
            .content(Content::Flex)
            .background(c.background)
            .child(
                ui::panel_header(&c, 38.)
                    .child(
                        ui::row(8.)
                            .width(Size::flex(1.))
                            .child(
                                ui::subtitle("⚡ RustAgent — Revue & Éditeur", &c)
                                    .font_weight(FontWeight::BOLD),
                            ),
                    )
                    .maybe_child((has_diffs_count > 0).then(|| {
                        ui::btn_sm(
                            format!("📂 Voir les fichiers modifiés ({})", has_diffs_count),
                            Tone::Subtle,
                            &c,
                            move |_| on_view_diff(),
                        )
                    })),
            )
            .child(
                rect()
                    .width(Size::fill())
                    .height(Size::flex(1.))
                    .center()
                    .padding(24.)
                    .child(
                        ui::card(&c)
                            .width(Size::px(500.))
                            .padding(Gaps::new(28., 32., 28., 32.))
                            .corner_radius(16.)
                            .cross_align(Alignment::Center)
                            .spacing(14.)
                            .child(ui::pill(
                                "🛡️ RUSTAGENT · ALBERT API WORKSPACE",
                                Tone::Success,
                                &c,
                            ))
                            .child(
                                label()
                                    .width(Size::fill())
                                    .text("Pas de fichiers en diff pour le moment")
                                    .color(c.text_primary)
                                    .font_size(18.)
                                    .font_weight(FontWeight::BOLD)
                                    .text_align(TextAlign::Center),
                            )
                            .child(
                                ui::subtitle(
                                    "Envoyez une instruction à l'agent dans la conversation à droite pour générer ou modifier du code avec revue de diff ligne par ligne, ou ouvrez un fichier depuis l'explorateur à gauche.",
                                    &c,
                                )
                                .width(Size::fill())
                                .text_align(TextAlign::Center),
                            )
                            .child(
                                ui::row(8.)
                                    .padding(Gaps::new(6., 0., 0., 0.))
                                    .child(ui::pill("⚡ Quick Edit", Tone::Subtle, &c))
                                    .child(ui::pill("🛡️ Mode Spec", Tone::Subtle, &c))
                                    .child(ui::pill("📝 Revue Diff", Tone::Subtle, &c)),
                            ),
                    ),
            )
            .into_element();
    }

    let mut tabs_list = open_tabs.read().clone();
    if !tabs_list.contains(&active_file) {
        tabs_list.push(active_file.clone());
    }

    let tab_elements = tabs_list.into_iter().map({
        let active_file = active_file.clone();
        let editor = editor.clone();
        let current_language = current_language.clone();
        let file_name = file_name.clone();
        let open_tabs = open_tabs.clone();
        let c = c.clone();
        move |tab_path| {
            let is_active = tab_path == active_file;
            let short_name = Path::new(&tab_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| tab_path.clone());
            let tab_key = format!("editor_tab_{}", tab_path);

            let select_tab = {
                let tab_path = tab_path.clone();
                let editor = editor.clone();
                let current_language = current_language.clone();
                let file_name = file_name.clone();
                let open_tabs = open_tabs.clone();
                move |_| {
                    load_file_into_editor(
                        Path::new(&tab_path),
                        editor.clone(),
                        current_language.clone(),
                        file_name.clone(),
                        open_tabs.clone(),
                    );
                }
            };

            let close_tab = {
                let tab_path = tab_path.clone();
                let mut editor = editor.clone();
                let mut current_language = current_language.clone();
                let mut file_name = file_name.clone();
                let mut open_tabs = open_tabs.clone();
                move |_| {
                    let next_tab = {
                        let mut tabs = open_tabs.write();
                        tabs.retain(|t| t != &tab_path);
                        tabs.last().cloned()
                    };
                    let currently_open = file_name.read().clone();
                    if currently_open == tab_path {
                        if let Some(next) = next_tab {
                            load_file_into_editor(
                                Path::new(&next),
                                editor.clone(),
                                current_language.clone(),
                                file_name.clone(),
                                open_tabs.clone(),
                            );
                        } else {
                            *file_name.write() = String::new();
                            *editor.write() = create_editor_data("", None);
                            let _ = &mut current_language;
                        }
                    }
                }
            };

            rect()
                .key(&tab_key)
                .padding(Gaps::new(4., 8., 4., 10.))
                .background(if is_active {
                    c.surface_tertiary
                } else {
                    c.surface_secondary
                })
                .border(
                    Border::new()
                        .fill(if is_active { c.primary } else { c.border })
                        .width(1.),
                )
                .corner_radius(6.)
                .horizontal()
                .cross_align(Alignment::Center)
                .spacing(6.)
                .child(
                    rect()
                        .horizontal()
                        .cross_align(Alignment::Center)
                        .spacing(5.)
                        .on_press(select_tab)
                        .child(
                            label()
                                .text(format!("📄 {}", short_name))
                                .color(if is_active {
                                    c.text_primary
                                } else {
                                    c.text_secondary
                                })
                                .font_size(12.)
                                .font_weight(if is_active {
                                    FontWeight::BOLD
                                } else {
                                    FontWeight::NORMAL
                                }),
                        ),
                )
                .child(
                    rect()
                        .width(Size::px(18.))
                        .height(Size::px(18.))
                        .corner_radius(4.)
                        .background(if is_active {
                            c.surface_secondary
                        } else {
                            Color::TRANSPARENT
                        })
                        .center()
                        .on_press(close_tab)
                        .child(
                            label()
                                .text("✕")
                                .color(c.text_secondary)
                                .font_size(10.5)
                                .font_weight(FontWeight::BOLD),
                        ),
                )
                .into_element()
        }
    });

    rect()
        .expanded()
        .content(Content::Flex)
        .background(c.background)
        .child(
            ui::panel_header(&c, 42.)
                .child(
                    ScrollView::new()
                        .direction(Direction::Horizontal)
                        .width(Size::flex(1.))
                        .height(Size::px(32.))
                        .child(ui::row(6.).children(tab_elements)),
                )
                .child(
                    ui::row(6.)
                        .maybe_child((has_diffs_count > 0).then(|| {
                            ui::btn_sm(
                                format!("📂 Diffs ({})", has_diffs_count),
                                Tone::Subtle,
                                &c,
                                move |_| on_view_diff(),
                            )
                        }))
                        .child(ui::btn_sm("💾 Sauvegarder", Tone::Neutral, &c, move |_| {
                            on_save()
                        }))
                        .child(ui::btn_sm("▶ Exécuter", Tone::Primary, &c, move |_| {
                            on_execute()
                        })),
                ),
        )
        .child(
            rect()
                .width(Size::fill())
                .height(Size::flex(1.))
                .padding(Gaps::new(6., 6., 6., 0.))
                .child(
                    CodeEditor::new(editor, a11y_id)
                        .background(c.surface_secondary)
                        .line_selected_background(Color::from_argb(20, 122, 162, 247))
                        .gutter_selected(c.primary)
                        .gutter_unselected(c.text_placeholder)
                        .cursor(c.text_primary)
                        .highlight(Color::from_argb(65, 122, 162, 247)),
                ),
        )
        .into_element()
}

/// Helper function to create a clean CodeEditorData without stale AST nodes from previous files.
pub fn create_editor_data(
    content: &str,
    language: Option<crate::language::SupportedLanguage>,
) -> CodeEditorData {
    let rope = ropey::Rope::from_str(content);
    let editor_lang = language.map(|l| l.editor_language());
    let mut data = CodeEditorData::new(rope, editor_lang);
    data.set_theme(EditorSyntaxTheme::dark());
    data.parse();
    data.measure(14., "Jetbrains Mono");
    data
}

/// Updates the editor content and language if the modified file matches the currently opened file.
pub fn update_editor_if_matching(
    diff_path: &str,
    content: &str,
    file_name: &str,
    mut editor: State<CodeEditorData>,
    mut current_language: State<crate::language::SupportedLanguage>,
) {
    if crate::diff::matches_open_editor(diff_path, file_name) {
        let ext = std::path::Path::new(diff_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let lang = crate::language::SupportedLanguage::from_extension(ext);
        if let Some(l) = lang {
            *current_language.write() = l;
        }
        *editor.write() = create_editor_data(content, lang);
    }
}
