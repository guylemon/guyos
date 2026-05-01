# ADR 0003: Wire protocol and compatibility

## Status

Accepted

## Context

The chat plane is **relayed through a daemon** (hub); clients connect over **QUIC** with **custom application framing** on a **single bidirectional stream** per connection (**Q1**). [ADR 0002](0002-hexagonal-boundaries-and-ownership.md) keeps transport concerns in adapters; **this ADR** defines the **normative client ↔ daemon application protocol**: QUIC binding, framing, JSON payloads, versioning, ordering, and compatibility rules.

## Decision

### Reference identifiers

Parenthetical tags (**Q1**, **H2**, **P1**, **TV2**, etc.) are shorthand for the rules in the subsections below; **subsection titles and bullets remain normative.** Use this table as the index.


| Tag         | Meaning                                                                                                                                                                                                                                              |
| ----------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **V1**      | Normative protocol major version 1 in this ADR.                                                                                                                                                                                                      |
| **Q1**      | **QUIC v1 transport:** ALPN `guyos-wire-v1` (no fallback), **no** 0-RTT application data, **one** client-opened bidirectional stream for all **F1** frames. See **QUIC transport binding (Q1)**.                                                     |
| **H2**      | `protocol_major` / effective `protocol_minor` are negotiated once on **attach** (`**server_protocol_minor` = `**min(C, H)`**) and inherited for all later application frames on that connection. See **Protocol versioning (V1, H2)**.               |
| **P1**      | On decode, ignore unknown JSON object keys. See **Forward compatibility (P1)**.                                                                                                                                                                      |
| **TV2**     | No separate ticket-format version field on the wire; concrete ticket bytes are defined by **ticket profiles** (server-side; separate spec). See **Protocol versioning (V1, H2)** and **Tickets and ticket profiles**.                                |
| **S1**      | UTF-8 JSON application bodies. See **Serialization and framing (S1, F1)**.                                                                                                                                                                           |
| **F1**      | **Framing:** `u32` big-endian length prefix, then UTF-8 JSON. **Also** (with **M1**): canonical lowercase UUID string for `client_message_id`. See **Serialization and framing (S1, F1)** and **Publish, deduplication, identity (D2, M1, R3, F1)**. |
| **J1**      | Attach carries `protocol_major`, `protocol_minor`, and opaque `ticket`. See **Attach (join / negotiation)**.                                                                                                                                         |
| **A1**      | Minimum required content on `attach_ack`. See **Attach acknowledgement**.                                                                                                                                                                            |
| **O1**      | Monotonic per-room `seq` and total order. See **Ordering (O1)**.                                                                                                                                                                                     |
| **D2**      | Dedup scope and publish rules tied to the **room key**. See **Publish, deduplication, identity (D2, M1, R3, F1)**.                                                                                                                                   |
| **M1**      | `client_message_id` as RFC 9562 / 4122 canonical UUID. See **Publish, deduplication, identity (D2, M1, R3, F1)**.                                                                                                                                    |
| **R3**      | Idempotent publish: no duplicate fan-out; `publish_ack` returns the same `seq` as the first acceptance. See **Publish, deduplication, identity (D2, M1, R3, F1)**.                                                                                   |
| **N2**      | `seq` encoded as a decimal string in JSON (lossless **u64**). See `**seq` in JSON (N2)**.                                                                                                                                                            |
| **K1**      | Flat **snake_case** for `type` and keys. See **Message typing (T1)**.                                                                                                                                                                                |
| **E1**      | `error` object shape (`code`, `message`, optional `details`). See **Errors (E1)**.                                                                                                                                                                   |
| **T1**      | Required top-level JSON `type` field and dispatch. See **Message typing (T1)**.                                                                                                                                                                      |
| **S2**      | Optional `sender_endpoint_id` on `chat_message`. See **Appendix A** (required keys per `type`).                                                                                                                                                      |
| **TS1**     | Optional `server_timestamp` (**RFC 3339** UTC, fractional seconds) on `publish_ack` / `chat_message`; acceptance instant; ordering remains **O1** only. See **Server timestamp (optional) (TS1)**.                                                   |
| **DT1**     | Fire-and-forget C→S `detach`; **no** normative observer fan-out. See **Graceful detach (DT1)**.                                                                                                                                                      |
| **KA1**     | Optional hub-suggested keepalive interval and fire-and-forget C→S `keepalive`. See **Application keepalive (optional) (KA1)**.                                                                                                                       |
| **Sec1**    | TLS identity, pinning, authorization, threat modeling — out of normative scope here. See **Normative scope**.                                                                                                                                        |
| **E-allow** | `text` may be the empty string on publish / `chat_message`. See **Publish, deduplication, identity (D2, M1, R3, F1)**.                                                                                                                               |


