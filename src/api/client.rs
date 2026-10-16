//! Multi-turn agent loops (streaming and non-streaming) for LLM interactions.

use std::time::Duration;
use futures_util::StreamExt;
use serde_json::{json, Value};
use super::error::{backoff_delay, MAX_ATTEMPTS};
use super::parser::{describe_tool_call, parse_lenient_tool_args};
use super::stream::{cancelled_summary, is_cancel_requested, reset_cancel};
use super::types::{ChatMessage, ChatCompletionRequest, ChatCompletionResponse, FunctionCall, ToolCall};

/// Boucle agentique multi-tours supportant `tool_choice: "auto"` avec les outils natifs Rust.
pub async fn run_agent_loop(
    endpoint: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));

    let tools_payload = crate::tools::get_tool_definitions();
    let has_tools = !tools_payload.is_empty();

    let mut messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: Some(system_prompt.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
        ChatMessage {
            role: "user".to_string(),
            content: Some(user_prompt.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
    ];

    for iteration in 0..6 {
        println!("[AGENT] Itération {}", iteration + 1);

        let req = ChatCompletionRequest {
            model: model.to_string(),
            messages: messages.clone(),
            tools: if has_tools { Some(tools_payload.clone()) } else { None },
            tool_choice: if has_tools { Some("auto".to_string()) } else { None },
            max_tokens: Some(8192),
        };

        let res = client
            .post(&url)
            .bearer_auth(api_key)
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("Erreur réseau: {}", e))?;

        if !res.status().is_success() {
            let err_text = res.text().await.unwrap_or_default();
            return Err(format!("Erreur API: {}", err_text));
        }

        let body: ChatCompletionResponse = res
            .json()
            .await
            .map_err(|e| format!("Erreur parsing JSON: {}", e))?;

        let choice = body.choices.into_iter().next().ok_or("Réponse API vide")?;
        let assistant_msg = choice.message;

        if let Some(tool_calls) = &assistant_msg.tool_calls {
            if !tool_calls.is_empty() {
                messages.push(assistant_msg.clone());

                for call in tool_calls {
                    println!("[TOOL EXEC] Appel de {} avec args: {}", call.function.name, call.function.arguments);

                    let args: Value = serde_json::from_str(&call.function.arguments)
                        .unwrap_or_else(|_| json!({}));

                    let tool_result = crate::tools::execute_native_tool(&call.function.name, args).await;

                    messages.push(ChatMessage {
                        role: "tool".to_string(),
                        content: Some(tool_result.content),
                        tool_calls: None,
                        tool_call_id: Some(call.id.clone()),
                        name: Some(call.function.name.clone()),
                    });
                }
                continue;
            }
        }

        if let Some(content) = assistant_msg.content {
            if !content.trim().is_empty() {
                return Ok(content);
            }
        }
    }

    Err("Nombre maximum d'itérations atteint sans réponse".to_string())
}

