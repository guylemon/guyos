//! Attach negotiation, policy, and preamble validation (**ADR 0003**).
//!
//! Session dispatch (`handle_client_message`) lives in [`super::guyos_wire_v1`] to avoid a
//! dependency cycle with [`GuyosWireV1Message`].

use crate::domain::reference_ticket_v1::ReferenceTicketError;
use serde_json::{Map, Value};

/// Inclusive upper bound for **`protocol_major`** / **`protocol_minor`** on attach (**ADR 0003**).
pub const U31_MAX: u32 = 2_147_483_647;

// --- Payload shapes (Appendix A) -----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attach {
    pub protocol_major: u32,
    pub protocol_minor: u32,
    pub ticket: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachAck {
    pub room_id: String,
    pub max_frame_bytes: u32,
    pub max_message_bytes: u32,
    pub protocol_major: u32,
    pub server_protocol_minor: u32,
    pub keepalive_interval_seconds: Option<u32>,
}

/// Hub-side attach negotiation limits and per-major minor ceiling (**ADR 0003** planning).
#[derive(Debug, Clone)]
pub struct GuyosWireAttachPolicy {
    pub max_frame_bytes: u32,
    pub max_message_bytes: u32,
    pub keepalive_interval_seconds: Option<u32>,
    supported_major: u32,
    hub_minor_ceiling_for_supported_major: u32,
}

impl GuyosWireAttachPolicy {
    /// v1-style hub: supports exactly one `protocol_major` with a fixed minor ceiling `H`.
    #[cfg_attr(not(test), allow(dead_code))] // Used from unit tests; hub adapters call when wired.
    pub fn single_major(
        supported_major: u32,
        hub_minor_ceiling: u32,
        max_frame_bytes: u32,
        max_message_bytes: u32,
        keepalive_interval_seconds: Option<u32>,
    ) -> Self {
        Self {
            max_frame_bytes,
            max_message_bytes,
            keepalive_interval_seconds,
            supported_major,
            hub_minor_ceiling_for_supported_major: hub_minor_ceiling,
        }
    }

    /// Largest `protocol_minor` the hub implements for `major`, or `None` if unsupported.
    pub fn hub_protocol_minor_ceiling(&self, major: u32) -> Option<u32> {
        if major == self.supported_major {
            Some(self.hub_minor_ceiling_for_supported_major)
        } else {
            None
        }
    }
}

/// Ticket verification injected at the hub boundary (ADR 0003 — no crypto in this module).
pub trait AttachTicketVerifier: Send + Sync {
    fn verify_ticket(&self, ticket: &str) -> Result<String, ReferenceTicketError>;
}

// --- Preamble + JSON attach extraction -----------------------------------------------------------

/// Validate attach **after** JSON parses and `type = attach`, **before** major policy and ticket
/// decode (**ADR 0003** ordering).
pub fn validate_attach_preamble(attach: &Attach) -> Result<(), ()> {
    if attach.protocol_major > U31_MAX || attach.protocol_minor > U31_MAX {
        return Err(());
    }
    Ok(())
}

/// Decode attach fields from a JSON object map (`type` already dispatched).
pub(crate) fn attach_from_decoded_map(
    map: &Map<String, Value>,
) -> Result<Attach, ()> {
    let protocol_major = require_attach_u31(map, "protocol_major")?;
    let protocol_minor = require_attach_u31(map, "protocol_minor")?;
    let ticket = require_attach_string(map, "ticket")?;
    let attach = Attach {
        protocol_major,
        protocol_minor,
        ticket,
    };
    validate_attach_preamble(&attach)?;
    Ok(attach)
}

fn require_attach_u31(map: &Map<String, Value>, key: &str) -> Result<u32, ()> {
    match map.get(key) {
        Some(Value::Number(n)) => {
            let v = n.as_u64().ok_or(())?;
            let v = u32::try_from(v).map_err(|_| ())?;
            if v > U31_MAX {
                return Err(());
            }
            Ok(v)
        }
        _ => Err(()),
    }
}

fn require_attach_string(map: &Map<String, Value>, key: &str) -> Result<String, ()> {
    match map.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        _ => Err(()),
    }
}

// --- Handshake -----------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachHandshakeError {
    InvalidAttach,
    ProtocolMajorUnsupported,
    Ticket(ReferenceTicketError),
}

/// Ordered attach handling: preamble → major policy → ticket (**ADR 0003**).
pub fn accept_attach(
    attach: &Attach,
    policy: &GuyosWireAttachPolicy,
    verifier: &dyn AttachTicketVerifier,
) -> Result<AttachAck, AttachHandshakeError> {
    validate_attach_preamble(attach).map_err(|_| AttachHandshakeError::InvalidAttach)?;
    let h = policy
        .hub_protocol_minor_ceiling(attach.protocol_major)
        .ok_or(AttachHandshakeError::ProtocolMajorUnsupported)?;
    let room_id = verifier
        .verify_ticket(&attach.ticket)
        .map_err(AttachHandshakeError::Ticket)?;
    let server_minor = attach.protocol_minor.min(h);
    Ok(AttachAck {
        room_id,
        max_frame_bytes: policy.max_frame_bytes,
        max_message_bytes: policy.max_message_bytes,
        protocol_major: attach.protocol_major,
        server_protocol_minor: server_minor,
        keepalive_interval_seconds: policy.keepalive_interval_seconds,
    })
}

