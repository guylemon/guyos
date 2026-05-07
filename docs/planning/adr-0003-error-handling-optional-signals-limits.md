# Planning: Error handling, optional signals, and limits — ADR 0003 (E1, DT1, KA1, Limits, Appendix A)

## Document status

**Grill session complete** (Q1–Q8). Use this doc with the ADR and sibling planning files; amend if the ADR or crate layout changes.

## Task scope (authoritative brief)

**Error handling, optional signals, and limits (ADR 0003, E1, DT1, KA1, Limits, Appendix A)** — Map all failure conditions to the closed error-code set and implement graceful detach and optional keepalive as fire-and-forget domain signals. Enforce maximum frame and message sizes at the framing boundary and include the active limits in attach acknowledgements.

## References

- **Normative spec:** [`docs/adr/0003-wire-protocol-and-compatibility.md`](../adr/0003-wire-protocol-and-compatibility.md) — **E1** (`error` object), **DT1** (`detach`), **KA1** (`keepalive` + optional `attach_ack.keepalive_interval_seconds`), **Limits** (`max_frame_bytes`, `max_message_bytes`), **Appendix A** (shapes + closed `error.code` table).
- **Framing port (F1, internal errors):** [`docs/planning/adr-0003-guyos-wire-v1-framed-session-port.md`](adr-0003-guyos-wire-v1-framed-session-port.md) — `GuyosWireV1SessionError::FrameTooLarge` today is **not** a JSON `error` frame; this task bridges portable **`frame_too_large`** (Appendix A) to observable wire behavior where applicable.
- **Message layer (serde shapes, `GuyosWireV1MessageError`):** [`docs/planning/adr-0003-guyos-wire-framing-serde-shape-enforcement.md`](adr-0003-guyos-wire-framing-serde-shape-enforcement.md) — message-layer errors vs session errors split.
- **Attach / attach_ack construction:** [`docs/planning/adr-0003-guyos-wire-attach-negotiation-rules.md`](adr-0003-guyos-wire-attach-negotiation-rules.md) — echo **`max_frame_bytes`** / **`max_message_bytes`** on **`attach_ack`** (aligned with Limits section).

## Code anchors (starting point)

| Concern | Location |
| --- | --- |
| F1 length cap constant | `guyos_core/src/ports/outbound/f1_framing.rs` — `MAX_FRAME_BYTES_ADR_V1` |
| Session boundary errors | `guyos_core/src/ports/outbound/guyos_wire_v1_session.rs` — `GuyosWireV1SessionError` |
| Typed messages + message-layer errors | `guyos_core/src/domain/guyos_wire_v1.rs` |

## Resolved design decisions (grill session)

| Topic | Decision |
| --- | --- |
| Inbound **`FrameTooLarge`** → wire **`error`** | On **`GuyosWireV1SessionError::FrameTooLarge`** during inbound F1 decode, the hub **should send one** S→C **`error`** JSON message with **`error.code` = `frame_too_large`** (E1 / Appendix A), then tear down. If writing that **`error`** fails (I/O), **fall back** to closing the stream without treating that as a protocol ambiguity about oversize frames. |
| Inbound **`InvalidUtf8`** → wire **`error`** | On **`GuyosWireV1SessionError::InvalidUtf8`** (declared F1 payload is not valid UTF-8), the hub **should send one** S→C **`error`** with **`error.code` = `malformed_json`**, then tear down. If the write fails, **fall back** to close (same pattern as **`frame_too_large`**). |
| Inbound **`UnexpectedEof`** (mid-frame) | **No** normative S→C **`error`** frame — Appendix A has **no** closed code for truncated streams mid-frame; **`malformed_json`** applies to UTF-8 / JSON-object parse failures on a **complete** frame payload. Treat as **transport-level** teardown; clients distinguish **`Ok(None)`** (clean between frames) from read **`Err`**. |
| **`attach_ack` limit fields** | **`max_frame_bytes`** and **`max_message_bytes`** echo **the caps this hub enforces for the session** (aligned with the F1 session limit and **`publish.text`** validation). For a hub that implements only ADR v1 maxima, those are **`1_048_576`** and **`65_536`**. Stricter deployment policy → **echo the stricter values**; do not advertise ADR maxima while enforcing lower limits. |
| **`message_too_large` enforcement** | After JSON parses and **`type`** is **`publish`**, measure **`text`** UTF-8 byte length vs **`max_message_bytes`** from negotiated **`attach_ack`** — **single authoritative check** for this code. Do **not** map raw frame / body size to **`message_too_large`** (**`frame_too_large`** covers oversized F1 payloads; Appendix A ties **`message_too_large`** to **`publish.text`** only). |
| Hub outbound **`publish_ack`** / **`chat_message`** | Before **`write_application_frame`**, ensure the UTF-8 JSON body **`≤ max_frame_bytes`**. For **`chat_message`**, also **`text`** UTF-8 byte length **`≤ max_message_bytes`** — same caps as **`attach_ack`**. Do **not** emit frames that violate limits advertised to the client; implement via encode-time checks / tests (treat violations as bugs). |
| **`attach_ack.keepalive_interval_seconds`** (KA1) | **Omit by default** — include **only** when deployment/product configures a hint. Clients **must not** infer keepalives are required when absent (**ADR**). Tests should cover **both** omission and a finite hint when exercising **`keepalive`** behavior. |
| **`detach`** / **`keepalive`** and P1 | **Ignore unknown top-level keys** on **`detach`** and **`keepalive`** like all other v1 shapes — extras do **not** turn a well-typed signal into an **`error`**; required key remains **`type`** only (**Appendix A**). |

