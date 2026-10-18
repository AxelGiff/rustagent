//! Grouped, per-session mutable state for the agent.
//!
//! The cancellation flag, the shell-command approval queue, the staged-file
//! registry and the verification-workspace cache are all *session-scoped*. They
//! used to be independent process-wide `static`s; they now live together behind
//! [`AgentSession`], which makes the state cohesive, documented and easy to
//! instantiate in isolation.
//!
//! The running application uses a single process-wide instance
//! ([`default_session`]); tests can build an isolated session with
//! [`AgentSession::new`] so they no longer contaminate each other.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tokio::sync::{Mutex as AsyncMutex, oneshot, watch};

/// Maximum time to wait for a user decision before denying by default.
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

/// A shell command proposed by the assistant that awaits explicit user approval.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingCommand {
    /// Unique identifier used to resolve the request.
    pub id: String,
    /// The shell command to be executed.
    pub command: String,
    /// Optional working directory for the command.
    pub cwd: Option<String>,
}

struct ApprovalState {
    pending: Option<PendingCommand>,
    responder: Option<oneshot::Sender<bool>>,
}

/// All mutable state owned by one agent session.
///
/// Cheap to construct; the application keeps a single instance via
/// [`default_session`] while tests build their own.
pub struct AgentSession {
    cancel: AtomicBool,
    auto_approve: AtomicBool,
    approval_counter: AtomicU64,
    approval: AsyncMutex<ApprovalState>,
    approval_channel: watch::Sender<Option<PendingCommand>>,
    staging: Mutex<BTreeMap<String, String>>,
    workspaces: Mutex<BTreeMap<String, PathBuf>>,
}

impl Default for AgentSession {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentSession {
    /// Create a fresh, isolated session.
    pub fn new() -> Self {
        Self {
            cancel: AtomicBool::new(false),
            auto_approve: AtomicBool::new(false),
            approval_counter: AtomicU64::new(1),
            approval: AsyncMutex::new(ApprovalState {
                pending: None,
                responder: None,
            }),
            approval_channel: watch::channel(None).0,
            staging: Mutex::new(BTreeMap::new()),
            workspaces: Mutex::new(BTreeMap::new()),
        }
    }

    // -- Cancellation (Stop button) ----------------------------------------

    /// Request the current generation to stop.
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Reset the cancellation state before starting a new generation.
    pub fn reset_cancel(&self) {
        self.cancel.store(false, Ordering::SeqCst);
    }