// --- Stable `error.message` strings (implementation-defined; stable per code in tests) ----------

pub(crate) fn error_message_protocol_major_unsupported() -> &'static str {
    "Protocol major is not supported by this hub."
}

pub(crate) fn error_message_invalid_attach() -> &'static str {
    "Attach payload failed semantic validation."
}

/// Second `attach` after a successful `attach_ack` on the same stream (**ADR 0003**).
pub(crate) fn error_message_invalid_attach_duplicate() -> &'static str {
    "A second attach is not allowed after attach_ack on this stream."
}

pub(crate) fn error_message_ticket_decode_failed() -> &'static str {
    "Ticket could not be decoded."
}

pub(crate) fn error_message_invalid_ticket() -> &'static str {
    "Ticket validation failed."
}

pub(crate) fn error_message_attach_required() -> &'static str {
    "A successful attach is required before this message."
}

fn map_ticket_err(e: ReferenceTicketError) -> &'static str {
    match e {
        ReferenceTicketError::TicketDecodeFailed => error_message_ticket_decode_failed(),
        ReferenceTicketError::InvalidTicket => error_message_invalid_ticket(),
    }
}

/// Map handshake failure to portable code + stable message.
pub fn attach_handshake_portable(
    err: AttachHandshakeError,
) -> (&'static str, &'static str) {
    match err {
        AttachHandshakeError::InvalidAttach => ("invalid_attach", error_message_invalid_attach()),
        AttachHandshakeError::ProtocolMajorUnsupported => (
            "protocol_major_unsupported",
            error_message_protocol_major_unsupported(),
        ),
        AttachHandshakeError::Ticket(e) => (e.portable_code(), map_ticket_err(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingVerifier {
        pub calls: AtomicUsize,
        pub ok_room: String,
    }

    impl AttachTicketVerifier for CountingVerifier {
        fn verify_ticket(&self, _ticket: &str) -> Result<String, ReferenceTicketError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.ok_room.clone())
        }
    }

    fn policy_v1() -> GuyosWireAttachPolicy {
        GuyosWireAttachPolicy::single_major(
            1,
            5,
            1_048_576,
            65_536,
            Some(30),
        )
    }

    #[test]
    fn negotiation_uses_min_client_and_hub_minor() {
        let policy = policy_v1();
        let v = CountingVerifier {
            calls: AtomicUsize::new(0),
            ok_room: "room-a".into(),
        };
        let attach = Attach {
            protocol_major: 1,
            protocol_minor: 10,
            ticket: "t".into(),
        };
        let ack = accept_attach(&attach, &policy, &v).unwrap();
        assert_eq!(ack.server_protocol_minor, 5);
        assert_eq!(ack.protocol_major, 1);
        assert_eq!(ack.room_id, "room-a");
        assert_eq!(v.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn negotiation_when_client_minor_lower_than_hub() {
        let policy = policy_v1();
        let v = CountingVerifier {
            calls: AtomicUsize::new(0),
            ok_room: "r".into(),
        };
        let attach = Attach {
            protocol_major: 1,
            protocol_minor: 3,
            ticket: "t".into(),
        };
        let ack = accept_attach(&attach, &policy, &v).unwrap();
        assert_eq!(ack.server_protocol_minor, 3);
    }

    #[test]
    fn unsupported_major_after_preamble() {
        let policy = policy_v1();
        let v = CountingVerifier {
            calls: AtomicUsize::new(0),
            ok_room: "r".into(),
        };
        let attach = Attach {
            protocol_major: 2,
            protocol_minor: 0,
            ticket: "t".into(),
        };
        let e = accept_attach(&attach, &policy, &v).unwrap_err();
        assert!(matches!(
            e,
            AttachHandshakeError::ProtocolMajorUnsupported
        ));
        assert_eq!(v.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn ticket_failure_mapping() {
        struct BadTicket;
        impl AttachTicketVerifier for BadTicket {
            fn verify_ticket(&self, _ticket: &str) -> Result<String, ReferenceTicketError> {
                Err(ReferenceTicketError::TicketDecodeFailed)
            }
        }
        let policy = policy_v1();
        let attach = Attach {
            protocol_major: 1,
            protocol_minor: 0,
            ticket: "x".into(),
        };
        let e = accept_attach(&attach, &policy, &BadTicket).unwrap_err();
        let (code, msg) = attach_handshake_portable(e);
        assert_eq!(code, "ticket_decode_failed");
        assert_eq!(msg, error_message_ticket_decode_failed());
    }

    #[test]
    fn preamble_u31_rejection() {
        let policy = policy_v1();
        let v = CountingVerifier {
            calls: AtomicUsize::new(0),
            ok_room: "r".into(),
        };
        let attach = Attach {
            protocol_major: 1,
            protocol_minor: 2_147_483_648_u32,
            ticket: "t".into(),
        };
        assert!(matches!(
            accept_attach(&attach, &policy, &v),
            Err(AttachHandshakeError::InvalidAttach)
        ));
    }
}