### Normative scope

- **In scope:** **Client ↔ daemon (hub)** application messages, **QUIC transport binding** for that application (**Q1**), and **compatibility policy**. **Hub behavior** that is visible on the wire (fan-out, ordering, deduplication) is specified here **where it affects** interoperability.
- **Out of scope (normative v1):** TLS identity, certificate pinning, authorization policies, and threat modeling (**Sec1**). Deployment binds trust and access control. **Concrete ticket encoding** (base64, JWT, CBOR, signatures, claims) is **out of scope** here and is defined per deployment by a **ticket profile** (see **Tickets and ticket profiles**).
- **Implementation freedom:** How the daemon stores sessions, trims history, and schedules I/O—as long as wire obligations below are met.

### QUIC transport binding (Q1)

- **ALPN:** After the TLS handshake on the QUIC connection, **Application-Layer Protocol Negotiation** **MUST** yield **exactly** the identifier `guyos-wire-v1` (US-ASCII, case-sensitive) for this wire contract to apply. If any other ALPN (or no ALPN) is negotiated, the connection is **not** normative V1 of this ADR; endpoints **MUST NOT** send or interpret **F1** application frames on that connection as this protocol. There is **no** in-band fallback to another ALPN for V1.
- **0-RTT (early data):** For normative V1, implementations **MUST NOT** send **F1** frames (or any application payload defined here) inside TLS **0-RTT** / QUIC **early** data. **0-RTT** is **out of scope** for this protocol version; a future revision may define it if replay-safe rules are added.
- **Streams:** The **client** **MUST** open **exactly one** **bidirectional** QUIC stream for all **F1** / JSON application traffic **after** the connection is ready and **ALPN** is `guyos-wire-v1`. The hub **MUST** run the application protocol **only** on that stream. **Additional** bidirectional (or unidirectional) streams opened for this same application protocol are **invalid** for V1; the hub **SHOULD** **reset** or **abandon** them (implementation-defined), and **MUST NOT** split the **F1** session across multiple streams.
- **Non-normative:** Library APIs may still expose stream IDs; this ADR cares only that **all** V1 **F1** frames use the **one** client-opened bidi stream.

```mermaid
sequenceDiagram
    autonumber
    participant C as Client
    participant H as Hub
    Note over C,H: QUIC + TLS; handshake completes
    Note over C,H: ALPN is guyos-wire-v1
    C->>H: Open single bidirectional stream
    Note over C,H: All F1 frames on this stream only
    C->>H: First F1 frame (e.g. attach)
```



### Sessions and routing

- **Room key:** The daemon maps each accepted connection to **exactly one** stable **room identifier**: the **canonical topic / room id** string the hub derives when it **decodes** the join ticket using the **active ticket profile** (see **Tickets and ticket profiles**). On the wire, `**attach_ack.room_id` is an opaque UTF-8 string**; routing and deduplication use **byte-for-byte equality** of that string (after JSON unescape). **Non-normative:** Deployments often use RFC 9562 / 4122 **canonical lowercase UUID** strings for `room_id`.
- **Resume:** A new connection that presents the **same** ticket **or** another ticket that decodes to the **same** room id joins the **same** logical room. **History** and **replay** after attach are **daemon policy** (what the hub sends after attach or on demand)—they do **not** change the fan-out key.

