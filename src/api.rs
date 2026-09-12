//! API error handling, retry, and timeout logic.
//!
//! This module wraps the Albert API completion calls so that failures are categorized
//! into user-friendly buckets and transient failures are retried with exponential backoff.
//! It also provides the multi-turn agentic loop supporting MCP tool calling.

use std::fs;
use std::time::Duration;
use futures_util::StreamExt;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::mcp::McpClient;

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
/// Conserve et propage systématiquement les détails de l'erreur brute.
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

                // Si l'erreur n'est pas transitoire, on s'arrête tout de suite
                // et on renvoie immédiatement le message détaillé brut !
                if !is_transient(last_category) {
                    let full_err_msg = format!("{}\n\nDétails techniques :\n```\n{}\n```", last_category.user_message(), last_raw);
                    return Err((last_category, full_err_msg));
                }
            }
            Err(_elapsed) => {
                last_category = ApiErrorCategory::Timeout;
                last_raw = "La requête vers Albert API a dépassé le délai imparti (timeout).".to_string();
            }
        }

        if attempt_index + 1 >= MAX_ATTEMPTS {
            break;
        }

        tokio::time::sleep(backoff_delay(attempt_index)).await;
    }

    let full_err_msg = format!("{}\n\nDétails techniques :\n```\n{}\n```", last_category.user_message(), last_raw);
    Err((last_category, full_err_msg))
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

/// Boucle agentique multi-tours supportant `tool_choice: "auto"` pour Albert / DeepSeek.
pub async fn run_agent_loop(
    endpoint: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
    mcp: &McpClient,
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));

    // 1. Récupération dynamique des outils MCP
    let mcp_tools = mcp.list_tools().await.map_err(|e| e.to_string())?;
    let tools_payload: Vec<Value> = mcp_tools
        .into_iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description.unwrap_or_default(),
                    "parameters": t.input_schema
                }
            })
        })
        .collect();

    let has_tools = !tools_payload.is_empty();

    // 2. Historique initial
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

    // 3. Boucle agentique (jusqu'à 20 itérations max)
    for iteration in 0..20 {
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

        // Le modèle a-t-il décidé d'appeler des outils ?
        if let Some(tool_calls) = &assistant_msg.tool_calls {
            if !tool_calls.is_empty() {
                messages.push(assistant_msg.clone());

                for call in tool_calls {
                    println!("[MCP EXEC] Appel de {} avec args: {}", call.function.name, call.function.arguments);

                    let args: Value = serde_json::from_str(&call.function.arguments)
                        .unwrap_or_else(|_| json!({}));

                    // Exécution sur le serveur MCP via stdio
                    let tool_result = match mcp.call_tool(&call.function.name, args).await {
                        Ok(res) => res,
                        Err(e) => format!("Erreur MCP: {}", e),
                    };

                    println!("[MCP RÉSULTAT] {} octets", tool_result.len());

                    messages.push(ChatMessage {
                        role: "tool".to_string(),
                        content: Some(tool_result),
                        tool_calls: None,
                        tool_call_id: Some(call.id.clone()),
                    });
                }
                continue;
            }
        }

        // Réponse finale formulée par le modèle
        if let Some(content) = assistant_msg.content {
            if !content.trim().is_empty() {
                return Ok(content);
            }
        }
    }

    Err("Nombre maximum d'itérations atteint sans réponse".to_string())
}

