use anyhow::{Context, Result};
use bytes::BytesMut;
use futures_lite::StreamExt;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::LlmRelayConfig;

#[derive(Clone, Debug)]
pub(crate) struct HistoryMessage {
    pub(crate) from: String,
    pub(crate) text: String,
}

#[derive(Clone)]
pub(crate) struct OpenAiCompatClient {
    http: reqwest::Client,
}

impl OpenAiCompatClient {
    pub(crate) fn new() -> Self {
        // Default client is fine for localhost; keep it simple.
        Self {
            http: reqwest::Client::new(),
        }
    }

    pub(crate) async fn stream_chat_completions(
        &self,
        base_url: &str,
        req: ChatCompletionsRequest,
    ) -> Result<impl futures_lite::Stream<Item = Result<ChatCompletionsStreamEvent>> + Unpin> {
        let mut url = Url::parse(base_url).context("invalid --llm-base-url")?;
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("invalid --llm-base-url path"))?
            .pop_if_empty()
            .extend(["chat", "completions"]);

        let resp = self
            .http
            .post(url)
            .json(&req)
            .send()
            .await
            .context("failed to call OpenAI-compatible API")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "llm http error {}: {}",
                status.as_u16(),
                body
            ));
        }

        let byte_stream = resp.bytes_stream();
        let sse = SseStream::new(byte_stream);
        Ok(sse)
    }

    pub(crate) async fn chat_completions(
        &self,
        base_url: &str,
        req: ChatCompletionsRequest,
    ) -> Result<String> {
        let mut url = Url::parse(base_url).context("invalid --llm-base-url")?;
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("invalid --llm-base-url path"))?
            .pop_if_empty()
            .extend(["chat", "completions"]);

        let resp = self
            .http
            .post(url)
            .json(&req)
            .send()
            .await
            .context("failed to call OpenAI-compatible API")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "llm http error {}: {}",
                status.as_u16(),
                body
            ));
        }

        let parsed: ChatCompletionsResponse = resp
            .json()
            .await
            .context("failed to parse chat completions response")?;
        let content = parsed
            .choices
            .get(0)
            .map(|c| c.message.content.clone())
            .unwrap_or_default();
        Ok(content)
    }
}

