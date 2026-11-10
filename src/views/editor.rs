//! Code editor view panel component.

use crate::views::ui::{self, Tone};
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
                ui::panel_header(&c, 38.)
                    .child(
                        ui::row(8.)
                            .width(Size::flex(1.))
                            .child(ui::subtitle("⚡ RustAgent — Revue & Éditeur", &c).font_weight(FontWeight::BOLD)),
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

    rect()
        .expanded()
        .content(Content::Flex)
        .background(c.background)
        .child(
            ui::panel_header(&c, 38.)
                .child(
                    ui::row(8.)
                        .width(Size::flex(1.))
                        .child(ui::title(file_name, &c).font_size(13.)),
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
                        .child(ui::btn_sm("💾 Sauvegarder", Tone::Neutral, &c, move |_| on_save()))
                        .child(ui::btn_sm("▶ Exécuter", Tone::Primary, &c, move |_| on_execute())),
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