/// Boucle agentique multi-tours native supportant le streaming SSE, l'historique de conversation
/// et la détection d'aperçus de diffs nécessitant validation utilisateur.
/// Si `spec_mode` est vrai (Mode Spec), les outils `ask_spec_questions` et `write_plan` sont actifs.
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
    let specs_already_answered = user_prompt.contains("[RÉPONSES AUX SPÉCIFICATIONS]")
        || lower_prompt.contains("fais au mieux")
        || history.iter().rev().take(4).any(|m| m.content.contains("SPEC_QUESTIONS_JSON:"));
    let project_preloaded = system_prompt.contains("[PROJET CIBLE PRÉ-CHARGÉ AUTOMATIQUEMENT");
    let wants_terminal_cmd = ["terminal", "bash", "commande", "npm ", "cargo ", "pip ", "pytest"]
        .iter()
        .any(|k| lower_prompt.contains(k));
    let mentions_existing_folder = project_preloaded
        || user_prompt.contains('/')
        || user_prompt.contains('\\')
        || lower_prompt.contains("dans mon projet")
        || lower_prompt.contains("dans le dossier");

    let tools_payload: Vec<Value> = crate::tools::get_tool_definitions()
        .into_iter()
        .filter(|t| {
            let name = t
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("");
            if (!spec_mode || specs_already_answered) && name == "ask_spec_questions" {
                return false;
            }
            if specs_already_answered && name == "write_plan" {
                return false;
            }
            if !wants_terminal_cmd && name == "run_command" {
                return false;
            }
            if (project_preloaded || !mentions_existing_folder || specs_already_answered)
                && (name == "list_directory" || name == "search_files")
            {
                return false;
            }
            if !mentions_existing_folder && name == "read_file" {
                return false;
            }
            true
        })
        .collect();
    let has_tools = !tools_payload.is_empty();
    reset_cancel();

    let mut messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: Some(system_prompt.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
    ];

    // Inclure l'historique récent (jusqu'à 6 messages) pour maintenir le fil de discussion sans alourdir le prompt
    let history_slice = if history.len() > 6 {
        &history[history.len() - 6..]
    } else {
        history
    };

    for msg in history_slice {
        if msg.role == crate::Role::AI && (msg.content.starts_with("Hello!") || msg.content.starts_with("⚠️")) {
            continue;
        }
        let role = match msg.role {
            crate::Role::User => "user",
            crate::Role::AI => "assistant",
        };
        if !msg.content.trim().is_empty() {
            messages.push(ChatMessage {
                role: role.to_string(),
                content: Some(crate::tools::truncate_output(&msg.content, 25, 1200)),
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
    let max_read_turns: usize = 1;
    const MAX_TOTAL_READ_CALLS: usize = 2;
    const MAX_AGENT_ITERATIONS: usize = 14;

    for iteration in 0..MAX_AGENT_ITERATIONS {
        // En dernière itération, forcer le modèle à synthétiser en texte en désactivant les outils
        let is_last_iteration = iteration == MAX_AGENT_ITERATIONS - 1;
        let allow_tools = has_tools && !is_last_iteration;

        // Dès que le budget de lecture (1 tour max) est atteint,
        // on retire TOUS les outils d'exploration (`read_file`, `list_directory`, `search_files`, `run_command`)
        // pour forcer le modèle à passer immédiatement à `write_file` / `edit_file` / `ask_spec_questions`.
        let active_tools = if allow_tools {
            if consecutive_read_turns >= max_read_turns || total_read_calls >= MAX_TOTAL_READ_CALLS {
                let filtered: Vec<Value> = tools_payload
                    .iter()
                    .filter(|t| {
                        let name = t.get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("");
                        name == "write_file"
                            || name == "write_plan"
                            || name == "edit_file"
                            || name == "ask_spec_questions"
                    })
                    .cloned()
                    .collect();
                Some(filtered)
            } else if list_dir_calls >= 1 {
                let filtered: Vec<Value> = tools_payload
                    .iter()
                    .filter(|t| {
                        let name = t.get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("");
                        name != "list_directory"
                    })
                    .cloned()
                    .collect();
                Some(filtered)
            } else {
                Some(tools_payload.clone())
            }
        } else {
            None
        };
        let has_active_tools = active_tools.as_ref().map(|t| !t.is_empty()).unwrap_or(false);

        // Tronquage des sorties d'outils volumineuses (lecture, commandes) des tours anciens pour ménager la mémoire
        let total_msgs = messages.len();
        if total_msgs > 8 {
            for idx in 0..total_msgs.saturating_sub(4) {
                if messages[idx].role == "tool" {
                    if let Some(content) = &mut messages[idx].content {
                        if content.len() > 1000 {
                            *content = crate::tools::truncate_output(content, 20, 800);
                        }
                    }
                }
            }
        }

        let history_has_tools = messages.iter().any(|m| m.role == "tool" || m.tool_calls.is_some());
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
            "max_tokens": 8192
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
                            created_files.iter().map(|f| format!("- `{}`", f)).collect::<Vec<_>>().join("\n")
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
                            created_files.iter().map(|f| format!("- `{}`", f)).collect::<Vec<_>>().join("\n")
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
                    return Err("Délai d'attente dépassé lors de la connexion à l'API Albert (25s).".to_string());
                }
            }
        };
        let Some(res) = res else {
            return Ok(cancelled_summary(&accumulated_content));
        };

        let mut stream = res.bytes_stream();
        let mut buffer = String::new();
        let mut byte_buffer: Vec<u8> = Vec::new();
        let mut full_content = String::new();
        let mut tool_calls_map: std::collections::BTreeMap<usize, (String, String, String)> =
            std::collections::BTreeMap::new();
        let mut announced_tools: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
        let mut streamed_arg_bytes: usize = 0;
        let mut thinking_filter = crate::tools::ThinkingFilter::new();

        loop {
            if is_cancel_requested() {
                accumulated_content.push_str(&thinking_filter.clean_content);
                return Ok(cancelled_summary(&accumulated_content));
            }
            let next = match tokio::time::timeout(Duration::from_secs(30), stream.next()).await {
                Ok(n) => n,
                Err(_) => {
                    if !accumulated_content.trim().is_empty() || !tool_calls_map.is_empty() {
                        break;
                    }
                    return Err("Le modèle n'a rien renvoyé depuis 30 s (timeout du flux).".to_string());
                }
            };
            let Some(chunk_res) = next else { break };
            let bytes = match chunk_res {
                Ok(b) => b,
                Err(e) => {
                    if !accumulated_content.trim().is_empty() || !tool_calls_map.is_empty() {
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

                if line.starts_with("data: ") {
                    let data = line["data: ".len()..].trim();
                    if data == "[DONE]" {
                        break;
                    }
                    if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                        if let Some(err) = parsed.get("error") {
                            let err_msg = err["message"].as_str().unwrap_or(data);
                            if !accumulated_content.trim().is_empty() {
                                return Ok(accumulated_content);
                            }
                            return Err(format!("Erreur Stream API: {}", err_msg));
                        }
                        if let Some(choices) = parsed.get("choices").and_then(|c| c.as_array()) {
                            if let Some(first) = choices.get(0) {
                                let delta_obj = first.get("delta").or_else(|| first.get("message")).and_then(|d| d.as_object());
                                if let Some(delta) = delta_obj {
                                    // 1. Détection du raisonnement natif (DeepSeek-R1 / Albert)
                                    if let Some(reasoning) = delta.get("reasoning_content")
                                        .or_else(|| delta.get("reasoning"))
                                        .or_else(|| delta.get("thought"))
                                        .and_then(|v| v.as_str())
                                    {
                                        if let Some(banner) = thinking_filter.ingest_reasoning(reasoning) {
                                            accumulated_content.push_str(banner);
                                            on_chunk(banner);
                                        }
                                    }

                                    // 2. Traitement du contenu texte avec masquage de <think>...</think>
                                    if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                        if !content.is_empty() {
                                            full_content.push_str(content);
                                            let safe_chunk = thinking_filter.process_chunk(content);
                                            if !safe_chunk.is_empty() {
                                                accumulated_content.push_str(&safe_chunk);
                                                on_chunk(&safe_chunk);
                                            }
                                        }
                                    }
                                    if let Some(calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                                        for call in calls {
                                            let idx = call["index"].as_u64().unwrap_or(0) as usize;
                                            let entry = tool_calls_map
                                                .entry(idx)
                                                .or_insert((String::new(), String::new(), String::new()));
                                            if let Some(id) = call.get("id").and_then(|v| v.as_str()) {
                                                if entry.0.is_empty() {
                                                    entry.0.push_str(id);
                                                }
                                            }
                                            let fn_name = call.get("function")
                                                .and_then(|f| f.get("name"))
                                                .or_else(|| call.get("name"))
                                                .and_then(|v| v.as_str());
                                            if let Some(name) = fn_name {
                                                if entry.1.is_empty() {
                                                    entry.1.push_str(name);
                                                }
                                                if !entry.1.is_empty() && announced_tools.insert(idx) {
                                                    let msg = format!("\n⏳ *Préparation de `{}` en cours* ", entry.1);
                                                    accumulated_content.push_str(&msg);
                                                    on_chunk(&msg);
                                                }
                                            }

                                            let raw_args_val = call.get("function")
                                                .and_then(|f| f.get("arguments").or_else(|| f.get("parameters")))
                                                .or_else(|| call.get("arguments"))
                                                .or_else(|| call.get("parameters"));

                                            if let Some(val) = raw_args_val {
                                                if let Some(args_str) = val.as_str() {
                                                    entry.2.push_str(args_str);
                                                    let prev_dots = streamed_arg_bytes / 500;
                                                    streamed_arg_bytes += args_str.len();
                                                    let new_dots = streamed_arg_bytes / 500;
                                                    if new_dots > prev_dots {
                                                        accumulated_content.push('·');
                                                        on_chunk("·");
                                                    }
                                                } else {
                                                    let serialized = serde_json::to_string(val).unwrap_or_default();
                                                    if entry.2.is_empty() {
                                                        entry.2 = serialized;
                                                    } else {
                                                        entry.2.push_str(&serialized);
                                                    }
                                                }
                                            }
                                        }
                                    } else if let Some(legacy_fn) = delta.get("function_call").or_else(|| delta.get("function")) {
                                        let entry = tool_calls_map
                                            .entry(0)
                                            .or_insert((String::new(), String::new(), String::new()));
                                        if let Some(name) = legacy_fn.get("name").and_then(|v| v.as_str()) {
                                            entry.1.push_str(name);
                                            if !entry.1.is_empty() && announced_tools.insert(0) {
                                                let msg = format!("\n⏳ *Préparation de `{}` en cours* ", entry.1);
                                                accumulated_content.push_str(&msg);
                                                on_chunk(&msg);
                                            }
                                        }
                                        let raw_args_val = legacy_fn.get("arguments").or_else(|| legacy_fn.get("parameters"));
                                        if let Some(val) = raw_args_val {
                                            if let Some(args_str) = val.as_str() {
                                                entry.2.push_str(args_str);
                                            } else {
                                                entry.2 = serde_json::to_string(val).unwrap_or_default();
                                            }
                                        }
                                    }
                                } else if let Some(text) = first.get("text").and_then(|v| v.as_str()) {
                                    if !text.is_empty() {
                                        full_content.push_str(text);
                                        let safe_chunk = thinking_filter.process_chunk(text);
                                        if !safe_chunk.is_empty() {
                                            accumulated_content.push_str(&safe_chunk);
                                            on_chunk(&safe_chunk);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let trailing_safe = thinking_filter.finalize();
        if !trailing_safe.is_empty() {
            accumulated_content.push_str(&trailing_safe);
            on_chunk(&trailing_safe);
        }

        if !tool_calls_map.is_empty() {
            let mut calls_vec = Vec::new();
            for (idx, (id, name, args)) in tool_calls_map.values().enumerate() {
                if name.trim().is_empty() {
                    continue;
                }
                let call_id = if id.is_empty() || id == "0" || id == "call_0" {
                    format!("call_{}_{}", iteration, idx)
                } else {
                    id.clone()
                };
                let clean_args = if let Ok(val) = serde_json::from_str::<Value>(args) {
                    val.to_string()
                } else {
                    parse_lenient_tool_args(args).to_string()
                };
                calls_vec.push(ToolCall {
                    id: call_id,
                    call_type: "function".to_string(),
                    function: FunctionCall {
                        name: name.clone(),
                        arguments: clean_args,
                    },
                });
            }

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

                if is_explore_call && (read_calls_this_turn >= 2 || total_read_calls >= MAX_TOTAL_READ_CALLS) {
                    messages.push(ChatMessage {
                        role: "tool".to_string(),
                        content: Some(
                            "⚠️ BUDGET D'EXPLORATION ATTEINT (2 lectures max) : N'appelle plus d'outils de lecture/recherche/commande. Passe IMMÉDIATEMENT à l'écriture (`write_plan` puis `write_file` / `edit_file`) dès maintenant !".to_string(),
                        ),
                        tool_calls: None,
                        tool_call_id: Some(call.id.clone()),
                        name: Some(call.function.name.clone()),
                    });
                    continue;
                }

                tool_executions_count += 1;
                let mut args = parse_lenient_tool_args(&call.function.arguments);

                // Détection et mise à jour automatique du dossier de projet actif (ex: C:/Users/vadzt/Pictures/halloween-menu)
                if call.function.name == "read_file" || call.function.name == "list_directory" {
                    let raw_p = ["path", "file", "dir", "directory"]
                        .iter()
                        .find_map(|k| args.get(*k).and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()))
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
                        extracted = crate::tools::extract_files_from_raw_str(&call.function.arguments);
                    }
                    if extracted.is_empty() && !full_content.is_empty() {
                        let base_dir = active_project_dir.as_deref().or_else(|| {
                            if user_prompt.to_lowercase().contains("pictures") {
                                Some("Pictures/halloween-menu")
                            } else {
                                None
                            }
                        });
                        extracted = crate::tools::extract_code_blocks_from_text(&full_content, base_dir);
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
                                        format!("{}/{}", clean_base, resolved.trim_start_matches("./"))
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
                        || args.get("path").and_then(|v| v.as_str()).map(|s| s.trim().is_empty()).unwrap_or(true))
                {
                    if let Some(cand) = crate::tools::extract_candidate_path_from_text(&full_content) {
                        args["path"] = json!(cand);
                    }
                }

                let detail = describe_tool_call(&call.function.name, &args);

                // Exécution de l'outil natif ultra-rapide
                let started = std::time::Instant::now();
                let tool_result = crate::tools::execute_native_tool(&call.function.name, args).await;
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

                if let Some(spec_json) = tool_result.content.strip_prefix("SPEC_QUESTIONS_DISPLAYED:") {
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
                        if detail.is_empty() { "plan.md".to_string() } else { detail },
                        status_icon,
                        elapsed_ms,
                        tool_result.content
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
                            if detail.is_empty() { String::new() } else { format!("`{}`", detail) },
                            status_icon,
                            elapsed_ms
                        )
                    };
                    accumulated_content.push_str(&notice);
                    on_chunk(&notice);
                }

                let mut content_to_send = tool_result.content;
                if call.function.name == "write_file" && failed {
                    eprintln!("[RUSTAGENT DEBUG] write_file failed. Raw arguments: {:?}", call.function.arguments);
                    content_to_send = format!(
                        "⚠️ ERREUR CRITIQUE : Tu as appelé 'write_file' avec des arguments vides ou invalides. Tu dois OBLIGATOIREMENT passer les paramètres JSON 'path' et 'content', par exemple : {{\"path\": \"Pictures/mon-projet/index.html\", \"content\": \"...code complet...\"}}. Fournis le code complet dès maintenant !",
                    );
                }
                if !has_mutation
                    && !failed
                    && (consecutive_read_turns + 1 >= max_read_turns || total_read_calls >= MAX_TOTAL_READ_CALLS)
                    && is_explore_call
                {
                    if spec_mode && !specs_already_answered {
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

            // Si la demande de l'utilisateur concernait un simple fichier unique et qu'il a été généré
            let is_single_file_intent = !lower_prompt.contains("projet")
                && !lower_prompt.contains("plusieurs")
                && !lower_prompt.contains("application")
                && !lower_prompt.contains("composant")
                && (lower_prompt.contains("un simple")
                    || lower_prompt.contains("un seul")
                    || lower_prompt.contains("un fichier")
                    || lower_prompt.contains("le fichier")
                    || lower_prompt.contains("index.html")
                    || lower_prompt.contains("page html")
                    || lower_prompt.contains("fichier html")
                    || lower_prompt.contains("simple html")
                    || lower_prompt.contains("script ")
                    || (!lower_prompt.contains(" et ")
                        && (lower_prompt.ends_with(".html")
                            || lower_prompt.ends_with(".rs")
                            || lower_prompt.ends_with(".py")
                            || lower_prompt.ends_with(".js")
                            || lower_prompt.ends_with(".css"))));

            if !created_files.is_empty() && is_single_file_intent {
                let files_summary = format!(
                    "\n\n✅ **Fichier prêt dans l'onglet Diff :**\n- `{}`\n\n*(Vous pouvez vérifier et valider la modification ci-contre.)*",
                    created_files[0]
                );
                accumulated_content.push_str(&files_summary);
                on_chunk(&files_summary);
                return Ok(accumulated_content);
            }

            // Pour les projets complets, couper une fois 8+ fichiers préparés
            if created_files.len() >= 8 {
                let files_summary = format!(
                    "\n\n✅ **Tous les fichiers du projet sont prêts dans l'onglet Diff ({} fichier(s)) :**\n{}",
                    created_files.len(),
                    created_files.iter().map(|f| format!("- `{}`", f)).collect::<Vec<_>>().join("\n")
                );
                accumulated_content.push_str(&files_summary);
                on_chunk(&files_summary);
                return Ok(accumulated_content);
            }

            if any_read_succeeded {
                consecutive_read_turns += 1;
            }
            continue;
        }

        if !full_content.trim().is_empty() {
            let mut final_text = if !accumulated_content.trim().is_empty() && accumulated_content != full_content {
                accumulated_content
            } else {
                full_content
            };
            if !created_files.is_empty() && !final_text.contains("Fichiers préparés pour le projet") {
                let files_summary = format!(
                    "\n\n📁 **Fichiers préparés pour le projet :**\n{}",
                    created_files.iter().map(|f| format!("- `{}`", f)).collect::<Vec<_>>().join("\n")
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
            if !created_files.is_empty() && !summary.contains("Fichiers préparés pour le projet") {
                let files_summary = format!(
                    "\n\n📁 **Fichiers préparés pour le projet :**\n{}",
                    created_files.iter().map(|f| format!("- `{}`", f)).collect::<Vec<_>>().join("\n")
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
            format!("✅ L'agent a traité {} étape(s).{}", tool_executions_count, note)
        };
        if !created_files.is_empty() && !final_summary.contains("Fichiers préparés pour le projet") {
            let files_summary = format!(
                "\n\n📁 **Fichiers préparés pour le projet :**\n{}",
                created_files.iter().map(|f| format!("- `{}`", f)).collect::<Vec<_>>().join("\n")
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
    use super::super::types::ChatMessage;

    #[test]
    fn chat_message_serializes_content_null_when_none() {
        let msg = ChatMessage {
            role: "assistant".to_string(),
            content: None,
            tool_calls: Some(vec![]),
            tool_call_id: None,
            name: None,
        };
        let serialized = serde_json::to_string(&msg).expect("serialization failed");
        assert!(
            serialized.contains("\"content\":null"),
            "expected '\"content\":null', got: {}",
            serialized
        );
    }
}