pub(crate) async fn run_streaming_reply(
    client: &OpenAiCompatClient,
    cfg: &LlmRelayConfig,
    history: Vec<HistoryMessage>,
    chat: guyos_core::Chat,
) -> Result<()> {
    let mut messages: Vec<OpenAiMessage> = Vec::new();
    if let Some(system) = &cfg.system_prompt {
        messages.push(OpenAiMessage {
            role: OpenAiRole::System,
            content: system.clone(),
        });
    }

    for m in history {
        let role = if m.from == cfg.assistant_name {
            OpenAiRole::Assistant
        } else {
            OpenAiRole::User
        };
        messages.push(OpenAiMessage {
            role,
            content: m.text,
        });
    }

    if !cfg.stream {
        let req = ChatCompletionsRequest {
            model: cfg.model.clone(),
            messages,
            stream: false,
        };
        let text = client.chat_completions(&cfg.base_url, req).await?;
        if !text.is_empty() {
            let _ = chat.send(text).await;
        }
        return Ok(());
    }

    let req = ChatCompletionsRequest {
        model: cfg.model.clone(),
        messages,
        stream: true,
    };

    let mut stream = client
        .stream_chat_completions(&cfg.base_url, req)
        .await?;

    // Throttle partial updates.
    let mut acc = String::new();
    let mut last_send = Instant::now().checked_sub(cfg.stream_interval).unwrap_or_else(Instant::now);
    let mut sent_any_partial = false;

    while let Some(ev) = stream.next().await {
        let ev = ev?;
        match ev {
            ChatCompletionsStreamEvent::Done => break,
            ChatCompletionsStreamEvent::DeltaText(delta) => {
                acc.push_str(&delta);

                let should_send = acc.len() >= cfg.stream_chunk_min_chars
                    && last_send.elapsed() >= cfg.stream_interval;
                if should_send {
                    let partial = std::mem::take(&mut acc);
                    last_send = Instant::now();
                    sent_any_partial = true;
                    let text = format!("(partial) {partial}");
                    let _ = chat.send(text).await;
                }
            }
        }
    }

    // Flush remainder.
    if !acc.is_empty() {
        if sent_any_partial {
            let text = format!("(partial) {acc}");
            let _ = chat.send(text).await;
        } else {
            // If we never sent partials, just send the final as a single message.
            let _ = chat.send(acc).await;
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// OpenAI-compatible request/response types

#[derive(Debug, Serialize)]
pub(crate) struct ChatCompletionsRequest {
    pub(crate) model: String,
    pub(crate) messages: Vec<OpenAiMessage>,
    pub(crate) stream: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct OpenAiMessage {
    pub(crate) role: OpenAiRole,
    pub(crate) content: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OpenAiRole {
    System,
    User,
    Assistant,
}

#[derive(Debug)]
pub(crate) enum ChatCompletionsStreamEvent {
    DeltaText(String),
    Done,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionsResponse {
    choices: Vec<ChatCompletionsResponseChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionsResponseChoice {
    message: ChatCompletionsResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionsResponseMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionsChunk {
    choices: Vec<ChatCompletionsChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionsChoice {
    delta: ChatCompletionsDelta,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionsDelta {
    content: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Minimal SSE parser for OpenAI streaming responses.

struct SseStream<S> {
    inner: S,
    buf: BytesMut,
    done: bool,
}

impl<S> SseStream<S> {
    fn new(inner: S) -> Self {
        Self {
            inner,
            buf: BytesMut::new(),
            done: false,
        }
    }
}

impl<S> futures_lite::Stream for SseStream<S>
where
    S: futures_lite::Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Unpin,
{
    type Item = Result<ChatCompletionsStreamEvent>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        if self.done {
            return std::task::Poll::Ready(None);
        }

        // Try to parse a complete SSE event from existing buffer.
        if let Some(ev) = try_parse_one_sse_event(&mut self.buf) {
            return std::task::Poll::Ready(Some(ev));
        }

        // Otherwise, read more bytes.
        match std::pin::Pin::new(&mut self.inner).poll_next(cx) {
            std::task::Poll::Ready(Some(Ok(chunk))) => {
                self.buf.extend_from_slice(&chunk);
                if let Some(ev) = try_parse_one_sse_event(&mut self.buf) {
                    std::task::Poll::Ready(Some(ev))
                } else {
                    std::task::Poll::Pending
                }
            }
            std::task::Poll::Ready(Some(Err(e))) => {
                self.done = true;
                std::task::Poll::Ready(Some(Err(e.into())))
            }
            std::task::Poll::Ready(None) => {
                self.done = true;
                std::task::Poll::Ready(None)
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

fn try_parse_one_sse_event(buf: &mut BytesMut) -> Option<Result<ChatCompletionsStreamEvent>> {
    // SSE events are separated by a blank line. We split on \n\n and also tolerate \r\n\r\n.
    let Some(idx) = find_event_delimiter(buf) else {
        return None;
    };

    let event = buf.split_to(idx);
    // Drain delimiter too.
    let delim_len = if buf.starts_with(b"\r\n\r\n") { 4 } else { 2 };
    buf.advance(delim_len);

    // Parse lines like: "data: ...."
    let text = String::from_utf8_lossy(&event).to_string();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        let Some(rest) = line.strip_prefix("data:") else {
            continue;
        };
        let data = rest.trim();
        if data == "[DONE]" {
            return Some(Ok(ChatCompletionsStreamEvent::Done));
        }

        let chunk: ChatCompletionsChunk = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(e) => return Some(Err(e.into())),
        };
        let delta = chunk
            .choices
            .get(0)
            .and_then(|c| c.delta.content.clone());
        if let Some(content) = delta {
            return Some(Ok(ChatCompletionsStreamEvent::DeltaText(content)));
        }
    }

    // No data line with content; ignore and parse next.
    Some(Ok(ChatCompletionsStreamEvent::DeltaText(String::new())))
}

fn find_event_delimiter(buf: &[u8]) -> Option<usize> {
    // Find first occurrence of "\n\n" or "\r\n\r\n". Return start index.
    // We prefer CRLF if present.
    let mut i = 0usize;
    while i + 1 < buf.len() {
        if i + 3 < buf.len() && &buf[i..i + 4] == b"\r\n\r\n" {
            return Some(i);
        }
        if &buf[i..i + 2] == b"\n\n" {
            return Some(i);
        }
        i += 1;
    }
    None
}

// BytesMut::advance is in Buf trait.
use bytes::Buf;

// ─────────────────────────────────────────────────────────────────────────────
// Tests

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all_events(mut bytes: BytesMut) -> Vec<ChatCompletionsStreamEvent> {
        let mut out = vec![];
        while let Some(ev) = try_parse_one_sse_event(&mut bytes) {
            match ev {
                Ok(ChatCompletionsStreamEvent::DeltaText(s)) if s.is_empty() => {}
                Ok(e) => out.push(e),
                Err(_) => break,
            }
        }
        out
    }

    #[test]
    fn sse_parses_delta_and_done() {
        let payload = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let bytes = BytesMut::from(payload.as_bytes());
        let events = parse_all_events(bytes);
        assert!(matches!(
            events.as_slice(),
            [
                ChatCompletionsStreamEvent::DeltaText(a),
                ChatCompletionsStreamEvent::DeltaText(b),
                ChatCompletionsStreamEvent::Done
            ] if a == "Hel" && b == "lo"
        ));
    }

    #[test]
    fn sse_tolerates_crlf_delimiters() {
        let payload = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\r\n\r\n",
            "data: [DONE]\r\n\r\n"
        );
        let bytes = BytesMut::from(payload.as_bytes());
        let events = parse_all_events(bytes);
        assert!(matches!(
            events.as_slice(),
            [ChatCompletionsStreamEvent::DeltaText(a), ChatCompletionsStreamEvent::Done] if a == "Hi"
        ));
    }

    #[test]
    fn non_stream_response_parses_first_choice_message_content() {
        let payload = r#"{
          "choices": [
            { "message": { "content": "Hello!" } }
          ]
        }"#;
        let parsed: ChatCompletionsResponse = serde_json::from_str(payload).unwrap();
        assert_eq!(parsed.choices[0].message.content, "Hello!");
    }
}

