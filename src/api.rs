//! API error handling, retry, and timeout logic.
//!
//! This module wraps the rig completion call so that failures are categorized
//! into user-friendly buckets (authentication, rate limit, network, timeout,
//! model, other) and transient failures are retried with exponential backoff.

use std::time::Duration;
use futures_util::StreamExt;

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

/// Run a completion future with a timeout and retry/backoff for transient
/// failures.
///
/// `attempt` is the closure that performs one request and returns
/// `Result<String, String>` (the error string is the raw error text). The
/// closure is called up to `MAX_ATTEMPTS` times.
///
/// Returns `Ok(response)` on success, or `Err((category, message))` where
/// `message` is a user-friendly explanation.
#[allow(dead_code)]
pub async fn prompt_with_retry<F, Fut>(mut attempt: F) -> Result<String, (ApiErrorCategory, String)>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    let mut last_category = ApiErrorCategory::Other;
    let mut last_raw = String::new();

    for attempt_index in 0..MAX_ATTEMPTS {
        // Wrap the request in a timeout so the UI never hangs indefinitely.
        let result = tokio::time::timeout(REQUEST_TIMEOUT, attempt()).await;

        match result {
            Ok(Ok(response)) => return Ok(response),
            Ok(Err(raw)) => {
                last_raw = raw.clone();
                last_category = classify_error(&raw);
                if !is_transient(last_category) {
                    // Non-transient: don't retry.
                    return Err((last_category, last_category.user_message().to_string()));
                }
            }
            Err(_elapsed) => {
                last_category = ApiErrorCategory::Timeout;
                last_raw = "request timed out".to_string();
            }
        }

        // If this was the last attempt, give up.
        if attempt_index + 1 >= MAX_ATTEMPTS {
            break;
        }

        // Wait with exponential backoff before retrying.
        tokio::time::sleep(backoff_delay(attempt_index)).await;
    }

    let mut message = last_category.user_message().to_string();
    if !last_raw.is_empty() {
        message.push_str(&format!("\n\nDetails: {}", last_raw));
    }
    Err((last_category, message))
}

/// Run a streaming completion request with retries for transient stream initialization failures,
/// token-by-token callback dispatching (`on_chunk`), mid-stream error reporting, and cancellation support.
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
                count > 2 // Cancel after first chunk check
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

