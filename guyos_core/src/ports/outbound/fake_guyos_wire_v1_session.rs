//! Test double for [`GuyosWireV1Session`](super::GuyosWireV1Session) — paired Tokio duplex.

use std::future::Future;

use tokio::sync::mpsc;

use super::f1_framing::{self, F1FrameDecoder, MAX_FRAME_BYTES_ADR_V1};
use super::{GuyosWireV1Session, GuyosWireV1SessionError};

type Chunk = Vec<u8>;

/// In-memory **F1** session end connected to a peer via bounded Tokio channels.
pub struct FakeGuyosWireV1Session {
    max_frame_bytes: u32,
    decoder: F1FrameDecoder,
    rx: mpsc::Receiver<Chunk>,
    tx: mpsc::Sender<Chunk>,
}

impl FakeGuyosWireV1Session {
    /// Paired ends using [`MAX_FRAME_BYTES_ADR_V1`](super::f1_framing::MAX_FRAME_BYTES_ADR_V1).
    pub fn paired() -> (Self, Self) {
        Self::paired_with_max_frame_bytes(MAX_FRAME_BYTES_ADR_V1)
    }

    /// Paired ends with a custom **per-session** `max_frame_bytes` cap.
    pub fn paired_with_max_frame_bytes(limit: u32) -> (Self, Self) {
        let (tx_a_to_b, rx_b_from_a) = mpsc::channel::<Chunk>(64);
        let (tx_b_to_a, rx_a_from_b) = mpsc::channel::<Chunk>(64);

        let a = Self {
            max_frame_bytes: limit,
            decoder: F1FrameDecoder::new(limit),
            rx: rx_a_from_b,
            tx: tx_a_to_b,
        };
        let b = Self {
            max_frame_bytes: limit,
            decoder: F1FrameDecoder::new(limit),
            rx: rx_b_from_a,
            tx: tx_b_to_a,
        };
        (a, b)
    }

    async fn read_application_frame_inner(
        &mut self,
    ) -> Result<Option<String>, GuyosWireV1SessionError> {
        loop {
            match self.decoder.pop_complete_frame()? {
                Some(frame) => return Ok(Some(frame)),
                None => {}
            }
            match self.rx.recv().await {
                Some(chunk) => self.decoder.feed(&chunk),
                None => {
                    if self.decoder.has_buffered_data() {
                        return Err(GuyosWireV1SessionError::UnexpectedEof);
                    }
                    return Ok(None);
                }
            }
        }
    }

    async fn write_application_frame_inner(
        &mut self,
        body: String,
    ) -> Result<(), GuyosWireV1SessionError> {
        let wire = f1_framing::encode_application_frame(&body, self.max_frame_bytes)?;
        self.tx
            .send(wire)
            .await
            .map_err(|_| GuyosWireV1SessionError::Io(std::io::Error::other("send channel closed")))?;
        Ok(())
    }

    /// Sends **raw** bytes on the wire (bypasses **F1** encode). Test-only hook for framing edge cases.
    #[cfg(test)]
    pub(crate) async fn send_raw_chunk(
        &mut self,
        chunk: Vec<u8>,
    ) -> Result<(), GuyosWireV1SessionError> {
        self.tx
            .send(chunk)
            .await
            .map_err(|_| GuyosWireV1SessionError::Io(std::io::Error::other("send channel closed")))?;
        Ok(())
    }
}

impl GuyosWireV1Session for FakeGuyosWireV1Session {
    fn read_application_frame(
        &mut self,
    ) -> impl Future<Output = Result<Option<String>, GuyosWireV1SessionError>> + Send {
        self.read_application_frame_inner()
    }

    fn write_application_frame(
        &mut self,
        body: String,
    ) -> impl Future<Output = Result<(), GuyosWireV1SessionError>> + Send {
        self.write_application_frame_inner(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn paired_ends_exchange_frames_independently() {
        let (mut alice, mut bob) = FakeGuyosWireV1Session::paired();

        let msg = r#"{"type":"hello"}"#.to_string();
        alice.write_application_frame(msg.clone()).await.unwrap();

        let got = bob.read_application_frame().await.unwrap().unwrap();
        assert_eq!(got, msg);

        let reply = r#"{"type":"world"}"#.to_string();
        bob.write_application_frame(reply.clone()).await.unwrap();
        let got2 = alice.read_application_frame().await.unwrap().unwrap();
        assert_eq!(got2, reply);
    }

    #[tokio::test]
    async fn clean_close_between_frames_returns_none() {
        let (alice, mut bob) = FakeGuyosWireV1Session::paired_with_max_frame_bytes(1024);

        drop(alice);

        let got = bob.read_application_frame().await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn truncated_payload_returns_unexpected_eof() {
        let (mut alice, mut bob) = FakeGuyosWireV1Session::paired_with_max_frame_bytes(1024);

        let declared = 100_u32;
        let mut partial = declared.to_be_bytes().to_vec();
        partial.extend_from_slice(b"short");
        alice.send_raw_chunk(partial).await.unwrap();
        drop(alice);

        let err = bob.read_application_frame().await.unwrap_err();
        assert!(matches!(err, GuyosWireV1SessionError::UnexpectedEof));
    }

    #[tokio::test]
    async fn write_payload_over_limit_returns_frame_too_large() {
        let (mut alice, _bob) = FakeGuyosWireV1Session::paired_with_max_frame_bytes(4);
        let err = alice
            .write_application_frame("abcde".to_string())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            GuyosWireV1SessionError::FrameTooLarge {
                declared: 5,
                limit: 4
            }
        ));
    }
}
