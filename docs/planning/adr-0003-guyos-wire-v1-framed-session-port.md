# Planning: Guyos wire v1 framed session port — ADR 0003 (Q1, S1, F1)

## Document status

**Grill session complete** (Questions **18–24** resolved; earlier decisions unchanged). Use this doc as the implementation guide; amend if the ADR or crate layout changes.

## References

- **Normative spec:** [`docs/adr/0003-wire-protocol-and-compatibility.md`](../adr/0003-wire-protocol-and-compatibility.md) — **Q1** (QUIC binding), **S1** / **F1** (serialization and framing), **Limits** (`max_frame_bytes`), closed v1 error **`frame_too_large`** (framing layer).
- **Layering:** [`docs/adr/0002-hexagonal-boundaries-and-ownership.md`](../adr/0002-hexagonal-boundaries-and-ownership.md)
- **Existing outbound port pattern:** [`guyos_core/src/ports/outbound/relay_chat_backend.rs`](../../guyos_core/src/ports/outbound/relay_chat_backend.rs)

## Audience and intent

This plan is a **recipe for an AI implementer**: concrete crate paths, type names, behaviors, and acceptance checks. It does **not** replace the ADR; where they differ, **follow the ADR** and update this document.

## Goal (this milestone)

Introduce a **`pub(crate)` outbound port** that models **exactly one** client-initiated **application** session stream **after** QUIC + TLS are ready, with **F1 framing enforced at the port boundary**:

- **Outbound frames:** `u32` **big-endian** length prefix, then **exactly** that many bytes of **UTF-8** text that constitute a **JSON object** on the wire (per ADR). At this port, expose **only** full JSON bodies as **`String`** — callers never see the length prefix.
- **Inbound frames:** deliver **`Some(String)`** only after the prefix and full payload are received; enforce **`max_frame_bytes`** on the **declared payload length** **before** allocating/interpreting the body; validate **UTF-8** before returning **`String`**.
- **Clean close:** if the peer finishes the stream **between** frames, **`read_application_frame`** returns **`Ok(None)`**. If the stream ends **mid-frame**, return **`Err(UnexpectedEof)`** (or equivalent; see error section).
- **ALPN / 0-RTT:** The trait performs **no** ALPN negotiation and exposes **no** early-data path. Document normatively that **concrete adapters** MUST NOT send F1 / application payloads in **0-RTT / early data**, and MUST rely on **`guyos-wire-v1`** having been negotiated **before** this session type is constructed. **This milestone does not implement** a QUIC adapter.

**Explicitly out of scope for this milestone**

- Any **iroh** / QUIC **adapter** (future work will map port errors to [`ChatError`](../../guyos_core/src/error.rs) or other façade types at the boundary — **decision: error mapping style (c)**).
- **JSON** message typing, **`attach` / `attach_ack`**, **`serde`** DTOs, and closed v1 **application** error codes beyond framing (those belong to a codec / session layer above this port).
- UniFFI or **Swift** exposure of the new types (keep **`pub(crate)`** unless a later change promotes them).

## Resolved design decisions (grill session)