### History and replay (non-normative)

History and replay behavior after `attach_ack` is **explicitly not part of the v1 wire contract**. Implementations may choose to send zero or more historical `chat_message` frames (or none at all).

Normative v1 defines **no** messages that allow a client to request history (e.g. by count or `seq` cursor). Any history synchronization mechanism is out-of-band or a future protocol extension.

When a hub elects to send historical messages, the following **invariants MUST still hold**:

- Any historical `chat_message` frames **MUST** conform to the same schema, `seq` ordering, `client_message_id` deduplication rules, and `type` requirements as live fan-out messages.
- The client **MUST** treat any such replay as **best-effort and incomplete**. It **MUST** function correctly whether it receives zero messages, a partial tail, a non-contiguous subset, or even duplicates of messages it already possesses.
- Clients that require reliable or complete history **SHOULD** obtain it through an out-of-band channel (e.g. REST/GraphQL) rather than depending on opportunistic wire replay.

This section may be made normative in a future revision once product requirements for history synchronization stabilize.

### Protocol versioning (V1, H2)

- **Integers:** Each connection negotiates `**protocol_major`** and an effective `**protocol_minor`** once, on **join / attach** (**H2**). Both sides **inherit** that pair for **all** subsequent **application** frames on that connection until disconnect.
- **Negotiation:** On `**attach`**, the client sends `**protocol_minor` = `C`**, the highest minor it is built to use (desired ceiling). The hub implements `**H**`, the **largest** `**protocol_minor`** it supports for that `**protocol_major`** (implementation-defined ceiling). The effective negotiated minor for the connection is `**min(C, H)`**. The hub MUST send `**attach_ack.server_protocol_minor` = `min(C, H)`** (**Appendix A**). The hub **MUST NOT** reject `**attach`** solely because `**C > H`**; lowering to `**H`** is the **only** downgrade.
- **Bounds:** `**protocol_major`**, `**attach.protocol_minor`**, `**attach_ack.protocol_major**`, and `**attach_ack.server_protocol_minor**` MUST be integers in `**0 … 2_147_483_647**` (**inclusive**, **u31**). Values outside that range → `**invalid_attach`** (**Appendix A**).
- **Client obligation:** After `**attach_ack`**, the client MUST treat `**attach_ack.server_protocol_minor`** as the **effective negotiated maximum** for optional / minor-gated behavior. It **MUST NOT** assume behavior from minors **above** that value (even if `**attach`** sent a larger `**C`**). **P1** still applies at decode time for unknown keys.
- **Rules:**
  - **Unknown `protocol_major`** from the client → **reject** attach with `**error.code` = `protocol_major_unsupported`** (see **Appendix A**) **after** `**attach`** passes semantic validation (**Attach (join / negotiation)**).
  - `**protocol_minor`:** **Additive** evolution only within the **same `protocol_major`**; receivers **MUST** ignore unknown JSON object keys (**P1**). **Breaking** changes require a `**protocol_major`** bump.
- **Ticket payload:** There is **no** separate **ticket-format version** field on `**attach`** (**TV2**). **Concrete ticket bytes** (encoding, signing, claims) are specified by **ticket profiles** and **may** evolve **without** a `**protocol_major`** bump as long as this wire ADR’s obligations are met; **breaking** wire-level ticket handling remains a `**protocol_major`** concern.

### Tickets and ticket profiles

- `**attach.ticket`:** From the client’s perspective, an **opaque UTF-8 string** (JSON string value). Clients **MUST NOT** substitute decoded topic or room fields for this string on normative v1 `**attach`**.
- **Hub decode:** The hub interprets `ticket` using the **active ticket profile** for this deployment (**server-side selection** for v1 — configuration, listener, or other deployment surface; **no** normative `**attach`** field names which profile). [ADR 0004](0004-ticket-profiles-and-reference-profile.md) defines the **ticket profile framework** and the normative **`guyos.reference_v1`** profile, including each profile’s byte→meaning steps, validation rules, and **ordered decode/validate checklist** with **mapping** of failures to `**ticket_decode_failed`** vs `**invalid_ticket`** (see **Appendix A** portable meanings).
- **Interop:** Any client that speaks this wire protocol can connect to any hub that implements it; **ticket acceptance** depends on that hub’s configured profiles and issued tickets (deployment contract).

