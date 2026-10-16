//! Specification questions, architecture plans, and project snapshot detection.

use super::fs::{resolve_tool_path, search_dir_recursive, truncate_output, user_home_dir};
use serde_json::{Value, json};
use std::path::Path;

/// Si un fichier `plan.md` est généré ou modifié, garantit qu'il est inscrit dans le `.gitignore`
/// du dossier cible (ou de ses parents proches) s'il existe un `.gitignore`.
pub fn ensure_plan_in_gitignore(plan_path: &Path) -> bool {
    let file_name = plan_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !file_name.eq_ignore_ascii_case("plan.md") {
        return false;
    }

    let mut candidate_dirs = Vec::new();
    if let Some(parent) = plan_path.parent()
        && !parent.as_os_str().is_empty()
    {
        candidate_dirs.push(parent.to_path_buf());
        if let Some(grand_parent) = parent.parent()
            && !grand_parent.as_os_str().is_empty()
        {
            candidate_dirs.push(grand_parent.to_path_buf());
        }
    }
    candidate_dirs.push(std::path::PathBuf::from("."));

    let mut updated = false;
    for dir in candidate_dirs {
        let gitignore_path = dir.join(".gitignore");
        if gitignore_path.is_file() {
            if let Ok(existing) = std::fs::read_to_string(&gitignore_path) {
                let already_ignored = existing.lines().any(|line| {
                    let t = line.trim();
                    t == "plan.md" || t == "/plan.md" || t == "**/plan.md"
                });
                if !already_ignored {
                    let mut new_gitignore = existing;
                    if !new_gitignore.ends_with('\n') && !new_gitignore.is_empty() {
                        new_gitignore.push('\n');
                    }
                    new_gitignore
                        .push_str("\n# RustAgent Specification Plan\nplan.md\n**/plan.md\n");
                    if std::fs::write(&gitignore_path, new_gitignore).is_ok() {
                        updated = true;
                    }
                } else {
                    updated = true;
                }
            }
            break;
        }
    }
    updated
}

/// Génère 3 questions Security by Design contextuelles de secours lorsque le modèle appelle
/// `ask_spec_questions` avec des arguments vides ou incomplets.
pub fn fallback_spec_questions(user_hint: &str) -> Vec<Value> {
    let lower = user_hint.to_lowercase();
    if lower.contains("pdf") || lower.contains("menu") || lower.contains("export") {
        vec![
            json!({
                "id": "stack_pdf",
                "question": "Quelle stack technique souhaites-tu pour créer et exporter le document PDF ?",
                "options": [
                    "(Recommandé) Application Web HTML/CSS/JS autonome avec export PDF intégré (zéro serveur)",
                    "Script / Application Python (génération PDF locale propre & isolée)",
                    "Projet Node.js / TypeScript modulaire"
                ],
                "security_note": "Exécution 100% locale sans envoi de données vers un service tiers."
            }),
            json!({
                "id": "content_ux",
                "question": "Comment souhaites-tu personnaliser le contenu (ex: Entrée, Plat, Dessert & thème visuel) ?",
                "options": [
                    "(Recommandé) Éditeur interactif avec aperçu en direct + thème pré-configuré modifiable",
                    "Modèle prêt à l'emploi directement exportable en 1 clic",
                    "Configuration via fichier structuré (JSON / Markdown)"
                ]
            }),
            json!({
                "id": "sec_validation",
                "question": "Quelles protections Security by Design appliquer aux saisies et à l'export ?",
                "options": [
                    "(Recommandé) Échappement strict anti-XSS/injection + validation de longueur des champs",
                    "Validation standard des champs texte uniquement"
                ]
            }),
        ]
    } else if lower.contains("chiffr")
        || lower.contains("aes")
        || lower.contains("crypto")
        || lower.contains("pbkdf")
        || lower.contains("bkdf")
        || lower.contains("mot de passe")
    {
        vec![
            json!({
                "id": "kdf_cipher",
                "question": "Quels paramètres cryptographiques Security by Design souhaites-tu ?",
                "options": [
                    "(Recommandé) PBKDF2-HMAC-SHA256 (600 000 itérations, sel 16 octets) + AES-256-GCM (nonce 12 octets)",
                    "Argon2id + AES-256-GCM authentifié",
                    "Clé symétrique 256 bits générée aléatoirement (CSPRNG)"
                ],
                "security_note": "Sel et nonce uniques générés via CSPRNG à chaque chiffrement."
            }),
            json!({
                "id": "ux_mode",
                "question": "Quel mode d'utilisation souhaites-tu pour ce projet ?",
                "options": [
                    "(Recommandé) CLI complète (chiffrement/déchiffrement avec saisie masquée getpass)",
                    "Module réutilisable + script d'exemple et tests automatisés",
                    "Interface graphique / Web locale"
                ]
            }),
            json!({
                "id": "file_safety",
                "question": "Comment gérer les fichiers source et chiffrés sur le disque ?",
                "options": [
                    "(Recommandé) Écriture atomique dans un fichier .enc séparé + vérification du tag GCM avant écriture",
                    "Remplacement direct du fichier cible après chiffrement"
                ]
            }),
        ]
    } else {
        vec![
            json!({
                "id": "architecture",
                "question": "Quelle architecture privilégier pour ce projet ?",
                "options": [
                    "(Recommandé) Architecture modulaire concise (1 à 3 fichiers clairs, typés et sécurisés)",
                    "Fichier unique autonome (tout-en-un, exécution immédiate)",
                    "Architecture complète avec tests unitaires intégrés"
                ],
                "security_note": "Séparation claire de la logique métier et validation systématique des entrées."
            }),
            json!({
                "id": "interface",
                "question": "Quel type d'interface ou d'interaction souhaites-tu ?",
                "options": [
                    "(Recommandé) Interface interactive moderne clé-en-main avec valeurs par défaut",
                    "Ligne de commande (CLI) directe et rapide",
                    "API / Bibliothèque injectable dans un autre module"
                ]
            }),
            json!({
                "id": "security",
                "question": "Quel niveau de contrôle Security by Design appliquer ?",
                "options": [
                    "(Recommandé) Validation stricte des entrées, moindre privilège et gestion robuste des erreurs",
                    "Protections essentielles standard"
                ]
            }),
        ]
    }
}