## Normative reminders (non-duplicative)

- **Closed `error.code` values** — Authoritative list is **Appendix A** table in the ADR; do not fork a second code table in this doc except as a **verification pointer**.
- **`detach` / `keepalive` before `attach_ack`** — ADR: **no-op**, **must not** emit **`error`** solely for that reason (**DT1**, **KA1**).
- **`message_too_large`** — Appendix A ties this to **`publish`** **`text`** UTF-8 length **`>`** **`max_message_bytes`**; enforce at the message-validation boundary consistent with the codec task.

## Implementation checklist

1. **Portable code coverage** — Ensure every failure path that crosses the wire as **`error`** maps to exactly one Appendix A **`error.code`** (including **`frame_too_large`** vs **`message_too_large`** vs message-layer codes).
2. **Framing boundary** — Enforce **`max_frame_bytes`** on declared F1 length (port); on inbound **`FrameTooLarge`** / **`InvalidUtf8`**, attempt S→C **`error`** with **`frame_too_large`** / **`malformed_json`** per resolved decisions. **`UnexpectedEof`** mid-frame: teardown without wire **`error`**.
3. **Message size** — Enforce **`max_message_bytes`** on inbound **`publish`** **`text`** via **`text`** byte length (single check — **`message_too_large` enforcement**). Hub outbound **`publish_ack`** / **`chat_message`** must satisfy **`attach_ack`** limits (**Hub outbound** row).
4. **`attach_ack`** — Include **`max_frame_bytes`** and **`max_message_bytes`** equal to **this hub’s enforced caps** for the session (ADR v1 maxima when no stricter policy — see resolved decision **`attach_ack` limit fields**). **`keepalive_interval_seconds`** — see **`attach_ack.keepalive_interval_seconds`** row.
5. **DT1 / KA1** — **`detach`** / **`keepalive`**: fire-and-forget, **no** S→C reply; **no-op** before join except as already specified in the ADR; **ignore** unknown top-level keys (**`detach` / `keepalive` and P1**). **`keepalive_interval_seconds`** on **`attach_ack`**: see resolved row.
6. **Verification** — Walk Appendix A rows relevant to E1, DT1, KA1, Limits, and framing vs application errors against tests or fixture-backed checks.

## Application-layer error mapping (message decode)

- **`GuyosWireV1MessageError`** variants already align **one-to-one** with portable **`error.code`** values at the message layer ([framing-serde planning doc](adr-0003-guyos-wire-framing-serde-shape-enforcement.md)).
- The hub protocol handler **`encode`s** an **`error`** JSON message from **`GuyosWireV1MessageError`** (or equivalent mapping) when rejecting client behavior **after** a full UTF-8 frame is available — preserve **`GuyosWireV1SessionError`** vs **`GuyosWireV1MessageError`** split from that planning doc.
- **`GuyosWireV1SessionError::Io`** on read/write: **transport-level** — **no** Appendix A mapping unless product adds an explicit policy (default: surface as connection failure, not **`malformed_json`**).

