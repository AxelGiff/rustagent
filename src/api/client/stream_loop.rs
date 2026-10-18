//! Orchestration of the streaming, multi-turn agent loop.

use super::super::error::{MAX_ATTEMPTS, backoff_delay};
use super::super::parser::{describe_tool_call, parse_lenient_tool_args};
use super::super::stream::{cancelled_summary, is_cancel_requested, reset_cancel};
use super::super::types::ChatMessage;
use super::policy;
use super::sse::DeltaAccumulator;
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::time::Duration;

/// A single step of the agent's self-maintained plan.
#[derive(Debug, Clone)]
struct Task {
    id: String,
    title: String,
    status: String,
}

/// Parse the `tasks` array of an `update_tasks` call.
fn parse_tasks(args: &Value) -> Vec<Task> {
    let Some(arr) = args.get("tasks").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let title = t
                .get("title")
                .or_else(|| t.get("task"))
                .or_else(|| t.get("content"))
                .or_else(|| t.get("description"))
                .and_then(|v| v.as_str())?
                .trim()
                .to_string();
            if title.is_empty() {
                return None;
            }
            let id = t
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| (i + 1).to_string());
            let status_raw = t
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending")
                .trim()
                .to_lowercase();
            let status = match status_raw.as_str() {
                "done" | "completed" | "complete" | "terminé" | "termine" | "fait" => "done",
                "in_progress" | "in-progress" | "doing" | "en cours" | "en_cours" | "current" => {
                    "in_progress"
                }
                _ => "pending",
            }
            .to_string();
            Some(Task { id, title, status })
        })
        .collect()
}

