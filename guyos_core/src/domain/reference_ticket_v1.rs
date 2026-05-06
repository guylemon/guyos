//! Reference ticket profile **`guyos.reference_v1`** (ADR 0004).
//!
//! Library-only encode/decode; protocol wiring (`attach.ticket`, …) is out of scope for this module.

use data_encoding::BASE64URL_NOPAD;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use uuid::Uuid;

/// Active profile id string for registry / future wire alignment (ADR 0004 **R2**).
pub const REFERENCE_TICKET_PROFILE_ID: &str = "guyos.reference_v1";

/// Normative body version octet for this profile revision.
pub const VERSION_V1: u8 = 0x01;

/// `ticket_binary = body || signature` — **57** octets (**25** + **32**).
pub const TICKET_BINARY_LEN: usize = 57;

/// UTF-8 `guyos-ticket-v1` immediately followed by **NUL** — **D1** (exactly **16** octets).
pub const PREFIX: &[u8; 16] = b"guyos-ticket-v1\0";

/// Portable **`ticket_decode_failed`** vs **`invalid_ticket`** (ADR 0003 / 0004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceTicketError {
    /// Shape / encoding / early structural checks (**PAD-strict**, length, version).
    TicketDecodeFailed,
    /// MAC mismatch or expiry failure after structural acceptance.
    InvalidTicket,
}

impl ReferenceTicketError {
    /// ADR 0003 portable code string for this failure.
    pub fn portable_code(&self) -> &'static str {
        match self {
            ReferenceTicketError::TicketDecodeFailed => "ticket_decode_failed",
            ReferenceTicketError::InvalidTicket => "invalid_ticket",
        }
    }
}

impl std::fmt::Display for ReferenceTicketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.portable_code())
    }
}

impl std::error::Error for ReferenceTicketError {}

/// Issuer path: **`ticket = BASE64URL_NOPAD(body || HMAC-SHA256(k_mac, PREFIX || body))`** (PAD-strict output).
pub fn encode_reference_v1(k_mac: &[u8; 32], room_id: &[u8; 16], expires_unix: u64) -> String {
    let mut body = [0u8; 25];
    body[0] = VERSION_V1;
    body[1..17].copy_from_slice(room_id);
    body[17..25].copy_from_slice(&expires_unix.to_be_bytes());

    let signature = hmac_sha256(k_mac, &body);

    let mut ticket_binary = [0u8; TICKET_BINARY_LEN];
    ticket_binary[..25].copy_from_slice(&body);
    ticket_binary[25..].copy_from_slice(&signature);

    BASE64URL_NOPAD.encode(&ticket_binary)
}

/// Hub path: ordered checklist (ADR 0004); success returns canonical **`room_id`** (**C3**).
pub fn decode_reference_v1(
    ticket: &str,
    k_mac: &[u8; 32],
    now_unix: u64,
) -> Result<String, ReferenceTicketError> {
    pad_strict_scan(ticket)?;

    let decoded = BASE64URL_NOPAD
        .decode(ticket.as_bytes())
        .map_err(|_| ReferenceTicketError::TicketDecodeFailed)?;

    if decoded.len() != TICKET_BINARY_LEN {
        return Err(ReferenceTicketError::TicketDecodeFailed);
    }

    let ticket_binary: &[u8; TICKET_BINARY_LEN] =
        decoded.as_slice().try_into().expect("length checked");

    if ticket_binary[0] != VERSION_V1 {
        return Err(ReferenceTicketError::TicketDecodeFailed);
    }

    let body: &[u8; 25] = ticket_binary[..25].try_into().expect("split body");
    let signature: &[u8; 32] = ticket_binary[25..].try_into().expect("split sig");

    let expected_mac = hmac_sha256(k_mac, body);
    if !bool::from(signature.ct_eq(&expected_mac)) {
        return Err(ReferenceTicketError::InvalidTicket);
    }

    let expires_unix = u64::from_be_bytes(body[17..25].try_into().expect("expires"));
    if expires_unix != 0 && now_unix > expires_unix {
        return Err(ReferenceTicketError::InvalidTicket);
    }

    let room_bytes: [u8; 16] = body[1..17].try_into().expect("room id");
    Ok(Uuid::from_bytes(room_bytes).to_string())
}

fn pad_strict_scan(ticket: &str) -> Result<(), ReferenceTicketError> {
    for &b in ticket.as_bytes() {
        if !is_base64url_byte(b) {
            return Err(ReferenceTicketError::TicketDecodeFailed);
        }
    }
    Ok(())
}

#[inline]
fn is_base64url_byte(b: u8) -> bool {
    matches!(
        b,
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'
    )
}

type HmacSha256 = Hmac<Sha256>;

fn hmac_sha256(k_mac: &[u8; 32], body: &[u8; 25]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(k_mac).expect("HMAC key length is 32 octets");
    mac.update(PREFIX.as_slice());
    mac.update(body.as_slice());
    mac.finalize().into_bytes().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_is_16_octets() {
        assert_eq!(PREFIX.len(), 16);
        assert_eq!(&PREFIX[..15], b"guyos-ticket-v1");
        assert_eq!(PREFIX[15], 0);
    }
}
