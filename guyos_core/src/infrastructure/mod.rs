//! Composition root and bootstrap wiring (ADR 0002).

use std::sync::Arc;

use crate::adapters::outbound::IrohGossipRelayBackend;
use crate::application::ClientChatService;

/// Production wiring: constructs [`IrohGossipRelayBackend`] and [`ClientChatService`].
///
/// This is the single documented composition site for the real relay stack (Phase 0).
/// [`crate::adapters::inbound::Chat::new`] wraps the returned handle in the UniFFI shell.
pub(crate) fn wire_chat_for_clients(
    name: Option<String>,
) -> Arc<ClientChatService<IrohGossipRelayBackend>> {
    let (message_tx, message_rx) = tokio::sync::mpsc::channel(100);
    let backend = IrohGossipRelayBackend::new();
    Arc::new(ClientChatService::new(
        backend, name, message_tx, message_rx,
    ))
}