### Serialization and framing (S1, F1)

- **Payload encoding:** **UTF-8 JSON** (**S1**) for all normative **v1** application bodies.
- **Framing:** `**u32` big-endian** byte length prefix, followed by **exactly** that many bytes of **UTF-8** JSON (**F1**). On the **single** application bidi stream (**Q1**), **many** **F1** frames are sequenced back-to-back; **decoders** **MUST** enforce a **maximum frame size** (see **Limits**).
- **Non-normative:** Future **bulk** or **binary** data (e.g. file transfer) may use **additional** stream types, **different** framing, or `**protocol_major`** bumps; **S1** does **not** forbid **non-JSON** elsewhere when specified.

### Attach (join / negotiation)

- **Purpose:** Deliver `**protocol_major`**, `**protocol_minor`**, and the **opaque UTF-8 ticket string** (**J1**). The **hub** decodes it per the **active ticket profile** and derives the **room key**; clients **MUST NOT** send decoded topic fields in place of the ticket for normative v1 attach.
- **Version negotiation:** Occurs **only** here (**H2**).
- **Validation order (normative):** After bytes parse as **UTF-8 JSON** and `**type`** is `**attach`**, the hub MUST validate `**attach`** **required keys**, **field types**, and **integer bounds** (**Protocol versioning**) **before** decoding `**ticket`**. Failures → `**invalid_attach`**. If `**protocol_major**` is unsupported → `**protocol_major_unsupported**`. **Only then** may the hub decode `**ticket`** (`**ticket_decode_failed`** / `**invalid_ticket`**). **UTF-8** or **JSON object** parse failures → `**malformed_json`** (**Appendix A**).
- **Single successful attach per stream (v1):** After `**attach_ack`** has been sent on an application stream, a **second** `**attach`** on that stream **MUST** be rejected with `**invalid_attach`**.
- **Cold start:** The **first** client→hub application frame **SHOULD** be `**attach`**; `**detach`** **MAY** appear **before** `**attach_ack`** as a **no-op** (**DT1**) and **does not** satisfy cold start by itself. Otherwise `**attach_required`** applies (see **Appendix A**).
- **Idle keepalive before join:** `**keepalive`** **MAY** appear **before** `**attach_ack`** as a **no-op** (**KA1**) and **does not** satisfy cold start by itself.
- **Retry after failed attach:** After `**error`** in response to `**attach`**, the client MAY send `**attach`** again on the **same** application stream until `**attach_ack`** or the connection closes.

### Attach acknowledgement

- `**attach_ack`** is the **first successful** server→client application reply **after** an `**attach`** that the hub **accepts** — i.e. not necessarily after the **first** `**attach`** on that connection if earlier attempts failed with `**error`**.
- **Minimal normative content (A1):** Confirm success, echo the **canonical room id** string the hub uses for routing (**opaque UTF-8** — see **Sessions and routing**), report **limits** relevant to this connection (see **Limits**), echo `**protocol_major`** (the **accepted** major from the successful `**attach`**), and `**server_protocol_minor` = `min(C, H)`** (**Protocol versioning**).
- **Optional keepalive hint (KA1):** The hub **MAY** include `**keepalive_interval_seconds`** (integer) to suggest a cadence for client `**keepalive`** frames. If omitted, the client **MUST NOT** infer that keepalives are required.
- **Non-normative:** Implementations **may** piggyback **history** or **cursor** data in the same JSON object or in **follow-up** frames; that is **not** required for interoperability of **fan-out** and **publish**.

### Ordering (O1)

