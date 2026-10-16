//! User approval gate for shell commands proposed by the assistant.
//!
//! Commands the assistant wants to run are gated behind an explicit user
//! decision. The assistant's `run_command` tool blocks (asynchronously) until
//! the UI resolves the request, so nothing is executed without consent.
//!
//! Read-only/inspection commands are pre-approved (see
//! [`crate::tools::is_pre_approved_command`]) to keep the workflow smooth, and
//! the user can opt to auto-approve every command for the current session.

use tokio::sync::watch;

pub use crate::session::PendingCommand;
use crate::session::default_session;

/// Subscribe to pending-command changes. Used by the UI to render the
/// confirmation bar and hide it once the request is resolved.
pub fn subscribe() -> watch::Receiver<Option<PendingCommand>> {
    default_session().subscribe_pending_commands()
}

/// Enable or disable auto-approval of all commands for the current session.
pub fn set_auto_approve(enabled: bool) {
    default_session().set_auto_approve(enabled);
}

/// Ask the user to approve `command`. Blocks until the user answers, or denies
/// after the approval timeout. Returns `true` when the command may run.
pub async fn request_command_approval(command: &str, cwd: Option<&str>) -> bool {
    default_session()
        .request_command_approval(command, cwd)
        .await
}

/// Resolve a pending command. Returns `true` if it matched an active request.
pub async fn resolve_command(id: &str, approved: bool) -> bool {
    default_session().resolve_command(id, approved).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::AgentSession;
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn resolve_unknown_id_is_noop() {
        assert!(!resolve_command("does-not-exist", true).await);
    }

    #[tokio::test]
    async fn approval_roundtrip_on_isolated_session() {
        let session = Arc::new(AgentSession::new());
        let session_for_task = Arc::clone(&session);
        let handle = tokio::spawn(async move {
            session_for_task
                .request_command_approval("custom-tool --run", None)
                .await
        });

        // Wait for the request to be registered (watch retains the latest value).
        let mut rx = session.subscribe_pending_commands();
        let mut pending = None;
        for _ in 0..100 {
            if let Some(p) = rx.borrow().clone() {
                pending = Some(p);
                break;
            }
            let _ = tokio::time::timeout(Duration::from_millis(10), rx.changed()).await;
        }
        let pending = pending.expect("a pending command should be registered");
        assert!(session.resolve_command(&pending.id, true).await);
        assert!(handle.await.unwrap());
    }
}