/// Render a task list (checkbox style) for display and for the model.
fn render_tasks(tasks: &[Task]) -> String {
    tasks
        .iter()
        .map(|t| {
            let mark = match t.status.as_str() {
                "done" => "[x]",
                "in_progress" => "[~]",
                _ => "[ ]",
            };
            format!("{} {}. {}", mark, t.id, t.title)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Tasks that are not yet complete.
fn remaining_tasks(tasks: &[Task]) -> Vec<&Task> {
    tasks.iter().filter(|t| t.status != "done").collect()
}

/// Boucle agentique multi-tours native supportant le streaming SSE, l'historique de conversation
/// et la détection d'aperçus de diffs nécessitant validation utilisateur.
/// Si `spec_mode` est vrai (Mode Spec), les outils `ask_spec_questions` et `write_plan` sont actifs.
#[allow(clippy::too_many_arguments)] // the streaming loop needs the full turn context
pub async fn run_agent_loop_stream<C, D>(
    endpoint: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
    history: &[crate::Message],
    user_prompt: &str,
    spec_mode: bool,
    mut on_chunk: C,
    mut on_pending_diff: D,
) -> Result<String, String>
where
    C: FnMut(&str),
    D: FnMut(crate::diff::PendingDiff),
{
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("Erreur client HTTP: {}", e))?;
    let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));

    let lower_prompt = user_prompt.to_lowercase();
    let intent = policy::analyze_intent(&lower_prompt, user_prompt, system_prompt, history);

    let tools_payload: Vec<Value> =
        policy::select_initial_tools(crate::tools::get_tool_definitions(), &intent, spec_mode);
    let has_tools = !tools_payload.is_empty();
    reset_cancel();

    // Repart d'un registre de fichiers générés vide à chaque nouveau tour.
    crate::tools::clear_staging();

    // Budget de sortie du modèle. Un tour qui tente d'écrire plusieurs gros fichiers
    // d'un coup peut dépasser cette limite : le flux est alors coupé (`finish_reason:
    // "length"`) et les arguments de l'outil arrivent tronqués ou vides.
    // Surchargeable sans recompiler via `RUSTAGENT_MAX_TOKENS`.
    let max_tokens: u32 = std::env::var("RUSTAGENT_MAX_TOKENS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(8192);

    let mut messages = vec![ChatMessage {
        role: "system".to_string(),
        content: Some(system_prompt.to_string()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }];

    // Inclure l'historique récent (jusqu'à 12 messages) pour maintenir le fil de
    // discussion sans alourdir le prompt.
    let history_slice = if history.len() > 12 {
        &history[history.len() - 12..]
    } else {
        history
    };

    for msg in history_slice {
        if msg.role == crate::Role::AI
            && (msg.content.starts_with("Hello!") || msg.content.starts_with("⚠️"))
        {
            continue;
        }
        let role = match msg.role {
            crate::Role::User => "user",
            crate::Role::AI => "assistant",
        };
        if !msg.content.trim().is_empty() {
            messages.push(ChatMessage {
                role: role.to_string(),
                content: Some(crate::tools::truncate_output(&msg.content, 60, 4000)),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            });
        }
    }

    // Ajouter le prompt actuel de l'utilisateur
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: Some(user_prompt.to_string()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    });

    let mut accumulated_content = String::new();
    let mut tool_executions_count: usize = 0;
    let mut consecutive_read_turns: usize = 0;
    let mut total_read_calls: usize = 0;
    let mut list_dir_calls: usize = 0;
    let mut created_files: Vec<String> = Vec::new();
    let mut active_project_dir: Option<String> = None;
    let mut tasks: Vec<Task> = Vec::new();
    let mut continuation_nudges: usize = 0;

    // When the user asks to modify/fix existing code, give the model room to
    // actually read the code (and re-read after a failed edit). Project creation
    // keeps a tighter budget that pushes it to write without over-exploring.
    let editing = intent.modification_intent;
    let max_read_turns: usize = if editing { 6 } else { 2 };
    let max_read_calls_per_turn: usize = if editing { 6 } else { 3 };
    let max_total_read_calls: usize = if editing { 16 } else { 4 };
    // A feature may need many read/edit/verify cycles: keep a generous budget.
    let max_agent_iterations: usize = if editing { 30 } else { 20 };

    for iteration in 0..max_agent_iterations {
        // La dernière itération sert uniquement à conclure proprement.
        let is_last_iteration = iteration == max_agent_iterations - 1;
        // Les outils restent disponibles le plus longtemps possible : un agent qui
        // doit finir une fonctionnalité ne doit pas être amputé en plein travail.
        let allow_tools = has_tools;

        // Dès que le budget de lecture (1 tour max) est atteint,
        // on retire TOUS les outils d'exploration (`read_file`, `list_directory`, `search_files`, `run_command`)
        // pour forcer le modèle à passer immédiatement à `write_file` / `edit_file` / `ask_spec_questions`.
        let active_tools = policy::active_tools_for_iteration(
            &tools_payload,
            allow_tools,
            consecutive_read_turns,
            total_read_calls,
            list_dir_calls,
            max_read_turns,
            max_total_read_calls,
        );
        let has_active_tools = active_tools
            .as_ref()
            .map(|t| !t.is_empty())
            .unwrap_or(false);

        // Tronquage des sorties d'outils volumineuses (lecture, commandes) des tours anciens pour ménager la mémoire
        policy::truncate_old_tool_outputs(&mut messages);

        let history_has_tools = messages
            .iter()
            .any(|m| m.role == "tool" || m.tool_calls.is_some());
        let tools_to_send = if has_active_tools {
            active_tools
        } else if history_has_tools && has_tools {
            Some(tools_payload.clone())
        } else {
            None
        };
        let send_tool_choice = if has_active_tools {
            Some("auto".to_string())
        } else {
            None
        };

        let req = json!({
            "model": model,
            "messages": messages,
            "tools": tools_to_send,
            "tool_choice": send_tool_choice,
            "stream": true,
            "max_tokens": max_tokens
        });

        // L'envoi de la requête est protégé par un timeout de 25s pour ne jamais bloquer indéfiniment.
        let mut attempt: u32 = 0;
        let res = loop {
            if is_cancel_requested() {
                break None;
            }
            let send_fut = client.post(&url).bearer_auth(api_key).json(&req).send();
            let sent = tokio::time::timeout(Duration::from_secs(25), send_fut).await;
            match sent {
                Ok(Ok(r)) if r.status().is_success() => break Some(r),
                Ok(Ok(r)) => {
                    let status = r.status();
                    let transient = status.as_u16() == 429 || status.is_server_error();
                    if transient && attempt + 1 < MAX_ATTEMPTS {
                        tokio::time::sleep(backoff_delay(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    let err_text = r.text().await.unwrap_or_default();
                    eprintln!("[RUSTAGENT API ERROR] Status {}: {}", status, err_text);
                    if !created_files.is_empty() {
                        let files_summary = format!(
                            "\n\n✅ **Les fichiers demandés ont été préparés dans l'onglet Diff ({} fichier(s)) :**\n{}\n\n*(Vous pouvez vérifier et valider les modifications ci-contre.)*",
                            created_files.len(),
                            created_files
                                .iter()
                                .map(|f| format!("- `{}`", f))
                                .collect::<Vec<_>>()
                                .join("\n")
                        );
                        accumulated_content.push_str(&files_summary);
                        on_chunk(&files_summary);
                        return Ok(accumulated_content);
                    }
                    if !accumulated_content.trim().is_empty() {
                        let clean_err = err_text.trim();
                        let err_suffix = if clean_err.is_empty() {
                            String::new()
                        } else {
                            format!(": {}", crate::tools::truncate_output(clean_err, 2, 160))
                        };
                        return Ok(format!(
                            "{}\n\n⚠️ *Cycle terminé (HTTP {}{}), les opérations précédentes ont été conservées.*",
                            accumulated_content, status, err_suffix
                        ));
                    }
                    return Err(format!("Erreur API (HTTP {}): {}", status, err_text));
                }
                Ok(Err(e)) => {
                    if attempt + 1 < MAX_ATTEMPTS {
                        tokio::time::sleep(backoff_delay(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    if !created_files.is_empty() {
                        let files_summary = format!(
                            "\n\n✅ **Les fichiers demandés ont été préparés dans l'onglet Diff ({} fichier(s)) :**\n{}\n\n*(Vous pouvez vérifier et valider les modifications ci-contre.)*",
                            created_files.len(),
                            created_files
                                .iter()
                                .map(|f| format!("- `{}`", f))
                                .collect::<Vec<_>>()
                                .join("\n")
                        );
                        accumulated_content.push_str(&files_summary);
                        on_chunk(&files_summary);
                        return Ok(accumulated_content);
                    }
                    if !accumulated_content.trim().is_empty() {
                        return Ok(format!(
                            "{}\n\n⚠️ *Connexion interrompue en fin de tour ({}), les opérations précédentes ont été conservées.*",
                            accumulated_content, e
                        ));
                    }
                    return Err(format!("Erreur réseau: {}", e));
                }
                Err(_) => {
                    if attempt + 1 < MAX_ATTEMPTS {
                        tokio::time::sleep(backoff_delay(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    if !accumulated_content.trim().is_empty() {
                        return Ok(accumulated_content);
                    }
                    return Err(
                        "Délai d'attente dépassé lors de la connexion à l'API Albert (25s)."
                            .to_string(),
                    );
                }
            }
        };
        let Some(res) = res else {
            return Ok(cancelled_summary(&accumulated_content));
        };

        let mut stream = res.bytes_stream();
        let mut buffer = String::new();
        let mut byte_buffer: Vec<u8> = Vec::new();
        let mut acc = DeltaAccumulator::new();

        loop {
            if is_cancel_requested() {
                accumulated_content.push_str(acc.clean_content());
                return Ok(cancelled_summary(&accumulated_content));
            }
            let next = match tokio::time::timeout(Duration::from_secs(30), stream.next()).await {
                Ok(n) => n,
                Err(_) => {
                    if !accumulated_content.trim().is_empty() || acc.has_tool_calls() {
                        break;
                    }
                    return Err(
                        "Le modèle n'a rien renvoyé depuis 30 s (timeout du flux).".to_string()
                    );
                }
            };
            let Some(chunk_res) = next else { break };
            let bytes = match chunk_res {
                Ok(b) => b,
                Err(e) => {
                    if !accumulated_content.trim().is_empty() || acc.has_tool_calls() {
                        break;
                    }
                    return Err(e.to_string());
                }
            };
            byte_buffer.extend_from_slice(&bytes);

            let valid_len = match std::str::from_utf8(&byte_buffer) {
                Ok(s) => {
                    buffer.push_str(s);
                    byte_buffer.len()
                }
                Err(e) => {
                    let valid_up_to = e.valid_up_to();
                    if valid_up_to > 0 {
                        let valid_s = std::str::from_utf8(&byte_buffer[..valid_up_to]).unwrap();
                        buffer.push_str(valid_s);
                    }
                    valid_up_to
                }
            };
            byte_buffer.drain(..valid_len);

            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim().to_string();
                buffer = buffer[pos + 1..].to_string();

                if let Some(rest) = line.strip_prefix("data: ") {
                    let data = rest.trim();
                    if data == "[DONE]" {
                        break;
                    }
                    match acc.process_data(data) {
                        Ok(pieces) => {
                            for piece in pieces {
                                accumulated_content.push_str(&piece);
                                on_chunk(&piece);
                            }
                        }
                        Err(err_msg) => {
                            if !accumulated_content.trim().is_empty() {
                                return Ok(accumulated_content);
                            }
                            return Err(format!("Erreur Stream API: {}", err_msg));
                        }
                    }
                }
            }
        }

        let trailing_safe = acc.finalize();
        if !trailing_safe.is_empty() {
            accumulated_content.push_str(&trailing_safe);
            on_chunk(&trailing_safe);
        }

        // Raison de fin : `"length"` signale une troncature par `max_tokens`.
        let finish_reason = acc.finish_reason().map(|s| s.to_string());
        let truncated = finish_reason.as_deref() == Some("length");
        if truncated {
            let notice = format!(
                "\n\n⚠️ *Réponse tronquée par la limite de sortie ({} tokens) — je découpe le travail en appels plus petits.*\n\n",
                max_tokens
            );
            accumulated_content.push_str(&notice);
            on_chunk(&notice);
        }

        let full_content = acc.take_full_content();

        if acc.has_tool_calls() {
            let calls_vec = acc.tool_calls(iteration);

            if calls_vec.is_empty() {
                continue;
            }

            let clean_assistant_content = crate::tools::strip_thinking_tags(&full_content);
            let assistant_content = if clean_assistant_content.trim().is_empty() {
                None
            } else {
                Some(clean_assistant_content.trim().to_string())
            };

            let assistant_msg = ChatMessage {
                role: "assistant".to_string(),
                content: assistant_content,
                tool_calls: Some(calls_vec.clone()),
                tool_call_id: None,
                name: None,
            };
            messages.push(assistant_msg);

            let has_mutation = calls_vec.iter().any(|c| {
                c.function.name == "write_file"
                    || c.function.name == "write_plan"
                    || c.function.name == "edit_file"
                    || c.function.name == "ask_spec_questions"
                    || c.function.name == "verify_project"
            });
            if has_mutation {
                consecutive_read_turns = 0;
            }
            let mut any_read_succeeded = false;
            let mut read_calls_this_turn: usize = 0;

            for call in &calls_vec {
                if is_cancel_requested() {
                    return Ok(cancelled_summary(&accumulated_content));
                }
                let is_explore_call = matches!(
                    call.function.name.as_str(),
                    "read_file" | "list_directory" | "search_files" | "run_command"
                );
                let is_list_call = call.function.name == "list_directory";

                if is_list_call && list_dir_calls >= 1 {
                    messages.push(ChatMessage {
                        role: "tool".to_string(),
                        content: Some(
                            "⚠️ STOP LISTING : Tu as déjà listé le dossier. Passe directement à `write_plan`, `write_file` ou `edit_file`.".to_string(),
                        ),
                        tool_calls: None,
                        tool_call_id: Some(call.id.clone()),
                        name: Some(call.function.name.clone()),
                    });
                    continue;
                }

                if is_explore_call
                    && (read_calls_this_turn >= max_read_calls_per_turn
                        || total_read_calls >= max_total_read_calls)
                {
                    messages.push(ChatMessage {
                        role: "tool".to_string(),
                        content: Some(
                            format!("⚠️ BUDGET D'EXPLORATION ATTEINT ({} lectures max) : N'appelle plus d'outils de lecture/recherche/commande. Passe MAINTENANT à l'écriture (`edit_file` pour une modification ciblée, `write_file` pour un nouveau fichier) ou à `verify_project`.", max_total_read_calls),
                        ),
                        tool_calls: None,
                        tool_call_id: Some(call.id.clone()),
                        name: Some(call.function.name.clone()),
                    });
                    continue;
                }

                tool_executions_count += 1;
                let mut args = parse_lenient_tool_args(&call.function.arguments);

                // `update_tasks` pilote la boucle (plan auto-maintenu de l'agent) :
                // traité ici plutôt que par l'exécuteur d'outils générique.
                if call.function.name == "update_tasks" {
                    let parsed = parse_tasks(&args);
                    let content = if parsed.is_empty() {
                        "⚠️ 'tasks' est vide ou invalide : renvoie la liste COMPLÈTE des tâches, chacune avec 'id', 'title' et 'status' ('pending' | 'in_progress' | 'done').".to_string()
                    } else {
                        tasks = parsed;
                        let remaining = remaining_tasks(&tasks).len();
                        format!(
                            "📋 Plan mis à jour — {} tâche(s), {} restante(s) :\n{}\n\n{}",
                            tasks.len(),
                            remaining,
                            render_tasks(&tasks),
                            if remaining == 0 {
                                "Toutes les tâches sont terminées. Si ce n'est pas déjà fait, lance `verify_project`, corrige si besoin, puis conclus."
                            } else {
                                "Continue avec la prochaine tâche incomplète. N'arrête PAS tant que toutes les tâches ne sont pas 'done'."
                            }
                        )
                    };
                    messages.push(ChatMessage {
                        role: "tool".to_string(),
                        content: Some(content),
                        tool_calls: None,
                        tool_call_id: Some(call.id.clone()),
                        name: Some(call.function.name.clone()),
                    });
                    if !tasks.is_empty() {
                        let notice = format!("\n\n📋 `update_tasks`\n{}\n\n", render_tasks(&tasks));
                        accumulated_content.push_str(&notice);
                        on_chunk(&notice);
                    }
                    continue;
                }

                // Détection et mise à jour automatique du dossier de projet actif (ex: C:/Users/vadzt/Pictures/halloween-menu)
                if call.function.name == "read_file" || call.function.name == "list_directory" {
                    let raw_p = ["path", "file", "dir", "directory"]
                        .iter()
                        .find_map(|k| {
                            args.get(*k)
                                .and_then(|v| v.as_str())
                                .filter(|s| !s.trim().is_empty())
                        })
                        .unwrap_or("");
                    let resolved = crate::tools::resolve_tool_path(raw_p);
                    let p = std::path::Path::new(&resolved);
                    if call.function.name == "list_directory" {
                        active_project_dir = Some(resolved.clone());
                    } else if let Some(parent) = p.parent() {
                        let parent_str = parent.to_string_lossy().replace('\\', "/");
                        if !parent_str.is_empty() && parent_str != "." {
                            active_project_dir = Some(parent_str);
                        }
                    }
                }

                if call.function.name == "ask_spec_questions" {
                    if let Some(obj) = args.as_object_mut() {
                        obj.insert("_user_hint".to_string(), json!(user_prompt));
                    } else {
                        args = json!({
                            "questions": args,
                            "_user_hint": user_prompt
                        });
                    }
                }

                // Support universel de `write_file` (mode batch, fichiers uniques, objets, arrays ou extraction Markdown)
                if call.function.name == "write_file" {
                    let mut extracted = crate::tools::extract_files_from_args(&args);
                    if extracted.is_empty() {
                        extracted =
                            crate::tools::extract_files_from_raw_str(&call.function.arguments);
                    }
                    if extracted.is_empty() && !full_content.is_empty() {
                        let base_dir = active_project_dir.as_deref().or_else(|| {
                            if user_prompt.to_lowercase().contains("pictures") {
                                Some("Pictures/halloween-menu")
                            } else {
                                None
                            }
                        });
                        extracted =
                            crate::tools::extract_code_blocks_from_text(&full_content, base_dir);
                    }
                    if !extracted.is_empty() {
                        let mut batch_outputs = Vec::new();
                        for (raw_p, code_content) in extracted {
                            if code_content.trim() == "[Contenu déjà fourni au tour précédent]"
                                || code_content.trim().starts_with("[Contenu déjà fourni")
                            {
                                continue;
                            }
                            let started = std::time::Instant::now();
                            let resolved = crate::tools::resolve_tool_path(&raw_p);
                            let target_p = if let Some(ref base_dir) = active_project_dir {
                                let clean_base = base_dir.trim_end_matches('/');
                                if resolved.starts_with(clean_base) {
                                    resolved
                                } else {
                                    let path_obj = std::path::Path::new(&resolved);
                                    if path_obj.is_absolute() {
                                        resolved
                                    } else {
                                        format!(
                                            "{}/{}",
                                            clean_base,
                                            resolved.trim_start_matches("./")
                                        )
                                    }
                                }
                            } else {
                                resolved
                            };

                            let item_res = crate::tools::execute_native_tool(
                                "write_file",
                                json!({ "path": target_p, "content": code_content }),
                            )
                            .await;
                            let elapsed_ms = started.elapsed().as_millis();
                            if let Some(diff) = item_res.pending_diff {
                                let path = diff.path.clone();
                                if !created_files.contains(&path) {
                                    created_files.push(path.clone());
                                }
                                on_pending_diff(diff);
                                let notice = format!(
                                    "\n\n📝 `write_file` · `{}` — diff prêt à valider ({} ms)\n\n",
                                    path, elapsed_ms
                                );
                                accumulated_content.push_str(&notice);
                                on_chunk(&notice);
                            }
                            batch_outputs.push(item_res.content);
                        }
                        messages.push(ChatMessage {
                            role: "tool".to_string(),
                            content: Some(batch_outputs.join("\n")),
                            tool_calls: None,
                            tool_call_id: Some(call.id.clone()),
                            name: Some(call.function.name.clone()),
                        });
                        continue;
                    }
                }

                // Si 'path' est manquant dans write_file, tenter de le déduire du texte du tour en cours
                if call.function.name == "write_file"
                    && (args.get("path").is_none()
                        || args
                            .get("path")
                            .and_then(|v| v.as_str())
                            .map(|s| s.trim().is_empty())
                            .unwrap_or(true))
                    && let Some(cand) =
                        crate::tools::extract_candidate_path_from_text(&full_content)
                {
                    args["path"] = json!(cand);
                }

                let detail = describe_tool_call(&call.function.name, &args);

                if call.function.name == "verify_project" {
                    let notice = "\n\n🔎 *Vérification du projet en cours (copie temporaire, la compilation peut prendre un moment)...*\n\n";
                    accumulated_content.push_str(notice);
                    on_chunk(notice);
                }

                // Exécution de l'outil natif ultra-rapide
                let started = std::time::Instant::now();
                // Command text (for `run_command`) captured before `args` is moved.
                let tool_command = args
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool_result = if call.function.name == "verify_project" {
                    // Vérification en streaming : chaque commande et sa sortie
                    // s'affichent en direct dans le transcript (style terminal).
                    let cwd = args
                        .get("cwd")
                        .or_else(|| args.get("path"))
                        .or_else(|| args.get("project"))
                        .or_else(|| args.get("dir"))
                        .and_then(|v| v.as_str())
                        .map(crate::tools::resolve_tool_path);
                    let report =
                        crate::tools::verify::run_verification_streaming(cwd.as_deref(), |chunk| {
                            accumulated_content.push_str(chunk);
                            on_chunk(chunk);
                        })
                        .await;
                    crate::tools::ToolResult {
                        content: report,
                        pending_diff: None,
                    }
                } else {
                    crate::tools::execute_native_tool(&call.function.name, args).await
                };
                let elapsed_ms = started.elapsed().as_millis();
                let failed = tool_result.content.starts_with("Erreur");
                if is_explore_call {
                    read_calls_this_turn += 1;
                    if !failed {
                        total_read_calls += 1;
                        if is_list_call {
                            list_dir_calls += 1;
                        }
                        if !has_mutation {
                            any_read_succeeded = true;
                        }
                    }
                }
                let status_icon = if failed { "❌" } else { "✅" };

                if let Some(spec_json) = tool_result
                    .content
                    .strip_prefix("SPEC_QUESTIONS_DISPLAYED:")
                {
                    let marker = format!("\n\nSPEC_QUESTIONS_JSON:{}\n", spec_json);
                    accumulated_content.push_str(&marker);
                    on_chunk(&marker);
                    return Ok(accumulated_content);
                }

                if let Some(diff) = tool_result.pending_diff {
                    let path = diff.path.clone();
                    if !created_files.contains(&path) {
                        created_files.push(path.clone());
                    }
                    on_pending_diff(diff);
                    let notice = format!(
                        "\n\n📝 `{}` · `{}` — diff prêt à valider ({} ms)\n\n",
                        call.function.name, path, elapsed_ms
                    );
                    accumulated_content.push_str(&notice);
                    on_chunk(&notice);
                } else if call.function.name == "write_plan" {
                    let notice = format!(
                        "\n\n🛡️ `write_plan` · `{}` {} — {} ms ({})\n\n",
                        if detail.is_empty() {
                            "plan.md".to_string()
                        } else {
                            detail
                        },
                        status_icon,
                        elapsed_ms,
                        tool_result.content
                    );
                    accumulated_content.push_str(&notice);
                    on_chunk(&notice);
                } else if call.function.name == "verify_project" {
                    let first_line = tool_result.content.lines().next().unwrap_or("").to_string();
                    let notice = format!(
                        "\n\n🔎 `verify_project` — {} ms\n{}\n\n",
                        elapsed_ms, first_line
                    );
                    accumulated_content.push_str(&notice);
                    on_chunk(&notice);
                } else if call.function.name == "run_command" {
                    // Affichage style terminal : la commande ET sa sortie.
                    let header = if detail.is_empty() {
                        tool_command.clone()
                    } else {
                        detail.clone()
                    };
                    let notice = format!(
                        "\n\n🖥️ `run_command` {} — {} ms\n$ {}\n{}\n\n",
                        status_icon, elapsed_ms, header, tool_result.content
                    );
                    accumulated_content.push_str(&notice);
                    on_chunk(&notice);
                } else {
                    let notice = if failed {
                        format!(
                            "\n\n⚙️ `{}` ❌ — {} ms ({})\n\n",
                            call.function.name, elapsed_ms, tool_result.content
                        )
                    } else {
                        format!(
                            "\n\n⚙️ `{}` {} {} — {} ms\n\n",
                            call.function.name,
                            if detail.is_empty() {
                                String::new()
                            } else {
                                format!("`{}`", detail)
                            },
                            status_icon,
                            elapsed_ms
                        )
                    };
                    accumulated_content.push_str(&notice);
                    on_chunk(&notice);
                }

                let mut content_to_send = tool_result.content;
                if call.function.name == "write_file" && failed {
                    eprintln!(
                        "[RUSTAGENT DEBUG] write_file échoué (finish_reason={:?}, args_len={}). Arguments bruts: {:?}",
                        finish_reason,
                        call.function.arguments.len(),
                        crate::tools::truncate_output(&call.function.arguments, 2, 200)
                    );
                    content_to_send = if truncated {
                        format!(
                            "⚠️ TA RÉPONSE A ÉTÉ TRONQUÉE (limite de {} tokens atteinte) : les arguments de 'write_file' sont arrivés vides ou incomplets. Écris désormais UN SEUL fichier par appel 'write_file' (n'envoie JAMAIS plusieurs gros fichiers d'un coup), en fournissant 'path' et 'content'. Commence par le fichier le plus important.",
                            max_tokens
                        )
                    } else {
                        "⚠️ ERREUR CRITIQUE : Tu as appelé 'write_file' avec des arguments vides ou invalides. Tu dois OBLIGATOIREMENT passer les paramètres JSON 'path' et 'content', par exemple : {\"path\": \"Pictures/mon-projet/index.html\", \"content\": \"...code complet...\"}. Fournis le code complet dès maintenant !".to_string()
                    };
                }
                if !has_mutation
                    && !failed
                    && (consecutive_read_turns + 1 >= max_read_turns
                        || total_read_calls >= max_total_read_calls)
                    && is_explore_call
                {
                    if spec_mode && !intent.specs_already_answered {
                        content_to_send.push_str(
                            "\n\n⚠️ ACTION OBLIGATOIRE AU PROCHAIN TOUR : Les outils de lecture sont maintenant désactivés. Si des choix manquent, appelle `ask_spec_questions` (1 à 3 questions max). Sinon, appelle `write_file` (avec `files: [...]`) ou `edit_file` !",
                        );
                    } else {
                        content_to_send.push_str(
                            "\n\n⚠️ ACTION OBLIGATOIRE AU PROCHAIN TOUR : Les outils de lecture sont maintenant désactivés. Génère TOUS les fichiers en un seul tour avec `write_file` (utilise `files: [{path, content}, ...]` s'il y a plusieurs fichiers) ou `edit_file` !",
                        );
                    }
                }

                messages.push(ChatMessage {
                    role: "tool".to_string(),
                    content: Some(content_to_send),
                    tool_calls: None,
                    tool_call_id: Some(call.id.clone()),
                    name: Some(call.function.name.clone()),
                });
            }

            // IMPORTANT : ne pas s'arrêter ici. On continue tant que le modèle
            // appelle des outils ; la boucle ne se termine que sur une réponse
            // finale (texte sans appel d'outil) ou à l'épuisement du budget.
            // (L'ancien arrêt après 1 ou 8 fichiers empêchait de finir une
            // fonctionnalité en un seul tour.)
            if any_read_succeeded {
                consecutive_read_turns += 1;
            }
            continue;
        }

        if !full_content.trim().is_empty() {
            // Relance l'agent s'il veut conclure alors qu'il reste du travail :
            // soit des tâches inachevées, soit aucune action encore effectuée
            // (l'utilisateur a demandé du code mais le modèle s'est contenté de
            // parler). Sans cela, l'agent « s'arrête » et il faut re-prompter.
            let outstanding = remaining_tasks(&tasks);
            let no_action = created_files.is_empty() && tool_executions_count == 0;
            let wants_work = spec_mode || intent.modification_intent;
            let should_continue = !is_last_iteration
                && continuation_nudges < 4
                && has_active_tools
                && (!outstanding.is_empty() || (no_action && wants_work));

            if should_continue {
                continuation_nudges += 1;
                messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: Some(full_content.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                });
                let nudge = if !outstanding.is_empty() {
                    format!(
                        "⚠️ Tu ne peux pas conclure : {} tâche(s) encore incomplète(s).\n{}\n\nAppelle IMMÉDIATEMENT l'outil nécessaire (`read_file`/`edit_file`/`write_file`/`verify_project`) pour avancer, puis `update_tasks` pour mettre à jour le plan. Ne termine qu'une fois TOUTES les tâches 'done' ET `verify_project` OK.",
                        outstanding.len(),
                        render_tasks(&tasks)
                    )
                } else {
                    "⚠️ Tu n'as encore rien modifié ni vérifié. N'explique pas : AGIS MAINTENANT. Commence par `update_tasks` (plan des étapes), puis `read_file` le code concerné et `edit_file`/`write_file`, et termine par `verify_project`.".to_string()
                };
                messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: Some(nudge),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                });
                continue;
            }

            let mut final_text =
                if !accumulated_content.trim().is_empty() && accumulated_content != full_content {
                    accumulated_content
                } else {
                    full_content
                };
            if !created_files.is_empty() && !final_text.contains("Fichiers préparés pour le projet")
            {
                let files_summary = format!(
                    "\n\n📁 **Fichiers préparés pour le projet :**\n{}",
                    created_files
                        .iter()
                        .map(|f| format!("- `{}`", f))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                final_text.push_str(&files_summary);
            }
            return Ok(final_text);
        }

        if tool_executions_count > 0 || !accumulated_content.trim().is_empty() {
            let mut summary = if !accumulated_content.trim().is_empty() {
                accumulated_content
            } else {
                "✅ Opérations terminées avec succès. Vous pouvez vérifier et valider les modifications ci-contre.".to_string()
            };
            if !created_files.is_empty() && !summary.contains("Fichiers préparés pour le projet")
            {
                let files_summary = format!(
                    "\n\n📁 **Fichiers préparés pour le projet :**\n{}",
                    created_files
                        .iter()
                        .map(|f| format!("- `{}`", f))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                summary.push_str(&files_summary);
            }
            return Ok(summary);
        }

        return Err("L'API a renvoyé une réponse vide sans appel d'outil.".to_string());
    }

    if tool_executions_count > 0 || !accumulated_content.trim().is_empty() {
        let note = if created_files.is_empty() {
            "\n\n*(L'analyse est terminée. Les fichiers ci-dessus ont été consultés. Vous pouvez demander d'appliquer les modifications ou d'écrire le code pour continuer.)*"
        } else {
            "\n\n*(Jalon d'exécution atteint : les étapes ci-dessus ont été préparées. Vous pouvez vérifier et valider les modifications ci-contre, ou envoyer un message pour continuer.)*"
        };
        let mut final_summary = if !accumulated_content.trim().is_empty() {
            format!("{}{}", accumulated_content.trim(), note)
        } else {
            format!(
                "✅ L'agent a traité {} étape(s).{}",
                tool_executions_count, note
            )
        };
        if !created_files.is_empty() && !final_summary.contains("Fichiers préparés pour le projet")
        {
            let files_summary = format!(
                "\n\n📁 **Fichiers préparés pour le projet :**\n{}",
                created_files
                    .iter()
                    .map(|f| format!("- `{}`", f))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            final_summary.push_str(&files_summary);
        }
        Ok(final_summary)
    } else {
        Err("Nombre maximum d'itérations atteint sans réponse finale du modèle.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tasks_with_aliases_and_status_normalization() {
        let args = json!({
            "tasks": [
                { "id": "1", "title": "Lire le fichier", "status": "done" },
                { "id": "2", "task": "Modifier la fonction", "status": "in_progress" },
                { "title": "Vérifier", "status": "pending" },
                { "title": "   ", "status": "pending" }
            ]
        });
        let tasks = parse_tasks(&args);
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].status, "done");
        assert_eq!(tasks[1].status, "in_progress");
        assert_eq!(tasks[1].title, "Modifier la fonction");
        // Missing id falls back to the 1-based index; blank titles are dropped.
        assert_eq!(tasks[2].title, "Vérifier");
        assert!(!tasks[2].id.is_empty());
    }

    #[test]
    fn renders_and_counts_remaining_tasks() {
        let args = json!({
            "tasks": [
                { "id": "a", "title": "Étape A", "status": "done" },
                { "id": "b", "title": "Étape B", "status": "pending" }
            ]
        });
        let tasks = parse_tasks(&args);
        let rendered = render_tasks(&tasks);
        assert!(rendered.contains("[x] a. Étape A"), "{rendered}");
        assert!(rendered.contains("[ ] b. Étape B"), "{rendered}");
        assert_eq!(remaining_tasks(&tasks).len(), 1);
        assert_eq!(remaining_tasks(&tasks)[0].id, "b");
    }

    #[test]
    fn empty_or_invalid_tasks_yield_nothing() {
        assert!(parse_tasks(&json!({})).is_empty());
        assert!(parse_tasks(&json!({ "tasks": "nope" })).is_empty());
    }
}