- The hub assigns a **monotonic per-room** `**seq`** (**u64** semantics). **Every** fan-out delivery of a **published** message **includes** the authoritative `**seq`** for that message in that room.
- **Total order:** All subscribers attached to the room **observe** the **same** `**seq`** ordering for **accepted** publishes.
- **Wall-clock fields:** If `**server_timestamp`** appears (**TS1**), receivers **MUST** still **merge**, **sort**, and **deduplicate** chat solely by `**seq`** / `**client_message_id`** per **O1** / **R3**; timestamps are for **display** and **diagnostics** only (they **need not** be monotonic across messages).

### Server timestamp (optional) (TS1)

- The hub **MAY** attach `**server_timestamp`** (string) to `**publish_ack`** and to matching `**chat_message`** fan-out for the **same** accepted publish (**same** `**seq`** and `**client_message_id`**).
- **Format:** **RFC 3339** profile **UTC**, including **fractional seconds** (e.g. `2026-05-02T14:32:01.234Z`). **Exactly one** encoding—no alternate epoch or local-offset forms in normative v1.
- **Semantics:** The hub’s **instant of acceptance** for that publish—the **same** logical step where it assigns `**seq`** and applies dedup (**R3**).
- **Idempotent retry:** For an **R3** replay with the **same** `**client_message_id`**, `**publish_ack`** SHOULD echo the same `**server_timestamp**` value as the **first** acceptance when timestamps are used (**TS1**).
- **Publisher vs observers:** The publisher **does not** receive `**chat_message`** for its own line (**Publisher vs observers**); `**publish_ack`** is therefore the **only** normative place that **may** carry **authoritative** wall time **to the publisher**. Observers **may** read `**server_timestamp`** from `**chat_message`**.

### Publish, deduplication, identity (D2, M1, R3, F1)

- **Client-originated publish** **MUST** include a `**client_message_id`**: **UUID** string in **RFC 9562 / 4122 canonical** form (**lowercase** hex with hyphens) (**F1**, **M1**).
- `**text` UTF-8 payload** **may** be the **empty string** (**E-allow**).
- **Dedup scope:** **Same room** (same **room key**).
- **Idempotent retry (R3):** If a publish uses a `**client_message_id`** **already accepted** in that room, the hub **MUST NOT** fan out a **second** copy. The **publisher** **MUST** receive a **successful** acknowledgement carrying the **same** `**seq`** as the **first** acceptance.

### Publisher vs observers

- The **publishing** connection **only** receives `**publish_ack`** for its **accepted** publishes (`**client_message_id` + `seq`**).
- **Every other** connection in the room receives `**chat_message`** fan-out for that line.
- The hub **MUST NOT** send `**chat_message`** **back to the publisher** for the **same** logical message (**no** hub echo of own line).

### Graceful detach (DT1)

- Clients **MAY** send `**detach`** (**C→S**) as a **fire-and-forget** hint that they are **leaving** the room on this connection (**DT1**). The hub **MUST NOT** require a **server→client** reply for correctness.
- **Payload:** Required top-level key `**type`** only (see **Appendix A**).
- **When attached:** The hub **SHOULD** **release** room-related resources for that connection **promptly** after processing `**detach`** (implementation-defined beyond that).
- **When not joined:** If there is **no** successful room attachment yet, the hub **SHOULD** treat `**detach`** as a **no-op** and **MUST NOT** emit `**error`** **solely** because `**detach`** preceded `**attach_ack`** or arrived without an active room.
- **Non-normative:** Observer-visible “user left” or **presence** fan-out is **not** specified in normative v1; hubs **may** layer policy or future `**minor`** extensions.

### Application keepalive (optional) (KA1)

- Clients **MAY** send `**keepalive`** (**C→S**) as a **fire-and-forget** hint that the connection is still active, even when otherwise idle (**KA1**).
- **When not joined:** If there is **no** successful room attachment yet, the hub **SHOULD** treat `**keepalive`** as a **no-op** and **MUST NOT** emit `**error`** solely because `**keepalive`** preceded `**attach_ack`** or arrived without an active room.
- **Suggested interval:** If the hub provided `**keepalive_interval_seconds`** on `**attach_ack`**, the client **MAY** send keepalives at approximately that cadence, or use its own policy. The hub’s value is a **hint**, not a contract.
- **No required reply:** The hub **MUST NOT** require a **server→client** reply for correctness (same philosophy as **DT1**).
- **Resource management signal:** The hub **MAY** treat the absence of `**keepalive`** (and the absence of other client activity) as a signal to **reap** per-connection resources on an implementation-defined schedule. This does **not** introduce normative **presence** semantics or observer-visible fan-out in v1.