/// Normalise n'importe quel format d'arguments envoyé par le LLM pour `ask_spec_questions`
/// (tableau direct, JSON stringifié, question unique à la racine, clés alternatives `choices`/`items`/`text`)
/// et garantit qu'au moins 2 à 3 questions avec options cliquables sont toujours retournées.
pub fn normalize_spec_questions(args: &Value, user_hint: &str) -> Value {
    let default_options = || -> Vec<String> {
        vec![
            "(Recommandé) Standard sécurisé (Security by Design)".to_string(),
            "Version simplifiée et autonome".to_string(),
            "Configuration personnalisée (préciser dans Autre)".to_string(),
        ]
    };

    let extract_options = |val: &Value| -> Vec<String> {
        let mut opts = Vec::new();
        match val {
            Value::Array(arr) => {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        let t = s.trim();
                        if !t.is_empty() {
                            opts.push(t.to_string());
                        }
                    } else if let Some(obj) = item.as_object() {
                        for k in &["label", "text", "value", "title", "name", "option"] {
                            if let Some(s) = obj.get(*k).and_then(|v| v.as_str()) {
                                let t = s.trim();
                                if !t.is_empty() {
                                    opts.push(t.to_string());
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            Value::String(s) => {
                let trimmed = s.trim();
                if let Ok(parsed_arr) = serde_json::from_str::<Value>(trimmed)
                    && parsed_arr.is_array()
                {
                    for item in parsed_arr.as_array().unwrap() {
                        if let Some(st) = item.as_str()
                            && !st.trim().is_empty()
                        {
                            opts.push(st.trim().to_string());
                        }
                    }
                }
                if opts.is_empty() {
                    let sep = if trimmed.contains(';') {
                        ';'
                    } else if trimmed.contains('|') {
                        '|'
                    } else {
                        ','
                    };
                    for part in trimmed.split(sep) {
                        let clean = part.trim().trim_matches('"').trim_matches('\'');
                        if !clean.is_empty() {
                            opts.push(clean.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
        if opts.len() < 2 {
            for def in default_options() {
                if opts.len() >= 3 {
                    break;
                }
                if !opts.contains(&def) {
                    opts.push(def);
                }
            }
        }
        opts
    };

    let normalize_one = |idx: usize, item: &Value| -> Option<Value> {
        match item {
            Value::String(s) => {
                let q_text = s.trim();
                if q_text.is_empty() {
                    return None;
                }
                Some(json!({
                    "id": format!("q_{}", idx + 1),
                    "question": q_text,
                    "options": default_options()
                }))
            }
            Value::Object(map) => {
                let mut q_text = String::new();
                for k in &[
                    "question",
                    "text",
                    "title",
                    "label",
                    "prompt",
                    "ask",
                    "description",
                    "name",
                ] {
                    if let Some(s) = map.get(*k).and_then(|v| v.as_str()) {
                        let t = s.trim();
                        if !t.is_empty() {
                            q_text = t.to_string();
                            break;
                        }
                    }
                }
                if q_text.is_empty() {
                    return None;
                }
                let id = map
                    .get("id")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|| format!("q_{}", idx + 1));

                let mut raw_opts = &Value::Null;
                for k in &[
                    "options",
                    "choices",
                    "items",
                    "values",
                    "answers",
                    "propositions",
                ] {
                    if let Some(v) = map.get(*k) {
                        raw_opts = v;
                        break;
                    }
                }
                let options = extract_options(raw_opts);
                let security_note = map
                    .get("security_note")
                    .or_else(|| map.get("note"))
                    .or_else(|| map.get("security"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());

                Some(json!({
                    "id": id,
                    "question": q_text,
                    "options": options,
                    "security_note": security_note
                }))
            }
            _ => None,
        }
    };

    let root_val: Value = if let Some(s) = args.as_str() {
        serde_json::from_str::<Value>(s.trim()).unwrap_or_else(|_| args.clone())
    } else {
        args.clone()
    };

    let mut raw_items: Vec<Value> = Vec::new();
    match &root_val {
        Value::Array(arr) => {
            raw_items.extend(arr.iter().cloned());
        }
        Value::Object(map) => {
            for key in &[
                "questions",
                "items",
                "specs",
                "clarifications",
                "choices",
                "list",
            ] {
                if let Some(v) = map.get(*key) {
                    match v {
                        Value::Array(arr) => {
                            raw_items.extend(arr.iter().cloned());
                            break;
                        }
                        Value::String(s) => {
                            if let Ok(parsed) = serde_json::from_str::<Value>(s.trim()) {
                                if let Some(arr) = parsed.as_array() {
                                    raw_items.extend(arr.iter().cloned());
                                    break;
                                } else if parsed.is_object() {
                                    raw_items.push(parsed);
                                    break;
                                }
                            }
                            for line in s.lines() {
                                let clean = line
                                    .trim()
                                    .trim_start_matches('-')
                                    .trim_start_matches('*')
                                    .trim();
                                if !clean.is_empty() {
                                    raw_items.push(Value::String(clean.to_string()));
                                }
                            }
                            if !raw_items.is_empty() {
                                break;
                            }
                        }
                        Value::Object(_) => {
                            raw_items.push(v.clone());
                            break;
                        }
                        _ => {}
                    }
                }
            }

            if raw_items.is_empty() {
                let has_direct_question = ["question", "text", "title", "label", "prompt", "ask"]
                    .iter()
                    .any(|k| map.get(*k).and_then(|v| v.as_str()).is_some());
                if has_direct_question {
                    raw_items.push(root_val.clone());
                } else {
                    for (k, v) in map {
                        if k.starts_with('_') || k == "context_summary" {
                            continue;
                        }
                        if v.is_object() {
                            raw_items.push(v.clone());
                        } else if v.is_array() {
                            raw_items.push(json!({
                                "id": k,
                                "question": k,
                                "options": v
                            }));
                        } else if let Some(s) = v.as_str()
                            && s.contains('?')
                        {
                            raw_items.push(Value::String(s.to_string()));
                        }
                    }
                }
            }
        }
        _ => {}
    }

    let mut normalized: Vec<Value> = raw_items
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| normalize_one(idx, item))
        .collect();

    if normalized.is_empty() {
        normalized = fallback_spec_questions(user_hint);
    }

    json!(normalized)
}

/// Détecte automatiquement un dossier de projet mentionné dans le prompt ou l'historique
/// et pré-charge la liste de ses fichiers principaux pour éviter au modèle d'appeler
/// `list_directory` / `search_files` en boucle.
pub fn detect_project_snapshot(user_msg: &str, history_texts: &[String]) -> String {
    let mut combined = String::from(user_msg);
    for h in history_texts.iter().rev().take(6) {
        combined.push('\n');
        combined.push_str(h);
    }

    let mut candidate_dirs: Vec<std::path::PathBuf> = Vec::new();

    for raw_token in combined.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '`' | '"' | '\'' | '(' | ')' | '[' | ']' | ',' | ';' | ':'
            )
    }) {
        let token = raw_token
            .trim()
            .trim_matches(|c: char| c == '.' || c == ',' || c == ';');
        if token.len() < 3 {
            continue;
        }
        if token.contains('/') || token.contains('\\') || token.starts_with('~') {
            let resolved = resolve_tool_path(token);
            let p = Path::new(&resolved);
            let dir = if p.is_dir() {
                Some(p.to_path_buf())
            } else {
                p.parent()
                    .filter(|par| par.is_dir())
                    .map(|par| par.to_path_buf())
            };
            if let Some(d) = dir
                && !candidate_dirs.contains(&d)
            {
                candidate_dirs.push(d);
            }
        } else if (token.contains('-') || token.contains('_')) && !token.starts_with('-') {
            if let Some(home) = user_home_dir() {
                for sub in &[
                    "Pictures",
                    "Documents",
                    "Desktop",
                    "Projects",
                    "Downloads",
                    "",
                ] {
                    let base = if sub.is_empty() {
                        home.clone()
                    } else {
                        home.join(sub)
                    };
                    let cand = base.join(token);
                    if cand.is_dir() && !candidate_dirs.contains(&cand) {
                        candidate_dirs.push(cand);
                    }
                }
            }
            let local_cand = Path::new(token).to_path_buf();
            if local_cand.is_dir() && !candidate_dirs.contains(&local_cand) {
                candidate_dirs.push(local_cand);
            }
        }
    }

    let cwd = std::env::current_dir().ok();
    let target_dir = candidate_dirs.into_iter().find(|d| {
        if let Some(ref c) = cwd
            && d == c
        {
            return false;
        }
        if let Some(home) = user_home_dir()
            && d == &home
        {
            return false;
        }
        true
    });

    let Some(project_dir) = target_dir else {
        return String::new();
    };

    let mut files = Vec::new();
    search_dir_recursive(&project_dir, "", 0, 4, &mut files);
    if files.len() > 25 {
        files.truncate(25);
    }

    let dir_display = project_dir.to_string_lossy().replace('\\', "/");
    let mut out = format!(
        "\n\n[PROJET CIBLE PRÉ-CHARGÉ AUTOMATIQUEMENT : `{}`]\nFichiers du projet (DÉJÀ CONNUS — INTERDICTION d'appeler `list_directory` ou `search_files` !) :\n{}",
        dir_display,
        if files.is_empty() {
            "(Dossier vide)".to_string()
        } else {
            files.join("\n")
        }
    );

    let project_plan = project_dir.join("plan.md");
    if let Ok(plan_text) = std::fs::read_to_string(&project_plan) {
        let truncated = truncate_output(&plan_text, 30, 1500);
        out.push_str(&format!(
            "\n\n[PLAN DU PROJET (`{}/plan.md`)]\n{}",
            dir_display, truncated
        ));
    }

    out
}

/// Génère et enregistre instantanément (0 ms, sans appel LLM) le fichier `plan.md`
/// et l'ajoute au `.gitignore` dès que l'utilisateur valide ses réponses aux spécifications.
pub fn write_auto_plan(user_msg: &str, history_texts: &[String]) -> Option<String> {
    if !user_msg.contains("[RÉPONSES AUX SPÉCIFICATIONS]")
        && !user_msg.to_lowercase().contains("fais au mieux")
    {
        return None;
    }

    let original_request = history_texts
        .iter()
        .rev()
        .find(|t| {
            !t.contains("SPEC_QUESTIONS_JSON:") && !t.starts_with("Hello!") && !t.starts_with("⚠️")
        })
        .cloned()
        .unwrap_or_default();

    let plan_md = format!(
        "# Plan d'Architecture & Security by Design (RustAgent)\n\n## 1. Objectif\n{}\n\n## 2. Spécifications & Choix Security by Design validés\n{}\n\n## 3. Directives de sécurité appliquées\n- Validation stricte des entrées et paramètres\n- Gestion sécurisée des secrets / clés et primitives cryptographiques\n- Moindre privilège et gestion propre des erreurs sans fuite d'information\n",
        original_request.trim(),
        user_msg.trim()
    );

    let plan_path = Path::new("plan.md");
    ensure_plan_in_gitignore(plan_path);
    let _ = std::fs::write(plan_path, &plan_md);
    Some("plan.md".to_string())
}
