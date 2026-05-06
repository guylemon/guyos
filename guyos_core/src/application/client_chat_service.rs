use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex as TokioMutex, mpsc};

use crate::ChatError;
use crate::domain::ChatMessage;
use crate::ports::inbound::HubClientSession;
use crate::ports::outbound::RelayChatBackend;

/// Application service coordinating relay chat with a pluggable [`RelayChatBackend`](crate::ports::outbound::RelayChatBackend).
pub struct ClientChatService<B: RelayChatBackend> {
    pub(crate) backend: TokioMutex<B>,
    pub(crate) name: Option<String>,
    pub(crate) names: Arc<TokioMutex<HashMap<String, String>>>,
    pub(crate) message_tx: mpsc::Sender<ChatMessage>,
    pub(crate) message_rx: Arc<TokioMutex<mpsc::Receiver<ChatMessage>>>,
}

impl<B: RelayChatBackend> ClientChatService<B> {
    /// Channel capacity mirrors the prior [`Chat`](crate::adapters::inbound::Chat) constructor.
    pub fn new(
        backend: B,
        name: Option<String>,
        message_tx: mpsc::Sender<ChatMessage>,
        message_rx: mpsc::Receiver<ChatMessage>,
    ) -> Self {
        Self {
            backend: TokioMutex::new(backend),
            name,
            names: Arc::new(TokioMutex::new(HashMap::new())),
            message_tx,
            message_rx: Arc::new(TokioMutex::new(message_rx)),
        }
    }
}

impl<B: RelayChatBackend + 'static> HubClientSession for Arc<ClientChatService<B>> {
    async fn open(&self) -> Result<String, ChatError> {
        let mut backend = self.backend.lock().await;
        let ticket = backend.begin_open_room().await?;
        drop(backend);

        let this = Arc::clone(self);
        // TODO(phase-3): supervised join completion / explicit shutdown instead of fire-and-forget spawn.
        tokio::spawn(async move {
            let mut backend = this.backend.lock().await;
            let display_name = this.name.as_deref();
            let _ = backend
                .finish_open_join_pending(
                    this.message_tx.clone(),
                    Arc::clone(&this.names),
                    display_name,
                )
                .await;
        });

        Ok(ticket)
    }

    async fn join(&self, ticket_str: String) -> Result<(), ChatError> {
        let mut backend = self.backend.lock().await;
        let display_name = self.name.as_deref();
        backend
            .join_room(
                &ticket_str,
                self.message_tx.clone(),
                Arc::clone(&self.names),
                display_name,
            )
            .await
    }

    async fn send(&self, text: String) -> Result<(), ChatError> {
        let mut backend = self.backend.lock().await;
        let display_name = self.name.as_deref();
        backend
            .send_user_message(display_name, text, &self.message_tx)
            .await
    }

    async fn next_message(&self) -> Option<ChatMessage> {
        let mut rx = self.message_rx.lock().await;
        rx.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::outbound::fake_relay_chat_backend::FakeRelayChatBackend;

    #[tokio::test]
    async fn fake_backend_join_and_send_roundtrip() {
        let (tx, rx) = mpsc::channel(100);
        let service = Arc::new(ClientChatService::new(
            FakeRelayChatBackend::new(),
            Some("alice".to_string()),
            tx,
            rx,
        ));

        HubClientSession::join(&service, "not-empty-ticket".to_string())
            .await
            .unwrap();
        assert!(service.backend.lock().await.joined);

        HubClientSession::send(&service, "hello".to_string())
            .await
            .unwrap();
        assert_eq!(
            service.backend.lock().await.last_send.as_deref(),
            Some("hello")
        );

        let msg = HubClientSession::next_message(&service)
            .await
            .expect("echo");
        assert_eq!(msg.text, "hello");
    }

    #[tokio::test]
    async fn fake_backend_open_spawns_finish_path() {
        let (tx, rx) = mpsc::channel(100);
        let service = Arc::new(ClientChatService::new(
            FakeRelayChatBackend::new(),
            None,
            tx,
            rx,
        ));

        let ticket = HubClientSession::open(&service).await.unwrap();
        assert_eq!(ticket, "fake-open-ticket");

        // Allow spawned join to run.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert!(service.backend.lock().await.joined);
    }
}
