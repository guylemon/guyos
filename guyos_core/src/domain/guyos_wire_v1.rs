//! GuyOS wire v1 application JSON codec (**ADR 0003** — T1, K1, N2, P1, E1, Appendix A).
//!
//! Synchronous encode/decode of UTF-8 JSON bodies only. Compose with
//! [`GuyosWireV1Session`](crate::ports::outbound::GuyosWireV1Session) framing separately.
//!
//! This module is **`pub(crate)`** and not yet referenced from application code; suppress
//! `dead_code` until attach/publish wiring lands.

#![allow(dead_code)]

use serde_json::{Map, Value};
use std::fmt;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

/// Upper bound on UTF-8 **`text`** byte length (**ADR 0003** Limits).
pub const MAX_MESSAGE_BYTES_ADR_V1: usize = 65_536;

/// Upper bound on **F1** JSON body byte length (**ADR 0003** Limits) — matches
/// [`crate::ports::outbound::f1_framing::MAX_FRAME_BYTES_ADR_V1`].
pub const MAX_FRAME_BYTES_ADR_V1: u32 = 1_048_576;

pub use super::guyos_wire_attach::{Attach, AttachAck, U31_MAX};

use super::guyos_wire_attach::{
    accept_attach, attach_from_decoded_map, attach_handshake_portable, validate_attach_preamble,
    AttachTicketVerifier, GuyosWireAttachPolicy,
};
use super::guyos_wire_publish::{PublishDecision, RoomPublishLedger};

// --- Domain payloads (one struct per Appendix A shape) -----------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Publish {
    pub client_message_id: Uuid,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishAck {
    pub seq: u64,
    pub client_message_id: Uuid,
    pub server_timestamp: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireChatMessage {
    pub seq: u64,
    pub client_message_id: Uuid,
    pub text: String,
    pub sender_endpoint_id: Option<String>,
    pub server_timestamp: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detach;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keepalive;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorEnvelope {
    pub code: String,
    pub message: String,
    pub details: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuyosWireV1Message {
    Attach(Attach),
    AttachAck(AttachAck),
    Publish(Publish),
    PublishAck(PublishAck),
    ChatMessage(WireChatMessage),
    Detach(Detach),
    Keepalive(Keepalive),
    Error(ErrorEnvelope),
}

// --- Message-layer errors (portable codes used by this layer; manual Display / Error) ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuyosWireV1MessageError {
    /// `malformed_json`
    MalformedJson,
    /// `unknown_message_type`
    UnknownMessageType {
        /// Raw `type` field when present but not a v1 type; `None` when missing or non-string.
        type_field: Option<String>,
    },
    /// `invalid_attach`
    InvalidAttach,
    /// `message_too_large` — `publish.text` UTF-8 length exceeds negotiated `max_message_bytes`.
    MessageTooLarge { bytes: usize, limit: u32 },
    /// Hub encode invariant: minified JSON body exceeds `max_frame_bytes` (not a portable wire code).
    JsonBodyTooLarge { bytes: usize, limit: u32 },
    /// `invalid_client_message_id`
    InvalidClientMessageId { raw: String },
}

impl fmt::Display for GuyosWireV1MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GuyosWireV1MessageError::MalformedJson => {
                write!(f, "malformed_json: invalid UTF-8 JSON or not a JSON object")
            }
            GuyosWireV1MessageError::UnknownMessageType { type_field } => match type_field {
                Some(t) => write!(f, "unknown_message_type: {t:?}"),
                None => write!(f, "unknown_message_type: missing or invalid `type` field"),
            },
            GuyosWireV1MessageError::InvalidAttach => {
                write!(f, "invalid_attach: attach payload failed semantic validation")
            }
            GuyosWireV1MessageError::MessageTooLarge { bytes, limit } => write!(
                f,
                "message_too_large: text is {bytes} bytes (limit {limit})"
            ),
            GuyosWireV1MessageError::JsonBodyTooLarge { bytes, limit } => write!(
                f,
                "JSON body is {bytes} bytes (max_frame_bytes limit {limit})"
            ),
            GuyosWireV1MessageError::InvalidClientMessageId { raw } => write!(
                f,
                "invalid_client_message_id: {raw:?} is not a canonical UUID string"
            ),
        }
    }
}

impl std::error::Error for GuyosWireV1MessageError {}

// --- Public API ---------------------------------------------------------------------------------

/// Decode a UTF-8 JSON document (**object-first**, **P1** unknown keys ignored for dispatch).
pub fn decode_guyos_wire_v1_json(text: &str) -> Result<GuyosWireV1Message, GuyosWireV1MessageError> {
    let value: Value =
        serde_json::from_str(text).map_err(|_| GuyosWireV1MessageError::MalformedJson)?;
    let map = match value {
        Value::Object(m) => m,
        _ => return Err(GuyosWireV1MessageError::MalformedJson),
    };
    let t = match map.get("type") {
        Some(Value::String(s)) => s.as_str(),
        Some(_) => return Err(GuyosWireV1MessageError::MalformedJson),
        None => {
            return Err(GuyosWireV1MessageError::UnknownMessageType {
                type_field: None,
            })
        }
    };
    match t {
        "attach" => decode_attach(&map),
        "attach_ack" => decode_attach_ack(&map),
        "publish" => decode_publish(&map),
        "publish_ack" => decode_publish_ack(&map),
        "chat_message" => decode_chat_message(&map),
        "detach" => decode_detach(&map),
        "keepalive" => decode_keepalive(&map),
        "error" => decode_error_message(&map),
        other => Err(GuyosWireV1MessageError::UnknownMessageType {
            type_field: Some(other.to_string()),
        }),
    }
}

/// Encode to minified JSON (**K1**, **N2** decimal string `seq`, no insignificant whitespace).
///
/// Uses **ADR v1** default `max_frame_bytes` / `max_message_bytes` caps (see [`MAX_FRAME_BYTES_ADR_V1`],
/// [`MAX_MESSAGE_BYTES_ADR_V1`]). Hubs echoing stricter negotiated limits **must** use
/// [`encode_guyos_wire_v1_json_within_limits`] for outbound frames on that session.
pub fn encode_guyos_wire_v1_json(msg: &GuyosWireV1Message) -> Result<String, GuyosWireV1MessageError> {
    encode_guyos_wire_v1_json_within_limits(
        msg,
        MAX_FRAME_BYTES_ADR_V1,
        MAX_MESSAGE_BYTES_ADR_V1 as u32,
    )
}

/// Encode like [`encode_guyos_wire_v1_json`], enforcing **`max_message_bytes`** on `publish` /
/// `chat_message` **`text`** and ensuring the UTF-8 JSON body fits **`max_frame_bytes`** (hub
/// outbound invariant per **ADR 0003** Limits).
pub fn encode_guyos_wire_v1_json_within_limits(
    msg: &GuyosWireV1Message,
    max_frame_bytes: u32,
    max_message_bytes: u32,
) -> Result<String, GuyosWireV1MessageError> {
    let v = encode_to_value_with_limits(msg, max_message_bytes)?;
    let s = serde_json::to_string(&v).map_err(|_| GuyosWireV1MessageError::MalformedJson)?;
    let limit = max_frame_bytes as usize;
    if s.len() > limit {
        return Err(GuyosWireV1MessageError::JsonBodyTooLarge {
            bytes: s.len(),
            limit: max_frame_bytes,
        });
    }
    Ok(s)
}

// --- Encode -------------------------------------------------------------------------------------

