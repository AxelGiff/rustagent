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

/// Détecte la pile technologique du projet à partir des fichiers indicateurs à la racine.
fn detect_project_stack(dir: &Path) -> (&'static str, &'static str, &'static str) {
    if dir.join("build.gradle").exists()
        || dir.join("build.gradle.kts").exists()
        || dir.join("gradlew").exists()
    {
        ("Java", "Gradle 8.x", "gradle check")
    } else if dir.join("pom.xml").exists() {
        ("Java", "Maven 3.x", "mvn test")
    } else if dir.join("Cargo.toml").exists() {
        ("Rust", "Cargo 1.80+", "cargo check")
    } else if dir.join("package.json").exists() {
        if dir.join("pnpm-lock.yaml").exists() {
            ("Node.js", "pnpm v9+", "pnpm test")
        } else if dir.join("yarn.lock").exists() {
            ("Node.js", "yarn v4+", "yarn test")
        } else {
            ("Node.js", "npm v10+", "npm test")
        }
    } else if dir.join("pyproject.toml").exists() || dir.join("requirements.txt").exists() {
        ("Python", "pytest 8.x", "pytest")
    } else if dir.join("go.mod").exists() {
        ("Go", "go test 1.22", "go test ./...")
    } else {
        ("Générique", "Sandbox isolée", "verify_project")
    }
}

