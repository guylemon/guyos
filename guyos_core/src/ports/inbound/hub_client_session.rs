use crate::ChatError;
use crate::domain::ChatMessage;

/// Inbound port for a client-facing hub chat session (UniFFI-relevant surface).
///
/// Generic `impl` only in Phase 0 — no `dyn` inbound port.
pub trait HubClientSession {
    async fn open(&self) -> Result<String, ChatError>;
    async fn join(&self, ticket_str: String) -> Result<(), ChatError>;
    async fn send(&self, text: String) -> Result<(), ChatError>;
    async fn next_message(&self) -> Option<ChatMessage>;
}
