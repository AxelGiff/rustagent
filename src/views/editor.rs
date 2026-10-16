//! Code editor view panel component.

use freya::code_editor::*;
use freya::prelude::*;

/// Builds the code editor pane. The editor state is lifted up into `app()` so
/// that the chat and terminal panels can read/write the same content.
pub fn code_editor_panel(
    editor: Writable<CodeEditorData>,
    file_name: String,
    has_pending_diffs: usize,
    mut on_view_diff: impl FnMut() + 'static,
    mut on_save: impl FnMut() + 'static,
    mut on_execute: impl FnMut() + 'static,
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
                .expanded()
                .padding(Gaps::new(6., 6., 6., 0.))
                .child(CodeEditor::new(editor, a11y_id).background(c.surface_secondary)),
        )
}

/// Helper function to create a clean CodeEditorData without stale AST nodes from previous files.
pub fn create_editor_data(content: &str, language: Option<crate::language::SupportedLanguage>) -> CodeEditorData {
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
        let ext = std::path::Path::new(diff_path).extension().and_then(|e| e.to_str()).unwrap_or("");
        let lang = crate::language::SupportedLanguage::from_extension(ext);
        if let Some(l) = lang {
            *current_language.write() = l;
        }
        *editor.write() = create_editor_data(content, lang);
    }
}

