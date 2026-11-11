//! Agent prompt construction, turn execution, and context assembly for RustAgent.

use crate::api;
use crate::config;
use crate::diff;
use crate::flow;
use crate::language::SupportedLanguage;
use crate::tools;
use crate::types::Message;

// Albert API configuration (French government AI service)
pub const ALBERT_ENDPOINT: &str = "https://albert.api.etalab.gouv.fr/v1";
pub const ALBERT_MODEL: &str = "deepseek-v4-flash";

/// Builds the active context of the sub-project based on pending diffs and recent history.
pub fn build_active_context(pending_diffs: &[diff::PendingDiff], history: &[Message]) -> String {
    if !pending_diffs.is_empty() {
        let list = pending_diffs
            .iter()
            .map(|d| format!("- `{}`", d.path))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "\n\n[CONTEXTE DU SOUS-PROJET EN COURS]\nFichiers déjà préparés pour ce projet :\n{}\nIMPORTANT : L'utilisateur travaille sur ce sous-projet. Tu DOIS impérativement continuer dans ce même dossier et ses sous-dossiers. Ne réinitialise JAMAIS le contexte vers la racine de l'agent !",
            list
        )
    } else {
        let mut recent_paths = Vec::new();
        for msg in history.iter().rev().take(8) {
            for line in msg.content.lines() {
                if let Some(pos) = line.find("- `")
                    && let Some(end) = line[pos + 3..].find('`')
                {
                    let p = &line[pos + 3..pos + 3 + end];
                    if !recent_paths.contains(&p.to_string())
                        && (p.contains('/') || p.contains('\\'))
                    {
                        recent_paths.push(p.to_string());
                    }
                }
            }
        }
        if !recent_paths.is_empty() {
            format!(
                "\n\n[CONTEXTE DU SOUS-PROJET EN COURS]\nFichiers récemment manipulés :\n{}\nIMPORTANT : L'utilisateur poursuit le travail sur ce sous-projet. Reste impérativement dans ce sous-dossier et n'explore pas la racine de l'agent !",
                recent_paths
                    .iter()
                    .map(|p| format!("- `{}`", p))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        } else {
            String::new()
        }
    }
}

/// Builds the environment and file context (home dir, active project, cwd, project snapshot, active editor file).
pub fn build_env_context(
    editor_code: &str,
    editor_file: &str,
    project_snapshot: &str,
    project_root: &std::path::Path,
) -> String {
    let home_dir_str = tools::user_home_dir()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| "~".to_string());
    let project_dir_str = project_root.to_string_lossy().replace('\\', "/");
    let cwd_dir_str = std::env::current_dir()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| ".".to_string());

    let editor_context = if !editor_code.trim().is_empty() && project_snapshot.is_empty() {
        let truncated_code = tools::truncate_output(editor_code, 80, 3000);
        format!(
            "\n\n[FICHIER ACTUELLEMENT OUVERT DANS L'ÉDITEUR : `{}`]\n```\n{}\n```",
            editor_file, truncated_code
        )
    } else {
        String::new()
    };

    format!(
        "\n\n[PROJET ACTIF CIBLE OUVERT DANS RUSTAGENT]\n- Dossier racine du projet ouvert : `{}`\n- RÈGLE CRITIQUE : L'utilisateur a ouvert le projet situé dans `{}`. TOUTES tes analyses (audit de sécurité OWASP, modèle de menaces, refactoring), lectures (`read_file`), recherches (`list_directory`, `search_files`), modifications (`edit_file`) et créations (`write_file`) DOIVENT impérativement cibler ce projet `{}` et ses fichiers sources.\nTu ne dois JAMAIS analyser ni modifier le code source de l'application RustAgent (`{}`) !\n\n[ENVIRONNEMENT & CHEMINS]\n- Dossier Home utilisateur (`~` / `$HOME`) : `{}`{}{}",
        project_dir_str, project_dir_str, project_dir_str, cwd_dir_str, home_dir_str, project_snapshot, editor_context
    )
}

