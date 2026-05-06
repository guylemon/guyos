use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use tokio::sync::{Mutex as TokioMutex, mpsc};

use crate::ChatError;
use crate::domain::ChatMessage;

use super::RelayChatBackend;

/// Test double for [`RelayChatBackend`](super::RelayChatBackend).
pub struct FakeRelayChatBackend {
    pending_ticket: Option<String>,
    pub joined: bool,
    pub last_send: Option<String>,
}

impl FakeRelayChatBackend {
    pub fn new() -> Self {
        Self {
            pending_ticket: None,
            joined: false,
            last_send: None,
        }
    }

    async fn begin_open_room_inner(&mut self) -> Result<String, ChatError> {
        let ticket = "fake-open-ticket".to_string();
        self.pending_ticket = Some(ticket.clone());
        Ok(ticket)
    }

    async fn finish_open_join_pending_inner(
        &mut self,
        _message_tx: mpsc::Sender<ChatMessage>,
        _names: Arc<TokioMutex<HashMap<String, String>>>,
        _display_name: Option<&str>,
    ) -> Result<(), ChatError> {
        self.joined = true;
        self.pending_ticket = None;
        Ok(())
    }

    async fn join_room_inner(
        &mut self,
        ticket_str: &str,
        _message_tx: mpsc::Sender<ChatMessage>,
        _names: Arc<TokioMutex<HashMap<String, String>>>,
        _display_name: Option<&str>,
    ) -> Result<(), ChatError> {
        if ticket_str.is_empty() {
            return Err(ChatError::invalid_ticket("empty ticket"));
        }
        self.joined = true;
        Ok(())
    }

    async fn send_user_message_inner(
        &mut self,
        _display_name: Option<&str>,
        text: String,
        message_tx: &mpsc::Sender<ChatMessage>,
    ) -> Result<(), ChatError> {
        self.last_send = Some(text.clone());
        let _ = message_tx
            .send(ChatMessage {
                id: "fake".to_string(),
                from: "self".to_string(),
                text,
            })
            .await;
        Ok(())
    }
}

impl RelayChatBackend for FakeRelayChatBackend {
    fn begin_open_room(&mut self) -> impl Future<Output = Result<String, ChatError>> + Send {
        self.begin_open_room_inner()
    }

    fn finish_open_join_pending(
        &mut self,
        message_tx: mpsc::Sender<ChatMessage>,
        names: Arc<TokioMutex<HashMap<String, String>>>,
        display_name: Option<&str>,
    ) -> impl Future<Output = Result<(), ChatError>> + Send {
        self.finish_open_join_pending_inner(message_tx, names, display_name)
    }

    fn join_room(
        &mut self,
        ticket_str: &str,
        message_tx: mpsc::Sender<ChatMessage>,
        names: Arc<TokioMutex<HashMap<String, String>>>,
        display_name: Option<&str>,
    ) -> impl Future<Output = Result<(), ChatError>> + Send {
        self.join_room_inner(ticket_str, message_tx, names, display_name)
    }

    fn send_user_message(
        &mut self,
        display_name: Option<&str>,
        text: String,
        message_tx: &mpsc::Sender<ChatMessage>,
    ) -> impl Future<Output = Result<(), ChatError>> + Send {
        self.send_user_message_inner(display_name, text, message_tx)
    }
}
