//! Pure **F1** framing helpers: length prefix, incremental decode, UTF-8 validation.
//!
//! No `serde`; JSON semantics live above this layer.

use super::guyos_wire_v1_session::GuyosWireV1SessionError;

/// Maximum **F1** payload size (bytes) per ADR v1 **`max_frame_bytes`** — single literal for docs,
/// defaults, and decoder construction.
pub(crate) const MAX_FRAME_BYTES_ADR_V1: u32 = 1_048_576;

/// Encodes `body` as one **F1** frame: `u32` big-endian length, then payload bytes.
///
/// Returns [`GuyosWireV1SessionError::FrameTooLarge`] if `body` does not fit the session limit or
/// does not fit **`u32`**.
pub(crate) fn encode_application_frame(
    body: &str,
    max_frame_bytes: u32,
) -> Result<Vec<u8>, GuyosWireV1SessionError> {
    let declared = u32::try_from(body.len()).map_err(|_| GuyosWireV1SessionError::FrameTooLarge {
        declared: u32::MAX,
        limit: max_frame_bytes,
    })?;
    if declared > max_frame_bytes {
        return Err(GuyosWireV1SessionError::FrameTooLarge {
            declared,
            limit: max_frame_bytes,
        });
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&declared.to_be_bytes());
    out.extend_from_slice(body.as_bytes());
    Ok(out)
}

/// Incremental **F1** decoder: buffers bytes until one full frame is available.
pub(crate) struct F1FrameDecoder {
    max_frame_bytes: u32,
    buf: Vec<u8>,
}

impl F1FrameDecoder {
    pub(crate) fn new(max_frame_bytes: u32) -> Self {
        Self {
            max_frame_bytes,
            buf: Vec::new(),
        }
    }

    pub(crate) fn feed(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    /// Pops one complete UTF-8 payload, or [`None`] if more bytes are needed.
    ///
    /// [`GuyosWireV1SessionError::FrameTooLarge`] is returned once a length prefix is available
    /// and **`declared > max_frame_bytes`**, **before** reserving space for the body.
    pub(crate) fn pop_complete_frame(
        &mut self,
    ) -> Result<Option<String>, GuyosWireV1SessionError> {
        if self.buf.len() < 4 {
            return Ok(None);
        }
        let declared = u32::from_be_bytes(self.buf[..4].try_into().unwrap());
        if declared > self.max_frame_bytes {
            return Err(GuyosWireV1SessionError::FrameTooLarge {
                declared,
                limit: self.max_frame_bytes,
            });
        }
        let frame_len = 4usize.saturating_add(declared as usize);
        if self.buf.len() < frame_len {
            return Ok(None);
        }
        let payload = self.buf[4..frame_len].to_vec();
        self.buf.drain(..frame_len);
        String::from_utf8(payload).map_err(|_| GuyosWireV1SessionError::InvalidUtf8).map(Some)
    }

    /// Returns true if buffered bytes remain (incomplete frame or not yet consumed).
    pub(crate) fn has_buffered_data(&self) -> bool {
        !self.buf.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_small_json_object_string() {
        let limit = 1024;
        let body = r#"{"type":"ping"}"#.to_string();
        let wire = encode_application_frame(&body, limit).unwrap();

        let mut dec = F1FrameDecoder::new(limit);
        dec.feed(&wire);
        let got = dec.pop_complete_frame().unwrap().unwrap();
        assert_eq!(got, body);
        assert!(!dec.has_buffered_data());
    }

    #[test]
    fn declared_length_over_limit_returns_frame_too_large_before_body() {
        let limit = 8;
        let mut dec = F1FrameDecoder::new(limit);
        let declared = 999_u32;
        dec.feed(&declared.to_be_bytes());
        let err = dec.pop_complete_frame().unwrap_err();
        assert!(matches!(
            err,
            GuyosWireV1SessionError::FrameTooLarge {
                declared: 999,
                limit: 8
            }
        ));
    }

    #[test]
    fn encode_rejects_body_len_over_limit() {
        let limit = 4;
        let body = "hello";
        let err = encode_application_frame(body, limit).unwrap_err();
        assert!(matches!(
            err,
            GuyosWireV1SessionError::FrameTooLarge {
                declared: 5,
                limit: 4
            }
        ));
    }

    #[test]
    fn invalid_utf8_payload_returns_invalid_utf8() {
        let limit = 64;
        let mut dec = F1FrameDecoder::new(limit);
        let declared = 2_u32;
        let mut wire = declared.to_be_bytes().to_vec();
        wire.extend_from_slice(&[0xFF, 0xFE]);
        dec.feed(&wire);
        let err = dec.pop_complete_frame().unwrap_err();
        assert!(matches!(err, GuyosWireV1SessionError::InvalidUtf8));
    }

    #[test]
    fn empty_body_frame_round_trips() {
        let limit = 16;
        let wire = encode_application_frame("", limit).unwrap();
        assert_eq!(wire.len(), 4);
        let mut dec = F1FrameDecoder::new(limit);
        dec.feed(&wire);
        let got = dec.pop_complete_frame().unwrap().unwrap();
        assert!(got.is_empty());
    }
}