### Illustrative sequence (non-normative)

The figure below is **illustrative** only; **Appendix A** and the Decision bullets remain **normative**.

```mermaid
sequenceDiagram
    autonumber
    participant Pub as Publisher
    participant Hub as Hub
    participant Obs as Observer

    Pub->>Hub: attach
    Hub->>Pub: attach_ack
    Obs->>Hub: attach
    Hub->>Obs: attach_ack
    Pub->>Hub: publish
    Hub->>Pub: publish_ack
    Hub->>Obs: chat_message
    Note over Pub: No chat_message to publisher for its own line (see Publisher vs observers).
    opt Optional graceful teardown (DT1)
        Pub->>Hub: detach
        Note over Pub,Hub: Fire-and-forget; no required S→C reply.
    end
```



Each arrow corresponds to **application** JSON objects sent as **F1** frames (u32 big-endian length prefix, UTF-8 JSON body **S1**) on the **single** client-opened bidirectional stream (**Q1**); see **QUIC transport binding (Q1)** and **Serialization and framing (S1, F1)**.

### `seq` in JSON (N2)

- `**seq`** **MUST** appear as a **decimal string** encoding a **u64** (e.g. `"0"`, `"18446744073709551615"`) so values stay **lossless** in JSON tooling (**N2**).

### Message typing (T1)

- **Every** JSON application object **MUST** include a top-level `**type`** string (**flat `snake_case`** values — **K1**). **Routers** dispatch on `**type`** before interpreting other keys.
- **Normative v1 `type` values and required keys** are listed in **Appendix A**.

### Errors (E1)

- Failure payloads **MUST** use **an `error`** object with:
  - `**code`:** stable machine-readable string (library and UI mapping).
  - `**message`:** human-oriented description (debugging; not a contract for parsing).
  - `**details`:** optional JSON object for structured extras.
- `**malformed_json`** vs `**invalid_attach`:** `**malformed_json`** applies to **UTF-8** failure or **failure to parse** bytes as a **JSON object**. `**invalid_attach`** applies when UTF-8 JSON parses and `**attach`** **semantic** validation fails (**Appendix A**).
- **Closed v1 `error.code` values** are listed in **Appendix A**. Senders **MAY** introduce **new** codes in `**minor`** revisions only if receivers treat **unknown** `**code`** values as **generic failures** (**recommended**).

### Forward compatibility (P1)

- On **decode**, implementations **MUST** ignore **unknown** object keys for **every** normative message shape, **including** attach and errors. **Unknown `type`** values **SHOULD** be treated as an **error** or **logged** per policy (**major** bumps introduce new `**type`** values deliberately).

### Limits (normative v1)

- `**max_message_bytes`:** **65_536** — upper bound on **UTF-8 byte length** of `**text`** on `**publish`** (and `**chat_message`**).
- `**max_frame_bytes`:** **1_048_576** — upper bound on **payload byte length** **after** the `**u32`** length prefix (the JSON body). **MUST** be **≥** the largest legal framed message (including `**publish`** / `**chat_message`** envelopes under `**max_message_bytes`**).
- `**attach_ack`** **MUST** echo `**max_message_bytes`** and `**max_frame_bytes`** so clients need not hard-code limits.

## Consequences

