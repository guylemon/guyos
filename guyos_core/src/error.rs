use std::fmt;

/// Error type exposed to Swift via uniffi.
/// No third-party crates required.
#[derive(Debug, uniffi::Error)]
pub enum ChatError {
    /// Failed to create or bind the iroh endpoint
    EndpointBind(String),

    /// Ticket was malformed (bad base32 or invalid JSON)
    InvalidTicket(String),

    /// Failed to join or subscribe to the gossip topic
    Gossip(String),

    /// Failed to broadcast a message
    Send(String),

    /// Any other unexpected internal error
    Internal(String),
}

impl fmt::Display for ChatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChatError::EndpointBind(msg) => {
                write!(f, "Failed to create network endpoint: {}", msg)
            }
            ChatError::InvalidTicket(msg) => {
                write!(f, "Invalid chat ticket: {}", msg)
            }
            ChatError::Gossip(msg) => {
                write!(f, "Gossip protocol error: {}", msg)
            }
            ChatError::Send(msg) => {
                write!(f, "Failed to send message: {}", msg)
            }
            ChatError::Internal(msg) => {
                write!(f, "Internal chat error: {}", msg)
            }
        }
    }
}

impl std::error::Error for ChatError {}

// Convenience constructors (optional but nice for the library)
impl ChatError {
    pub fn endpoint_bind<E: std::fmt::Display>(e: E) -> Self {
        ChatError::EndpointBind(e.to_string())
    }

    pub fn invalid_ticket<E: std::fmt::Display>(e: E) -> Self {
        ChatError::InvalidTicket(e.to_string())
    }

    pub fn gossip<E: std::fmt::Display>(e: E) -> Self {
        ChatError::Gossip(e.to_string())
    }

    pub fn send<E: std::fmt::Display>(e: E) -> Self {
        ChatError::Send(e.to_string())
    }

    pub fn internal<E: std::fmt::Display>(e: E) -> Self {
        ChatError::Internal(e.to_string())
    }
}