fn encode_to_value_with_limits(
    msg: &GuyosWireV1Message,
    max_message_bytes: u32,
) -> Result<Value, GuyosWireV1MessageError> {
    match msg {
        GuyosWireV1Message::Attach(a) => {
            validate_attach_preamble(a).map_err(|_| GuyosWireV1MessageError::InvalidAttach)?;
            let mut m = Map::new();
            m.insert("type".into(), Value::String("attach".into()));
            m.insert(
                "protocol_major".into(),
                Value::Number(a.protocol_major.into()),
            );
            m.insert(
                "protocol_minor".into(),
                Value::Number(a.protocol_minor.into()),
            );
            m.insert("ticket".into(), Value::String(a.ticket.clone()));
            Ok(Value::Object(m))
        }
        GuyosWireV1Message::AttachAck(a) => {
            validate_u31_generic(a.protocol_major)?;
            validate_u31_generic(a.server_protocol_minor)?;
            let mut m = Map::new();
            m.insert("type".into(), Value::String("attach_ack".into()));
            m.insert("room_id".into(), Value::String(a.room_id.clone()));
            m.insert(
                "max_frame_bytes".into(),
                Value::Number(a.max_frame_bytes.into()),
            );
            m.insert(
                "max_message_bytes".into(),
                Value::Number(a.max_message_bytes.into()),
            );
            m.insert(
                "protocol_major".into(),
                Value::Number(a.protocol_major.into()),
            );
            m.insert(
                "server_protocol_minor".into(),
                Value::Number(a.server_protocol_minor.into()),
            );
            if let Some(k) = a.keepalive_interval_seconds {
                validate_u31_generic(k)?;
                m.insert(
                    "keepalive_interval_seconds".into(),
                    Value::Number(k.into()),
                );
            }
            Ok(Value::Object(m))
        }
        GuyosWireV1Message::Publish(p) => {
            validate_text_len(&p.text, max_message_bytes)?;
            let mut m = Map::new();
            m.insert("type".into(), Value::String("publish".into()));
            m.insert(
                "client_message_id".into(),
                Value::String(uuid_wire_string(p.client_message_id)),
            );
            m.insert("text".into(), Value::String(p.text.clone()));
            Ok(Value::Object(m))
        }
        GuyosWireV1Message::PublishAck(p) => {
            let mut m = Map::new();
            m.insert("type".into(), Value::String("publish_ack".into()));
            m.insert(
                "client_message_id".into(),
                Value::String(uuid_wire_string(p.client_message_id)),
            );
            m.insert("seq".into(), Value::String(seq_to_wire(p.seq)));
            if let Some(ts) = p.server_timestamp {
                m.insert(
                    "server_timestamp".into(),
                    Value::String(format_rfc3339_ts(ts)?),
                );
            }
            Ok(Value::Object(m))
        }
        GuyosWireV1Message::ChatMessage(c) => {
            validate_text_len(&c.text, max_message_bytes)?;
            let mut m = Map::new();
            m.insert("type".into(), Value::String("chat_message".into()));
            m.insert("seq".into(), Value::String(seq_to_wire(c.seq)));
            m.insert(
                "client_message_id".into(),
                Value::String(uuid_wire_string(c.client_message_id)),
            );
            m.insert("text".into(), Value::String(c.text.clone()));
            if let Some(ref sid) = c.sender_endpoint_id {
                m.insert("sender_endpoint_id".into(), Value::String(sid.clone()));
            }
            if let Some(ts) = c.server_timestamp {
                m.insert(
                    "server_timestamp".into(),
                    Value::String(format_rfc3339_ts(ts)?),
                );
            }
            Ok(Value::Object(m))
        }
        GuyosWireV1Message::Detach(_) => {
            let mut m = Map::new();
            m.insert("type".into(), Value::String("detach".into()));
            Ok(Value::Object(m))
        }
        GuyosWireV1Message::Keepalive(_) => {
            let mut m = Map::new();
            m.insert("type".into(), Value::String("keepalive".into()));
            Ok(Value::Object(m))
        }
        GuyosWireV1Message::Error(e) => {
            let mut err_obj = Map::new();
            err_obj.insert("code".into(), Value::String(e.code.clone()));
            err_obj.insert("message".into(), Value::String(e.message.clone()));
            if let Some(ref d) = e.details {
                err_obj.insert("details".into(), Value::Object(d.clone()));
            }
            let mut m = Map::new();
            m.insert("type".into(), Value::String("error".into()));
            m.insert("error".into(), Value::Object(err_obj));
            Ok(Value::Object(m))
        }
    }
}

// --- Decode helpers ------------------------------------------------------------------------------

fn decode_attach(map: &Map<String, Value>) -> Result<GuyosWireV1Message, GuyosWireV1MessageError> {
    attach_from_decoded_map(map)
        .map(GuyosWireV1Message::Attach)
        .map_err(|_| GuyosWireV1MessageError::InvalidAttach)
}

fn decode_attach_ack(map: &Map<String, Value>) -> Result<GuyosWireV1Message, GuyosWireV1MessageError> {
    let room_id = require_string(map, "room_id").map_err(|_| GuyosWireV1MessageError::MalformedJson)?;
    let max_frame_bytes =
        require_u32(map, "max_frame_bytes").map_err(|_| GuyosWireV1MessageError::MalformedJson)?;
    let max_message_bytes =
        require_u32(map, "max_message_bytes").map_err(|_| GuyosWireV1MessageError::MalformedJson)?;
    let protocol_major = require_u31(map, "protocol_major").map_err(|_| {
        GuyosWireV1MessageError::MalformedJson
    })?;
    let server_protocol_minor = require_u31(map, "server_protocol_minor").map_err(|_| {
        GuyosWireV1MessageError::MalformedJson
    })?;
    let keepalive_interval_seconds = match map.get("keepalive_interval_seconds") {
        None => None,
        Some(Value::Null) => None,
        Some(_) => Some(require_u31(map, "keepalive_interval_seconds").map_err(|_| {
            GuyosWireV1MessageError::MalformedJson
        })?),
    };
    Ok(GuyosWireV1Message::AttachAck(AttachAck {
        room_id,
        max_frame_bytes,
        max_message_bytes,
        protocol_major,
        server_protocol_minor,
        keepalive_interval_seconds,
    }))
}

fn decode_publish(map: &Map<String, Value>) -> Result<GuyosWireV1Message, GuyosWireV1MessageError> {
    let raw_id = require_string(map, "client_message_id").map_err(|_| {
        GuyosWireV1MessageError::MalformedJson
    })?;
    let client_message_id = parse_uuid_canonical(&raw_id)?;
    let text = require_string(map, "text").map_err(|_| GuyosWireV1MessageError::MalformedJson)?;
    Ok(GuyosWireV1Message::Publish(Publish {
        client_message_id,
        text,
    }))
}

fn decode_publish_ack(map: &Map<String, Value>) -> Result<GuyosWireV1Message, GuyosWireV1MessageError> {
    let raw_id = require_string(map, "client_message_id").map_err(|_| {
        GuyosWireV1MessageError::MalformedJson
    })?;
    let client_message_id = parse_uuid_canonical(&raw_id)?;
    let seq = require_seq(map, "seq")?;
    let server_timestamp = parse_optional_rfc3339(map, "server_timestamp")?;
    Ok(GuyosWireV1Message::PublishAck(PublishAck {
        seq,
        client_message_id,
        server_timestamp,
    }))
}

fn decode_chat_message(map: &Map<String, Value>) -> Result<GuyosWireV1Message, GuyosWireV1MessageError> {
    let seq = require_seq(map, "seq")?;
    let raw_id = require_string(map, "client_message_id").map_err(|_| {
        GuyosWireV1MessageError::MalformedJson
    })?;
    let client_message_id = parse_uuid_canonical(&raw_id)?;
    let text = require_string(map, "text").map_err(|_| GuyosWireV1MessageError::MalformedJson)?;
    let sender_endpoint_id = match map.get("sender_endpoint_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return Err(GuyosWireV1MessageError::MalformedJson),
    };
    let server_timestamp = parse_optional_rfc3339(map, "server_timestamp")?;
    Ok(GuyosWireV1Message::ChatMessage(WireChatMessage {
        seq,
        client_message_id,
        text,
        sender_endpoint_id,
        server_timestamp,
    }))
}

