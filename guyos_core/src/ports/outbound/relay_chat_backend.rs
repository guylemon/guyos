use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use tokio::sync::{Mutex as TokioMutex, mpsc};

use crate::ChatError;
use crate::domain::ChatMessage;

/// Operation-level outbound port for relay chat (spike shape; evolves in later phases).
///
/// Implemented by the concrete outbound adapter and by test doubles (`#[cfg(test)]` only).
///
/// Return futures are [`Send`] so callers may `tokio::spawn` join completions (Phase 3 may tighten this).
pub trait RelayChatBackend: Send + Sync {
    /// Binds the stack if needed and creates a new topic; returns a ticket string.
    /// Caller must invoke [`Self::finish_open_join_pending`] in a follow-up task.
    fn begin_open_room(&mut self) -> impl Future<Output = Result<String, ChatError>> + Send;

    /// Completes the “open” flow by joining the pending topic with no remote peers.
    fn finish_open_join_pending(
        &mut self,
        message_tx: mpsc::Sender<ChatMessage>,
        names: Arc<TokioMutex<HashMap<String, String>>>,
        display_name: Option<&str>,
    ) -> impl Future<Output = Result<(), ChatError>> + Send;

    /// Joins an existing room from a ticket string.
    fn join_room(
        &mut self,
        ticket_str: &str,
        message_tx: mpsc::Sender<ChatMessage>,
        names: Arc<TokioMutex<HashMap<String, String>>>,
        display_name: Option<&str>,
    ) -> impl Future<Output = Result<(), ChatError>> + Send;

    /// Broadcasts a user message to peers and echoes locally through `message_tx`.
    fn send_user_message(
        &mut self,
        display_name: Option<&str>,
        text: String,
        message_tx: &mpsc::Sender<ChatMessage>,
    ) -> impl Future<Output = Result<(), ChatError>> + Send;
}
