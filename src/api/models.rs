//! Model discovery and fetching from OpenAI-compatible endpoints.

use serde_json::Value;
use std::time::Duration;

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