fn decode_detach(_map: &Map<String, Value>) -> Result<GuyosWireV1Message, GuyosWireV1MessageError> {
    Ok(GuyosWireV1Message::Detach(Detach))
}

fn decode_keepalive(_map: &Map<String, Value>) -> Result<GuyosWireV1Message, GuyosWireV1MessageError> {
    Ok(GuyosWireV1Message::Keepalive(Keepalive))
}

fn decode_error_message(map: &Map<String, Value>) -> Result<GuyosWireV1Message, GuyosWireV1MessageError> {
    let err_val = map.get("error").ok_or(GuyosWireV1MessageError::MalformedJson)?;
    let err_map = match err_val {
        Value::Object(o) => o,
        _ => return Err(GuyosWireV1MessageError::MalformedJson),
    };
    let code = match err_map.get("code") {
        Some(Value::String(s)) => s.clone(),
        _ => return Err(GuyosWireV1MessageError::MalformedJson),
    };
    let message = match err_map.get("message") {
        Some(Value::String(s)) => s.clone(),
        _ => return Err(GuyosWireV1MessageError::MalformedJson),
    };
    let details = match err_map.get("details") {
        None | Some(Value::Null) => None,
        Some(Value::Object(o)) => Some(o.clone()),
        Some(_) => return Err(GuyosWireV1MessageError::MalformedJson),
    };
    Ok(GuyosWireV1Message::Error(ErrorEnvelope {
        code,
        message,
        details,
    }))
}

fn validate_u31_generic(v: u32) -> Result<(), GuyosWireV1MessageError> {
    if v > U31_MAX {
        return Err(GuyosWireV1MessageError::MalformedJson);
    }
    Ok(())
}

fn validate_text_len(text: &str, max_message_bytes: u32) -> Result<(), GuyosWireV1MessageError> {
    let bytes = text.len();
    let limit = max_message_bytes as usize;
    if bytes > limit {
        return Err(GuyosWireV1MessageError::MessageTooLarge {
            bytes,
            limit: max_message_bytes,
        });
    }
    Ok(())
}

fn uuid_wire_string(u: Uuid) -> String {
    u.to_string()
}

fn seq_to_wire(seq: u64) -> String {
    seq.to_string()
}

fn format_rfc3339_ts(ts: OffsetDateTime) -> Result<String, GuyosWireV1MessageError> {
    ts.format(&Rfc3339)
        .map_err(|_| GuyosWireV1MessageError::MalformedJson)
}

fn parse_uuid_canonical(raw: &str) -> Result<Uuid, GuyosWireV1MessageError> {
    let u = Uuid::parse_str(raw).map_err(|_| GuyosWireV1MessageError::InvalidClientMessageId {
        raw: raw.to_string(),
    })?;
    if raw != u.to_string() {
        return Err(GuyosWireV1MessageError::InvalidClientMessageId {
            raw: raw.to_string(),
        });
    }
    Ok(u)
}

fn parse_seq_wire(raw: &str) -> Result<u64, GuyosWireV1MessageError> {
    if raw.is_empty() {
        return Err(GuyosWireV1MessageError::MalformedJson);
    }
    if raw.bytes().any(|b| b.is_ascii_whitespace()) {
        return Err(GuyosWireV1MessageError::MalformedJson);
    }
    if raw == "0" {
        return Ok(0);
    }
    let bytes = raw.as_bytes();
    if bytes[0] < b'1' || bytes[0] > b'9' {
        return Err(GuyosWireV1MessageError::MalformedJson);
    }
    for &b in &bytes[1..] {
        if !b.is_ascii_digit() {
            return Err(GuyosWireV1MessageError::MalformedJson);
        }
    }
    raw.parse::<u64>()
        .map_err(|_| GuyosWireV1MessageError::MalformedJson)
}

fn require_seq(map: &Map<String, Value>, key: &str) -> Result<u64, GuyosWireV1MessageError> {
    match map.get(key) {
        Some(Value::String(s)) => parse_seq_wire(s),
        _ => Err(GuyosWireV1MessageError::MalformedJson),
    }
}

fn require_string(map: &Map<String, Value>, key: &str) -> Result<String, GuyosWireV1MessageError> {
    match map.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        _ => Err(GuyosWireV1MessageError::MalformedJson),
    }
}

fn require_u32(map: &Map<String, Value>, key: &str) -> Result<u32, GuyosWireV1MessageError> {
    match map.get(key) {
        Some(Value::Number(n)) => {
            let v = n.as_u64().ok_or(GuyosWireV1MessageError::MalformedJson)?;
            u32::try_from(v).map_err(|_| GuyosWireV1MessageError::MalformedJson)
        }
        _ => Err(GuyosWireV1MessageError::MalformedJson),
    }
}

fn require_u31(map: &Map<String, Value>, key: &str) -> Result<u32, GuyosWireV1MessageError> {
    let v = require_u32(map, key)?;
    validate_u31_generic(v)?;
    Ok(v)
}

fn parse_optional_rfc3339(
    map: &Map<String, Value>,
    key: &str,
) -> Result<Option<OffsetDateTime>, GuyosWireV1MessageError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => OffsetDateTime::parse(s, &Rfc3339)
            .map(Some)
            .map_err(|_| GuyosWireV1MessageError::MalformedJson),
        Some(_) => Err(GuyosWireV1MessageError::MalformedJson),
    }
}

// --- Attach session dispatch (ADR 0003) ---------------------------------------------------------

fn outbound_wire_error(code: &'static str, message: &'static str) -> GuyosWireV1Message {
    GuyosWireV1Message::Error(ErrorEnvelope {
        code: code.to_string(),
        message: message.to_string(),
        details: None,
    })
}

/// After a full UTF-8 **F1** body is available, map a [`GuyosWireV1MessageError`] to one S→C
/// **`error`** message (**E1** / Appendix A portable `error.code`).
pub fn outbound_error_from_message_error(err: &GuyosWireV1MessageError) -> GuyosWireV1Message {
    match err {
        GuyosWireV1MessageError::MalformedJson => outbound_wire_error(
            "malformed_json",
            "Bytes are not valid UTF-8 or do not parse as a JSON object for this protocol.",
        ),
        GuyosWireV1MessageError::UnknownMessageType { type_field } => {
            let message = match type_field {
                Some(t) => format!("Unknown message type {t:?}."),
                None => "Missing or invalid `type` field.".to_string(),
            };
            GuyosWireV1Message::Error(ErrorEnvelope {
                code: "unknown_message_type".into(),
                message,
                details: None,
            })
        }
        GuyosWireV1MessageError::InvalidAttach => outbound_wire_error(
            "invalid_attach",
            "Attach payload failed semantic validation.",
        ),
        GuyosWireV1MessageError::MessageTooLarge { bytes, limit } => {
            outbound_message_too_large(*bytes, *limit)
        }
        GuyosWireV1MessageError::InvalidClientMessageId { raw } => GuyosWireV1Message::Error(
            ErrorEnvelope {
                code: "invalid_client_message_id".into(),
                message: format!("client_message_id {raw:?} is not a canonical UUID string."),
                details: None,
            },
        ),
        GuyosWireV1MessageError::JsonBodyTooLarge { .. } => outbound_wire_error(
            "malformed_json",
            "Internal encode error: JSON body exceeds max_frame_bytes.",
        ),
    }
}

/// Hub-side attach phase for one application stream (**ADR 0003**).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuyosWireSession {
    AwaitingAttach,
    /// Attach handshake complete; limits echo [`AttachAck`] for this session.
    Attached {
        room_id: String,
        max_frame_bytes: u32,
        max_message_bytes: u32,
    },
}

