//! Code editor & Security Audit Cockpit view panel component.
//!
//! Provides multi-file tabs with close (`✕`) buttons, a 1-click `verify_project`
//! compilation & audit dashboard, and direct bridges to VS Code.

use crate::language::SupportedLanguage;
use crate::views::ui::{self, Tone};
use freya::code_editor::*;
use freya::prelude::*;
use std::path::{Path, PathBuf};

/// Opens `path_str` from disk into `editor`, `current_language`, `file_name`, and `open_tabs`.
#[allow(dead_code)]
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

/// Builds the code editor or Security Audit Cockpit pane.
#[allow(clippy::too_many_arguments)]
pub fn code_editor_panel(
    _editor: Writable<CodeEditorData>,
    _current_language: Writable<SupportedLanguage>,
    file_name: Writable<String>,
    _open_tabs: Writable<Vec<String>>,
    current_dir: Readable<PathBuf>,
    audit_running: Writable<bool>,
    audit_result: Writable<Option<String>>,
    has_diffs_count: usize,
    mut on_view_diff: impl FnMut() + 'static,
    _on_save: impl FnMut() + 'static,
    _on_execute: impl FnMut() + 'static,
    on_quick_prompt: impl Fn(String) + Clone + 'static,
    _a11y_id: AccessibilityId,
    c: ColorsSheet,
) -> impl IntoElement {
    let active_file = file_name.read().clone();
    let dir = current_dir.read().clone();

    let _ = active_file;
    let dir_display = dir.to_string_lossy().replace('\\', "/");
    let plan_path = dir.join("plan.md");
    let has_plan = plan_path.is_file();

    let run_audit_verify = {
            let mut audit_running = audit_running.clone();
            let mut audit_result = audit_result.clone();
            let dir_clone = dir.clone();
            move |_| {
                *audit_running.write() = true;
                *audit_result.write() = None;
                let dir_str = dir_clone.to_string_lossy().to_string();
                let mut audit_running = audit_running.clone();
                let mut audit_result = audit_result.clone();
                spawn(async move {
                    let report = crate::tools::run_verification(Some(&dir_str)).await;
                    *audit_result.write() = Some(report);
                    *audit_running.write() = false;
                });
            }
        };

        let dir_for_vscode = dir.clone();
        let open_dir_in_vscode = move |_| {
            crate::platform::open_in_vscode(&dir_for_vscode, None);
        };

        let plan_for_vscode = plan_path.clone();
        let open_plan_in_vscode = move |_| {
            crate::platform::open_in_vscode(&plan_for_vscode, None);
        };

        let on_prompt_audit = on_quick_prompt.clone();
        let on_prompt_fix = on_quick_prompt.clone();
        let on_prompt_refactor = on_quick_prompt.clone();
        let dir_display_audit = dir_display.clone();
        let dir_display_refactor = dir_display.clone();

        return rect()
            .expanded()
            .content(Content::Flex)
            .background(c.background)
            .child(
                ui::panel_header(&c, 44.)
                    .child(
                        ui::row(8.)
                            .width(Size::flex(1.))
                            .child(ui::title("🛡️ Cockpit Architecte & Auditeur Security by Design", &c).font_size(14.))
                            .child(ui::pill("Albert API", Tone::Primary, &c)),
                    )
                    .child(ui::btn_sm(
                        "💙 Ouvrir dans VS Code",
                        Tone::Primary,
                        &c,
                        open_dir_in_vscode,
                    ))
                    .maybe_child((has_diffs_count > 0).then(|| {
                        ui::btn_sm(
                            format!("📂 Voir diffs ({})", has_diffs_count),
                            Tone::Success,
                            &c,
                            move |_| on_view_diff(),
                        )
                    })),
            )
            .child(
                rect()
                    .width(Size::fill())
                    .height(Size::flex(1.))
                    .padding(16.)
                    .child(
                        ScrollView::new().child(
                            ui::col(14.)
                                .width(Size::fill())
                                .child(
                                    // Card 1: 🔎 Auto-Vérification de compilation en 1 clic
                                    ui::card(&c)
                                        .child(
                                            ui::row_fill(8.)
                                                .child(
                                                    ui::col(2.)
                                                        .width(Size::flex(1.))
                                                        .child(ui::title("🔎 Audit & Compilation du projet", &c))
                                                        .child(ui::subtitle(
                                                            format!("Dossier cible : {} (testé sur une copie temporaire isolée)", dir_display),
                                                            &c,
                                                        )),
                                                )
                                                .child(ui::btn_sm(
                                                    if *audit_running.read() {
                                                        "⏳ Compilation en cours..."
                                                    } else {
                                                        "▶ Lancer l'Audit de Compilation"
                                                    },
                                                    if *audit_running.read() { Tone::Neutral } else { Tone::Success },
                                                    &c,
                                                    run_audit_verify,
                                                )),
                                        )
                                        .maybe_child(audit_result.read().as_ref().map(|rep| {
                                            let is_success = rep.contains("Vérification OK ✅");
                                            let err_report = rep.clone();
                                            let on_send_fix = on_prompt_fix.clone();

                                            ui::col(8.)
                                                .width(Size::fill())
                                                .padding(Gaps::new(8., 10., 8., 10.))
                                                .background(c.surface_secondary)
                                                .corner_radius(6.)
                                                .border(Border::new().fill(if is_success { Color::from_rgb(16, 185, 129) } else { Color::from_rgb(239, 68, 68) }).width(1.))
                                                .child(
                                                    ui::row_fill(6.)
                                                        .child(ui::tag(if is_success { "SUCCÈS ✅" } else { "ERREURS DÉTECTÉES ❌" }, if is_success { Tone::Success } else { Tone::Danger }, &c))
                                                        .maybe_child((!is_success).then(|| {
                                                            ui::btn_sm("⚡ Faire corriger par l'agent", Tone::Danger, &c, move |_| {
                                                                on_send_fix(format!("Corrige les erreurs de compilation suivantes dans le projet :\n```\n{}\n```", err_report));
                                                            })
                                                        })),
                                                )
                                                .child(
                                                    rect()
                                                        .width(Size::fill())
                                                        .height(Size::px(140.))
                                                        .background(c.surface_primary)
                                                        .corner_radius(5.)
                                                        .padding(6.)
                                                        .child(
                                                            ScrollView::new().child(
                                                                ui::mono(rep.clone(), 11., if is_success { Color::from_rgb(134, 239, 172) } else { Color::from_rgb(252, 165, 165) })
                                                            )
                                                        )
                                                )
                                        })),
                                )
                                .child(
                                    // Card 2: 🛡️ Spécifications & Architecture (plan.md)
                                    ui::card(&c)
                                        .child(
                                            ui::row_fill(8.)
                                                .child(
                                                    ui::col(2.)
                                                        .width(Size::flex(1.))
                                                        .child(ui::title("🛡️ Cadrage d'Architecture & Security by Design (`plan.md`)", &c))
                                                        .child(ui::subtitle(
                                                            if has_plan {
                                                                "Un plan de spécification et d'architecture `plan.md` est actif et protégé par le `.gitignore`."
                                                            } else {
                                                                "Aucun plan d'architecture formalisé. Utilisez le 🛡️ Mode Spec dans le chat pour cadrer vos fonctionnalités."
                                                            },
                                                            &c,
                                                        )),
                                                )
                                                .maybe_child(has_plan.then(|| {
                                                    ui::btn_sm("📄 Ouvrir plan.md dans VS Code", Tone::Subtle, &c, open_plan_in_vscode)
                                                })),
                                        )
                                        .child(
                                            ui::row(6.)
                                                .child(ui::pill("Validation stricte des entrées", Tone::Subtle, &c))
                                                .child(ui::pill("Moindre privilège", Tone::Subtle, &c))
                                                .child(ui::pill("Chiffrement fort & Zéro fuite de données", Tone::Subtle, &c)),
                                        ),
                                )
                                .child(
                                    // Card 3: 🚀 Raccourcis rapides d'Architecture & d'Audit
                                    ui::card(&c)
                                        .child(ui::title("🚀 Actions Rapides d'Architecture & d'Audit", &c))
                                        .child(ui::subtitle(
                                            "Cliquez sur une action ci-dessous pour lancer l'analyse autonome par Albert API :",
                                            &c,
                                        ))
                                        .child(
                                            ui::row_fill(8.)
                                                .child(ui::btn_sm(
                                                    "🛡️ Auditer la sécurité (OWASP)",
                                                    Tone::Primary,
                                                    &c,
                                                    move |_| {
                                                        on_prompt_audit(format!(
                                                            "Audite la sécurité du projet situé dans `{}` selon les principes Security by Design (OWASP, validation des entrées, contrôle des erreurs, gestion des secrets). Rédige ton rapport d'audit complet directement dans la conversation (sans créer de fichier sur le disque).",
                                                            dir_display_audit
                                                        ));
                                                    },
                                                ))
                                                .child(ui::btn_sm(
                                                    "🧹 Proposer un refactoring modulaire",
                                                    Tone::Neutral,
                                                    &c,
                                                    move |_| {
                                                        on_prompt_refactor(format!(
                                                            "Analyse la structure des fichiers du projet situé dans `{}` et propose un refactoring propre, modulaire et typé sans casser l'existant.",
                                                            dir_display_refactor
                                                        ));
                                                    },
                                                )),
                                        ),
                                )
                                .child(
                                    // Card 4: 💙 Guide de symbiose avec VS Code
                                    ui::accent_card(&c, c.primary)
                                        .child(ui::title("💙 Travailler en symbiose avec VS Code", &c))
                                        .child(ui::subtitle(
                                            "VS Code reste votre éditeur principal. RustAgent est votre sas de sécurité hermétique : l'agent prépare le code, teste la compilation en tâche de fond, et n'écrit sur votre disque que les blocs de diff que vous avez validés avec ✓.",
                                            &c,
                                        ))
                                        .child(ui::muted(
                                            "Astuce : Cliquez sur n'importe quel fichier dans l'explorateur pour l'ouvrir directement dans VS Code ! Tapez `@` dans le chat pour cibler vos fichiers (les fichiers récemment enregistrés avec Ctrl+S dans VS Code apparaissent tout en haut du menu).",
                                            &c,
                                        )),
                                ),
                        ),
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
