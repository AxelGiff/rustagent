//! Streaming SSE delta accumulator for the agent loop.
//!
//! Encapsulates the mutable state that accumulates an OpenAI-compatible
//! streaming response: visible text, hidden reasoning (via [`ThinkingFilter`]),
//! and tool calls that may be spread across many chunks.

use super::super::types::ToolCall;
use super::policy::finalize_tool_calls;
use crate::tools::ThinkingFilter;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Accumulates content and tool calls from a streaming completion response.
pub struct DeltaAccumulator {
    /// Full raw assistant content (including `<think>` blocks).
    full_content: String,
    /// Streaming thinking-tag filter producing user-visible text.
    thinking: ThinkingFilter,
    tool_calls: BTreeMap<usize, (String, String, String)>,
    announced_tools: BTreeSet<usize>,
    streamed_arg_bytes: usize,
    /// Last non-empty `finish_reason` reported by the provider
    /// (`"stop"`, `"tool_calls"`, `"length"`, ...). `"length"` means the
    /// completion was cut off by `max_tokens`.
    finish_reason: Option<String>,
}

impl Default for DeltaAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl DeltaAccumulator {
    /// Create an empty accumulator.
    pub fn new() -> Self {
        Self {
            full_content: String::new(),
            thinking: ThinkingFilter::new(),
            tool_calls: BTreeMap::new(),
            announced_tools: BTreeSet::new(),
            streamed_arg_bytes: 0,
            finish_reason: None,
        }
    }

    /// Feed one `data:` payload of the SSE stream.
    ///
    /// Returns the pieces of text to append to the transcript (and stream to
    /// the UI), or an error message when the API signalled a failure.
    pub fn process_data(&mut self, data: &str) -> Result<Vec<String>, String> {
        let mut out = Vec::new();

        let Ok(parsed) = serde_json::from_str::<Value>(data) else {
            return Ok(out);
        };

        if let Some(err) = parsed.get("error") {
            let err_msg = err["message"].as_str().unwrap_or(data);
            return Err(err_msg.to_string());
        }

        let Some(choices) = parsed.get("choices").and_then(|c| c.as_array()) else {
            return Ok(out);
        };
        let Some(first) = choices.first() else {
            return Ok(out);
        };

        // Mémorise la raison de fin (utile pour détecter une troncature `max_tokens`).
        if let Some(fr) = first.get("finish_reason").and_then(|v| v.as_str())
            && !fr.is_empty()
            && fr != "null"
        {
            self.finish_reason = Some(fr.to_string());
        }

        let delta_obj = first
            .get("delta")
            .or_else(|| first.get("message"))
            .and_then(|d| d.as_object());
        if let Some(delta) = delta_obj {
            // 1. Détection du raisonnement natif (DeepSeek-R1 / Albert)
            if let Some(reasoning) = delta
                .get("reasoning_content")
                .or_else(|| delta.get("reasoning"))
                .or_else(|| delta.get("thought"))
                .and_then(|v| v.as_str())
                && let Some(banner) = self.thinking.ingest_reasoning(reasoning)
            {
                out.push(banner.to_string());
            }

            // 2. Traitement du contenu texte avec masquage de <think>...</think>
            if let Some(content) = delta.get("content").and_then(|v| v.as_str())
                && !content.is_empty()
            {
                self.full_content.push_str(content);
                let safe_chunk = self.thinking.process_chunk(content);
                if !safe_chunk.is_empty() {
                    out.push(safe_chunk);
                }
            }

            if let Some(calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                for call in calls {
                    let idx = call["index"].as_u64().unwrap_or(0) as usize;
                    let entry = self.tool_calls.entry(idx).or_insert((
                        String::new(),
                        String::new(),
                        String::new(),
                    ));
                    if let Some(id) = call.get("id").and_then(|v| v.as_str())
                        && entry.0.is_empty()
                    {
                        entry.0.push_str(id);
                    }
                    let fn_name = call
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .or_else(|| call.get("name"))
                        .and_then(|v| v.as_str());
                    if let Some(name) = fn_name {
                        if entry.1.is_empty() {
                            entry.1.push_str(name);
                        }
                        if !entry.1.is_empty() && self.announced_tools.insert(idx) {
                            out.push(format!("\n⏳ *Préparation de `{}` en cours* ", entry.1));
                        }
                    }

                    let raw_args_val = call
                        .get("function")
                        .and_then(|f| f.get("arguments").or_else(|| f.get("parameters")))
                        .or_else(|| call.get("arguments"))
                        .or_else(|| call.get("parameters"));

                    if let Some(val) = raw_args_val {
                        if let Some(args_str) = val.as_str() {
                            entry.2.push_str(args_str);
                            let prev_dots = self.streamed_arg_bytes / 500;
                            self.streamed_arg_bytes += args_str.len();
                            let new_dots = self.streamed_arg_bytes / 500;
                            if new_dots > prev_dots {
                                out.push("·".to_string());
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
            } else if let Some(legacy_fn) =
                delta.get("function_call").or_else(|| delta.get("function"))
            {
                let entry = self.tool_calls.entry(0).or_insert((
                    String::new(),
                    String::new(),
                    String::new(),
                ));
                if let Some(name) = legacy_fn.get("name").and_then(|v| v.as_str()) {
                    entry.1.push_str(name);
                    if !entry.1.is_empty() && self.announced_tools.insert(0) {
                        out.push(format!("\n⏳ *Préparation de `{}` en cours* ", entry.1));
                    }
                }
                let raw_args_val = legacy_fn
                    .get("arguments")
                    .or_else(|| legacy_fn.get("parameters"));
                if let Some(val) = raw_args_val {
                    if let Some(args_str) = val.as_str() {
                        entry.2.push_str(args_str);
                    } else {
                        entry.2 = serde_json::to_string(val).unwrap_or_default();
                    }
                }
            }
        } else if let Some(text) = first.get("text").and_then(|v| v.as_str())
            && !text.is_empty()
        {
            self.full_content.push_str(text);
            let safe_chunk = self.thinking.process_chunk(text);
            if !safe_chunk.is_empty() {
                out.push(safe_chunk);
            }
        }

        Ok(out)
    }

    /// Whether the model requested at least one tool call.
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }

    /// Finalize the accumulated tool-call fragments into a stable list.
    pub fn tool_calls(&self, iteration: usize) -> Vec<ToolCall> {
        finalize_tool_calls(&self.tool_calls, iteration)
    }

    /// The last `finish_reason` seen on the stream, if any.
    pub fn finish_reason(&self) -> Option<&str> {
        self.finish_reason.as_deref()
    }

    /// Move the full raw content out of the accumulator.
    pub fn take_full_content(&mut self) -> String {
        std::mem::take(&mut self.full_content)
    }

    /// Flush any remaining buffered visible text from the thinking filter.
    pub fn finalize(&mut self) -> String {
        self.thinking.finalize()
    }

    /// The user-visible text accumulated so far (used on cancellation).
    pub fn clean_content(&self) -> &str {
        &self.thinking.clean_content
    }
}
