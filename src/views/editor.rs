//! Code editor view panel component.

use freya::code_editor::*;
use freya::prelude::*;

/// Builds the code editor pane. When no file is opened (`file_name.is_empty()`),
/// displays a styled RustAgent empty-state card indicating that no diff or file is active.
#[allow(clippy::too_many_arguments)] // a UI panel receives one handle per piece of state
pub fn code_editor_panel(
    editor: Writable<CodeEditorData>,
    file_name: String,
    has_diffs_count: usize,
    mut on_view_diff: impl FnMut() + 'static,
    mut on_save: impl FnMut() + 'static,
    mut on_execute: impl FnMut() + 'static,
    a11y_id: AccessibilityId,
    c: ColorsSheet,
) -> impl IntoElement {
    if file_name.trim().is_empty() {
        return rect()
            .expanded()
            .content(Content::Flex)
            .background(c.background)
            .child(
                // Top subtle header bar
                rect()
                    .width(Size::fill())
                    .height(Size::px(38.))
                    .padding(Gaps::new(4., 12., 4., 12.))
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
                    .child(
                        rect()
                            .width(Size::flex(1.))
                            .horizontal()
                            .cross_align(Alignment::Center)
                            .spacing(8.)
                            .child(
                                label()
                                    .text("⚡ RustAgent — Revue & Éditeur")
                                    .color(c.text_secondary)
                                    .font_size(12.)
                                    .font_weight(FontWeight::BOLD),
                            ),
                    )
                    .maybe_child((has_diffs_count > 0).then(|| {
                        Button::new()
                            .background(c.surface_tertiary)
                            .hover_background(c.tertiary)
                            .border_fill(c.border)
                            .color(c.text_primary)
                            .padding(Gaps::new(4., 10., 4., 10.))
                            .on_press(move |_| on_view_diff())
                            .child(
                                label()
                                    .text(format!("📂 Voir les fichiers modifiés ({})", has_diffs_count))
                                    .font_size(11.)
                                    .font_weight(FontWeight::BOLD),
                            )
                    })),
            )
            .child(
                // Centered empty-state showcase card
                rect()
                    .width(Size::fill())
                    .height(Size::flex(1.))
                    .main_align(Alignment::Center)
                    .cross_align(Alignment::Center)
                    .padding(24.)
                    .child(
                        rect()
                            .width(Size::px(500.))
                            .padding(Gaps::new(28., 32., 28., 32.))
                            .background(c.surface_primary)
                            .corner_radius(16.)
                            .border(Border::new().fill(c.border).width(1.))
                            .cross_align(Alignment::Center)
                            .spacing(14.)
                            .child(
                                // Top accent pill
                                rect()
                                    .padding(Gaps::new(4., 12., 4., 12.))
                                    .background(Color::from_af32rgb(0.14, 16, 185, 129))
                                    .border(
                                        Border::new()
                                            .fill(Color::from_rgb(16, 185, 129))
                                            .width(1.),
                                    )
                                    .corner_radius(99.)
                                    .child(
                                        label()
                                            .text("🛡️ RUSTAGENT · ALBERT API WORKSPACE")
                                            .color(Color::from_rgb(52, 211, 153))
                                            .font_size(10.)
                                            .font_weight(FontWeight::BOLD),
                                    ),
                            )
                            .child(
                                label()
                                    .text("Pas de fichiers en diff pour le moment")
                                    .color(c.text_primary)
                                    .font_size(18.)
                                    .font_weight(FontWeight::BOLD)
                                    .text_align(TextAlign::Center),
                            )
                            .child(
                                label()
                                    .text("Envoyez une instruction à l'agent dans la conversation à droite pour générer ou modifier du code avec revue de diff ligne par ligne, ou ouvrez un fichier depuis l'explorateur à gauche.")
                                    .color(c.text_secondary)
                                    .font_size(12.)
                                    .text_align(TextAlign::Center),
                            )
                            .child(
                                // Feature badges row
                                rect()
                                    .horizontal()
                                    .main_align(Alignment::Center)
                                    .cross_align(Alignment::Center)
                                    .spacing(8.)
                                    .padding(Gaps::new(6., 0., 0., 0.))
                                    .child(
                                        rect()
                                            .padding(Gaps::new(5., 10., 5., 10.))
                                            .background(c.surface_secondary)
                                            .border(Border::new().fill(c.border).width(1.))
                                            .corner_radius(8.)
                                            .child(
                                                label()
                                                    .text("⚡ Quick Edit")
                                                    .color(c.text_secondary)
                                                    .font_size(11.),
                                            ),
                                    )
                                    .child(
                                        rect()
                                            .padding(Gaps::new(5., 10., 5., 10.))
                                            .background(c.surface_secondary)
                                            .border(Border::new().fill(c.border).width(1.))
                                            .corner_radius(8.)
                                            .child(
                                                label()
                                                    .text("🛡️ Mode Spec")
                                                    .color(c.text_secondary)
                                                    .font_size(11.),
                                            ),
                                    )
                                    .child(
                                        rect()
                                            .padding(Gaps::new(5., 10., 5., 10.))
                                            .background(c.surface_secondary)
                                            .border(Border::new().fill(c.border).width(1.))
                                            .corner_radius(8.)
                                            .child(
                                                label()
                                                    .text("📝 Revue Diff")
                                                    .color(c.text_secondary)
                                                    .font_size(11.),
                                            ),
                                    ),
                            ),
                    ),
            )
            .into_element();
    }

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
                .content(Content::Flex)
                .child(
                    rect()
                        .width(Size::flex(1.))
                        .horizontal()
                        .cross_align(Alignment::Center)
                        .spacing(8.)
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
                        .horizontal()
                        .cross_align(Alignment::Center)
                        .spacing(6.)
                        .maybe_child((has_diffs_count > 0).then(|| {
                            Button::new()
                                .background(c.surface_tertiary)
                                .hover_background(c.tertiary)
                                .border_fill(c.border)
                                .color(c.text_primary)
                                .on_press(move |_| on_view_diff())
                                .child(format!("📂 Diffs ({})", has_diffs_count))
                        }))
                        .child(
                            Button::new()
                                .background(c.surface_tertiary)
                                .hover_background(c.tertiary)
                                .border_fill(Color::TRANSPARENT)
                                .color(c.text_secondary)
                                .on_press(move |_| on_save())
                                .child("💾 Sauvegarder"),
                        )
                        .child(
                            Button::new()
                                .background(c.surface_tertiary)
                                .hover_background(c.tertiary)
                                .border_fill(Color::TRANSPARENT)
                                .color(c.text_secondary)
                                .on_press(move |_| on_execute())
                                .child("Execute Code"),
                        ),
                ),
        )
        .child(
            rect()
                .width(Size::fill())
                .height(Size::flex(1.))
                .padding(Gaps::new(6., 6., 6., 0.))
                .child(CodeEditor::new(editor, a11y_id).background(c.surface_secondary)),
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
