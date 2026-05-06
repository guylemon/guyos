use std::collections::HashMap;
use std::future::Future;
use std::str::FromStr;
use std::sync::Arc;

use bytes::Bytes;
use futures_lite::StreamExt;
use iroh::{Endpoint, EndpointAddr, EndpointId, endpoint::presets, protocol::Router};
use iroh_gossip::{
    api::{Event, GossipReceiver},
    net::Gossip,
    proto::TopicId,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex as TokioMutex, mpsc};

use crate::ChatError;
use crate::domain::ChatMessage;
use crate::ports::outbound::RelayChatBackend;

// ─────────────────────────────────────────────────────────────────────────────
// Wire types (adapter-local; not exposed to application)

#[derive(Debug, Serialize, Deserialize)]
struct Message {
    body: MessageBody,
    nonce: [u8; 16],
}

#[derive(Debug, Serialize, Deserialize)]
enum MessageBody {
    AboutMe { from: EndpointId, name: String },
    Message { from: EndpointId, text: String },
}

impl Message {
    fn from_bytes(bytes: &[u8]) -> Result<Self, ChatError> {
        serde_json::from_slice(bytes).map_err(ChatError::internal)
    }

    pub fn new(body: MessageBody) -> Self {
        Self {
            body,
            nonce: rand::random(),
        }
    }

    pub fn to_vec(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serde_json::to_vec is infallible")
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Ticket {
    topic: TopicId,
    endpoints: Vec<EndpointAddr>,
}

impl Ticket {
    fn from_bytes(bytes: &[u8]) -> Result<Self, ChatError> {
        serde_json::from_slice(bytes).map_err(ChatError::invalid_ticket)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serde_json::to_vec is infallible")
    }
}

impl std::fmt::Display for Ticket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut text = data_encoding::BASE32_NOPAD.encode(&self.to_bytes()[..]);
        text.make_ascii_lowercase();
        write!(f, "{text}")
    }
}

impl std::str::FromStr for Ticket {
    type Err = ChatError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let bytes = data_encoding::BASE32_NOPAD
            .decode(s.to_ascii_uppercase().as_bytes())
            .map_err(ChatError::invalid_ticket)?;
        Self::from_bytes(&bytes)
    }
}

fn peer_key(id: EndpointId) -> String {
    id.fmt_short().to_string()
}

/// Concrete [`RelayChatBackend`] for the current iroh-gossip spike.
pub struct IrohGossipRelayBackend {
    endpoint: Option<Endpoint>,
    gossip: Option<Arc<Gossip>>,
    router: Option<Router>,
    sender: Option<iroh_gossip::api::GossipSender>,
    pending_topic: Option<TopicId>,
}

impl IrohGossipRelayBackend {
    pub fn new() -> Self {
        Self {
            endpoint: None,
            gossip: None,
            router: None,
            sender: None,
            pending_topic: None,
        }
    }

    async fn ensure_stack(&mut self) -> Result<(), ChatError> {
        if self.endpoint.is_none() {
            let endpoint = Endpoint::bind(presets::N0)
                .await
                .map_err(ChatError::endpoint_bind)?;
            let gossip = Arc::new(Gossip::builder().spawn(endpoint.clone()));
            let router = Router::builder(endpoint.clone())
                .accept(iroh_gossip::ALPN, gossip.clone())
                .spawn();

            self.endpoint = Some(endpoint);
            self.gossip = Some(gossip);
            self.router = Some(router);
        }
        Ok(())
    }

    async fn join_internal(
        &mut self,
        topic: TopicId,
        endpoint_ids: Vec<EndpointId>,
        message_tx: mpsc::Sender<ChatMessage>,
        names: Arc<TokioMutex<HashMap<String, String>>>,
        display_name: Option<&str>,
    ) -> Result<(), ChatError> {
        let gossip = self.gossip.as_ref().unwrap();
        let (sender, receiver) = gossip
            .subscribe_and_join(topic, endpoint_ids)
            .await
            .map_err(ChatError::gossip)?
            .split();

        self.sender = Some(sender);

        if let (Some(name), Some(endpoint)) = (display_name, &self.endpoint) {
            let msg = Message::new(MessageBody::AboutMe {
                from: endpoint.id(),
                name: name.to_string(),
            });
            if let Some(sender) = &self.sender {
                sender
                    .broadcast(msg.to_vec().into())
                    .await
                    .map_err(ChatError::send)?;
            }
        }

        // TODO(phase-3): replace fire-and-forget spawn with supervised task / explicit shutdown.
        let message_tx_clone = message_tx.clone();
        let names_clone = Arc::clone(&names);
        tokio::spawn(async move {
            let _ = subscribe_loop(receiver, message_tx_clone, names_clone).await;
        });

        Ok(())
    }

