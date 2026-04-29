mod error;

pub use crate::error::ChatError;
pub type Result<T> = std::result::Result<T, ChatError>;

use std::collections::HashMap;
use std::sync::Arc;

use futures_lite::StreamExt;
use iroh::{protocol::Router, Endpoint, EndpointAddr, EndpointId, endpoint::presets};
use iroh_gossip::{
    api::{GossipReceiver, Event},
    net::Gossip,
    proto::TopicId,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex as TokioMutex};

// ─────────────────────────────────────────────────────────────────────────────
// Internal types (same logic as original main.rs, kept private)
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
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
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
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
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
        write!(f, "{}", text)
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

// ─────────────────────────────────────────────────────────────────────────────
// Public uniffi interface (exactly what the Swift hello-world expects)

#[derive(uniffi::Record)]
pub struct ChatMessage {
    pub id: String,
    pub from: String,
    pub text: String,
}

#[derive(uniffi::Object)]
pub struct Chat {
    inner: Arc<TokioMutex<ChatInner>>,
    message_rx: Arc<TokioMutex<mpsc::Receiver<ChatMessage>>>,
}

impl Clone for Chat {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            message_rx: self.message_rx.clone(),
        }
    }
}

struct ChatInner {
    name: Option<String>,
    endpoint: Option<Endpoint>,
    gossip: Option<Arc<Gossip>>,
    router: Option<Router>,
    sender: Option<iroh_gossip::api::GossipSender>,
    message_tx: mpsc::Sender<ChatMessage>,
    names: HashMap<EndpointId, String>,
}

#[uniffi::export(async_runtime = "tokio")]
impl Chat {
    #[uniffi::constructor]
    pub fn new(name: Option<String>) -> Self {
        // Bounded buffer so an unresponsive UI can't grow memory without bound.
        // If this fills up, the receive loop will naturally apply backpressure.
        let (message_tx, message_rx) = mpsc::channel(100);
        Self {
            inner: Arc::new(TokioMutex::new(ChatInner {
                name,
                endpoint: None,
                gossip: None,
                router: None,
                sender: None,
                message_tx,
                names: HashMap::new(),
            })),
            message_rx: Arc::new(TokioMutex::new(message_rx)),
        }
    }

    pub async fn open(&self) -> Result<String> {
        let mut inner = self.inner.lock().await;

        if inner.endpoint.is_none() {
            let endpoint = Endpoint::bind(presets::N0)
                .await
                .map_err(ChatError::endpoint_bind)?;
            let gossip = Arc::new(Gossip::builder().spawn(endpoint.clone()));
            let router = Router::builder(endpoint.clone())
                .accept(iroh_gossip::ALPN, gossip.clone())
                .spawn();

            inner.endpoint = Some(endpoint);
            inner.gossip = Some(gossip);
            inner.router = Some(router);
        }

        let endpoint = inner.endpoint.as_ref().unwrap();
        let topic = TopicId::from_bytes(rand::random());
        let me = endpoint.addr();
        let ticket = Ticket { topic, endpoints: vec![me] };
        let ticket_str = ticket.to_string();

        // Join our own newly created topic (no peers yet), but don't block the caller.
        // The CLI expects to print the ticket immediately, even if joining takes time.
        drop(inner);
        let chat = self.clone();
        tokio::spawn(async move {
            let _ = chat.join_topic(topic, vec![]).await;
        });

        Ok(ticket_str)
    }

    pub async fn join(&self, ticket_str: String) -> Result<()> {
        let ticket: Ticket = ticket_str.parse()?;
        let endpoint_ids: Vec<EndpointId> = ticket.endpoints.iter().map(|p| p.id).collect();

        let mut inner = self.inner.lock().await;

        if inner.endpoint.is_none() {
            let endpoint = Endpoint::bind(presets::N0)
                .await
                .map_err(ChatError::endpoint_bind)?;
            let gossip = Arc::new(Gossip::builder().spawn(endpoint.clone()));
            let router = Router::builder(endpoint.clone())
                .accept(iroh_gossip::ALPN, gossip.clone())
                .spawn();

            inner.endpoint = Some(endpoint);
            inner.gossip = Some(gossip);
            inner.router = Some(router);
        }

        self.join_internal_locked(&mut inner, ticket.topic, endpoint_ids).await
    }

