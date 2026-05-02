# ADR 0003: Wire protocol and compatibility

## Status

Proposed

## Context

The chat plane is **relayed through a daemon** (hub); clients connect over **QUIC** with **custom application framing**. [ADR 0002](0002-hexagonal-boundaries-and-ownership.md) keeps transport concerns in adapters; **this ADR** defines the **normative client ↔ daemon application protocol**: framing, JSON payloads, versioning, ordering, and compatibility rules.

Background from the spike (non-normative history): tickets and messages were JSON-oriented with **no wire versioning**; see [spike code review](../reference/spike-code-review.md) and [GUYOS_CORE_SPIKE_REFACTOR_TASKS.md](../../GUYOS_CORE_SPIKE_REFACTOR_TASKS.md). Until this ADR is accepted, those references remain **working assumptions**.

## Decision

### Normative scope

- **In scope:** **Client ↔ daemon (hub)** application messages and **compatibility policy**. **Hub behavior** that is visible on the wire (fan-out, ordering, deduplication) is specified here **where it affects** interoperability.
- **Out of scope (normative v1):** TLS identity, certificate pinning, authorization policies, and threat modeling (**Sec1**). Deployment binds trust and access control.
- **Implementation freedom:** How the daemon stores sessions, trims history, and schedules I/O—as long as wire obligations below are met.

### Sessions and routing

- **Room key:** The daemon maps each accepted connection to **exactly one** stable **room identifier**: the **canonical topic / room id** derived when the hub **decodes** the join ticket (**T1**). Fan-out is keyed **only** by this id; **multiple connections** (same user, multiple devices, reconnects) attach **multiple QUIC connections** to the **same** room key when their tickets decode to the **same** id.
- **Resume:** A new connection that presents the **same** ticket **or** another ticket that decodes to the **same** room id joins the **same** logical room. **History** and **replay** after attach are **daemon policy** (what the hub sends after attach or on demand)—they do **not** change the fan-out key.

### Protocol versioning (V1, H2)

- **Integers:** Each connection negotiates **`protocol_major`** and **`protocol_minor`** once, on **join / attach** (**H2**). Both sides **inherit** that pair for **all** subsequent **application** frames on that connection until disconnect.
- **Rules:**
  - **Unknown `protocol_major`** from the client → **reject** attach with a **stable** error **`code`** (see **Errors**).
  - **`protocol_minor`:** **Additive** evolution only within the **same `protocol_major`**; receivers **MUST** ignore unknown JSON object keys (**P1**). **Breaking** changes require a **`protocol_major`** bump.
- **Ticket payload:** There is **no** separate **ticket-format version** field (**TV2**). **Ticket** layout evolves **with** the **wire protocol** version this ADR ties to the same release expectations.

### Serialization and framing (S1, F1)

- **Payload encoding:** **UTF-8 JSON** (**S1**) for all normative **v1** application bodies.
- **Framing:** **`u32` big-endian** byte length prefix, followed by **exactly** that many bytes of **UTF-8** JSON (**F1**). **One** bidirectional stream **may** carry many frames; **decoders** **MUST** enforce a **maximum frame size** (see **Limits**).
- **Non-normative:** Future **bulk** or **binary** data (e.g. file transfer) may use **additional** stream types, **different** framing, or **`protocol_major`** bumps; **S1** does **not** forbid **non-JSON** elsewhere when specified.

### Attach (first client → server application frame)

- **Purpose:** Deliver **`protocol_major`**, **`protocol_minor`**, and the **opaque ticket string** (**J1**). The **hub** decodes the ticket and derives the **room key**; clients **MUST NOT** send decoded topic fields in place of the ticket for normative v1 attach.
- **Version negotiation:** Occurs **only** here (**H2**).

### Attach acknowledgement (first successful server → client application frame after attach)

- **Minimal normative content (**A1**):** Confirm success, echo the **canonical room id** string the hub uses for routing, and report **limits** relevant to this connection (see **Limits**).
- **Non-normative:** Implementations **may** piggyback **history** or **cursor** data in the same JSON object or in **follow-up** frames; that is **not** required for interoperability of **fan-out** and **publish**.

### Ordering (O1)

- The hub assigns a **monotonic per-room** **`seq`** (**u64** semantics). **Every** fan-out delivery of a **published** message **includes** the authoritative **`seq`** for that message in that room.
- **Total order:** All subscribers attached to the room **observe** the **same** **`seq`** ordering for **accepted** publishes.

### Publish, deduplication, identity (D2, M1, R3, F1)