- **Transport and application** are both pinned: **Q1** (ALPN `guyos-wire-v1`, one client bidi stream, no 0-RTT app data) plus **F1** / **S1** define a full interop surface for QUIC implementations (e.g. iroh **Endpoint** / **Router** in [ADR 0002](0002-hexagonal-boundaries-and-ownership.md)).
- **Clients and hub** can be implemented **independently** against **JSON schemata + framing rules**; **Swift** and **CLI** can share the **same** contract.
- **Rolling upgrades** follow `**protocol_minor`** **additive** rules and **P1**; hubs and clients negotiate `**min(C, H)`** on `**attach_ack`** without rejecting `**attach`** for `**C > H**` alone; **breaking** wire changes are explicit `**protocol_major`** bumps (**TV2**). **Ticket profile** revisions **may** ship **without** bumping `**protocol_major`** when only deployment-issued ticket bytes change.
- **Testing** can fix **golden frames**: **length prefix + JSON** **per `type`**; **ticket** golden vectors are **profile-specific** (see **Tickets and ticket profiles**).
- **Security** (who may reach the hub, TLS trust) remains **outside** this ADR’s **normative** text—operators must **compose** deployment controls.
- History replay after `attach_ack` is intentionally left as hub policy; clients **MUST NOT** depend on it for correctness.

## Open points

- **Future `minor` revisions:** Additional optional keys, normative **presence** / “participant left” fan-out (not in v1 **DT1**), and new `**error.code`** values—receivers remain **P1**-tolerant.
- **History synchronization:** Normative history request/response messages (with pagination, cursors, etc.) are candidates for a future revision once product requirements stabilize.
- **Future `major` revisions:** New `**type`** values, non-JSON payloads, or wire-breaking ticket handling changes (**TV2**).
- **Ticket profiles:** [ADR 0004](0004-ticket-profiles-and-reference-profile.md) defines the registry, framework rules, and **`guyos.reference_v1`**. **Optional** future `**protocol_minor`** extensions (e.g. client-supplied profile hint) are **not** in normative v1.

---

## Appendix A — Normative v1 message shapes

**Conventions:** Top-level keys **snake_case**. `**type`** uses **flat `snake_case`** (**K1**). `**type`** is **required** on every message.

### v1 `type` summary


| `type`         | Direction                            | Purpose                                                                                                                                                                                                                                       |
| -------------- | ------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `attach`       | C→S                                  | Join: `**protocol_major`**, `**protocol_minor`**, opaque `**ticket**`. Cold start: should be the first C→S frame; after `**error**` on `**attach**`, **may** retry `**attach`** on the **same** stream (see **Attach (join / negotiation)**). |
| `attach_ack`   | S→C                                  | Success after `**attach`**: `**room_id`** echo, limits, `**protocol_major**` echo, `**server_protocol_minor**` (= `**min(C, H)**` — **Protocol versioning**).                                                                                 |
| `publish`      | C→S                                  | Chat line + `**client_message_id`** + `**text`**. Only after successful `**attach`** (`**attach_ack`** received).                                                                                                                             |
| `detach`       | C→S                                  | Fire-and-forget **leave** hint (**DT1**); **no** required **S→C** reply; **no-op** when not joined.                                                                                                                                           |
| `keepalive`    | C→S                                  | Fire-and-forget “still here” hint for hub resource management (**KA1**); may be sent periodically; **no** required **S→C** reply; does not define observer-visible presence in v1.                                                            |
| `publish_ack`  | S→C                                  | `**seq`** assignment + `**client_message_id`** echo to the **publisher** (**R3**). Optional `**server_timestamp`** (**TS1**).                                                                                                                 |
| `chat_message` | S→C                                  | Fan-out to **non-publisher** connections (see **Publisher vs observers**). Optional `**server_timestamp`** (**TS1**).                                                                                                                         |
| `error`        | S→C (and **may** C→S if ever needed) | `**error`** object per **E1**.                                                                                                                                                                                                                |


### Required keys per `type`