    async fn begin_open_room_inner(&mut self) -> Result<String, ChatError> {
        self.ensure_stack().await?;

        let endpoint = self.endpoint.as_ref().unwrap();
        let topic = TopicId::from_bytes(rand::random());
        self.pending_topic = Some(topic);
        let me = endpoint.addr();
        let ticket = Ticket {
            topic,
            endpoints: vec![me],
        };
        Ok(ticket.to_string())
    }

    async fn finish_open_join_pending_inner(
        &mut self,
        message_tx: mpsc::Sender<ChatMessage>,
        names: Arc<TokioMutex<HashMap<String, String>>>,
        display_name: Option<&str>,
    ) -> Result<(), ChatError> {
        let topic = self
            .pending_topic
            .take()
            .ok_or_else(|| ChatError::internal("open room has no pending topic"))?;
        self.join_internal(topic, vec![], message_tx, names, display_name)
            .await
    }

    async fn join_room_inner(
        &mut self,
        ticket_str: &str,
        message_tx: mpsc::Sender<ChatMessage>,
        names: Arc<TokioMutex<HashMap<String, String>>>,
        display_name: Option<&str>,
    ) -> Result<(), ChatError> {
        let ticket = Ticket::from_str(ticket_str)?;
        let endpoint_ids: Vec<EndpointId> = ticket.endpoints.iter().map(|p| p.id).collect();

        self.ensure_stack().await?;

        self.join_internal(ticket.topic, endpoint_ids, message_tx, names, display_name)
            .await
    }

    async fn send_user_message_inner(
        &mut self,
        display_name: Option<&str>,
        text: String,
        message_tx: &mpsc::Sender<ChatMessage>,
    ) -> Result<(), ChatError> {
        if let (Some(sender), Some(endpoint)) = (&self.sender, &self.endpoint) {
            let msg = Message::new(MessageBody::Message {
                from: endpoint.id(),
                text: text.clone(),
            });
            let id = data_encoding::HEXLOWER.encode(&msg.nonce);
            sender
                .broadcast(Bytes::from(msg.to_vec()))
                .await
                .map_err(ChatError::send)?;
            let from_name = display_name
                .map(str::to_string)
                .unwrap_or_else(|| endpoint.id().fmt_short().to_string());
            let _ = message_tx
                .send(ChatMessage {
                    id,
                    from: from_name,
                    text,
                })
                .await;
        }
        Ok(())
    }
}

impl RelayChatBackend for IrohGossipRelayBackend {
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

async fn subscribe_loop(
    mut receiver: GossipReceiver,
    message_tx: mpsc::Sender<ChatMessage>,
    names: Arc<TokioMutex<HashMap<String, String>>>,
) -> Result<(), ChatError> {
    while let Some(event) = receiver.try_next().await.map_err(ChatError::gossip)? {
        if let Event::Received(msg) = event {
            if let Ok(message) = Message::from_bytes(&msg.content) {
                match message.body {
                    MessageBody::AboutMe { from, name } => {
                        let mut names = names.lock().await;
                        names.insert(peer_key(from), name);
                    }
                    MessageBody::Message { from, text } => {
                        let from_name = {
                            let names = names.lock().await;
                            names
                                .get(&peer_key(from))
                                .cloned()
                                .unwrap_or_else(|| from.fmt_short().to_string())
                        };
                        let id = data_encoding::HEXLOWER.encode(&message.nonce);
                        let _ = message_tx
                            .send(ChatMessage {
                                id,
                                from: from_name,
                                text,
                            })
                            .await;
                    }
                }
            }
        }
    }
    Ok(())
}
