//! Streaming primitives, cancellation flags, and retry logic.

use super::error::{
    ApiErrorCategory, MAX_ATTEMPTS, REQUEST_TIMEOUT, backoff_delay, classify_error, is_transient,
};
use futures_util::StreamExt;

/// Demande l'arrêt de la génération en cours.
pub fn request_cancel() {
    crate::session::default_session().request_cancel();
}

/// Réinitialise l'état d'annulation avant de démarrer une nouvelle génération.
pub fn reset_cancel() {
    crate::session::default_session().reset_cancel();
}

pub fn is_cancel_requested() -> bool {
    crate::session::default_session().is_cancel_requested()
}

pub fn cancelled_summary(accumulated: &str) -> String {
    let body = accumulated.trim();
    if body.is_empty() {
        "⏹ Génération interrompue.".to_string()
    } else {
        format!("{}\n\n⏹ *Génération interrompue par l'utilisateur.*", body)
    }
}

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

    #[tokio::test]
    async fn streams_multiple_chunks_successfully() {
        let mut chunks_received = Vec::new();
        let result = prompt_stream_with_retry(
            || async {
                let items: Vec<Result<String, String>> =
                    vec![Ok("Hello, ".to_string()), Ok("world!".to_string())];
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
                let items: Vec<Result<String, String>> =
                    vec![Ok("Chunk 1".to_string()), Ok("Chunk 2".to_string())];
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
}
