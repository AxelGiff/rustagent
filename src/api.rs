#![allow(dead_code)]
//! API error handling, retry, and timeout logic.
//!
//! This module wraps the Albert API completion calls so that failures are categorized
//! into user-friendly buckets and transient failures are retried with exponential backoff.
//! It also provides the multi-turn agentic loop supporting MCP tool calling.

use std::time::Duration;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The maximum number of attempts (including the initial one) for a request.
const MAX_ATTEMPTS: u32 = 3;

/// The base delay for exponential backoff between retries.
const BASE_BACKOFF: Duration = Duration::from_millis(500);

/// The maximum delay between retries.
const MAX_BACKOFF: Duration = Duration::from_secs(4);

/// The default timeout for a single completion request.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Maximum number of historical messages sent in API requests to bound token usage.
pub const MAX_HISTORY_MESSAGES: usize = 20;

/// Role of a message sender in the natural language chat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    AI,
    User,
}

/// A chat message entry containing sender role and string content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

/// Build a list of `rig::completion::Message` context history items from UI messages,
/// skipping initial system warnings/welcome messages and capping to `max_history` entries.
pub fn build_chat_history(messages: &[Message], max_history: usize) -> Vec<rig::completion::Message> {
    let mut history = Vec::new();

    for msg in messages {
        // Skip system/welcome messages that are not real conversation turns
        if msg.role == Role::AI
            && (msg.content.starts_with("Hello! I'm your coding assistant")
                || msg.content.starts_with("Chat cleared")
                || msg.content.starts_with("⚠️ "))
        {
            continue;
        }

        match msg.role {
            Role::User => {
                if !msg.content.trim().is_empty() {
                    history.push(rig::completion::Message::user(msg.content.clone()));
                }
            }
            Role::AI => {
                if !msg.content.trim().is_empty() {
                    history.push(rig::completion::Message::assistant(msg.content.clone()));
                }
            }
        }
    }

    if history.len() > max_history {
        history.drain(0..history.len() - max_history);
    }

    history
}

/// Categories of API failures, used to pick an appropriate user-facing message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiErrorCategory {
    /// The API key is missing or rejected (401/403).
    Authentication,
    /// The provider is rate-limiting us (429).
    RateLimit,
    /// A network/connection problem (DNS, refused, reset).
    Network,
    /// The request took too long.
    Timeout,
    /// The model/provider returned an error.
    Model,
    /// Anything else.
    Other,
}

impl ApiErrorCategory {
    /// A human-friendly message for this category.
    pub fn user_message(&self) -> &'static str {
        match self {
            ApiErrorCategory::Authentication => {
                "Authentication failed. Please check your Albert API key — it may be \
                 missing, invalid, or expired. Set the ALBERT_API_KEY environment \
                 variable or update your config file."
            }
            ApiErrorCategory::RateLimit => {
                "The Albert API is rate-limiting requests. Please wait a moment and \
                 try again."
            }
            ApiErrorCategory::Network => {
                "A network error occurred while contacting the Albert API. Please \
                 check your internet connection and try again."
            }
            ApiErrorCategory::Timeout => {
                "The request to the Albert API timed out. Please try again."
            }
            ApiErrorCategory::Model => {
                "The Albert model returned an error. Please try rephrasing your \
                 request."
            }
            ApiErrorCategory::Other => {
                "An unexpected error occurred while contacting the Albert API. Please \
                 try again."
            }
        }
    }
}

/// Classify a raw error string into a category.
pub fn classify_error(error: &str) -> ApiErrorCategory {
    let lower = error.to_lowercase();

    // Authentication failures.
    if lower.contains("401")
        || lower.contains("403")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("invalid api key")
        || lower.contains("authentication")
        || lower.contains("api key")
    {
        return ApiErrorCategory::Authentication;
    }

    // Rate limiting.
    if lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("quota")
    {
        return ApiErrorCategory::RateLimit;
    }

    // Network problems.
    if lower.contains("connection")
        || lower.contains("dns")
        || lower.contains("refused")
        || lower.contains("reset")
        || lower.contains("network")
        || lower.contains("unreachable")
        || lower.contains("tcp")
        || lower.contains("socket")
        || lower.contains("http")
    {
        return ApiErrorCategory::Network;
    }

    // Timeouts.
    if lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("deadline")
        || lower.contains("elapsed")
    {
        return ApiErrorCategory::Timeout;
    }

    // Model/provider errors.
    if lower.contains("provider")
        || lower.contains("model")
        || lower.contains("responseerror")
        || lower.contains("bad request")
        || lower.contains("400")
        || lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
    {
        return ApiErrorCategory::Model;
    }

    ApiErrorCategory::Other
}