| Topic | Decision |
| --- | --- |
| Primary artifact | **Implementation recipe** (paths, names, tests), not behavior-only prose. |
| Framing placement | **(1)** F1 is **inside** the port’s observable operations: callers read/write **JSON bodies** as **`String`**; shared **pure Rust** helpers implement prefix + limits + UTF-8 checks. |
| Error strategy | **(c)** Dedicated **internal** error enum for the port; **future** QUIC adapter maps into **`ChatError`** (or elsewhere). No **`ChatError`** changes required in this milestone. |
| Deliverables | **(i)** Port trait + **`f1_framing`** helpers + **paired in-memory fake** + **unit tests**. **No** network adapter. |
| Body type | **(β)** **`String`** on read and write — UTF-8 is part of the **write** contract; **reads** **`Err(InvalidUtf8)`** if the declared payload is not valid UTF-8. |
| Clean EOF | **(A)** **`read_application_frame` → `Result<Option<String>, E>`**; **`Ok(None)`** = graceful end **between** frames. |
| `max_frame_bytes` | **(2)** **Per-session** value supplied at construction (echoed from **`attach_ack`** in a future client stack; tests may use smaller limits). |
| Async style | **(a)** Same as **`RelayChatBackend`**: methods take **`&mut self`** and return **`impl Future<Output = …> + Send`** (no **`async fn`** in the trait unless the crate later standardizes on it). |
| Module layout | **Disciplined (y):** trait, fake, and **`f1_framing`** live under **`guyos_core/src/ports/outbound/`**. Add a **subdirectory / submodule** only if a single file would exceed reasonable size. Re-export the trait from **`ports/outbound/mod.rs`** alongside **`RelayChatBackend`**. Framing stays **out of** **`domain/`** and **out of** **`adapters/`** for this milestone. |
| Version in names | **(p)** Use **`V1`** in **trait**, **fake**, and **error** type names. Framing module name may stay **`f1_framing`** (F1 tag is ADR-stable). |
| Shutdown API | **(1)** **No** explicit half-close / **`finish_sending`**; **implicit** teardown via **`Drop`** (document the limitation). |
| Error variants (minimal) | **(m)** **`FrameTooLarge { declared: u32, limit: u32 }`**, **`UnexpectedEof`**, **`InvalidUtf8`**, **`Io`** (wrap **`std::io::Error`** or equivalent for the fake). |
| Fake topology | **(u)** **`FakeGuyosWireV1Session::paired() -> (Self, Self)`** (ADR **`MAX_FRAME_BYTES_ADR_V1`**) **and** **`paired_with_max_frame_bytes(u32) -> (Self, Self)`**; both ends implement **`GuyosWireV1Session`**. |
| Trait bounds | **Mirror relay port:** **`GuyosWireV1Session: Send + Sync`**, **`&mut self`** on methods. |
| Test placement | **(t)** Colocated **`#[cfg(test)] mod tests`** in **`f1_framing.rs`** and **`fake_guyos_wire_v1_session.rs`**. |
| Fake runtime | Use **`tokio`** channels / primitives; tests use **`#[tokio::test]`** (crate already depends on **`tokio`**). |
| Incremental decode (Q18) | **(buf)** Stateful **`F1FrameDecoder`** (name flexible): **`feed(&mut self, chunk: &[u8])`**, **`pop_complete_frame(&mut self) -> Result<Option<String>, GuyosWireV1SessionError>`** — **`None`** = need more bytes; **`Some(body)`** = one full UTF-8 JSON body. **`FrameTooLarge`** / **`InvalidUtf8`** surface from **`pop_complete_frame`** when applicable. **Truncated tail at EOF** is **`UnexpectedEof`** in the **session** read path when the stream ends, not an ambiguous decoder outcome. |
| Error impl (Q19) | **`(manual)`** Implement **`Display`** and **`std::error::Error`** by hand for **`GuyosWireV1SessionError`** — **no** **`thiserror`** (keeps **no new** production deps). |
| ADR v1 frame limit constant (Q20) | **`(const-yes)`** Define **`pub(crate) const MAX_FRAME_BYTES_ADR_V1: u32 = 1_048_576`** in **`f1_framing.rs`** (name flexible) as the single ADR-aligned literal for docs and **`F1FrameDecoder::new(limit)`**. |
| Fake pairing API (Q21) | **`(dual)`** **`paired()`** uses **`MAX_FRAME_BYTES_ADR_V1`**; **`paired_with_max_frame_bytes(limit)`** for tests with a custom cap. |
| Write payload rules (Q22) | **`(neutral)`** **`write_application_frame`** accepts **any** UTF-8 **`String`** (including **empty**); **JSON object** / message semantics are **above** this port. **Outbound** frames **must** reject **`body.len() > max_frame_bytes`** (same error as inbound oversize — **Q23**). |
| Oversize write error (Q23) | **`(reuse-frame-too-large)`** Use **`FrameTooLarge { declared, limit }`** for **write** as well as **read**; **`declared`** is the **payload** length in bytes (**`u32`**). Document that **`body.len()`** must fit **`u32`** for F1 encoding (true for all practical frames under ADR limits). |
| Fake module wiring (Q24) | **`(mirror-relay)`** Declare **`#[cfg(test)] pub(crate) mod fake_guyos_wire_v1_session;`** in **`ports/outbound/mod.rs`**; **no** facade re-export of the fake type (callers use the **`fake_guyos_wire_v1_session`** module path). Production builds omit the fake. |

## Open decisions

_None — last resolved: Question **24** (**`mirror-relay`**)._

## Implementation notes (non-branching)

- **`Io` (fake):** Use **`std::io::Error::other(...)`** (or equivalent) for channel closed / disconnect so **`Io(std::io::Error)`** matches a future network stack; assert in tests where useful.
- **Object safety:** **`dyn GuyosWireV1Session`** is **not** required; document if reviewers ask.
- **Allocation safety:** After reading the **4**-byte prefix, compare **`declared`** to **`max_frame_bytes`** **before** reserving or growing a buffer for the body (ADR **`frame_too_large`** semantics; avoid OOM on malicious lengths).

