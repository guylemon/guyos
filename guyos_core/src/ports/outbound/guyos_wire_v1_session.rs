//! Outbound port for a single GuyOS wire v1 **application** session (**F1** framing).
//!
//! This models **one** client-initiated bidirectional stream **after** QUIC + TLS are ready.
//! Concrete transport adapters enforce **Q1** (single stream, ALPN `guyos-wire-v1`).
//!
//! ## Single-stream invariant (Q1)
//!
//! This type represents **one** bidirectional application stream. Opening additional streams
//! for the same protocol instance is **invalid** — enforcement belongs in **concrete adapters**,
//! not in this trait.
//!
//! ## ALPN
//!
//! Callers construct implementations only after **`guyos-wire-v1`** has been negotiated.
//! This trait does **not** expose ALPN state.
//!
//! ## 0-RTT / early data
//!
//! Adapters **must not** send **F1** or other application payloads defined for this protocol
//! through TLS **0-RTT** / QUIC **early data**. Session types built on this port assume replay-safe
//! transport usage.
//!
//! ## Framing
//!
//! All reads and writes are **F1**-delimited: `u32` big-endian payload length, then UTF-8 bytes.
//! Implementations enforce the session **`max_frame_bytes`** cap on the declared length **before**
//! allocating or interpreting the body (see [`GuyosWireV1SessionError::FrameTooLarge`]).
//!
//! ## Teardown
//!
//! There is **no** explicit half-close API; teardown is **implicit** via [`Drop`] on concrete types.

use std::fmt;
use std::future::Future;

/// ADR-aligned errors for wire v1 session I/O and **F1** framing at this port boundary.
///
/// Future QUIC adapters map these into [`crate::ChatError`] or other façade types.
#[derive(Debug)]
pub enum GuyosWireV1SessionError {
    /// Declared payload length (bytes) exceeds the session **max_frame_bytes** cap (ADR-aligned;
    /// see [`crate::ports::outbound::f1_framing::MAX_FRAME_BYTES_ADR_V1`] for the normative v1 default).
    ///
    /// Used for **inbound** oversize length prefixes and **outbound** writes whose UTF-8 body is
    /// too large. **`declared`** is always the payload byte length as a **`u32`**; **`body.len()`**
    /// must fit **`u32`** for F1 encoding under practical ADR limits.
    FrameTooLarge { declared: u32, limit: u32 },
    /// Stream ended **mid-frame** (partial prefix or body), or an equivalent truncation.
    UnexpectedEof,
    /// Declared payload bytes are not valid UTF-8.
    InvalidUtf8,
    /// Transport or glue I/O failure (e.g. closed channel in test doubles).
    Io(std::io::Error),
}

impl fmt::Display for GuyosWireV1SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameTooLarge { declared, limit } => write!(
                f,
                "F1 frame payload length {declared} exceeds session limit {limit}"
            ),
            Self::UnexpectedEof => write!(
                f,
                "unexpected end of stream while reading an F1 frame"
            ),
            Self::InvalidUtf8 => write!(f, "F1 frame payload is not valid UTF-8"),
            Self::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for GuyosWireV1SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

/// Optional minified JSON **`error`** body for hub teardown after inbound **F1** read failures that map
/// to Appendix A portable **`error.code`** values (**ADR 0003** — `frame_too_large`, `malformed_json`).
///
/// Returns [`None`] for [`GuyosWireV1SessionError::UnexpectedEof`] and [`GuyosWireV1SessionError::Io`],
/// where the hub **must not** emit a normative wire `error` solely from that condition.
pub(crate) fn wire_error_json_body_for_session_read_failure(
    err: &GuyosWireV1SessionError,
) -> Option<String> {
    use crate::domain::guyos_wire_v1::{encode_guyos_wire_v1_json, ErrorEnvelope, GuyosWireV1Message};

    let envelope = match err {
        GuyosWireV1SessionError::FrameTooLarge { declared, limit } => GuyosWireV1Message::Error(
            ErrorEnvelope {
                code: "frame_too_large".into(),
                message: format!(
                    "Declared F1 payload length {declared} exceeds max_frame_bytes limit {limit}."
                ),
                details: None,
            },
        ),
        GuyosWireV1SessionError::InvalidUtf8 => GuyosWireV1Message::Error(ErrorEnvelope {
            code: "malformed_json".into(),
            message: "F1 frame payload is not valid UTF-8.".into(),
            details: None,
        }),
        GuyosWireV1SessionError::UnexpectedEof | GuyosWireV1SessionError::Io(_) => return None,
    };
    encode_guyos_wire_v1_json(&envelope).ok()
}

/// Best-effort: write one S→C **`error`** frame after an inbound framing failure, then the caller
/// tears down the stream (**ADR 0003**). Write failures are ignored (fallback to close-only).
pub(crate) async fn hub_try_send_wire_error_for_session_read_failure<S: GuyosWireV1Session + ?Sized>(
    session: &mut S,
    err: &GuyosWireV1SessionError,
) {
    let Some(body) = wire_error_json_body_for_session_read_failure(err) else {
        return;
    };
    let _ = session.write_application_frame(body).await;
}

/// One bidirectional **F1**-framed application session (wire v1).
///
/// Futures are [`Send`] so callers may use them from spawned tasks.
pub trait GuyosWireV1Session: Send + Sync {
    /// Reads the next complete application frame body (UTF-8 JSON text as [`String`]).
    ///
    /// Returns [`Ok(None)`] when the peer finished the stream **between** frames (graceful close).
    fn read_application_frame(
        &mut self,
    ) -> impl Future<Output = Result<Option<String>, GuyosWireV1SessionError>> + Send;

    /// Writes one **F1** frame whose payload is `body` (any UTF-8, including empty; JSON object
    /// rules are enforced above this port).
    fn write_application_frame(
        &mut self,
        body: String,
    ) -> impl Future<Output = Result<(), GuyosWireV1SessionError>> + Send;
}
