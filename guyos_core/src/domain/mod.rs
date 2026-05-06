//! Pure domain model, invariants, and value objects (ADR 0002).

/// One chat message delivered to app/UI code (UniFFI record; defined in `domain`).
#[derive(Clone, Debug, uniffi::Record)]
pub struct ChatMessage {
    pub id: String,
    pub from: String,
    pub text: String,
}