/// Boucle agentique multi-tours supportant `tool_choice: "auto"` avec streaming d'évènements SSE.
pub async fn run_agent_loop_stream<C>(
    endpoint: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
    mcp: &McpClient,
    mut on_chunk: C,
) -> Result<String, String>
where
    C: FnMut(&str),
{
    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));

    // Récupération dynamique de tous les outils MCP
    let mcp_tools = mcp.list_tools().await.map_err(|e| e.to_string())?;
    let tools_payload: Vec<Value> = mcp_tools
        .into_iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description.unwrap_or_default(),
                    "parameters": t.input_schema
                }
            })
        })
        .collect();

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

    for iteration in 0..20 {
        println!("[AGENT] Tour {}", iteration + 1);

        let req = json!({
            "model": model,
            "messages": messages,
            "tools": if has_tools { Some(&tools_payload) } else { None },
            "tool_choice": if has_tools { Some("auto") } else { None },
            "stream": true
        });

      let res = client
            .post(&url)
            .bearer_auth(api_key)
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("Erreur réseau (connexion impossible) : {}", e))?;

        if !res.status().is_success() {
            let status = res.status();
            let err_body = res.text().await.unwrap_or_else(|_| "Impossible de lire le corps de l'erreur".to_string());
            eprintln!("[ALBERT API ERROR {}] {}", status, err_body);
            return Err(format!("HTTP {} : {}", status, err_body));
        }

        let mut stream = res.bytes_stream();
        let mut buffer = String::new();
        let mut full_content = String::new();
        let mut tool_calls_map: std::collections::BTreeMap<usize, (String, String, String)> =
            std::collections::BTreeMap::new();

        while let Some(chunk_res) = stream.next().await {
            let bytes = chunk_res.map_err(|e| e.to_string())?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim().to_string();
                buffer = buffer[pos + 1..].to_string();

                if line.starts_with("data: ") {
                    let data = line["data: ".len()..].trim();
                    if data == "[DONE]" {
                        break;
                    }

                    if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                        if let Some(choices) = parsed.get("choices").and_then(|c| c.as_array()) {
                            if let Some(choice) = choices.get(0) {
                                if let Some(delta) = choice.get("delta") {
                                    // 1. Text streamé
                                    if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                        if !content.is_empty() {
                                            full_content.push_str(content);
                                            on_chunk(content);
                                        }
                                    }

                                    // 2. Assemblage des tool calls
                                    if let Some(calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                                        for call in calls {
                                            let idx = call.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                                            let entry = tool_calls_map
                                                .entry(idx)
                                                .or_insert((String::new(), String::new(), String::new()));

                                            if let Some(id) = call.get("id").and_then(|v| v.as_str()) {
                                                entry.0.push_str(id);
                                            }
                                            if let Some(func) = call.get("function") {
                                                if let Some(name) = func.get("name").and_then(|v| v.as_str()) {
                                                    entry.1.push_str(name);
                                                }
                                                if let Some(args) = func.get("arguments").and_then(|v| v.as_str()) {
                                                    entry.2.push_str(args);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Si le modèle a demandé l'exécution d'un ou plusieurs outils
        if !tool_calls_map.is_empty() {
            let mut calls_vec = Vec::new();
            for (id, name, args) in tool_calls_map.values() {
                calls_vec.push(ToolCall {
                    id: id.clone(),
                    call_type: "function".to_string(),
                    function: FunctionCall {
                        name: name.clone(),
                        arguments: args.clone(),
                    },
                });
            }

            messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: if full_content.is_empty() { None } else { Some(full_content.clone()) },
                tool_calls: Some(calls_vec.clone()),
                tool_call_id: None,
            });

            for call in &calls_vec {
                let status_msg = format!("\n\n⚙️ Execution de `{}`...\n\n", call.function.name);
                on_chunk(&status_msg);

               println!("[MCP CALL] {} avec args: {}", call.function.name, call.function.arguments);
                let args: Value = serde_json::from_str(&call.function.arguments).unwrap_or_else(|_| json!({}));
                
                let mut tool_result = match mcp.call_tool(&call.function.name, args).await {
                    Ok(res) => res,
                    Err(e) => format!("Erreur MCP: {}", e),
                };

                // Protection contre le débordement de contexte / rate limit tokens
                // ~12 000 caractères ≈ 3 000 tokens (largement suffisant pour l'agent)
                const MAX_TOOL_OUTPUT_CHARS: usize = 12_000;
                if tool_result.len() > MAX_TOOL_OUTPUT_CHARS {
                    println!("[MCP TRUNCATE] Sortie tronquée de {} à {} caractères", tool_result.len(), MAX_TOOL_OUTPUT_CHARS);
                    tool_result.truncate(MAX_TOOL_OUTPUT_CHARS);
                    tool_result.push_str("\n\n[... Sortie tronquée car trop volumineuse. Spécifiez un sous-dossier précis ou utilisez search_files / list_directory pour explorer pas à pas ...]");
                }

                messages.push(ChatMessage {
                    role: "tool".to_string(),
                    content: Some(tool_result),
                    tool_calls: None,
                    tool_call_id: Some(call.id.clone()),
                });
            }

            // Réinitialisation du texte pour le tour suivant
            full_content.clear();
            continue;
        }

        if !full_content.trim().is_empty() {
            return Ok(full_content);
        }
    }

    Err("Nombre maximum d'itérations atteint".to_string())
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