    /// Whether cancellation has been requested.
    pub fn is_cancel_requested(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    // -- Shell-command approval --------------------------------------------

    /// Subscribe to pending-command changes (used by the UI confirmation bar to
    /// show the bar and hide it once the request is resolved).
    pub fn subscribe_pending_commands(&self) -> watch::Receiver<Option<PendingCommand>> {
        self.approval_channel.subscribe()
    }

    /// Enable or disable auto-approval of all commands for this session.
    pub fn set_auto_approve(&self, enabled: bool) {
        self.auto_approve.store(enabled, Ordering::SeqCst);
    }

    /// Whether the user opted to auto-approve every command for this session.
    pub fn auto_approve(&self) -> bool {
        self.auto_approve.load(Ordering::SeqCst)
    }

    /// Ask the user to approve `command`. Blocks until the user answers, or
    /// denies after [`APPROVAL_TIMEOUT`]. Returns `true` when the command may run.
    pub async fn request_command_approval(&self, command: &str, cwd: Option<&str>) -> bool {
        if self.auto_approve() {
            return true;
        }

        let id = format!(
            "cmd_{}",
            self.approval_counter.fetch_add(1, Ordering::SeqCst)
        );
        let (tx, rx) = oneshot::channel();
        let pending = PendingCommand {
            id: id.clone(),
            command: command.to_string(),
            cwd: cwd.map(str::to_string),
        };

        {
            let mut st = self.approval.lock().await;
            st.pending = Some(pending.clone());
            st.responder = Some(tx);
        }
        let _ = self.approval_channel.send(Some(pending));

        match tokio::time::timeout(APPROVAL_TIMEOUT, rx).await {
            Ok(Ok(approved)) => approved,
            // Timeout or sender dropped: deny and clean up.
            _ => {
                self.clear_if_pending(&id).await;
                false
            }
        }
    }

    /// Resolve a pending command. Returns `true` if it matched an active request.
    pub async fn resolve_command(&self, id: &str, approved: bool) -> bool {
        let matched = {
            let mut st = self.approval.lock().await;
            if st.pending.as_ref().map(|p| p.id == id).unwrap_or(false) {
                if let Some(tx) = st.responder.take() {
                    let _ = tx.send(approved);
                }
                st.pending = None;
                true
            } else {
                false
            }
        };
        if matched {
            let _ = self.approval_channel.send(None);
        }
        matched
    }

    async fn clear_if_pending(&self, id: &str) {
        let cleared = {
            let mut st = self.approval.lock().await;
            if st.pending.as_ref().map(|p| p.id == id).unwrap_or(false) {
                st.pending = None;
                st.responder = None;
                true
            } else {
                false
            }
        };
        if cleared {
            let _ = self.approval_channel.send(None);
        }
    }

    // -- Staged generated files (used by verification) ---------------------

    /// Forget every staged file. Called at the start of each assistant turn so
    /// the registry never mixes files from unrelated requests.
    pub fn clear_staging(&self) {
        if let Ok(mut s) = self.staging.lock() {
            s.clear();
        }
    }

    /// Register a generated file so it is included in the verification
    /// workspace, keyed by its normalized path (forward slashes).
    pub fn stage_file(&self, path: &str, content: &str) {
        let key = path.trim().replace('\\', "/");
        if key.is_empty() {
            return;
        }
        if let Ok(mut s) = self.staging.lock() {
            s.insert(key, content.to_string());
        }
    }

    /// Snapshot of the currently staged files, as `(normalized path, content)`.
    pub fn staged_files(&self) -> Vec<(String, String)> {
        self.staging
            .lock()
            .map(|s| s.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }

    /// Content staged for `path` during the current turn, if any.
    ///
    /// Lets tools that read or edit a file see the version generated earlier in
    /// the same turn (which is not yet written to disk, since diffs await the
    /// user's validation).
    pub fn staged_content(&self, path: &str) -> Option<String> {
        let key = path.trim().replace('\\', "/");
        self.staging.lock().ok().and_then(|s| s.get(&key).cloned())
    }

    // -- Verification workspaces -------------------------------------------

    /// Return the temp verification workspace for `project_root`, creating it
    /// with `create` on first use. The lock is held during creation so two
    /// concurrent verifications of the same project cannot race, and workspaces
    /// of different projects coexist without disturbing each other.
    pub fn workspace_with<F>(&self, project_root: &str, create: F) -> Result<PathBuf, String>
    where
        F: FnOnce() -> Result<PathBuf, String>,
    {
        let mut guard = self
            .workspaces
            .lock()
            .map_err(|_| "verrou interne empoisonné".to_string())?;
        if let Some(root) = guard.get(project_root) {
            return Ok(root.clone());
        }
        let root = create()?;
        guard.insert(project_root.to_string(), root.clone());
        Ok(root)
    }
}

/// The process-wide session used by the running application.
pub fn default_session() -> &'static AgentSession {
    static SESSION: OnceLock<AgentSession> = OnceLock::new();
    SESSION.get_or_init(AgentSession::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_sessions_do_not_share_state() {
        let a = AgentSession::new();
        let b = AgentSession::new();

        a.stage_file("proj/a.js", "1");
        b.stage_file("proj/b.js", "2");
        assert!(a.staged_files().iter().any(|(p, _)| p == "proj/a.js"));
        assert!(!a.staged_files().iter().any(|(p, _)| p == "proj/b.js"));
        assert!(b.staged_files().iter().any(|(p, _)| p == "proj/b.js"));

        a.request_cancel();
        assert!(a.is_cancel_requested());
        assert!(!b.is_cancel_requested());

        a.set_auto_approve(true);
        assert!(a.auto_approve());
        assert!(!b.auto_approve());
    }

    #[test]
    fn staging_normalizes_backslashes_and_keeps_latest() {
        let s = AgentSession::new();
        s.stage_file("Pictures\\app\\app.js", "one");
        s.stage_file("Pictures/app/app.js", "two");
        let files = s.staged_files();
        let js: Vec<_> = files
            .iter()
            .filter(|(p, _)| p == "Pictures/app/app.js")
            .collect();
        assert_eq!(js.len(), 1);
        assert_eq!(js[0].1, "two");
    }
}
