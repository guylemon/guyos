use std::sync::Arc;

use crate::Result;
use crate::adapters::outbound::IrohGossipRelayBackend;
use crate::application::ClientChatService;
use crate::domain::ChatMessage;
use crate::infrastructure;
use crate::ports::inbound::HubClientSession;

/// UniFFI-facing chat handle. Production wiring builds the real relay stack in `infrastructure::wire_chat_for_clients`.
#[derive(uniffi::Object)]
pub struct Chat {
    inner: Arc<ClientChatService<IrohGossipRelayBackend>>,
}

impl Clone for Chat {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl Chat {
    /// Bounded buffer so an unresponsive UI cannot grow memory without bound.
    /// If this fills up, the receive loop will naturally apply backpressure.
    ///
    /// Uses `infrastructure::wire_chat_for_clients` for production composition.
    #[uniffi::constructor]
    pub fn new(name: Option<String>) -> Self {
        Self {
            inner: infrastructure::wire_chat_for_clients(name),
        }
    }

    pub async fn open(&self) -> Result<String> {
        HubClientSession::open(&self.inner).await
    }

    pub async fn join(&self, ticket_str: String) -> Result<()> {
        HubClientSession::join(&self.inner, ticket_str).await
    }

    pub async fn send(&self, text: String) -> Result<()> {
        HubClientSession::send(&self.inner, text).await
    }

    /// Receive the next incoming chat message (pull-based API for Swift).
    pub async fn next_message(&self) -> Option<ChatMessage> {
        HubClientSession::next_message(&self.inner).await
    }
}
