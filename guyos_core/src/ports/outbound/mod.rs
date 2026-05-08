//! Outbound ports — traits for external systems the application depends on.

#[allow(dead_code)] // F1 helpers used by the wire session fake (tests) and future QUIC adapters (ADR 0003).
mod f1_framing;
#[allow(dead_code)] // Outbound port + errors wired from application/adapters in a later milestone.
mod guyos_wire_v1_session;
mod relay_chat_backend;

// Re-export for crate-internal callers (application / future adapters).
#[allow(unused_imports)]
pub(crate) use guyos_wire_v1_session::{
    hub_try_send_wire_error_for_session_read_failure, wire_error_json_body_for_session_read_failure,
    GuyosWireV1Session, GuyosWireV1SessionError,
};
pub(crate) use relay_chat_backend::RelayChatBackend;

#[cfg(test)]
pub(crate) mod fake_guyos_wire_v1_session;

#[cfg(test)]
pub(crate) mod fake_relay_chat_backend;