/// Whether an error is transient and worth retrying.
fn is_transient(category: ApiErrorCategory) -> bool {
    matches!(
        category,
        ApiErrorCategory::Network | ApiErrorCategory::Timeout | ApiErrorCategory::RateLimit
    )
}

/// Compute the backoff delay for a given attempt (0-based).
fn backoff_delay(attempt: u32) -> Duration {
    let exp = BASE_BACKOFF.saturating_mul(1u32 << attempt.min(4));
    exp.min(MAX_BACKOFF)
}

/// Run a completion future with a timeout and retry/backoff for transient failures.
#[allow(dead_code)]
pub async fn prompt_with_retry<F, Fut>(mut attempt: F) -> Result<String, (ApiErrorCategory, String)>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    let mut last_category = ApiErrorCategory::Other;
    let mut last_raw = String::new();

    for attempt_index in 0..MAX_ATTEMPTS {
        let result = tokio::time::timeout(REQUEST_TIMEOUT, attempt()).await;

        match result {
            Ok(Ok(response)) => return Ok(response),
            Ok(Err(raw)) => {
                last_raw = raw.clone();
                last_category = classify_error(&raw);
                if !is_transient(last_category) {
                    let mut message = last_category.user_message().to_string();
                    if !raw.is_empty() {
                        message.push_str(&format!("\n\nDetails: {}", raw));
                    }
                    return Err((last_category, message));
                }
            }
            Err(_elapsed) => {
                last_category = ApiErrorCategory::Timeout;
                last_raw = "request timed out".to_string();
            }
        }

        if attempt_index + 1 >= MAX_ATTEMPTS {
            break;
        }

        tokio::time::sleep(backoff_delay(attempt_index)).await;
    }

    let mut message = last_category.user_message().to_string();
    if !last_raw.is_empty() {
        message.push_str(&format!("\n\nDetails: {}", last_raw));
    }
    Err((last_category, message))
}

