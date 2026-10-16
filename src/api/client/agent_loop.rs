//! Simple, non-streaming multi-turn agent loop (legacy, kept for compatibility).
//!
//! The main application uses [`super::stream_loop::run_agent_loop_stream`];
//! this variant is a straightforward request/response loop used for simple
//! tool-choosing interactions.

use super::super::types::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage};
use serde_json::{Value, json};

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
            tools: if has_tools {
                Some(tools_payload.clone())
            } else {
                None
            },
            tool_choice: if has_tools {
                Some("auto".to_string())
            } else {
                None
            },
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

        if let Some(tool_calls) = &assistant_msg.tool_calls
            && !tool_calls.is_empty()
        {
            messages.push(assistant_msg.clone());

            for call in tool_calls {
                println!(
                    "[TOOL EXEC] Appel de {} avec args: {}",
                    call.function.name, call.function.arguments
                );

                let args: Value =
                    serde_json::from_str(&call.function.arguments).unwrap_or_else(|_| json!({}));

                let tool_result =
                    crate::tools::execute_native_tool(&call.function.name, args).await;

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

        if let Some(content) = assistant_msg.content
            && !content.trim().is_empty()
        {
            return Ok(content);
        }
    }

    Err("Nombre maximum d'itérations atteint sans réponse".to_string())
}