## Prescriptive file map

All paths relative to **`guyos_core/src/ports/outbound/`** unless noted.

| File | Responsibility |
| --- | --- |
| **`f1_framing.rs`** | Pure **F1** encode (**`String` → bytes including prefix**), **`F1FrameDecoder`** (**`feed`** / **`pop_complete_frame`**), **`max_frame_bytes`** on **declared length** before body allocation, UTF-8 validation → **`String`**. **No** `serde`. |
| **`guyos_wire_v1_session.rs`** | **`trait GuyosWireV1Session`** + **`GuyosWireV1SessionError`** (or split if file size warrants). |
| **`fake_guyos_wire_v1_session.rs`** | **`FakeGuyosWireV1Session`**, **`paired()`** / **`paired_with_max_frame_bytes`**, Tokio-backed duplex. |
| **`mod.rs`** | `mod` lines for **`f1_framing`**, **`guyos_wire_v1_session`**; **`pub(crate) use`** trait + error. **`#[cfg(test)] pub(crate) mod fake_guyos_wire_v1_session;`** (same pattern as **`fake_relay_chat_backend`**). |

**Wire-up:** Add `mod` lines to **`mod.rs`**; **no** new top-level **`wire/`** crate module for this milestone.

## Informative trait sketch (refine during implementation)

Normative obligations from the ADR must hold; names below are **targets**, not law.

```rust
// Informative only — do not treat as copy-paste final signatures.
pub trait GuyosWireV1Session: Send + Sync {
    fn read_application_frame(
        &mut self,
    ) -> impl Future<Output = Result<Option<String>, GuyosWireV1SessionError>> + Send;

    fn write_application_frame(
        &mut self,
        body: String,
    ) -> impl Future<Output = Result<(), GuyosWireV1SessionError>> + Send;
}
```

**Normative documentation** (module / trait rustdoc, required):

- **Single-stream invariant (Q1):** This type represents **one** bidirectional application stream; opening additional streams for the same protocol instance is **invalid** — enforcement happens in **concrete adapters**, not in this trait.
- **ALPN:** Callers construct this only after **`guyos-wire-v1`** is in effect; the trait **does not** expose ALPN state.
- **0-RTT:** Adapters **must not** expose early-data transmission of F1 payloads through this interface.
- **Framing:** All reads/writes are **F1**-delimited; **`max_frame_bytes`** is enforced on the length prefix before body interpretation.

## Acceptance criteria

- **`cargo test --manifest-path guyos_core/Cargo.toml`** passes.
- **Framing tests** cover at least: **round-trip** encode/decode (or decode after feed) for a valid small JSON object string; **declared length > limit** → **`FrameTooLarge`**; **write** with **`body.len() > max_frame_bytes`** → **`FrameTooLarge`**; **truncated** payload → **`UnexpectedEof`** at session read (or documented equivalent); **invalid UTF-8** payload → **`InvalidUtf8`**; **clean close** with no partial frame → **`Ok(None)`**.
- **Paired fake tests** prove **independent** ends can exchange frames under **`#[tokio::test]`**.
- **No new** production dependency required for this milestone (**`bytes`** already exists if you choose to use it internally; optional).

## Changelog

- **2026-05-07:** Initial draft from grill session (decisions through Question 17; Question 18 open).
- **2026-05-07:** Question **18** resolved — **`(buf)`** stateful **`F1FrameDecoder`**; open-decisions section cleared; allocation-safety note added.
- **2026-05-07:** Question **19** resolved — **`(manual)`** **`Display`** / **`Error`** for **`GuyosWireV1SessionError`**.
- **2026-05-07:** Question **20** resolved — **`(const-yes)`** **`MAX_FRAME_BYTES_ADR_V1`** in **`f1_framing.rs`**.
- **2026-05-07:** Question **21** resolved — **`(dual)`** **`paired()`** + **`paired_with_max_frame_bytes`**.
- **2026-05-07:** Question **22** resolved — **`(neutral)`** write accepts any UTF-8 body including empty; JSON rules are out of scope for this port.
- **2026-05-07:** Question **23** resolved — **`(reuse-frame-too-large)`** for oversize **writes**.
- **2026-05-07:** Question **24** resolved — **`(mirror-relay)`** **`cfg(test)`** fake module wiring.