/// Result of inbound client→hub dispatch for wire v1 (**ADR 0003**).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuyosWireInboundOutcome {
    /// State updated; no outbound application JSON for this turn (cold-start `detach` / `keepalive`).
    NoReply,
    /// Encode and frame this hub→client message (**F1**).
    Reply(GuyosWireV1Message),
    /// Publish accepted or idempotently replayed (**ADR 0003** — publisher vs observers, R3, D2).
    ///
    /// **Publisher vs observers:** the publishing connection receives only [`PublishAck`] via
    /// `publisher_ack`. Other connections in the room receive `chat_message` when `observer_chat`
    /// is `Some`; the publisher must not receive a duplicate `chat_message` for its own line.
    ///
    /// **R3 / D2:** idempotent retry returns the same `seq` without advancing the per-room sequence
    /// or emitting a second observer fan-out (`observer_chat` is `None`). Deduplication is scoped to
    /// `(room_id, client_message_id)`.
    ///
    /// The hub **must** use one [`super::guyos_wire_publish::RoomPublishLedger`] shard per active
    /// `room_id` shared by all streams in that room.
    PublishHandled {
        /// Always sent to the publishing connection only.
        publisher_ack: PublishAck,
        /// Fan-out payload for observers only when `Some`; hub must not send to the publisher.
        observer_chat: Option<WireChatMessage>,
    },
}

fn dispatch_publish_for_room(
    room_id: &str,
    publish: &Publish,
    max_message_bytes: u32,
    ledger: &mut dyn RoomPublishLedger,
) -> GuyosWireInboundOutcome {
    if let Err(GuyosWireV1MessageError::MessageTooLarge { bytes, limit }) =
        validate_text_len(&publish.text, max_message_bytes)
    {
        return GuyosWireInboundOutcome::Reply(outbound_message_too_large(bytes, limit));
    }

    let decision = ledger.accept_or_replay(room_id, &publish.client_message_id);

    let seq = match decision {
        PublishDecision::FirstAcceptance { seq } | PublishDecision::IdempotentReplay { seq } => seq,
    };

    let publisher_ack = PublishAck {
        seq,
        client_message_id: publish.client_message_id,
        server_timestamp: None,
    };

    let observer_chat = match decision {
        PublishDecision::FirstAcceptance { .. } => Some(WireChatMessage {
            seq: publisher_ack.seq,
            client_message_id: publish.client_message_id,
            text: publish.text.clone(),
            sender_endpoint_id: None,
            server_timestamp: None,
        }),
        PublishDecision::IdempotentReplay { .. } => None,
    };

    GuyosWireInboundOutcome::PublishHandled {
        publisher_ack,
        observer_chat,
    }
}

fn outbound_message_too_large(bytes: usize, limit: u32) -> GuyosWireV1Message {
    GuyosWireV1Message::Error(ErrorEnvelope {
        code: "message_too_large".into(),
        message: format!("text is {bytes} bytes (limit {limit})"),
        details: None,
    })
}