## Implementation recipe (ordered)

1. **Session read loop** — Map **`FrameTooLarge`** → try send **`frame_too_large`**; **`InvalidUtf8`** → try send **`malformed_json`**; **`UnexpectedEof`** mid-frame → close without **`error`**; **`Io`** → propagate / close per adapter policy.
2. **Decode JSON → `GuyosWireV1Message`** — Map **`GuyosWireV1MessageError`** to outbound **`error`** **`code`** / **`message`** (E1); attach-flow ordering stays per [attach negotiation planning](adr-0003-guyos-wire-attach-negotiation-rules.md).
3. **Limits** — Construct **`attach_ack`** with echoed **`max_frame_bytes`** / **`max_message_bytes`** and optional **`keepalive_interval_seconds`** per resolved rows; wire **`FakeGuyosWireV1Session`** / production session with the same **`max_frame_bytes`** value echoed.
4. **`publish`** — After successful **`attach`**, validate **`text`** length → **`message_too_large`** when exceeded.
5. **Outbound hub messages** — **`publish_ack`** / **`chat_message`**: enforce JSON body and **`chat_message.text`** limits before write.
6. **`detach` / `keepalive`** — Dispatch after decode; ignore extras; update session resource state **promptly** when joined (**DT1** hint).

## Grill session log

| # | Question | Resolution |
| --- | --- | --- |
| 1 | When inbound F1 decoding fails because the declared payload length exceeds the session **`max_frame_bytes`**, must the hub send **one** S→C JSON **`error`** message with **`error.code` = `frame_too_large`** (E1 / Appendix A) before tearing down, or is **silent close** acceptable? | **Send** one **`error`** with **`frame_too_large`** when possible; if the write fails, close without implying the frame was within limits. |
| 2 | When F1 payload bytes fail UTF-8 validation (**`InvalidUtf8`**), must the hub send **`malformed_json`** before teardown? | **Yes** — same try-send-then-close pattern as Q1; **fallback** on write failure. |
| 3 | When **`read_application_frame`** returns **`UnexpectedEof`** (stream ended mid-frame after a length prefix), must the hub emit an S→C **`error`** with a portable **`error.code`**, or is **teardown without an `error` frame** acceptable? | **Teardown without `error`** — no Appendix A code for mid-frame truncation; do not overload **`malformed_json`**. |
| 4 | **`attach_ack.max_frame_bytes`** / **`max_message_bytes`**: always the normative ADR v1 literals, or **echo the caps this hub actually enforces** (including stricter operational limits)? | **Echo enforced caps** — literals when using ADR-only maxima; otherwise **stricter advertised values** matching enforcement. |
| 5 | **`message_too_large`** (**`publish.text`**): enforce **only** in the **`publish`** decode/validation path (single place), or **also** pre-check raw UTF-8 body size before JSON dispatch? | **`publish`** path only — measure **`text`** UTF-8 bytes vs **`max_message_bytes`**; **`frame_too_large`** covers oversize frames. |
| 6 | Hub-generated S→C **`publish_ack`** / **`chat_message`** with **`text`**: must the hub guarantee **`text`** UTF-8 length **`≤ max_message_bytes`** (and full JSON **`≤ max_frame_bytes`**) before **`write_application_frame`**, as an invariant aligned with **`attach_ack`**? | **Yes** — encode-time / invariant checks; never emit illegal frames after advertising those caps. |
| 7 | **`attach_ack.keepalive_interval_seconds`** (KA1): should the reference hub implementation **omit** the field unless configured, or **always emit** a default hint? | **Omit by default** — emit **only** when configured; test omit vs hint. |
| 8 | **`detach`** / **`keepalive`** inbound frames with **extra unknown top-level keys** (P1): treat as **no-op signal still valid** (ignore extras), or **reject** with a portable **`error.code`**? | **Ignore** unknown keys (**P1**) — **`type`**-only shapes remain valid with extras. |
