//! Multi-turn agent loops (streaming and non-streaming) for LLM interactions.
//!
//! The client is split into focused submodules:
//! - [`agent_loop`] — the simple, non-streaming retry loop (legacy);
//! - [`policy`] — turn intent analysis, tool selection and message hygiene;
//! - [`sse`] — the streaming SSE delta accumulator;
//! - [`stream_loop`] — the orchestration of the streaming agent loop.

pub mod agent_loop;
pub mod policy;
pub mod sse;
pub mod stream_loop;

#[allow(unused_imports)]
pub use agent_loop::run_agent_loop;
pub use stream_loop::run_agent_loop_stream;

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