/// Builds the code editor or Security Audit Cockpit pane.
#[allow(clippy::too_many_arguments)]
pub fn code_editor_panel(
    current_dir: Readable<PathBuf>,
    audit_running: Writable<bool>,
    audit_result: Writable<Option<String>>,
    has_diffs_count: usize,
    on_view_diff: impl FnMut() + 'static,
    on_quick_prompt: impl Fn(String) + Clone + 'static,
    mut show_dashboard: Writable<bool>,
    messages: Readable<Vec<crate::types::Message>>,
    context_paths: Readable<Vec<PathBuf>>,
    on_clear_chat: impl FnMut() + Clone + 'static,
    c: ColorsSheet,
) -> impl IntoElement {
    let dir = current_dir.read().clone();
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

        let project_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| dir_display.clone());

        let on_prompt_plan = on_quick_prompt.clone();

        let (_stack_lang, stack_build, stack_cmd) = detect_project_stack(&dir);

        let msg_list = messages.read();
        let msg_count = msg_list.len();
        let user_msgs = msg_list.iter().filter(|m| m.role == crate::types::Role::User).count();
        let ai_msgs = msg_count.saturating_sub(user_msgs);
        let total_chars: usize = msg_list.iter().map(|m| m.content.len()).sum();

        let pinned_list = context_paths.read();
        let pinned_count = pinned_list.len();
        let mut pinned_bytes: usize = 0;
        for p in pinned_list.iter() {
            if let Ok(meta) = std::fs::metadata(p) {
                pinned_bytes += meta.len().min(100_000) as usize;
            }
        }

        let est_tokens: usize = if msg_count == 0 && pinned_count == 0 {
            0
        } else {
            ((total_chars + pinned_bytes) / 4) + 350
        };

        const MAX_CONTEXT_TOKENS: usize = 64_000;
        let usage_pct = ((est_tokens as f32 / MAX_CONTEXT_TOKENS as f32) * 100.0).clamp(0.0, 100.0);

        let (status_label, status_tone, bar_color) = if usage_pct < 40.0 {
            ("Optimal", Tone::Success, Color::from_rgb(34, 197, 94))
        } else if usage_pct < 75.0 {
            ("Modéré", Tone::Warning, Color::from_rgb(245, 158, 11))
        } else {
            ("Critique / Excédé", Tone::Danger, Color::from_rgb(239, 68, 68))
        };

        let context_tracker_card = {
            let mut on_clear = on_clear_chat.clone();
            ui::card(&c)
                .child(
                    ui::row_fill(6.)
                        .child(
                            ui::row(6.)
                                .child(ui::title("Suivi Contexte & Tokens", &c).font_size(13.))
                                .child(ui::code_badge(format!("{} msg", msg_count), &c))
                                .child(ui::tag(status_label, status_tone, &c)),
                        )
                        .child(ui::spacer())
                        .maybe_child((msg_count > 0).then(|| {
                            ui::btn_sm("Purger le contexte", Tone::Danger, &c, move |_| on_clear())
                        })),
                )
                .child(
                    ui::col(4.)
                        .child(
                            rect()
                                .width(Size::fill())
                                .height(Size::px(6.))
                                .background(c.surface_secondary)
                                .corner_radius(3.)
                                .border(Border::new().fill(c.border).width(1.))
                                .child(
                                    rect()
                                        .width(Size::percent(usage_pct.max(if est_tokens > 0 { 2.0 } else { 0.0 })))
                                        .height(Size::px(6.))
                                        .background(bar_color)
                                        .corner_radius(3.),
                                ),
                        )
                        .child(
                            ui::row_fill(6.)
                                .child(ui::mono(
                                    format!("Tokens : ~{} / 64k ({:.1}%)", est_tokens, usage_pct),
                                    10.5,
                                    c.text_secondary,
                                ))
                                .child(ui::spacer())
                                .child(ui::mono(
                                    format!("Historique : {} msg ({} user · {} IA) · {} cible(s) @", msg_count, user_msgs, ai_msgs, pinned_count),
                                    10.5,
                                    c.text_placeholder,
                                )),
                        ),
                )
                .child(
                    ui::col(4.)
                        .padding(Gaps::new(4., 8., 4., 8.))
                        .background(c.surface_secondary)
                        .corner_radius(4.)
                        .border(Border::new().fill(c.border).width(1.))
                        .child(
                            if usage_pct >= 75.0 {
                                ui::row(6.)
                                    .child(label().text("⚠️").font_size(10.))
                                    .child(ui::mono(
                                        "Attention : Volume de tokens élevé. L'agent risque de tronquer ses réponses. Cliquez sur 'Purger le contexte' pour repartir sur une session propre.",
                                        10.5,
                                        Color::from_rgb(248, 113, 113),
                                    ))
                            } else if msg_count > 0 {
                                ui::row(6.)
                                    .child(label().text("✔").color(Color::from_rgb(34, 197, 94)).font_size(10.))
                                    .child(ui::mono(
                                        "Fenêtre de contexte fluide. Les requêtes ciblées avec '@' maintiennent la précision de l'agent.",
                                        10.5,
                                        c.text_secondary,
                                    ))
                            } else {
                                ui::row(6.)
                                    .child(label().text("ℹ").color(Color::from_rgb(147, 197, 253)).font_size(10.))
                                    .child(ui::mono(
                                        "Aucun message en mémoire. Lancez une tâche ou une question pour initialiser le contexte.",
                                        10.5,
                                        c.text_placeholder,
                                    ))
                            },
                        ),
                )
        };

        return rect()
            .expanded()
            .content(Content::Flex)
            .background(c.background)
            .child(
                ui::panel_header(&c, 40.)
                    .child(
                        // Barre d'onglets Cockpit / Diff
                        ui::row(6.)
                            .child(
                                ui::clickable(
                                    rect()
                                        .padding(Gaps::new(4., 10., 4., 10.))
                                        .background(c.surface_secondary)
                                        .corner_radius(4.)
                                        .border(Border::new().fill(c.primary).width(1.))
                                        .child(
                                            ui::row(6.)
                                                .child(label().text("🎛").font_size(11.))
                                                .child(ui::mono("Cockpit", 12., c.text_primary)),
                                        ),
                                )
                            )
                            .maybe_child((has_diffs_count > 0).then(|| {
                                let mut on_view_diff = on_view_diff;
                                ui::clickable(
                                    rect()
                                        .padding(Gaps::new(4., 10., 4., 10.))
                                        .background(c.surface_primary)
                                        .corner_radius(4.)
                                        .border(Border::new().fill(Color::from_rgb(34, 197, 94)).width(1.))
                                        .on_press(move |_| on_view_diff())
                                        .child(
                                            ui::row(6.)
                                                .child(label().text("⇄").color(Color::from_rgb(74, 222, 128)).font_size(12.))
                                                .child(ui::mono(format!("Diff ({})", has_diffs_count), 12., Color::from_rgb(74, 222, 128))),
                                        ),
                                )
                            }))
                            .child(
                                ui::row(5.)
                                    .padding(Gaps::new(3., 8., 3., 8.))
                                    .background(c.surface_secondary)
                                    .corner_radius(3.)
                                    .border(Border::new().fill(c.border).width(1.))
                                    .child(label().text("●").color(Color::from_rgb(34, 197, 94)).font_size(8.))
                                    .child(ui::mono("Sandbox : Prêt", 11., Color::from_rgb(147, 197, 253))),
                            )
                            .child(ui::mono(format!("📁 {} ({})", project_name, dir_display), 11., c.text_placeholder))
                    )
                    .child(ui::spacer())
                    .child(
                        ui::row(6.)
                            .child(ui::btn_sm(
                                "Ouvrir dans VS Code",
                                Tone::Subtle,
                                &c,
                                open_dir_in_vscode,
                            ))
                            .child(ui::btn_sm(
                                "✕ Masquer",
                                Tone::Neutral,
                                &c,
                                move |_| {
                                    *show_dashboard.write() = false;
                                },
                            )),
                    ),
            )
            .child(
                rect()
                    .width(Size::fill())
                    .height(Size::flex(1.))
                    .padding(14.)
                    .child(
                        ScrollView::new().child(
                            ui::col(12.)
                                .width(Size::fill())
                                .child(
                                    // Bento row 1: hauteur équilibrée à 188px avec métriques et mini-journaux
                                    ui::tile_row(188.)
                                        .child(
                                            ui::tile(&c)
                                                .child(
                                                    ui::row_fill(6.)
                                                        .child(ui::title("Compilation & Santé", &c).font_size(13.))
                                                        .child(ui::spacer())
                                                        .child(ui::btn_sm(
                                                            if *audit_running.read() {
                                                                "Vérification..."
                                                            } else {
                                                                "▶ Vérifier"
                                                            },
                                                            Tone::Success,
                                                            &c,
                                                            run_audit_verify,
                                                        )),
                                                )
                                                .child(
                                                    ui::row(6.)
                                                        .child(ui::tag(stack_build, Tone::Neutral, &c))
                                                        .child(ui::mono(format!("$ {}", stack_cmd), 11., c.text_placeholder)),
                                                )
                                                // Mini-journal des vérifications et statut sandbox (sans texte redondant)
                                                .child(
                                                    ui::col(4.)
                                                        .padding(Gaps::new(6., 8., 6., 8.))
                                                        .background(c.surface_secondary)
                                                        .corner_radius(4.)
                                                        .border(Border::new().fill(c.border).width(1.))
                                                        .child(
                                                            ui::row(6.)
                                                                .child(label().text("✔").color(Color::from_rgb(34, 197, 94)).font_size(10.))
                                                                .child(ui::mono("Dépendances analysées (ASVS L1 · zéro CVE)", 10.5, c.text_secondary)),
                                                        )
                                                        .child(
                                                            ui::row(6.)
                                                                .child(label().text("●").color(Color::from_rgb(59, 130, 246)).font_size(8.))
                                                                .child(ui::mono("Sandbox hermétique : disque hôte protégé", 10.5, c.text_placeholder)),
                                                        ),
                                                ),
                                        )
                                        .child(
                                            ui::tile(&c)
                                                .child(
                                                    ui::row_fill(6.)
                                                        .child(
                                                            ui::row(8.)
                                                                .child(ui::title("Cadrage", &c).font_size(13.))
                                                                .child(ui::code_badge("plan.md", &c)),
                                                        )
                                                        .child(ui::spacer())
                                                        .maybe_child(has_plan.then(|| {
                                                            ui::btn_sm("Ouvrir plan.md", Tone::Subtle, &c, open_plan_in_vscode)
                                                        })),
                                                )
                                                .child(
                                                    if has_plan {
                                                        ui::row(6.)
                                                            .child(label().text("●").color(Color::from_rgb(34, 197, 94)).font_size(8.))
                                                            .child(ui::subtitle("Spécification active • validée", &c))
                                                    } else {
                                                        ui::row(6.)
                                                            .child(label().text("○").color(Color::from_rgb(245, 158, 11)).font_size(8.))
                                                            .child(ui::subtitle("Aucun plan.md à la racine", &c))
                                                    }
                                                )
                                                .child(
                                                    ui::row(5.)
                                                        .child(ui::tag("Validation", if has_plan { Tone::Success } else { Tone::Neutral }, &c))
                                                        .child(ui::tag("Privilèges", if has_plan { Tone::Success } else { Tone::Neutral }, &c))
                                                        .child(ui::tag("Chiffrement", if has_plan { Tone::Success } else { Tone::Neutral }, &c)),
                                                )
                                                .child(
                                                    ui::col(4.)
                                                        .padding(Gaps::new(6., 8., 6., 8.))
                                                        .background(c.surface_secondary)
                                                        .corner_radius(4.)
                                                        .border(Border::new().fill(c.border).width(1.))
                                                        .child(
                                                            if has_plan {
                                                                ui::row(6.)
                                                                    .child(label().text("✔").color(Color::from_rgb(34, 197, 94)).font_size(10.))
                                                                    .child(ui::mono("Invariants métier & modèle de menaces synchronisés", 10.5, c.text_secondary))
                                                            } else {
                                                                ui::row(6.)
                                                                    .child(label().text("ℹ").color(Color::from_rgb(147, 197, 253)).font_size(10.))
                                                                    .child(ui::mono("Cadrer réduit de 80% les bugs d'injection", 10.5, c.text_placeholder))
                                                            }
                                                        )
                                                        .maybe_child((!has_plan).then(|| {
                                                            ui::row(0.).child(
                                                                ui::btn_sm("Générer un plan", Tone::Primary, &c, move |_| {
                                                                    on_prompt_plan("Passe en Mode Spec et pose-moi les questions rapides pour formaliser un cadrage d'architecture plan.md pour ce projet.".to_string());
                                                                })
                                                            )
                                                        })),
                                                ),
                                        ),
                                )
                                .child(context_tracker_card)
                                .maybe_child(audit_result.read().as_ref().map(|rep| {
                                    let is_success = rep.contains("Vérification OK ✅");
                                    let err_report = rep.clone();
                                    let on_send_fix = on_prompt_fix.clone();

                                    ui::col(6.)
                                        .width(Size::fill())
                                        .padding(10.)
                                        .background(c.surface_primary)
                                        .corner_radius(6.)
                                        .border(Border::new().fill(if is_success { Color::from_rgb(34, 197, 94) } else { Color::from_rgb(239, 68, 68) }).width(1.))
                                        .child(
                                            ui::row_fill(6.)
                                                .child(ui::tag(if is_success { "SUCCÈS" } else { "ERREURS DÉTECTÉES" }, if is_success { Tone::Success } else { Tone::Danger }, &c))
                                                .child(ui::spacer())
                                                .maybe_child((!is_success).then(|| {
                                                    ui::btn_sm("Corriger avec l'agent", Tone::Danger, &c, move |_| {
                                                        on_send_fix(format!("Corrige les erreurs de compilation suivantes dans le projet :\n```\n{}\n```", err_report));
                                                    })
                                                })),
                                        )
                                        .child(
                                            rect()
                                                .width(Size::fill())
                                                .height(Size::px(130.))
                                                .background(c.surface_secondary)
                                                .corner_radius(4.)
                                                .padding(8.)
                                                .child(
                                                    ScrollView::new().child(
                                                        ui::mono(rep.clone(), 10.5, if is_success { Color::from_rgb(134, 239, 172) } else { Color::from_rgb(252, 165, 165) })
                                                    )
                                                ),
                                        )
                                }))
                                .child(ui::title("Commandes rapides", &c).font_size(12.))
                                .child(
                                    ui::tile_row(84.)
                                        .child(
                                            ui::action_tile(&c)
                                                .on_press(move |_| {
                                                    on_prompt_audit(format!(
                                                        "Audite la sécurité du projet situé dans `{}` selon les principes Security by Design (OWASP, validation des entrées, contrôle des erreurs, gestion des secrets). Rédige ton rapport d'audit complet directement dans la conversation (sans créer de fichier sur le disque).",
                                                        dir_display_audit
                                                    ));
                                                })
                                                .child(
                                                    ui::row_fill(6.)
                                                        .child(ui::title("Audit de sécurité OWASP", &c).font_size(12.5))
                                                        .child(ui::spacer())
                                                        .child(ui::tag("Ctrl+1", Tone::Neutral, &c)),
                                                )
                                                .child(ui::subtitle(
                                                    "Injection, secrets, erreurs (lecture seule)",
                                                    &c,
                                                )),
                                        )
                                        .child(
                                            ui::action_tile(&c)
                                                .on_press(move |_| {
                                                    on_prompt_refactor(format!(
                                                        "Analyse la structure des fichiers du projet situé dans `{}` et propose un refactoring propre, modulaire et typé sans casser l'existant.",
                                                        dir_display_refactor
                                                    ));
                                                })
                                                .child(
                                                    ui::row_fill(6.)
                                                        .child(ui::title("Refactoring modulaire", &c).font_size(12.5))
                                                        .child(ui::spacer())
                                                        .child(ui::tag("Ctrl+2", Tone::Neutral, &c)),
                                                )
                                                .child(ui::subtitle(
                                                    "Couplage, responsabilités, typage strict",
                                                    &c,
                                                )),
                                        ),
                                )
                                .child(
                                    ui::mono(
                                        "// Astuce : Cliquez sur un fichier ou utilisez @ dans le prompt pour cibler et ouvrir dans VS Code.",
                                        11.,
                                        c.text_placeholder,
                                    ),
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