- `**attach`:** `type`, `protocol_major` (integer, `**0 … 2_147_483_647`**), `protocol_minor` (integer, `**0 … 2_147_483_647`** — desired ceiling `**C**`; **Protocol versioning**), `ticket` (string: opaque UTF-8 from the client’s perspective; hub decodes per **Tickets and ticket profiles**).
- `**attach_ack`:** `type`, `room_id` (string, opaque UTF-8 canonical room/topic id used for routing; byte equality defines the room key — **Sessions and routing**), `max_frame_bytes` (integer, **1_048_576** in v1), `max_message_bytes` (integer, **65_536** in v1), `protocol_major` (integer, `**0 … 2_147_483_647`** — echoed accepted major from the successful `**attach`**), `server_protocol_minor` (integer, `**0 … 2_147_483_647`** — `**min(C, H)**`, effective negotiated maximum for this connection — **Protocol versioning**). **Optional:** `keepalive_interval_seconds` (integer, **KA1**).
- `**publish`:** `type`, `client_message_id` (canonical UUID string), `text` (string, UTF-8; **empty allowed**).
- `**detach`:** `type` — **only** required top-level key (**DT1**).
- `**keepalive`:** `type` — **only** required top-level key (**KA1**).
- `**publish_ack`:** `type`, `client_message_id`, `seq` (decimal string, **N2**). **Optional:** `server_timestamp` (string, **RFC 3339** UTC with fractional seconds — **TS1**).
- `**chat_message`:** `type`, `seq` (decimal string), `client_message_id`, `text` (string, UTF-8). **Optional:** `sender_endpoint_id` (string, when the hub can attribute the source connection — **S2**); `server_timestamp` (string, **RFC 3339** UTC with fractional seconds — **TS1**).
- `**error`:** `type`, `error` → `{ code, message, details? }` (**E1**).

### Closed v1 `error.code` values


| `error.code`                 | When                                                                                                                                                                                                                                                                                                                                                  |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `protocol_major_unsupported` | Client `**attach`** `**protocol_major`** is not supported by the hub (**after** `**attach`** passes semantic validation — **Attach (join / negotiation)**).                                                                                                                                                                                           |
| `invalid_attach`             | `**attach`** fails **semantic** validation: missing / wrong-type / out-of-range `**protocol_major`** or `**protocol_minor`**, other `**attach`** shape violations, or a second `**attach**` after `**attach_ack`** on the same stream (**Attach (join / negotiation)**). **Not** used for UTF-8 or JSON-object parse failures (`**malformed_json`**). |
| `ticket_decode_failed`       | Failure **before** the ticket can be interpreted **far enough** to apply the **active profile’s** validation rules (e.g. wrong alphabet, truncated blob, not valid shape for a profile-defined JWT/JWS step). **Fine-grained** steps and ordering are **profile-defined** (see **Tickets and ticket profiles**).                                      |
| `invalid_ticket`             | Ticket was interpreted **according to the active profile** far enough to run its checks, but **failed** them (e.g. bad signature, expired `exp`, missing required claim, unknown room in tenant DB). **Profile-defined** mapping from checks to this code.                                                                                            |
| `frame_too_large`            | Declared frame length **>** `**max_frame_bytes`** (length-prefix / framing layer).                                                                                                                                                                                                                                                                    |
| `message_too_large`          | `**text`** UTF-8 byte length **>** `**max_message_bytes`** on `**publish`**.                                                                                                                                                                                                                                                                          |
| `malformed_json`             | Bytes are not valid **UTF-8**, or do not parse as a **JSON object**, for this protocol. **Does not** apply when JSON parses and `**attach`** semantics fail (`**invalid_attach`**).                                                                                                                                                                   |
| `unknown_message_type`       | JSON parses but `**type`** is **missing** or not a **known v1** `**type`**.                                                                                                                                                                                                                                                                           |
| `attach_required`            | `**publish`** or another C→S message **except** `**detach`** **before** successful `**attach`**, or cold start expectations (Attach) are violated. `**detach`** alone **never** triggers `**attach_required`** (**DT1**).                                                                                                                             |
| `invalid_client_message_id`  | `**publish`** `**client_message_id`** is not a **canonical UUID** (**F1**).                                                                                                                                                                                                                                                                           |


**Recommendations for operators:** Use `**error.message`** / `**details`** for debugging only; `**code`** is the **stable** contract.