// ---------------------------------------------------------------------------
// Modèles JSON OpenAI / Albert pour la gestion du Function Calling
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatCompletionResponse {
    pub choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
pub struct Choice {
    pub message: ChatMessage,
}

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
        },
        ChatMessage {
            role: "user".to_string(),
            content: Some(user_prompt.to_string()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];

    for iteration in 0..6 {
        println!("[AGENT] Itération {}", iteration + 1);

        let req = ChatCompletionRequest {
            model: model.to_string(),
            messages: messages.clone(),
            tools: if has_tools { Some(tools_payload.clone()) } else { None },
            tool_choice: if has_tools { Some("auto".to_string()) } else { None },
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
pub async fn run_agent_loop_stream<C, D>(
    endpoint: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
    history: &[crate::Message],
    user_prompt: &str,
    mut on_chunk: C,
    mut on_pending_diff: D,
) -> Result<String, String>
where
    C: FnMut(&str),
    D: FnMut(crate::diff::PendingDiff),
{
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
        },
    ];

    // Inclure l'historique récent (jusqu'à 12 messages) pour maintenir le fil de discussion
    let history_slice = if history.len() > 12 {
        &history[history.len() - 12..]
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
                content: Some(msg.content.clone()),
                tool_calls: None,
                tool_call_id: None,
            });
        }
    }

    // Ajouter le prompt actuel de l'utilisateur
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: Some(user_prompt.to_string()),
        tool_calls: None,
        tool_call_id: None,
    });

    let mut accumulated_content = String::new();
    let mut tool_executions_count: usize = 0;
    let mut consecutive_read_turns: usize = 0;
    let mut created_files: Vec<String> = Vec::new();
    const MAX_AGENT_ITERATIONS: usize = 20;

    for iteration in 0..MAX_AGENT_ITERATIONS {
        // En dernière itération, forcer le modèle à synthétiser en texte en désactivant les outils
        let is_last_iteration = iteration == MAX_AGENT_ITERATIONS - 1;
        let allow_tools = has_tools && !is_last_iteration;

        // Si le modèle a enchaîné trop de lectures sans modifier de code, on bloque les outils de lecture
        let active_tools = if allow_tools {
            if consecutive_read_turns >= 3 {
                let filtered: Vec<Value> = tools_payload
                    .iter()
                    .filter(|t| {
                        let name = t.get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("");
                        name == "write_file" || name == "edit_file" || name == "run_command"
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

        // Compaction de l'historique antérieur pour préserver la fenêtre de contexte
        let total_msgs = messages.len();
        if total_msgs > 8 {
            for idx in 0..total_msgs.saturating_sub(6) {
                if let Some(calls) = &mut messages[idx].tool_calls {
                    for call in calls.iter_mut() {
                        if call.function.name == "write_file" || call.function.name == "edit_file" {
                            if let Ok(mut parsed) = serde_json::from_str::<Value>(&call.function.arguments) {
                                if parsed.get("content").is_some() {
                                    parsed["content"] = json!("[Contenu déjà fourni au tour précédent]");
                                    call.function.arguments = parsed.to_string();
                                }
                            }
                        }
                    }
                }
                if messages[idx].role == "tool" {
                    if let Some(content) = &mut messages[idx].content {
                        if content.len() > 1000 {
                            *content = crate::tools::truncate_output(content, 15, 800);
                        }
                    }
                }
            }
        }

        let req = json!({
            "model": model,
            "messages": messages,
            "tools": if has_active_tools { active_tools } else { None },
            "tool_choice": if has_active_tools { Some("auto".to_string()) } else { None },
            "stream": true
        });

        let res = client
            .post(&url)
            .bearer_auth(api_key)
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("Erreur réseau: {}", e))?;

        if !res.status().is_success() {
            let status = res.status();
            let err_text = res.text().await.unwrap_or_default();
            return Err(format!("Erreur API (HTTP {}): {}", status, err_text));
        }

        let mut stream = res.bytes_stream();
        let mut buffer = String::new();
        let mut byte_buffer: Vec<u8> = Vec::new();
        let mut full_content = String::new();
        let mut tool_calls_map: std::collections::BTreeMap<usize, (String, String, String)> =
            std::collections::BTreeMap::new();

        while let Some(chunk_res) = stream.next().await {
            let bytes = chunk_res.map_err(|e| e.to_string())?;
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
                            return Err(format!("Erreur Stream API: {}", err_msg));
                        }
                        if let Some(choices) = parsed.get("choices").and_then(|c| c.as_array()) {
                            if let Some(first) = choices.get(0) {
                                if let Some(delta) = first.get("delta").and_then(|d| d.as_object()) {
                                    if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                        if !content.is_empty() {
                                            full_content.push_str(content);
                                            on_chunk(content);
                                        }
                                    }
                                    if let Some(calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                                        for call in calls {
                                            let idx = call["index"].as_u64().unwrap_or(0) as usize;
                                            let entry = tool_calls_map
                                                .entry(idx)
                                                .or_insert((String::new(), String::new(), String::new()));
                                            if let Some(id) = call.get("id").and_then(|v| v.as_str()) {
                                                entry.0.push_str(id);
                                            }
                                            if let Some(name) = call["function"].get("name").and_then(|v| v.as_str()) {
                                                entry.1.push_str(name);
                                            }
                                            if let Some(args) = call["function"].get("arguments").and_then(|v| v.as_str()) {
                                                entry.2.push_str(args);
                                            }
                                        }
                                    }
                                } else if let Some(text) = first.get("text").and_then(|v| v.as_str()) {
                                    if !text.is_empty() {
                                        full_content.push_str(text);
                                        on_chunk(text);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if !full_content.is_empty() {
            accumulated_content.push_str(&full_content);
        }

        if !tool_calls_map.is_empty() {
            let mut calls_vec = Vec::new();
            for (idx, (id, name, args)) in tool_calls_map.values().enumerate() {
                let call_id = if id.is_empty() {
                    format!("call_{}", idx)
                } else {
                    id.clone()
                };
                calls_vec.push(ToolCall {
                    id: call_id,
                    call_type: "function".to_string(),
                    function: FunctionCall {
                        name: name.clone(),
                        arguments: args.clone(),
                    },
                });
            }

            let assistant_msg = ChatMessage {
                role: "assistant".to_string(),
                content: if full_content.is_empty() {
                    None
                } else {
                    Some(full_content.clone())
                },
                tool_calls: Some(calls_vec.clone()),
                tool_call_id: None,
            };
            messages.push(assistant_msg);

            let has_mutation = calls_vec.iter().any(|c| {
                c.function.name == "write_file" || c.function.name == "edit_file" || c.function.name == "run_command"
            });
            if has_mutation {
                consecutive_read_turns = 0;
            } else {
                consecutive_read_turns += 1;
            }

            for call in &calls_vec {
                tool_executions_count += 1;
                let args: Value = serde_json::from_str(&call.function.arguments).unwrap_or_else(|_| json!({}));

                // Exécution de l'outil natif ultra-rapide
                let tool_result = crate::tools::execute_native_tool(&call.function.name, args).await;

                if let Some(diff) = tool_result.pending_diff {
                    let path = diff.path.clone();
                    if !created_files.contains(&path) {
                        created_files.push(path.clone());
                    }
                    on_pending_diff(diff);
                    let notice = format!(
                        "\n\n📝 **Proposition de modification pour `{}`** (aperçu diff généré)...\n\n",
                        path
                    );
                    accumulated_content.push_str(&notice);
                    on_chunk(&notice);
                } else {
                    let notice = format!("\n\n⚙️ Outil exécuté : `{}`\n\n", call.function.name);
                    accumulated_content.push_str(&notice);
                    on_chunk(&notice);
                }

                let mut content_to_send = tool_result.content;
                if !has_mutation
                    && consecutive_read_turns >= 2
                    && (call.function.name == "read_file"
                        || call.function.name == "list_directory"
                        || call.function.name == "search_files")
                {
                    content_to_send.push_str(
                        "\n\n⚠️ ALERTE : Tu as déjà inspecté plusieurs fichiers. Tu as désormais tout le contexte nécessaire. Arrête immédiatement les lectures et applique tes modifications avec `write_file` ou `edit_file` dès ce tour !",
                    );
                }

                messages.push(ChatMessage {
                    role: "tool".to_string(),
                    content: Some(content_to_send),
                    tool_calls: None,
                    tool_call_id: Some(call.id.clone()),
                });
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

#[derive(Debug, thiserror::Error)]
#[error("Math execution error: {0}")]
pub struct MathError(pub String);

#[derive(Deserialize)]
pub struct AddArgs {
    x: i32,
    y: i32,
}

#[derive(Deserialize)]
pub struct PathArgs {
    pub path: String,
}

#[derive(Deserialize, Serialize)]
pub struct Adder;

#[derive(Deserialize, Serialize)]
pub struct ReadFile;

/// Run a streaming completion request with retries for transient stream initialization failures.
pub async fn prompt_stream_with_retry<F, Fut, S, C, K>(
    mut make_stream: F,
    mut on_chunk: C,
    mut is_cancelled: K,
) -> Result<String, (ApiErrorCategory, String)>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<S, String>>,
    S: futures_util::Stream<Item = Result<String, String>> + Unpin,
    C: FnMut(&str),
    K: FnMut() -> bool,
{
    let mut last_category = ApiErrorCategory::Other;
    let mut last_raw = String::new();
    let mut accumulated = String::new();

    for attempt_index in 0..MAX_ATTEMPTS {
        if is_cancelled() {
            return Ok(accumulated);
        }

        let result = tokio::time::timeout(REQUEST_TIMEOUT, make_stream()).await;

        match result {
            Ok(Ok(mut stream)) => {
                let mut stream_failed_midway = false;
                while let Some(chunk_res) = stream.next().await {
                    let chunk_res: Result<String, String> = chunk_res;
                    if is_cancelled() {
                        return Ok(accumulated);
                    }
                    match chunk_res {
                        Ok(chunk) => {
                            accumulated.push_str(&chunk);
                            on_chunk(&chunk);
                        }
                        Err(raw) => {
                            last_raw = raw.clone();
                            last_category = classify_error(&raw);
                            stream_failed_midway = true;
                            break;
                        }
                    }
                }

                if !stream_failed_midway {
                    return Ok(accumulated);
                }

                let mut error_notice = format!("\n\n⚠️ {}", last_category.user_message());
                if !last_raw.is_empty() {
                    error_notice.push_str(&format!("\n\nDetails: {}", last_raw));
                }
                accumulated.push_str(&error_notice);
                on_chunk(&error_notice);
                return Err((last_category, accumulated));
            }
            Ok(Err(raw)) => {
                last_raw = raw.clone();
                last_category = classify_error(&raw);
                if !is_transient(last_category) {
                    return Err((last_category, last_category.user_message().to_string()));
                }
            }
            Err(_elapsed) => {
                last_category = ApiErrorCategory::Timeout;
                last_raw = "request timed out".to_string();
            }
        }

        if attempt_index + 1 >= MAX_ATTEMPTS {
            break;
        }

        tokio::time::sleep(backoff_delay(attempt_index)).await;
    }

    let mut message = last_category.user_message().to_string();
    if !last_raw.is_empty() {
        message.push_str(&format!("\n\nDetails: {}", last_raw));
    }
    Err((last_category, message))
}


/// Fetch the list of available model IDs from the OpenAI-compatible `/models` endpoint.
pub async fn fetch_models(endpoint: &str, api_key: &str) -> Result<Vec<String>, String> {
    let base = endpoint.trim_end_matches('/');
    let url = format!("{}/models", base);

    let client = reqwest::Client::new();
    let mut req = client.get(&url);
    if !api_key.trim().is_empty() {
        req = req.bearer_auth(api_key.trim());
    }

    let resp = req
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch models: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Model API returned status {}", resp.status()));
    }

    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse models JSON: {}", e))?;

    let mut models = Vec::new();
    if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
        for item in data {
            if let Some(id) = item.get("id").and_then(|id| id.as_str()) {
                models.push(id.to_string());
            }
        }
    } else if let Some(data) = body.as_array() {
        for item in data {
            if let Some(id) = item.get("id").and_then(|id| id.as_str()) {
                models.push(id.to_string());
            } else if let Some(id) = item.as_str() {
                models.push(id.to_string());
            }
        }
    }

    models.sort();
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;

    #[test]
    fn classifies_auth() {
        assert_eq!(
            classify_error("401 Unauthorized: invalid api key"),
            ApiErrorCategory::Authentication
        );
    }

    #[test]
    fn classifies_rate_limit() {
        assert_eq!(
            classify_error("429 Too Many Requests"),
            ApiErrorCategory::RateLimit
        );
    }

    #[test]
    fn classifies_network() {
        assert_eq!(
            classify_error("connection refused"),
            ApiErrorCategory::Network
        );
    }

    #[test]
    fn classifies_timeout() {
        assert_eq!(
            classify_error("request timed out"),
            ApiErrorCategory::Timeout
        );
    }

    #[test]
    fn classifies_unknown_as_other() {
        assert_eq!(classify_error("some weird error"), ApiErrorCategory::Other);
    }

    #[tokio::test]
    async fn retries_transient_then_succeeds() {
        let mut calls = 0;
        let result = prompt_with_retry(|| {
            calls += 1;
            async move {
                if calls < 3 {
                    Err("connection reset".to_string())
                } else {
                    Ok("success".to_string())
                }
            }
        })
        .await;
        assert_eq!(result, Ok("success".to_string()));
        assert_eq!(calls, 3);
    }

    #[tokio::test]
    async fn does_not_retry_non_transient() {
        let mut calls = 0;
        let result = prompt_with_retry(|| {
            calls += 1;
            async move { Err("401 Unauthorized".to_string()) }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }

    #[tokio::test]
    async fn streams_multiple_chunks_successfully() {
        let mut chunks_received = Vec::new();
        let result = prompt_stream_with_retry(
            || async {
                let items: Vec<Result<String, String>> = vec![
                    Ok("Hello, ".to_string()),
                    Ok("world!".to_string()),
                ];
                Ok(stream::iter(items))
            },
            |chunk| chunks_received.push(chunk.to_string()),
            || false,
        )
        .await;

        assert_eq!(result, Ok("Hello, world!".to_string()));
        assert_eq!(chunks_received, vec!["Hello, ", "world!"]);
    }

    #[tokio::test]
    async fn retries_transient_failure_before_stream_starts() {
        let mut calls = 0;
        let mut chunks_received = Vec::new();
        let result = prompt_stream_with_retry(
            || {
                calls += 1;
                async move {
                    if calls < 2 {
                        Err("connection reset".to_string())
                    } else {
                        let items: Vec<Result<String, String>> = vec![Ok("Done".to_string())];
                        Ok(stream::iter(items))
                    }
                }
            },
            |chunk| chunks_received.push(chunk.to_string()),
            || false,
        )
        .await;

        assert_eq!(result, Ok("Done".to_string()));
        assert_eq!(calls, 2);
        assert_eq!(chunks_received, vec!["Done"]);
    }

    #[tokio::test]
    async fn handles_mid_stream_error_with_partial_content() {
        let mut chunks_received = Vec::new();
        let result = prompt_stream_with_retry(
            || async {
                let items: Vec<Result<String, String>> = vec![
                    Ok("Part 1. ".to_string()),
                    Err("connection reset".to_string()),
                ];
                Ok(stream::iter(items))
            },
            |chunk| chunks_received.push(chunk.to_string()),
            || false,
        )
        .await;

        assert!(result.is_err());
        let (cat, accumulated) = result.unwrap_err();
        assert_eq!(cat, ApiErrorCategory::Network);
        assert!(accumulated.starts_with("Part 1. "));
        assert!(accumulated.contains("network error"));
        assert_eq!(chunks_received.len(), 2);
        assert_eq!(chunks_received[0], "Part 1. ");
    }

    #[tokio::test]
    async fn cancels_stream_early() {
        let mut chunks_received = Vec::new();
        let mut count = 0;
        let result = prompt_stream_with_retry(
            || async {
                let items: Vec<Result<String, String>> = vec![
                    Ok("Chunk 1".to_string()),
                    Ok("Chunk 2".to_string()),
                ];
                Ok(stream::iter(items))
            },
            |chunk| chunks_received.push(chunk.to_string()),
            || {
                count += 1;
                count > 2
            },
        )
        .await;

        assert_eq!(result, Ok("Chunk 1".to_string()));
        assert_eq!(chunks_received, vec!["Chunk 1"]);
    }

    #[test]
    fn build_chat_history_filters_system_welcome_and_orders_messages() {
        let msgs = vec![
            Message {
                role: Role::AI,
                content: "Hello! I'm your coding assistant...".to_string(),
            },
            Message {
                role: Role::User,
                content: "write python code".to_string(),
            },
            Message {
                role: Role::AI,
                content: "def foo(): pass".to_string(),
            },
        ];

        let history = build_chat_history(&msgs, 20);
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn build_chat_history_caps_length_to_max() {
        let mut msgs = Vec::new();
        for i in 0..30 {
            msgs.push(Message {
                role: if i % 2 == 0 { Role::User } else { Role::AI },
                content: format!("Message {}", i),
            });
        }

        let history = build_chat_history(&msgs, 10);
        assert_eq!(history.len(), 10);
    }
}