/// Route one decoded client message through attach-phase rules (**ADR 0003** Appendix A).
pub fn handle_client_message(
    session: GuyosWireSession,
    msg: &GuyosWireV1Message,
    policy: &GuyosWireAttachPolicy,
    verifier: &dyn AttachTicketVerifier,
    ledger: &mut dyn RoomPublishLedger,
) -> (GuyosWireSession, GuyosWireInboundOutcome) {
    use super::guyos_wire_attach::{
        error_message_attach_required, error_message_invalid_attach_duplicate,
    };

    match session {
        GuyosWireSession::AwaitingAttach => match msg {
            GuyosWireV1Message::Detach(_) | GuyosWireV1Message::Keepalive(_) => {
                (GuyosWireSession::AwaitingAttach, GuyosWireInboundOutcome::NoReply)
            }
            GuyosWireV1Message::Attach(attach) => match accept_attach(attach, policy, verifier) {
                Ok(ack) => {
                    let room_id = ack.room_id.clone();
                    let max_frame_bytes = ack.max_frame_bytes;
                    let max_message_bytes = ack.max_message_bytes;
                    (
                        GuyosWireSession::Attached {
                            room_id,
                            max_frame_bytes,
                            max_message_bytes,
                        },
                        GuyosWireInboundOutcome::Reply(GuyosWireV1Message::AttachAck(ack)),
                    )
                }
                Err(e) => {
                    let (code, message) = attach_handshake_portable(e);
                    (
                        GuyosWireSession::AwaitingAttach,
                        GuyosWireInboundOutcome::Reply(outbound_wire_error(code, message)),
                    )
                }
            },
            _ => (
                GuyosWireSession::AwaitingAttach,
                GuyosWireInboundOutcome::Reply(outbound_wire_error(
                    "attach_required",
                    error_message_attach_required(),
                )),
            ),
        },
        GuyosWireSession::Attached {
            ref room_id,
            max_frame_bytes,
            max_message_bytes,
        } => match msg {
            GuyosWireV1Message::Attach(_) => (
                GuyosWireSession::Attached {
                    room_id: room_id.clone(),
                    max_frame_bytes,
                    max_message_bytes,
                },
                GuyosWireInboundOutcome::Reply(outbound_wire_error(
                    "invalid_attach",
                    error_message_invalid_attach_duplicate(),
                )),
            ),
            GuyosWireV1Message::Publish(publish) => (
                GuyosWireSession::Attached {
                    room_id: room_id.clone(),
                    max_frame_bytes,
                    max_message_bytes,
                },
                dispatch_publish_for_room(room_id, publish, max_message_bytes, ledger),
            ),
            GuyosWireV1Message::Detach(_) | GuyosWireV1Message::Keepalive(_) => (
                GuyosWireSession::Attached {
                    room_id: room_id.clone(),
                    max_frame_bytes,
                    max_message_bytes,
                },
                GuyosWireInboundOutcome::NoReply,
            ),
            _ => (
                GuyosWireSession::Attached {
                    room_id: room_id.clone(),
                    max_frame_bytes,
                    max_message_bytes,
                },
                GuyosWireInboundOutcome::NoReply,
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::guyos_wire_publish::RoomPublishState;
    use crate::ports::outbound::fake_guyos_wire_v1_session::FakeGuyosWireV1Session;
    use crate::ports::outbound::GuyosWireV1Session;

    fn rt(msg: &GuyosWireV1Message) {
        let enc = encode_guyos_wire_v1_json(msg).unwrap();
        assert!(
            !enc.contains('\n') && !enc.contains('\r'),
            "expected minified single-line JSON: {enc}"
        );
        let dec = decode_guyos_wire_v1_json(&enc).unwrap();
        assert_eq!(&dec, msg);
    }

    #[test]
    fn round_trip_attach() {
        rt(&GuyosWireV1Message::Attach(Attach {
            protocol_major: 1,
            protocol_minor: 0,
            ticket: "opaque".into(),
        }));
    }

    #[test]
    fn round_trip_attach_ack_minimal() {
        rt(&GuyosWireV1Message::AttachAck(AttachAck {
            room_id: "room-1".into(),
            max_frame_bytes: 1_048_576,
            max_message_bytes: 65_536,
            protocol_major: 1,
            server_protocol_minor: 0,
            keepalive_interval_seconds: None,
        }));
    }

    #[test]
    fn round_trip_attach_ack_with_keepalive() {
        rt(&GuyosWireV1Message::AttachAck(AttachAck {
            room_id: "room-1".into(),
            max_frame_bytes: 1_048_576,
            max_message_bytes: 65_536,
            protocol_major: 1,
            server_protocol_minor: 0,
            keepalive_interval_seconds: Some(30),
        }));
    }

    #[test]
    fn round_trip_publish_empty_text() {
        rt(&GuyosWireV1Message::Publish(Publish {
            client_message_id: "550e8400-e29b-41d4-a716-446655440000".parse().unwrap(),
            text: String::new(),
        }));
    }

    #[test]
    fn round_trip_publish_ack_with_optional_ts() {
        let ts = OffsetDateTime::parse("2026-05-02T14:32:01.234Z", &Rfc3339).unwrap();
        rt(&GuyosWireV1Message::PublishAck(PublishAck {
            seq: 42,
            client_message_id: "550e8400-e29b-41d4-a716-446655440000".parse().unwrap(),
            server_timestamp: Some(ts),
        }));
    }

    #[test]
    fn round_trip_chat_message_full_optionals() {
        let ts = OffsetDateTime::parse("2026-05-02T14:32:01.234Z", &Rfc3339).unwrap();
        rt(&GuyosWireV1Message::ChatMessage(WireChatMessage {
            seq: 99,
            client_message_id: "550e8400-e29b-41d4-a716-446655440000".parse().unwrap(),
            text: "hello".into(),
            sender_endpoint_id: Some("ep1".into()),
            server_timestamp: Some(ts),
        }));
    }

    #[test]
    fn round_trip_detach_keepalive_error() {
        rt(&GuyosWireV1Message::Detach(Detach));
        rt(&GuyosWireV1Message::Keepalive(Keepalive));

        let mut details = Map::new();
        details.insert("k".into(), Value::Number(1.into()));
        rt(&GuyosWireV1Message::Error(ErrorEnvelope {
            code: "invalid_attach".into(),
            message: "bad".into(),
            details: Some(details),
        }));
    }

    #[test]
    fn round_trip_seq_max_u64() {
        let enc = encode_guyos_wire_v1_json(&GuyosWireV1Message::PublishAck(PublishAck {
            seq: u64::MAX,
            client_message_id: "550e8400-e29b-41d4-a716-446655440000".parse().unwrap(),
            server_timestamp: None,
        }))
        .unwrap();
        assert!(enc.contains("\"18446744073709551615\""));
        match decode_guyos_wire_v1_json(&enc).unwrap() {
            GuyosWireV1Message::PublishAck(p) => assert_eq!(p.seq, u64::MAX),
            _ => panic!("expected publish_ack"),
        }
    }

    #[test]
    fn unknown_top_level_keys_ignored_on_dispatch_shapes() {
        let json = r#"{"type":"publish","client_message_id":"550e8400-e29b-41d4-a716-446655440000","text":"","future_key":true}"#;
        let m = decode_guyos_wire_v1_json(json).unwrap();
        match m {
            GuyosWireV1Message::Publish(p) => {
                assert!(p.text.is_empty());
            }
            _ => panic!("expected publish"),
        }
    }

    #[test]
    fn attach_missing_key_invalid_attach() {
        let json = r#"{"type":"attach","protocol_major":1,"ticket":"x"}"#;
        assert!(matches!(
            decode_guyos_wire_v1_json(json),
            Err(GuyosWireV1MessageError::InvalidAttach)
        ));
    }

    #[test]
    fn publish_wrong_type_malformed() {
        let json = r#"{"type":"publish","client_message_id":"550e8400-e29b-41d4-a716-446655440000","text":[]}"#;
        assert!(matches!(
            decode_guyos_wire_v1_json(json),
            Err(GuyosWireV1MessageError::MalformedJson)
        ));
    }

    #[test]
    fn unknown_message_type_variant() {
        let json = r#"{"type":"nope"}"#;
        assert!(matches!(
            decode_guyos_wire_v1_json(json),
            Err(GuyosWireV1MessageError::UnknownMessageType { .. })
        ));
    }

    #[test]
    fn seq_as_json_number_rejected() {
        let json = r#"{"type":"publish_ack","client_message_id":"550e8400-e29b-41d4-a716-446655440000","seq":1}"#;
        assert!(matches!(
            decode_guyos_wire_v1_json(json),
            Err(GuyosWireV1MessageError::MalformedJson)
        ));
    }

    #[test]
    fn seq_leading_zero_rejected() {
        let json = r#"{"type":"publish_ack","client_message_id":"550e8400-e29b-41d4-a716-446655440000","seq":"01"}"#;
        assert!(matches!(
            decode_guyos_wire_v1_json(json),
            Err(GuyosWireV1MessageError::MalformedJson)
        ));
    }

    #[test]
    fn malformed_json_not_object() {
        assert!(matches!(
            decode_guyos_wire_v1_json("[]"),
            Err(GuyosWireV1MessageError::MalformedJson)
        ));
        assert!(matches!(
            decode_guyos_wire_v1_json("\"hi\""),
            Err(GuyosWireV1MessageError::MalformedJson)
        ));
    }

    #[test]
    fn non_canonical_uuid_rejected() {
        let json = r#"{"type":"publish","client_message_id":"550E8400-E29B-41D4-A716-446655440000","text":""}"#;
        assert!(matches!(
            decode_guyos_wire_v1_json(json),
            Err(GuyosWireV1MessageError::InvalidClientMessageId { .. })
        ));
    }

    #[test]
    fn error_details_must_be_object() {
        let json = r#"{"type":"error","error":{"code":"x","message":"y","details":[]}}"#;
        assert!(matches!(
            decode_guyos_wire_v1_json(json),
            Err(GuyosWireV1MessageError::MalformedJson)
        ));
    }

    #[test]
    fn detach_tolerates_unknown_top_level_keys() {
        let json = r#"{"type":"detach","extra":1}"#;
        assert!(matches!(
            decode_guyos_wire_v1_json(json),
            Ok(GuyosWireV1Message::Detach(_))
        ));
    }

    #[test]
    fn oversized_publish_decodes_then_dispatch_emits_message_too_large() {
        let text = "x".repeat(MAX_MESSAGE_BYTES_ADR_V1 + 1);
        let json = format!(
            r#"{{"type":"publish","client_message_id":"550e8400-e29b-41d4-a716-446655440000","text":{}}}"#,
            serde_json::to_string(&text).unwrap()
        );
        let msg = decode_guyos_wire_v1_json(&json).unwrap();
        let policy = test_attach_policy();
        let v = StubVerifier {
            room_id: "r".into(),
        };
        let attach = GuyosWireV1Message::Attach(Attach {
            protocol_major: 1,
            protocol_minor: 0,
            ticket: "t".into(),
        });
        let mut ledger = RoomPublishState::default();
        let (st, _) = handle_client_message(
            GuyosWireSession::AwaitingAttach,
            &attach,
            &policy,
            &v,
            &mut ledger,
        );
        let (_, out) = handle_client_message(st, &msg, &policy, &v, &mut ledger);
        match out {
            GuyosWireInboundOutcome::Reply(GuyosWireV1Message::Error(e)) => {
                assert_eq!(e.code, "message_too_large");
            }
            _ => panic!("expected message_too_large"),
        }
    }

    #[test]
    fn attach_wrong_field_kind_invalid_attach() {
        let json = r#"{"type":"attach","protocol_major":"1","protocol_minor":0,"ticket":"x"}"#;
        assert!(matches!(
            decode_guyos_wire_v1_json(json),
            Err(GuyosWireV1MessageError::InvalidAttach)
        ));
    }

    #[test]
    fn encode_publish_rejects_oversized_text() {
        let text = "x".repeat(MAX_MESSAGE_BYTES_ADR_V1 + 1);
        let msg = GuyosWireV1Message::Publish(Publish {
            client_message_id: "550e8400-e29b-41d4-a716-446655440000".parse().unwrap(),
            text,
        });
        assert!(matches!(
            encode_guyos_wire_v1_json(&msg),
            Err(GuyosWireV1MessageError::MessageTooLarge { .. })
        ));
    }

    #[tokio::test]
    async fn framed_round_trip_with_fake_session() {
        let (mut a, mut b) = FakeGuyosWireV1Session::paired();
        let msg = GuyosWireV1Message::Publish(Publish {
            client_message_id: "550e8400-e29b-41d4-a716-446655440000".parse().unwrap(),
            text: "hi".into(),
        });
        let body = encode_guyos_wire_v1_json(&msg).unwrap();
        a.write_application_frame(body.clone()).await.unwrap();
        let got = b.read_application_frame().await.unwrap().unwrap();
        let decoded = decode_guyos_wire_v1_json(&got).unwrap();
        assert_eq!(decoded, msg);
        assert_eq!(got, body);
    }

    struct StubVerifier {
        room_id: String,
    }

    impl AttachTicketVerifier for StubVerifier {
        fn verify_ticket(&self, _ticket: &str) -> Result<String, crate::domain::reference_ticket_v1::ReferenceTicketError> {
            Ok(self.room_id.clone())
        }
    }

    fn test_attach_policy() -> GuyosWireAttachPolicy {
        GuyosWireAttachPolicy::single_major(1, 7, 1_048_576, 65_536, None)
    }

    fn test_attach_policy_with_keepalive(seconds: u32) -> GuyosWireAttachPolicy {
        GuyosWireAttachPolicy::single_major(1, 7, 1_048_576, 65_536, Some(seconds))
    }

    #[test]
    fn dispatch_attach_success_yields_attach_ack() {
        let policy = test_attach_policy();
        let v = StubVerifier {
            room_id: "room-x".into(),
        };
        let attach = GuyosWireV1Message::Attach(Attach {
            protocol_major: 1,
            protocol_minor: 10,
            ticket: "opaque".into(),
        });
        let mut ledger = RoomPublishState::default();
        let (st, out) =
            handle_client_message(GuyosWireSession::AwaitingAttach, &attach, &policy, &v, &mut ledger);
        assert!(
            matches!(st, GuyosWireSession::Attached { ref room_id, .. } if room_id == "room-x")
        );
        match out {
            GuyosWireInboundOutcome::Reply(GuyosWireV1Message::AttachAck(a)) => {
                assert_eq!(a.room_id, "room-x");
                assert_eq!(a.server_protocol_minor, 7);
                assert_eq!(a.max_frame_bytes, 1_048_576);
                assert_eq!(a.max_message_bytes, 65_536);
                assert_eq!(a.keepalive_interval_seconds, None);
            }
            _ => panic!("expected attach_ack"),
        }
    }

    #[test]
    fn dispatch_attach_ack_includes_keepalive_when_configured() {
        let policy = test_attach_policy_with_keepalive(45);
        let v = StubVerifier {
            room_id: "room-k".into(),
        };
        let attach = GuyosWireV1Message::Attach(Attach {
            protocol_major: 1,
            protocol_minor: 0,
            ticket: "opaque".into(),
        });
        let mut ledger = RoomPublishState::default();
        let (_, out) =
            handle_client_message(GuyosWireSession::AwaitingAttach, &attach, &policy, &v, &mut ledger);
        match out {
            GuyosWireInboundOutcome::Reply(GuyosWireV1Message::AttachAck(a)) => {
                assert_eq!(a.keepalive_interval_seconds, Some(45));
            }
            _ => panic!("expected attach_ack"),
        }
    }

    #[test]
    fn dispatch_second_attach_after_ack_is_invalid_attach() {
        let policy = test_attach_policy();
        let v = StubVerifier {
            room_id: "r".into(),
        };
        let attach = GuyosWireV1Message::Attach(Attach {
            protocol_major: 1,
            protocol_minor: 0,
            ticket: "t".into(),
        });
        let mut ledger = RoomPublishState::default();
        let (st, _) =
            handle_client_message(GuyosWireSession::AwaitingAttach, &attach, &policy, &v, &mut ledger);
        assert!(
            matches!(st, GuyosWireSession::Attached { ref room_id, .. } if room_id == "r")
        );
        let (st2, out2) = handle_client_message(st, &attach, &policy, &v, &mut ledger);
        assert!(
            matches!(st2, GuyosWireSession::Attached { ref room_id, .. } if room_id == "r")
        );
        match out2 {
            GuyosWireInboundOutcome::Reply(GuyosWireV1Message::Error(e)) => {
                assert_eq!(e.code, "invalid_attach");
            }
            _ => panic!("expected invalid_attach error"),
        }
    }

    #[test]
    fn dispatch_pre_attach_publish_is_attach_required() {
        let policy = test_attach_policy();
        let v = StubVerifier {
            room_id: "r".into(),
        };
        let pub_msg = GuyosWireV1Message::Publish(Publish {
            client_message_id: "550e8400-e29b-41d4-a716-446655440000".parse().unwrap(),
            text: "hi".into(),
        });
        let mut ledger = RoomPublishState::default();
        let (st, out) =
            handle_client_message(GuyosWireSession::AwaitingAttach, &pub_msg, &policy, &v, &mut ledger);
        assert!(matches!(st, GuyosWireSession::AwaitingAttach));
        match out {
            GuyosWireInboundOutcome::Reply(GuyosWireV1Message::Error(e)) => {
                assert_eq!(e.code, "attach_required");
            }
            _ => panic!("expected attach_required"),
        }
    }

    #[test]
    fn dispatch_pre_attach_detach_and_keepalive_are_no_ops() {
        let policy = test_attach_policy();
        let v = StubVerifier {
            room_id: "r".into(),
        };
        let mut ledger = RoomPublishState::default();
        let (st, out) = handle_client_message(
            GuyosWireSession::AwaitingAttach,
            &GuyosWireV1Message::Detach(Detach),
            &policy,
            &v,
            &mut ledger,
        );
        assert!(matches!(st, GuyosWireSession::AwaitingAttach));
        assert!(matches!(out, GuyosWireInboundOutcome::NoReply));
        let (st, out) = handle_client_message(
            GuyosWireSession::AwaitingAttach,
            &GuyosWireV1Message::Keepalive(Keepalive),
            &policy,
            &v,
            &mut ledger,
        );
        assert!(matches!(st, GuyosWireSession::AwaitingAttach));
        assert!(matches!(out, GuyosWireInboundOutcome::NoReply));
    }

    #[test]
    fn dispatch_unsupported_major_after_preamble() {
        let policy = test_attach_policy();
        let v = StubVerifier {
            room_id: "r".into(),
        };
        let attach = GuyosWireV1Message::Attach(Attach {
            protocol_major: 99,
            protocol_minor: 0,
            ticket: "t".into(),
        });
        let mut ledger = RoomPublishState::default();
        let (_, out) =
            handle_client_message(GuyosWireSession::AwaitingAttach, &attach, &policy, &v, &mut ledger);
        match out {
            GuyosWireInboundOutcome::Reply(GuyosWireV1Message::Error(e)) => {
                assert_eq!(e.code, "protocol_major_unsupported");
            }
            _ => panic!("expected protocol_major_unsupported"),
        }
    }

    #[test]
    fn codec_round_trip_then_reference_ticket_accept() {
        use crate::domain::reference_ticket_v1::{decode_reference_v1, encode_reference_v1};

        let k_mac = [9u8; 32];
        let room_bytes = *uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000")
            .unwrap()
            .as_bytes();
        let ticket = encode_reference_v1(&k_mac, &room_bytes, 0);
        let policy = test_attach_policy();

        struct DecodingVerifier {
            k_mac: [u8; 32],
        }

        impl AttachTicketVerifier for DecodingVerifier {
            fn verify_ticket(&self, ticket: &str) -> Result<String, crate::domain::reference_ticket_v1::ReferenceTicketError> {
                decode_reference_v1(ticket, &self.k_mac, 0)
            }
        }

        let v = DecodingVerifier { k_mac };
        let json = encode_guyos_wire_v1_json(&GuyosWireV1Message::Attach(Attach {
            protocol_major: 1,
            protocol_minor: 3,
            ticket,
        }))
        .unwrap();
        let decoded = decode_guyos_wire_v1_json(&json).unwrap();
        let mut ledger = RoomPublishState::default();
        let (_, out) = handle_client_message(
            GuyosWireSession::AwaitingAttach,
            &decoded,
            &policy,
            &v,
            &mut ledger,
        );
        match out {
            GuyosWireInboundOutcome::Reply(GuyosWireV1Message::AttachAck(a)) => {
                assert_eq!(a.room_id, "550e8400-e29b-41d4-a716-446655440000");
                assert_eq!(a.server_protocol_minor, 3);
            }
            _ => panic!("expected attach_ack"),
        }
    }

    #[test]
    fn dispatch_publish_first_acceptance_seq_zero_ack_and_observer_chat() {
        let policy = test_attach_policy();
        let v = StubVerifier {
            room_id: "room-a".into(),
        };
        let attach = GuyosWireV1Message::Attach(Attach {
            protocol_major: 1,
            protocol_minor: 0,
            ticket: "t".into(),
        });
        let mut ledger = RoomPublishState::default();
        let (st, _) = handle_client_message(
            GuyosWireSession::AwaitingAttach,
            &attach,
            &policy,
            &v,
            &mut ledger,
        );
        let id = "550e8400-e29b-41d4-a716-446655440000".parse().unwrap();
        let pub_msg = GuyosWireV1Message::Publish(Publish {
            client_message_id: id,
            text: "hello observers".into(),
        });
        let (_, out) = handle_client_message(st, &pub_msg, &policy, &v, &mut ledger);
        match out {
            GuyosWireInboundOutcome::PublishHandled {
                publisher_ack,
                observer_chat,
            } => {
                assert_eq!(publisher_ack.seq, 0);
                assert_eq!(publisher_ack.client_message_id, id);
                assert!(publisher_ack.server_timestamp.is_none());
                let chat = observer_chat.expect("first fan-out");
                assert_eq!(chat.seq, 0);
                assert_eq!(chat.client_message_id, id);
                assert_eq!(chat.text, "hello observers");
                assert!(chat.sender_endpoint_id.is_none());
                assert!(chat.server_timestamp.is_none());
            }
            _ => panic!("expected PublishHandled"),
        }
    }

    #[test]
    fn dispatch_publish_duplicate_client_id_replays_seq_without_observer_fan_out() {
        let policy = test_attach_policy();
        let v = StubVerifier {
            room_id: "room-dup".into(),
        };
        let attach = GuyosWireV1Message::Attach(Attach {
            protocol_major: 1,
            protocol_minor: 0,
            ticket: "t".into(),
        });
        let mut ledger = RoomPublishState::default();
        let (st, _) = handle_client_message(
            GuyosWireSession::AwaitingAttach,
            &attach,
            &policy,
            &v,
            &mut ledger,
        );
        let id = "550e8400-e29b-41d4-a716-446655440000".parse().unwrap();
        let pub_msg = GuyosWireV1Message::Publish(Publish {
            client_message_id: id,
            text: "first".into(),
        });
        let (st, out1) = handle_client_message(st, &pub_msg, &policy, &v, &mut ledger);
        let seq_first = match out1 {
            GuyosWireInboundOutcome::PublishHandled { publisher_ack, observer_chat } => {
                assert!(observer_chat.is_some());
                publisher_ack.seq
            }
            _ => panic!("expected PublishHandled"),
        };

        let pub_retry = GuyosWireV1Message::Publish(Publish {
            client_message_id: id,
            text: "different text ignored".into(),
        });
        let (_, out2) = handle_client_message(st, &pub_retry, &policy, &v, &mut ledger);
        match out2 {
            GuyosWireInboundOutcome::PublishHandled {
                publisher_ack,
                observer_chat,
            } => {
                assert_eq!(publisher_ack.seq, seq_first);
                assert!(observer_chat.is_none());
            }
            _ => panic!("expected PublishHandled"),
        }
    }

    #[test]
    fn dispatch_publish_sequence_increases_per_room_across_distinct_ids() {
        let policy = test_attach_policy();
        let v = StubVerifier {
            room_id: "room-seq".into(),
        };
        let attach = GuyosWireV1Message::Attach(Attach {
            protocol_major: 1,
            protocol_minor: 0,
            ticket: "t".into(),
        });
        let mut ledger = RoomPublishState::default();
        let (mut st, _) = handle_client_message(
            GuyosWireSession::AwaitingAttach,
            &attach,
            &policy,
            &v,
            &mut ledger,
        );

        let id0 = "550e8400-e29b-41d4-a716-446655440000".parse().unwrap();
        let id1 = "6ba7b810-9dad-11d1-80b4-00c04fd430c8".parse().unwrap();
        let id2 = "6ba7b811-9dad-11d1-80b4-00c04fd430c8".parse().unwrap();

        for (i, id) in [id0, id1, id2].into_iter().enumerate() {
            let msg = GuyosWireV1Message::Publish(Publish {
                client_message_id: id,
                text: format!("m{i}"),
            });
            let (st_next, out) = handle_client_message(st, &msg, &policy, &v, &mut ledger);
            st = st_next;
            match out {
                GuyosWireInboundOutcome::PublishHandled { publisher_ack, .. } => {
                    assert_eq!(publisher_ack.seq, i as u64);
                }
                _ => panic!("expected PublishHandled"),
            }
        }
    }

    #[test]
    fn dispatch_publish_oversized_text_returns_message_too_large_error() {
        let policy = test_attach_policy();
        let v = StubVerifier {
            room_id: "room-big".into(),
        };
        let attach = GuyosWireV1Message::Attach(Attach {
            protocol_major: 1,
            protocol_minor: 0,
            ticket: "t".into(),
        });
        let mut ledger = RoomPublishState::default();
        let (st, _) = handle_client_message(
            GuyosWireSession::AwaitingAttach,
            &attach,
            &policy,
            &v,
            &mut ledger,
        );
        let text = "x".repeat(MAX_MESSAGE_BYTES_ADR_V1 + 1);
        let pub_msg = GuyosWireV1Message::Publish(Publish {
            client_message_id: "550e8400-e29b-41d4-a716-446655440000".parse().unwrap(),
            text,
        });
        let (_, out) = handle_client_message(st, &pub_msg, &policy, &v, &mut ledger);
        match out {
            GuyosWireInboundOutcome::Reply(GuyosWireV1Message::Error(e)) => {
                assert_eq!(e.code, "message_too_large");
            }
            _ => panic!("expected message_too_large error"),
        }
    }

    #[test]
    fn ledger_independent_sequences_per_room_id() {
        let policy = test_attach_policy();
        let mut ledger = RoomPublishState::default();

        let attach_a = GuyosWireV1Message::Attach(Attach {
            protocol_major: 1,
            protocol_minor: 0,
            ticket: "ta".into(),
        });
        let va = StubVerifier {
            room_id: "alpha".into(),
        };
        let (st_a, _) = handle_client_message(
            GuyosWireSession::AwaitingAttach,
            &attach_a,
            &policy,
            &va,
            &mut ledger,
        );

        let attach_b = GuyosWireV1Message::Attach(Attach {
            protocol_major: 1,
            protocol_minor: 0,
            ticket: "tb".into(),
        });
        let vb = StubVerifier {
            room_id: "beta".into(),
        };
        let (st_b, _) = handle_client_message(
            GuyosWireSession::AwaitingAttach,
            &attach_b,
            &policy,
            &vb,
            &mut ledger,
        );

        let id = "550e8400-e29b-41d4-a716-446655440000".parse().unwrap();
        let pub_a = GuyosWireV1Message::Publish(Publish {
            client_message_id: id,
            text: "a1".into(),
        });
        let (_, out_a) = handle_client_message(st_a, &pub_a, &policy, &va, &mut ledger);
        assert!(
            matches!(out_a, GuyosWireInboundOutcome::PublishHandled { ref publisher_ack, .. } if publisher_ack.seq == 0)
        );

        let pub_b = GuyosWireV1Message::Publish(Publish {
            client_message_id: id,
            text: "b1".into(),
        });
        let (_, out_b) = handle_client_message(st_b, &pub_b, &policy, &vb, &mut ledger);
        assert!(
            matches!(out_b, GuyosWireInboundOutcome::PublishHandled { ref publisher_ack, .. } if publisher_ack.seq == 0)
        );
    }

    #[test]
    fn keepalive_tolerates_unknown_top_level_keys() {
        let json = r#"{"type":"keepalive","noise":true}"#;
        assert!(matches!(
            decode_guyos_wire_v1_json(json),
            Ok(GuyosWireV1Message::Keepalive(_))
        ));
    }

    #[test]
    fn dispatch_publish_respects_strict_max_message_bytes_from_ack() {
        let policy = GuyosWireAttachPolicy::single_major(1, 0, 1_048_576, 16, None);
        let v = StubVerifier {
            room_id: "strict".into(),
        };
        let attach = GuyosWireV1Message::Attach(Attach {
            protocol_major: 1,
            protocol_minor: 0,
            ticket: "t".into(),
        });
        let mut ledger = RoomPublishState::default();
        let (st, _) = handle_client_message(
            GuyosWireSession::AwaitingAttach,
            &attach,
            &policy,
            &v,
            &mut ledger,
        );
        let pub_msg = GuyosWireV1Message::Publish(Publish {
            client_message_id: "550e8400-e29b-41d4-a716-446655440000".parse().unwrap(),
            text: "x".repeat(17),
        });
        let (_, out) = handle_client_message(st, &pub_msg, &policy, &v, &mut ledger);
        match out {
            GuyosWireInboundOutcome::Reply(GuyosWireV1Message::Error(e)) => {
                assert_eq!(e.code, "message_too_large");
            }
            _ => panic!("expected message_too_large"),
        }
    }

    #[test]
    fn outbound_error_from_message_error_maps_portable_codes() {
        use GuyosWireV1MessageError as E;
        match outbound_error_from_message_error(&E::MalformedJson) {
            GuyosWireV1Message::Error(e) => assert_eq!(e.code, "malformed_json"),
            _ => panic!("expected error"),
        }
        match outbound_error_from_message_error(&E::UnknownMessageType {
            type_field: Some("nope".into()),
        }) {
            GuyosWireV1Message::Error(e) => assert_eq!(e.code, "unknown_message_type"),
            _ => panic!("expected error"),
        }
        match outbound_error_from_message_error(&E::InvalidAttach) {
            GuyosWireV1Message::Error(e) => assert_eq!(e.code, "invalid_attach"),
            _ => panic!("expected error"),
        }
        match outbound_error_from_message_error(&E::MessageTooLarge {
            bytes: 9,
            limit: 8,
        }) {
            GuyosWireV1Message::Error(e) => assert_eq!(e.code, "message_too_large"),
            _ => panic!("expected error"),
        }
        match outbound_error_from_message_error(&E::InvalidClientMessageId {
            raw: "bad".into(),
        }) {
            GuyosWireV1Message::Error(e) => assert_eq!(e.code, "invalid_client_message_id"),
            _ => panic!("expected error"),
        }
    }

    #[test]
    fn encode_within_limits_rejects_oversized_json_body() {
        let msg = GuyosWireV1Message::Publish(Publish {
            client_message_id: "550e8400-e29b-41d4-a716-446655440000".parse().unwrap(),
            text: "ok".into(),
        });
        assert!(matches!(
            encode_guyos_wire_v1_json_within_limits(&msg, 8, 65_536),
            Err(GuyosWireV1MessageError::JsonBodyTooLarge { .. })
        ));
    }

    #[test]
    fn publish_handled_outbound_encodes_within_session_limits() {
        let policy = test_attach_policy();
        let v = StubVerifier {
            room_id: "room-a".into(),
        };
        let mut ledger = RoomPublishState::default();
        let (st, _) = handle_client_message(
            GuyosWireSession::AwaitingAttach,
            &GuyosWireV1Message::Attach(Attach {
                protocol_major: 1,
                protocol_minor: 0,
                ticket: "t".into(),
            }),
            &policy,
            &v,
            &mut ledger,
        );
        let GuyosWireSession::Attached {
            max_frame_bytes,
            max_message_bytes,
            ..
        } = st
        else {
            panic!("expected attached");
        };
        let id = "550e8400-e29b-41d4-a716-446655440000".parse().unwrap();
        let (_, out) = handle_client_message(
            st,
            &GuyosWireV1Message::Publish(Publish {
                client_message_id: id,
                text: "hello".into(),
            }),
            &policy,
            &v,
            &mut ledger,
        );
        match out {
            GuyosWireInboundOutcome::PublishHandled {
                publisher_ack,
                observer_chat,
            } => {
                let pa = GuyosWireV1Message::PublishAck(publisher_ack);
                encode_guyos_wire_v1_json_within_limits(&pa, max_frame_bytes, max_message_bytes)
                    .unwrap();
                let chat = GuyosWireV1Message::ChatMessage(observer_chat.expect("fan-out"));
                encode_guyos_wire_v1_json_within_limits(&chat, max_frame_bytes, max_message_bytes)
                    .unwrap();
            }
            _ => panic!("expected PublishHandled"),
        }
    }

    #[tokio::test]
    async fn hub_best_effort_error_after_inbound_frame_too_large() {
        use crate::ports::outbound::hub_try_send_wire_error_for_session_read_failure;
        use crate::ports::outbound::GuyosWireV1SessionError;

        let (mut client_wire, mut hub_side) =
            FakeGuyosWireV1Session::paired_with_max_frame_bytes(2048);
        let declared = 2049_u32;
        client_wire
            .send_raw_chunk(declared.to_be_bytes().to_vec())
            .await
            .unwrap();
        let err = hub_side
            .read_application_frame()
            .await
            .expect_err("frame too large");
        assert!(matches!(err, GuyosWireV1SessionError::FrameTooLarge { .. }));
        hub_try_send_wire_error_for_session_read_failure(&mut hub_side, &err).await;
        let body = client_wire.read_application_frame().await.unwrap().unwrap();
        match decode_guyos_wire_v1_json(&body).unwrap() {
            GuyosWireV1Message::Error(e) => assert_eq!(e.code, "frame_too_large"),
            _ => panic!("expected wire error"),
        }
    }

    #[tokio::test]
    async fn hub_best_effort_error_after_inbound_invalid_utf8_payload() {
        use crate::ports::outbound::hub_try_send_wire_error_for_session_read_failure;
        use crate::ports::outbound::GuyosWireV1SessionError;

        let (mut client_wire, mut hub_side) =
            FakeGuyosWireV1Session::paired_with_max_frame_bytes(2048);
        let declared = 2_u32;
        let mut chunk = declared.to_be_bytes().to_vec();
        chunk.extend_from_slice(&[0xFF, 0xFE]);
        client_wire.send_raw_chunk(chunk).await.unwrap();
        let err = hub_side
            .read_application_frame()
            .await
            .expect_err("invalid utf8");
        assert!(matches!(err, GuyosWireV1SessionError::InvalidUtf8));
        hub_try_send_wire_error_for_session_read_failure(&mut hub_side, &err).await;
        let body = client_wire.read_application_frame().await.unwrap().unwrap();
        match decode_guyos_wire_v1_json(&body).unwrap() {
            GuyosWireV1Message::Error(e) => assert_eq!(e.code, "malformed_json"),
            _ => panic!("expected wire error"),
        }
    }

    #[test]
    fn wire_error_body_skipped_for_unexpected_eof() {
        use crate::ports::outbound::wire_error_json_body_for_session_read_failure;
        use crate::ports::outbound::GuyosWireV1SessionError;

        assert!(
            wire_error_json_body_for_session_read_failure(&GuyosWireV1SessionError::UnexpectedEof)
                .is_none()
        );
        let io_err = GuyosWireV1SessionError::Io(std::io::Error::other("closed"));
        assert!(wire_error_json_body_for_session_read_failure(&io_err).is_none());
    }
}
