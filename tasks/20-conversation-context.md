# Task 20 — Add multi-turn conversation context to the AI chat

**Status:** 🟢 Completed

## Goal

Maintain a conversation history and send it with each request so the assistant
can reference earlier messages, instead of treating each message in isolation.

## Why

Without context, the assistant cannot answer follow-up questions or remember
earlier code. Multi-turn context makes the chat genuinely useful.

## Steps

- [x] **Step 1 — Add a conversation-history data structure.**
      Add a `Vec<Message>` (or similar) that accumulates user and assistant
      messages across the session.
      *Testable:* A unit test asserts messages are appended in order as they
      are sent and received.

- [x] **Step 2 — Cap the history length.**
      Limit the history to a maximum number of messages (e.g. the last 20) to
      bound token usage.
      *Testable:* A unit test sends more than the cap and asserts only the most
      recent messages are retained.

- [x] **Step 3 — Send history with each request.**
      Update the API call in `src/api.rs` to include the accumulated history in
      the request payload.
      *Testable:* A unit test (or mocked request) asserts the request contains
      the prior messages.

- [x] **Step 4 — Clear history on "clear chat".**
      Ensure the "Effacer le chat" action resets the conversation history as
      well as the visible chat.
      *Testable:* A unit test clears the chat and asserts the history is empty.

- [x] **Step 5 — Keep code extraction scoped to the latest response.**
      Ensure only the latest assistant response is parsed for code blocks, not
      the whole history.
      *Testable:* A test with a multi-turn history asserts the editor receives
      code only from the most recent response.

- [x] **Step 6 — Verify the full suite still passes.**
      *Testable:* `cargo test` passes (total count increases above 81) and
      `cargo clippy --all-targets -- -D warnings` is clean.

## Files changed

- `src/api.rs`
- `src/main.rs`
- `src/flow.rs` (if extraction needs adjustment)
