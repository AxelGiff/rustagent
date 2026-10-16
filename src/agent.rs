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

/// Builds the environment and file context (home dir, cwd, project snapshot, active editor file).
pub fn build_env_context(editor_code: &str, editor_file: &str, project_snapshot: &str) -> String {
    let home_dir_str = tools::user_home_dir()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| "~".to_string());
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
        "\n\n[ENVIRONNEMENT & CHEMINS]\n- Dossier Home utilisateur (`~` / `$HOME`) : `{}`\n- Dossier courant de l'application : `{}`{}{}",
        home_dir_str, cwd_dir_str, project_snapshot, editor_context
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
2. IMPLÉMENTATION COMPLÈTE ET AUTONOME DU PROJET AVEC `write_file` :
- Dès que les spécifications sont validées (ou si la demande est déjà claire), `plan.md` est déjà généré automatiquement !
- CRÉATION DE PROJET COMPLET EN UN SEUL APPEL : Pour une application ou un projet complet (ex: index.html, style.css, script.js, composants), utilise le paramètre `files` de `write_file` pour générer TOUS les fichiers d'un coup :
  `write_file(files: [{{ \"path\": \"dossier/index.html\", \"content\": \"...code complet...\" }}, {{ \"path\": \"dossier/style.css\", \"content\": \"...code complet...\" }}, {{ \"path\": \"dossier/app.js\", \"content\": \"...code complet...\" }}])`.
  Si un seul fichier est nécessaire : `write_file(path: \"...\", content: \"...\")`.
- ⚠️ BUDGET DE SORTIE LIMITÉ : si les fichiers sont longs (ex : app Three.js avec index.html + style.css + app.js de plusieurs centaines de lignes), n'utilise PAS `files` en un seul appel — écris UN SEUL fichier par appel `write_file`, en commençant par le plus important. Un appel trop gros dépasse la limite de tokens, la réponse est tronquée et tes arguments arrivent vides (erreur \\\"argument 'path' manquant\\\").
- RÈGLE D'AUTONOMIE TOTALE : Ne laisse aucun fichier manquant, aucun placeholder de type \"[À compléter]\", et n'interromps pas la génération. L'application créée doit être 100% exécutable, belle et directement fonctionnelle.
- RÈGLE ABSOLUE : Tu dois TOUJOURS inclure le code source COMPLET dans chaque argument \"content\". N'appelle JAMAIS `write_file` avec des arguments vides !
3. VÉRIFICATION ET CORRECTION AUTOMATIQUE (OBLIGATOIRE) :
- Dès que tous les fichiers du projet sont écrits, appelle `verify_project` (ex: `{{ \"cwd\": \"Pictures/montage-pc\" }}`) : il compile/teste le projet sur une COPIE TEMPORAIRE (le projet réel n'est PAS modifié).
- Lis attentivement les erreurs renvoyées (section STDERR), corrige les fichiers concernés avec `edit_file` (ou `write_file`), puis rappelle `verify_project`.
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
            "Tu es RustAgent en **⚡ Mode Quick Edit**.

DIRECTIVES DU MODE QUICK EDIT (EXÉCUTION RAPIDE ET COMPLÈTE) :
1. CRÉATION OU MODIFICATION DIRECTE :
- Agis IMMÉDIATEMENT en appelant l'outil natif `write_file` :
  * Si plusieurs PETITS fichiers pour un projet complet : utilise `files: [{{ \"path\": \"...\", \"content\": \"...\" }}, ...]` pour créer tous les fichiers d'un coup.
  * Si les fichiers sont longs (projet volumineux) : écris UN SEUL fichier par appel `write_file` — sinon la réponse dépasse la limite de tokens, est tronquée, et les arguments sont perdus.
  * Si un seul fichier : `write_file(path: \"...\", content: \"...code complet...\")` ou `edit_file` pour une modification ciblée.
- RÈGLE D'AUTONOMIE : Fournis toujours le code complet sans placeholder afin que le projet fonctionne immédiatement.
- VÉRIFICATION : Après avoir écrit ou modifié du code, appelle `verify_project` (ex: `{{ \"cwd\": \"<dossier du projet>\" }}`) pour compiler/tester sur une copie temporaire, et corrige les erreurs signalées avant de conclure.
2. REDIRECTION VERS MODE SPEC (UNIQUEMENT SI ARCHITECTURE COMPLEXE NON SPÉCIFIÉE) :
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

    let base_dir = if user_message.to_lowercase().contains("pictures") {
        Some("Pictures/halloween-menu")
    } else {
        None
    };

    let extracted_blocks = tools::extract_code_blocks_from_text(response, base_dir);
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
    pending_diffs: Vec<diff::PendingDiff>,
    wants_code: bool,
    detected_language: SupportedLanguage,
    mut on_chunk: impl FnMut(&str),
    on_tool_diff: impl FnMut(diff::PendingDiff),
) -> Result<(String, Vec<diff::PendingDiff>), String> {
    let api_key_config = config::ApiKeyConfig::load();
    if let Err(msg) = api_key_config.validate() {
        return Err(format!(
            "⚠️ {}\n\nPlease configure your API key and try again.",
            msg
        ));
    }

    let endpoint = api_key_config
        .endpoint
        .clone()
        .unwrap_or_else(|| ALBERT_ENDPOINT.to_string());
    let model = api_key_config
        .model
        .clone()
        .unwrap_or_else(|| ALBERT_MODEL.to_string());
    let api_key = api_key_config.key.clone();

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

    // `write_auto_plan` écrit sur disque : exécuté hors du runtime async.
    let plan_file = {
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

    // `detect_project_snapshot` parcourt récursivement le disque : hors runtime async.
    let project_snapshot = {
        let message = user_message.clone();
        let history_texts = history_texts.clone();
        tokio::task::spawn_blocking(move || {
            tools::detect_project_snapshot(&message, &history_texts)
        })
        .await
        .unwrap_or_default()
    };
    let env_context = build_env_context(
        &current_editor_code,
        &current_editor_file,
        &project_snapshot,
    );

    let plan_context = if let Ok(plan_text) = tokio::fs::read_to_string("plan.md").await {
        let truncated = tools::truncate_output(&plan_text, 25, 1200);
        format!(
            "\n\n[PLAN DE LA FONCTIONNALITÉ PRÉVUE (`plan.md`)]\n{}",
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
            if !pending_diffs.is_empty()
                || response.contains("SPEC_QUESTIONS_JSON:")
                || response.contains("[RECOMMEND_SPEC_MODE]")
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
