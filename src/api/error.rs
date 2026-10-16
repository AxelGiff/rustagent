//! API error classification, timeouts, and exponential backoff retry logic.

use std::time::Duration;

/// The maximum number of attempts (including the initial one) for a request.
pub const MAX_ATTEMPTS: u32 = 3;

/// The base delay for exponential backoff between retries.
pub const BASE_BACKOFF: Duration = Duration::from_millis(500);

/// The maximum delay between retries.
pub const MAX_BACKOFF: Duration = Duration::from_secs(4);

/// The default timeout for a single completion request.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Maximum number of historical messages sent in API requests to bound token usage.
pub const MAX_HISTORY_MESSAGES: usize = 20;

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
pub fn is_transient(category: ApiErrorCategory) -> bool {
    matches!(
        category,
        ApiErrorCategory::Network | ApiErrorCategory::Timeout | ApiErrorCategory::RateLimit
    )
}

/// Compute the backoff delay for a given attempt (0-based).
pub fn backoff_delay(attempt: u32) -> Duration {
    let exp = BASE_BACKOFF.saturating_mul(1u32 << attempt.min(4));
    exp.min(MAX_BACKOFF)
}

/// Run a completion future with a timeout and retry/backoff for transient failures.
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
