//! Outbound ports — traits for external systems the application depends on.

mod relay_chat_backend;

pub(crate) use relay_chat_backend::RelayChatBackend;

#[cfg(test)]
pub(crate) mod fake_relay_chat_backend;