    pub async fn send(&self, text: String) -> Result<()> {
        let inner = self.inner.lock().await;
        if let (Some(sender), Some(endpoint)) = (&inner.sender, &inner.endpoint) {
            let msg = Message::new(MessageBody::Message {
                from: endpoint.id(),
                text: text.clone(),
            });
            let id = data_encoding::HEXLOWER.encode(&msg.nonce);
            sender
                .broadcast(msg.to_vec().into())
                .await
                .map_err(ChatError::send)?;
            let from_name = inner
                .name
                .clone()
                .unwrap_or_else(|| endpoint.id().fmt_short().to_string());
            let _ = inner
                .message_tx
                .send(ChatMessage {
                    id,
                    from: from_name,
                    text,
                })
                .await;
        }
        Ok(())
    }

    /// Receive the next incoming chat message.
    ///
    /// This is a pull-based API (no callback interface), which avoids Swift callback vtables.
    pub async fn next_message(&self) -> Option<ChatMessage> {
        let mut rx = self.message_rx.lock().await;
        rx.recv().await
    }
}

// Private helpers (not exported to uniffi)
impl Chat {
    async fn join_topic(&self, topic: TopicId, endpoint_ids: Vec<EndpointId>) -> Result<()> {
        let mut inner = self.inner.lock().await;
        self.join_internal_locked(&mut inner, topic, endpoint_ids).await
    }

    async fn join_internal_locked(
        &self,
        inner: &mut ChatInner,
        topic: TopicId,
        endpoint_ids: Vec<EndpointId>,
    ) -> Result<()> {
        let gossip = inner.gossip.as_ref().unwrap();
        let (sender, receiver) = gossip
            .subscribe_and_join(topic, endpoint_ids)
            .await
            .map_err(ChatError::gossip)?
            .split();

        inner.sender = Some(sender);

        // announce ourselves if we have a name
        if let (Some(name), Some(endpoint)) = (&inner.name, &inner.endpoint) {
            let msg = Message::new(MessageBody::AboutMe {
                from: endpoint.id(),
                name: name.clone(),
            });
            if let Some(sender) = &inner.sender {
                sender
                    .broadcast(msg.to_vec().into())
                    .await
                    .map_err(ChatError::send)?;
            }
        }

        // start receive loop (only once)
        let message_tx = inner.message_tx.clone();
        let names = std::mem::take(&mut inner.names); // move current names
        let names = Arc::new(TokioMutex::new(names));

        tokio::spawn(async move {
            let _ = subscribe_loop(receiver, message_tx, names).await;
        });

        Ok(())
    }
}

async fn subscribe_loop(
    mut receiver: GossipReceiver,
    message_tx: mpsc::Sender<ChatMessage>,
    names: Arc<TokioMutex<HashMap<EndpointId, String>>>,
) -> Result<()> {
    while let Some(event) = receiver
        .try_next()
        .await
        .map_err(ChatError::gossip)?
    {
        if let Event::Received(msg) = event {
            if let Ok(message) = Message::from_bytes(&msg.content) {
                match message.body {
                    MessageBody::AboutMe { from, name } => {
                        let mut names = names.lock().await;
                        names.insert(from, name);
                    }
                    MessageBody::Message { from, text } => {
                        let from_name = {
                            let names = names.lock().await;
                            names
                                .get(&from)
                                .cloned()
                                .unwrap_or_else(|| from.fmt_short().to_string())
                        };
                        let id = data_encoding::HEXLOWER.encode(&message.nonce);
                        // Ignore send errors (receiver dropped / shutdown).
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

// ─────────────────────────────────────────────────────────────────────────────
// uniffi scaffolding (one line – everything else is generated)
uniffi::setup_scaffolding!();