- **Client-originated publish** **MUST** include a **`client_message_id`**: **UUID** string in **RFC 9562 / 4122 canonical** form (**lowercase** hex with hyphens) (**F1**, **M1**).
- **Dedup scope:** **Same room** (same **room key**).
- **Idempotent retry (**R3**):** If a publish uses a **`client_message_id`** **already accepted** in that room, the hub **MUST NOT** fan out a **second** copy. The **publisher** **MUST** receive a **successful** acknowledgement carrying the **same** **`seq`** as the **first** acceptance.

### `seq` in JSON (N2)

- **`seq`** **MUST** appear as a **decimal string** encoding a **u64** (e.g. `"0"`, `"18446744073709551615"`) so values stay **lossless** in JSON tooling (**N2**).

### Message typing (T1)

- **Every** JSON application object **MUST** include a top-level **`type`** string. **Routers** dispatch on **`type`** before interpreting other keys.
- **Provisional v1 `type` values and minimal fields** are listed in **Appendix A**; **finalize** naming and required keys in a **follow-up** design pass (see **Open points**).

### Errors (E1)

- Failure payloads **MUST** use a **`error`** object with:
  - **`code`:** stable machine-readable string (library and UI mapping).
  - **`message`:** human-oriented description (debugging; not a contract for parsing).
  - **`details`:** optional JSON object for structured extras.
- **Exact `code` strings** for v1 are **to be enumerated** with **Appendix A** tightening.

### Forward compatibility (P1)

- On **decode**, implementations **MUST** ignore **unknown** object keys for **every** normative message shape, **including** attach and errors. **Unknown `type`** values **SHOULD** be treated as an **error** or **logged** per policy (**major** bumps introduce new **`type`** values deliberately).

### Limits (normative minimum expectations)

- **`max_frame_bytes`:** ADR **requires** a **finite** maximum **per frame** after length prefix; **exact** value is **TBD** alongside **Appendix A** (must be **consistent** with **`max_message_bytes`** for chat text).
- **`max_message_bytes`:** Upper bound on **user-visible chat payload** per publish (**echoed** or **fixed** in **attach_ack**). **Exact** value **TBD** in **Appendix A**.

## Consequences

- **Clients and hub** can be implemented **independently** against **JSON schemata + framing rules**; **Swift** and **CLI** can share the **same** contract.
- **Rolling upgrades** follow **`protocol_minor`** **additive** rules and **P1**; **breaking** changes are explicit **`protocol_major`** bumps **including** ticket layout (**TV2**).
- **Testing** can fix **golden frames**: **length prefix + JSON** **per `type`**.
- **Security** (who may reach the hub, TLS trust) remains **outside** this ADR’s **normative** text—operators must **compose** deployment controls.

## Open points

- **Appendix A:** Final **`type` strings**, **minimal required fields per type**, **enumerated `error.code` values**, and **numeric limits** (`max_frame_bytes`, `max_message_bytes`).
- **Sender attribution on fan-out:** Provisional fields (e.g. endpoint id) need a **decision** if **UI** must show **which device** sent a line.

---

## Appendix A — Provisional v1 message shapes (for follow-up grill)

**Convention:** All keys **snake_case** unless changed during finalization. **`type`** is **required** on every message.

| `type` (provisional) | Direction | Purpose |
|----------------------|-----------|---------|
| `attach` | C→S | Join: protocol version + opaque ticket string |
| `attach_ack` | S→C | Success: room id echo + limits |
| `publish` | C→S | Chat payload + required `client_message_id` |
| `publish_ack` | S→C | Ack: echo `client_message_id` + authoritative `seq` |
| `message_delivered` | S→C | Fan-out to other connections in room (**provisional name**) |
| `protocol_error` | S→C (and **may** C→S if ever needed) | **`error`** object per **E1** |

**Provisional required keys (non-final):**

- **`attach`:** `type`, `protocol_major`, `protocol_minor`, `ticket` (string).
- **`attach_ack`:** `type`, `room_id` (canonical string), `max_frame_bytes`, `max_message_bytes`, `server_protocol_minor` (hub’s supported minor cap for this session—optional if fixed elsewhere).
- **`publish`:** `type`, `client_message_id` (UUID string), `text` (UTF-8 chat body; field name **TBD**).
- **`publish_ack`:** `type`, `client_message_id`, `seq` (decimal string).
- **`message_delivered`:** `type`, `seq`, `client_message_id`, `text`; optional **`sender_endpoint_id`** (or equivalent) **TBD**.
- **`protocol_error`:** `type`, `error` `{ code, message, details? }`.