/// Builds a high-priority context block for files and directories explicitly pinned
/// by the user via the `+` button in the file tree.
pub fn build_pinned_context(pinned_paths: &[std::path::PathBuf]) -> String {
    if pinned_paths.is_empty() {
        return String::new();
    }

    let mut sections = Vec::new();
    for path in pinned_paths {
        let display_path = path.to_string_lossy().replace('\\', "/");
        if path.is_dir() {
            let listing = crate::file_tree::list_directory_bounded(path, 35);
            let entries_preview = if listing.entries.is_empty() {
                "(dossier vide)".to_string()
            } else {
                listing
                    .entries
                    .iter()
                    .map(|e| {
                        if e.is_dir {
                            format!("`{}/`", e.name)
                        } else {
                            format!("`{}`", e.name)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            sections.push(format!(
                "- 📁 **Dossier cible** : `{}`\n  INSTRUCTION : Tu DOIS créer ou modifier les fichiers demandés à l'intérieur de ce dossier (`{}`).\n  Contenu immédiat : {}",
                display_path, display_path, entries_preview
            ));
        } else {
            let content_preview = match std::fs::read_to_string(path) {
                Ok(text) if !text.trim().is_empty() => {
                    let truncated = tools::truncate_output(&text, 100, 4000);
                    format!("```\n{}\n```", truncated)
                }
                Ok(_) => "(fichier vide)".to_string(),
                Err(_) => "(fichier non lisible ou à créer)".to_string(),
            };
            sections.push(format!(
                "- 📄 **Fichier cible** : `{}`\n  INSTRUCTION : Applique directement les modifications demandées sur ce fichier (`{}`) via `edit_file` ou `write_file`.\n{}",
                display_path, display_path, content_preview
            ));
        }
    }

    format!(
        "\n\n[🎯 CIBLES EXPLICITES ÉPINGLÉES PAR L'UTILISATEUR (PRIORITÉ ABSOLUE)]\nL'utilisateur a sélectionné manuellement ces cibles dans l'arborescence pour t'indiquer exactement où agir :\n{}",
        sections.join("\n")
    )
}

/// Builds the system prompt tailored to Spec Mode (Security by Design) or Quick Edit Mode.
pub fn build_system_prompt(
    spec_mode: bool,
    already_answered_specs: bool,
    env_context: &str,
    active_context: &str,
    plan_context: &str,
) -> String {
    if spec_mode {
        format!(
            "Tu es RustAgent en **🛡️ Mode Spec (Security by Design)**.

DIRECTIVES DU MODE SPEC (AUTONOME, COMPLET ET ROBUSTE) :
1. CADRAGE RAPIDE (SI AUCUNE QUESTION POSÉE) :
- Statut actuel : {}.
- Si des choix d'architecture/sécurité manquent ET qu'aucune question n'a encore été posée, appelle IMMÉDIATEMENT `ask_spec_questions` avec des questions claires au format : `{{ \"questions\": [{{ \"id\": \"q1\", \"question\": \"... ?\", \"options\": [\"(Recommandé) ...\", \"...\"] }}] }}` (1 à 3 questions courtes max, option sécurisée `(Recommandé)` en premier). Ne laisse JAMAIS le tableau vide.
2. PLANIFICATION PUIS IMPLÉMENTATION COMPLÈTE ET AUTONOME :
- Planifie d'abord avec `update_tasks` (étapes concrètes) et maintiens le `status` à jour (`pending`/`in_progress`/`done`). Ne t'arrête JAMAIS tant qu'une tâche reste incomplète.
- Implémente avec `write_file` ou `edit_file` :
- Dès que les spécifications sont validées (ou si la demande est déjà claire), `plan.md` est déjà généré automatiquement !
- RÈGLE STRICTE POUR `write_file` : Appelle TOUJOURS `write_file(path: \"...\", content: \"...\")` avec UN SEUL fichier par appel (fournis obligatoirement `path` et `content`). N'utilise JAMAIS de paramètre `files` ou de tableau de fichiers. Écris les fichiers un par un : la boucle te redonne immédiatement la main après chaque fichier créé !
- RÈGLE D'AUTONOMIE TOTALE : Ne laisse aucun fichier manquant, aucun placeholder de type \"[À compléter]\", et n'interromps pas la génération. L'application créée doit être 100% exécutable, belle et directement fonctionnelle.
- RÈGLE ABSOLUE : Tu dois TOUJOURS inclure le code source COMPLET dans chaque argument \"content\". N'appelle JAMAIS `write_file` avec des arguments vides !
3. VÉRIFICATION ET CORRECTION AUTOMATIQUE (OBLIGATOIRE) :
- Dès que tous les fichiers du projet sont écrits, appelle `verify_project` (ex: `{{ \"cwd\": \"Pictures/montage-pc\" }}`) : il compile/teste le projet sur une COPIE TEMPORAIRE (le projet réel n'est PAS modifié).
- Lis attentivement les erreurs renvoyées (section STDERR), corrige les fichiers concernés avec `edit_file` (recopie un `target` EXACT depuis un `read_file` ; en cas d'échec, l'erreur te renvoie le contenu du fichier) ou `write_file`, puis rappelle `verify_project`.
- Répète jusqu'à obtenir `🔎 Vérification OK ✅` (3 cycles maximum). Ne termine JAMAIS en laissant une erreur de compilation sans l'avoir corrigée ou expliquée.{}{}{}",
            if already_answered_specs {
                "SPÉCIFICATIONS DÉJÀ VALIDÉES (`plan.md` déjà créé) -> Appelle IMMÉDIATEMENT `write_file` avec le code complet dès ce tour !"
            } else {
                "aucune question posée pour l'instant"
            },
            env_context,
            active_context,
            plan_context
        )
    } else {
        format!(
            "Tu es RustAgent en **⚡ Mode Quick Edit** : un agent de code autonome.

OBJECTIF : réaliser ENTIÈREMENT la demande dans ce tour, avec autant d'appels d'outils que nécessaire. Ne t'arrête PAS après un seul fichier si la demande comporte plusieurs étapes.

DÉMARCHE :
1. PLANIFIE : commence par `update_tasks` en listant les étapes concrètes (explorer, chaque fichier à créer/modifier, vérifier). Mets à jour `status` après chaque étape (`pending`/`in_progress`/`done`).
2. EXPLORE : lis le code concerné (`read_file`, `search_files`) AVANT de modifier. Ne DEVINE JAMAIS un extrait.
3. IMPLÉMENTE :
- Modification ciblée : `edit_file` avec `target` = extrait EXACT copié depuis `read_file`. En cas d'échec, l'erreur renvoie le contenu du fichier : corrige `target` et réessaie.
- Nouveau fichier : appelle `write_file(path: \"...\", content: \"...\")` avec UN SEUL fichier par appel (`path` et `content` obligatoires). N'utilise JAMAIS de tableau `files`. Crée les fichiers un par un : la boucle agentique enchaîne automatiquement chaque fichier jusqu'à la fin du plan.
- Code COMPLET, sans placeholder, directement exécutable.
4. VÉRIFIE : `verify_project` (compile/tests sur une copie temporaire). Corrige les erreurs (STDERR) puis re-vérifie jusqu'à obtenir `🔎 Vérification OK ✅`.
5. CONCLUS seulement quand toutes les tâches sont `done` ET la vérification passe.

RÈGLES :
- Sois AUTONOME : n'attends aucune confirmation, n'annonce pas ce que tu vas faire — FAIS-LE.
- Si un outil échoue, adapte-toi et réessaie dans ce même tour.
- RÈGLE STRICTE POUR AUDITS ET ANALYSES DE SÉCURITÉ (OWASP) : si la demande est un audit, une analyse ou une revue, explore les fichiers sources du projet en profondeur avec `read_file` et `search_files`, puis rédige ton rapport d'audit COMPLET en Markdown directement dans ta réponse dans la conversation. Ne crée JAMAIS de fichier sur le disque (aucun fichier `.md`, aucun appel à `write_file` ou `edit_file`). L'audit est purement consultatif dans le chat !

REDIRECTION VERS MODE SPEC (UNIQUEMENT SI ARCHITECTURE COMPLEXE NON SPÉCIFIÉE) :
- Si l'utilisateur demande une grosse fonctionnalité d'architecture/sécurité floue (> 3 fichiers, auth/permissions complexes) et qu'aucun `plan.md` n'existe : explique en 2 phrases les enjeux Security by Design et termine par `[RECOMMEND_SPEC_MODE]`.{}{}{}",
            env_context,
            active_context,
            plan_context
        )
    }
}

/// Fallback extraction when LLM outputs markdown code blocks instead of using native tool calls directly.
pub fn extract_fallback_diffs(
    response: &str,
    user_message: &str,
    file_name: &str,
    wants_code: bool,
    detected_language: SupportedLanguage,
) -> (String, Vec<diff::PendingDiff>) {
    let mut new_diffs = Vec::new();

    // Aucun dossier codé en dur : les chemins proviennent du contenu généré
    // (bloc avec chemin) ou du fichier cible déduit de la demande.
    let extracted_blocks = tools::extract_code_blocks_from_text(response, None);
    if !extracted_blocks.is_empty() {
        for (path, code) in extracted_blocks {
            if code.trim() == "[Contenu déjà fourni au tour précédent]"
                || code.trim().starts_with("[Contenu déjà fourni")
            {
                continue;
            }
            let resolved_p = tools::resolve_tool_path(&path);
            let old = std::fs::read_to_string(&resolved_p).unwrap_or_default();
            let diff_lines = diff::computed_diff(&old, &code);
            new_diffs.push(diff::PendingDiff {
                id: format!(
                    "diff_{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                ),
                path: resolved_p,
                old_content: old,
                new_content: code,
                diff_lines,
                status: diff::DiffStatus::Pending,
            });
        }
        (response.to_string(), new_diffs)
    } else {
        match flow::decide_editor_action(response, wants_code, detected_language) {
            flow::EditorAction::Insert { language, code } => {
                let target_file = flow::find_target_file(user_message, file_name);
                let display_name = target_file.clone().unwrap_or_else(|| file_name.to_string());
                let target_p = if let Some(ref path) = target_file {
                    path.clone()
                } else {
                    tools::resolve_tool_path(&display_name)
                };
                let old_code = std::fs::read_to_string(&target_p).unwrap_or_default();
                let diff_lines = diff::computed_diff(&old_code, &code);
                new_diffs.push(diff::PendingDiff {
                    id: format!(
                        "diff_{}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis()
                    ),
                    path: target_p,
                    old_content: old_code,
                    new_content: code,
                    diff_lines,
                    status: diff::DiffStatus::Pending,
                });
                (flow::insertion_confirmation(language), new_diffs)
            }
            flow::EditorAction::ShowResponse => (response.to_string(), new_diffs),
        }
    }
}

/// Orchestrates an entire LLM agent turn: builds contexts, streams output, runs tools and fallbacks.
#[allow(clippy::too_many_arguments)] // an agent turn legitimately needs this much context
pub async fn run_agent_turn(
    user_message: String,
    spec_mode: bool,
    history: Vec<Message>,
    current_editor_code: String,
    current_editor_file: String,
    project_root: std::path::PathBuf,
    pinned_paths: Vec<std::path::PathBuf>,
    pending_diffs: Vec<diff::PendingDiff>,
    wants_code: bool,
    detected_language: SupportedLanguage,
    mut on_chunk: impl FnMut(&str),
    on_tool_diff: impl FnMut(diff::PendingDiff),
) -> Result<(String, Vec<diff::PendingDiff>), String> {
    let api_key_config = config::ApiKeyConfig::load();
    let model = api_key_config
        .model
        .clone()
        .unwrap_or_else(|| ALBERT_MODEL.to_string());

    if let Err(msg) = api_key_config.validate() {
        return Err(format!(
            "⚠️ {}\n\nVeuillez configurer votre clé dans les Réglages (⚙) et réessayer.",
            msg
        ));
    }

    let endpoint = api_key_config
        .endpoint
        .unwrap_or_else(|| ALBERT_ENDPOINT.to_string());
    let api_key = api_key_config.key;

    println!(
        "[DEBUG] Prompt envoyé (spec_mode={}) : {}",
        spec_mode, user_message
    );

    let active_context = build_active_context(&pending_diffs, &history);
    let history_texts: Vec<String> = history.iter().map(|m| m.content.clone()).collect();

    let lower_prompt = user_message.to_lowercase();
    let already_answered_specs = user_message.contains("[RÉPONSES AUX SPÉCIFICATIONS]")
        || lower_prompt.contains("fais au mieux")
        || lower_prompt.contains("plus de question")
        || lower_prompt.contains("pas de question")
        || lower_prompt.contains("sans question")
        || lower_prompt.contains("nouveau projet")
        || lower_prompt.contains("créer un")
        || lower_prompt.contains("creer un")
        || lower_prompt.contains("génère")
        || lower_prompt.contains("genere")
        || history
            .iter()
            .rev()
            .take(4)
            .any(|m| m.content.contains("SPEC_QUESTIONS_JSON:"));

    let is_audit = api::policy::analyze_intent(&lower_prompt, &user_message, "", &history).audit_intent;

    // `write_auto_plan` écrit sur disque : exécuté hors du runtime async (uniquement hors mode audit).
    let plan_file = if is_audit {
        None
    } else {
        let message = user_message.clone();
        let history_texts = history_texts.clone();
        tokio::task::spawn_blocking(move || tools::write_auto_plan(&message, &history_texts))
            .await
            .unwrap_or(None)
    };
    if let Some(plan_file) = plan_file {
        on_chunk(&format!(
            "🛡️ `write_plan` · `{}` ✅ — 0 ms (ajouté automatiquement au `.gitignore`)\n",
            plan_file
        ));
    }

    // `detect_project_snapshot` et `build_pinned_context` parcourent le disque : hors runtime async.
    let (project_snapshot, pinned_context) = {
        let message = user_message.clone();
        let history_texts = history_texts.clone();
        let pinned_paths = pinned_paths.clone();
        tokio::task::spawn_blocking(move || {
            let snap = tools::detect_project_snapshot(&message, &history_texts);
            let pinned = build_pinned_context(&pinned_paths);
            (snap, pinned)
        })
        .await
        .unwrap_or_default()
    };
    let env_context = format!(
        "{}{}",
        build_env_context(
            &current_editor_code,
            &current_editor_file,
            &project_snapshot,
            &project_root,
        ),
        pinned_context
    );

    let plan_path = project_root.join("plan.md");
    let plan_context = if let Ok(plan_text) = tokio::fs::read_to_string(&plan_path).await {
        let truncated = tools::truncate_output(&plan_text, 25, 1200);
        format!(
            "\n\n[PLAN DU PROJET ACTIF (`{}`)]\n{}",
            plan_path.to_string_lossy().replace('\\', "/"),
            truncated
        )
    } else {
        String::new()
    };

    let system_prompt = build_system_prompt(
        spec_mode,
        already_answered_specs,
        &env_context,
        &active_context,
        &plan_context,
    );

    let result = api::run_agent_loop_stream(
        &endpoint,
        &api_key,
        &model,
        &system_prompt,
        &history,
        &user_message,
        spec_mode,
        |chunk| on_chunk(chunk),
        on_tool_diff,
    )
    .await;

    match result {
        Ok(response) => {
            let was_cancelled = response.contains("⏹") || crate::session::default_session().is_cancel_requested();

            if !pending_diffs.is_empty()
                || response.contains("SPEC_QUESTIONS_JSON:")
                || response.contains("[RECOMMEND_SPEC_MODE]")
                || is_audit
                || was_cancelled
            {
                Ok((response, Vec::new()))
            } else {
                // L'extraction lit des fichiers sur disque : hors runtime async.
                let (fallback_resp, new_diffs) = tokio::task::spawn_blocking(move || {
                    extract_fallback_diffs(
                        &response,
                        &user_message,
                        &current_editor_file,
                        wants_code,
                        detected_language,
                    )
                })
                .await
                .map_err(|e| format!("Erreur interne (extraction des diffs): {}", e))?;
                Ok((fallback_resp, new_diffs))
            }
        }
        Err(raw_error) => {
            let category = api::classify_error(&raw_error);
            Err(format!(
                "{}\n\nDetails: {}",
                category.user_message(),
                raw_error
            ))
        }
    }
}
