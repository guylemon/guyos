# Publish, Deduplication, Ordering, and Fan-Out Logic (ADR 0003, D2, M1, R3, O1, “Publisher vs observers”)

The implementations delivered for previous tasks have established the framing layer, the complete set of normative message shapes with P1/N2/T1/K1/E1 enforcement, and the attach negotiation plus session state machine (`GuyosWireSession` / `handle_client_message`). No publish-specific logic, deduplication, sequencing, or fan-out distinction has yet been introduced. Consequently, the following discrete work items remain to satisfy this task exactly as defined.

> IMPORTANT: No concrete application wiring or multi-connection hub orchestration is required at this stage. The work should stay within the domain to support future wiring and orchestration.

**1. Domain publish-handler implementation (ADR 0003, D2, M1, R3, O1)**  
Develop pure domain functions (or a dedicated `handle_publish` operation invoked from the `Attached` arm of `handle_client_message`) that:  
- Validate the inbound `Publish` payload’s `client_message_id` as a canonical RFC 4122/9562 UUID string (M1).  
- Perform room-scoped deduplication keyed on the combination of `room_id` (derived at attach time) and `client_message_id` (D2).  
- Assign a monotonic per-room `u64` sequence number on first acceptance (O1).  
- Return an idempotent acknowledgement carrying the original sequence number when a duplicate `client_message_id` is presented for the same room, without advancing the sequence or generating new fan-out (R3).

**2. Publisher-versus-observer distinction as domain behavior (ADR 0003, “Publisher vs observers”)**  
Define the domain-level outcome of a successful publish such that:  
- The publishing connection receives a `PublishAck` containing the assigned (or original) `seq`, echoed `client_message_id`, and optional `server_timestamp`.  
- All other connections attached to the same room receive a distinct `ChatMessage` carrying the same `seq`, `client_message_id`, `text`, and optional `sender_endpoint_id` / `server_timestamp`.  
- On idempotent retry, only the `PublishAck` is produced; no `ChatMessage` is generated for observers.

**3. Integration into the existing session dispatch**  
Extend the `Attached` match arm inside `handle_client_message` (or route through the new publish handler) so that a well-formed `Publish` yields the unified outcome described under **Wire layer decisions** below (not a bare `Reply` carrying only `PublishAck`). All other post-attach messages continue to produce `NoReply` (or future publish-related outcomes) while preserving the single-attach invariant already implemented in task 5.0.

**4. Domain test coverage**  
Add unit tests that exercise:  
- First-time publish producing a new monotonic sequence and both `PublishAck` (publisher) and `ChatMessage` (observers).  
- Duplicate `client_message_id` within the same room returning the identical sequence number with no additional fan-out.  
- Sequence numbers remaining strictly increasing per room across multiple distinct publishes.  
- Canonical-UUID enforcement and oversized-text rejection on inbound publish (leveraging existing codec helpers).  
All tests must execute with in-memory room state only, keeping the logic strictly inside the domain layer.

**5. State and trait hygiene**  
Introduce any necessary per-room sequence counter and deduplication set management as domain abstractions (pure functions or a lightweight trait analogous to `AttachTicketVerifier`) so that concrete hub adapters may later supply persistent storage without altering the publish semantics. Ensure the new logic re-uses the existing `AttachAck`-derived `room_id`, `GuyosWireV1Message` variants, and `U31_MAX` / `MAX_MESSAGE_BYTES_ADR_V1` constants.

---

## Wire layer decisions (implementation guide)

### Extended `GuyosWireInboundOutcome` (single enum)

Keep **one** extended `GuyosWireInboundOutcome` rather than introducing a parallel `GuyosWirePublishOutcome`.

Rationale:

- Minimises the public API surface for the wire session layer.
- Allows `handle_client_message` to remain the sole entry point, returning a consistent `(GuyosWireSession, GuyosWireInboundOutcome)` pair.
- Makes future hub integration straightforward: the adapter matches the outcome variant and performs the appropriate send(s).
- Avoids an extra mapping step that a parallel publish-specific type would require.

Add a **`PublishHandled`** variant with **named fields** as specified under **[Specifier details for implementers](#specifier-details-for-implementers)**:

- **`publisher_ack`**: **`PublishAck`** for the publishing connection (assigned or echoed **`seq`**, echoed **`client_message_id`**, optional **`server_timestamp`**).
- **`observer_chat`**: **`Option<WireChatMessage>`** — fan-out payload for **other** connections in the room. **`Some`** only on first acceptance; **`None`** on idempotent retry (R3): no second fan-out; the publisher still receives **`publisher_ack`** with the original **`seq`**.

Document on the variant (normative contract for hub adapters):

- **Publisher vs observers:** the publisher does not receive `chat_message` for its own line; observers do not receive a second copy via `publish_ack`. Quote or paraphrase the normative **Publisher vs observers** bullets from [ADR 0003](../adr/0003-wire-protocol-and-compatibility.md).
- **R3 / D2:** idempotent retry returns the same `seq` without advancing sequence or emitting observer fan-out; deduplication scope is the room key.

Until hub wiring exists, domain tests assert both the ack and the optional observer message (or `None` on duplicate) using in-memory state only.

### Session shape: `Attached { room_id }`

Upgrade `GuyosWireSession::Attached` from a unit variant to a struct variant carrying **`room_id: String`** (opaque room key, byte-for-byte equality with `attach_ack.room_id`). This is the minimal extension needed for room-scoped deduplication (D2) inside the publish path. It does not change attach negotiation or the single-attach-per-stream rule already delivered in task 5.0.

Transition: on successful `accept_attach`, move to `Attached { room_id: ack.room_id.clone() }` (or equivalent).

### Room publish ledger (Question 2)

Use **both** a concrete **`RoomPublishState`** type and an optional **`RoomPublishLedger`** trait bound.

**Concrete `RoomPublishState`**

- Provides an immediately usable in-memory implementation for unit tests and for exercising dispatch logic without persistence.
- Keeps today’s surface small; avoids requiring a trait before any storage backend exists.
- Typical internal layout: structures keyed by **`room_id`** that hold a monotonic **`seq`** counter plus records of accepted **`client_message_id`** values for R3 (exact internal representation is implementation-defined; composite `(room_id, client_message_id)` indexing is one acceptable approach).

**Trait `RoomPublishLedger`**

- Parallel to **`AttachTicketVerifier`**: hub adapters inject **`&mut dyn RoomPublishLedger`** so one shard per active **`room_id`** can be shared across connections.
- See **[Specifier details for implementers](#specifier-details-for-implementers)** for the normative **`PublishDecision`** shape and the **`accept_or_replay`** signature and error-handling stance.

**Normative alignment**

- Matches ADR 0003 **Implementation freedom** (daemon chooses storage and scheduling) while the domain keeps publish semantics (D2, M1, R3, O1, Publisher vs observers).

**Hub guarantee**

- Document that the hub **must** use exactly **one** ledger instance (or equivalent shard) per active **`room_id`** for all streams attached to that room.

**`handle_client_message` signature**

Extend the existing task 5.0 entry point with a ledger parameter (same ownership style as today: **`session` passed by value**, returning an updated **`GuyosWireSession`**):

```rust
pub fn handle_client_message(
    session: GuyosWireSession,
    msg: &GuyosWireV1Message,
    policy: &GuyosWireAttachPolicy,
    verifier: &dyn AttachTicketVerifier,
    ledger: &mut dyn RoomPublishLedger,
) -> (GuyosWireSession, GuyosWireInboundOutcome)
```

In the **`Attached { room_id }`** arm, route **`Publish`** to **`accept_or_replay`** (or equivalent); map **`PublishDecision`** to **`PublishHandled`** so the publisher always receives **`PublishAck`** and **`WireChatMessage`** is omitted when fan-out is suppressed.

**Concrete type + trait:** Implement **`RoomPublishLedger`** for **`RoomPublishState`** in tests and for simple in-process hubs; production adapters may substitute a different **`RoomPublishLedger`** implementation without changing domain semantics.

### TS1, S2, and task 6.0 scope (Question 3)

**`server_timestamp` (TS1)**

- Supplied by the **hub** at the **instant of acceptance** (same logical step as assigning **`seq`** and applying dedup).
- For a given **first** acceptance, **`publish_ack`** and the matching **`chat_message`** **must** carry the **identical** **`server_timestamp`** value.
- On **idempotent retry (R3)**, **`publish_ack`** **must echo** the **`server_timestamp`** from the **first** acceptance; **no new** timestamp is generated. Observer fan-out does not recur; observers are unaffected on replay.

**`sender_endpoint_id` (S2)**

- Appears **only** on **`chat_message`** fan-out to observers, **only** when the hub can attribute the source connection (**Appendix A**, **S2**).
- **Never** present on **`publish_ack`**. Optional on **`chat_message`**.

**Task 6.0 (domain only)**

- Leave **`server_timestamp`** and **`sender_endpoint_id`** as **`None`** (omit on the wire where the codec treats optional fields as absent) inside domain publish handling.
- Introduce hub-supplied injection for TS1/S2 in a **later** phase when a concrete hub adapter is wired. Until then, unit tests validate **`seq`**, dedup, and fan-out shapes without timestamps or endpoint attribution.

**Later hub phase (normative target)**

- The ledger (or hub boundary) will need to retain the acceptance **`server_timestamp`** per accepted **`(room_id, client_message_id)`** so R3 replays can populate **`PublishAck.server_timestamp`** without assigning a new instant.

### Per-room sequence origin (Question 4)

The **first** accepted publish in a room (**O1**, **N2**) receives **`seq == 0`**. Subsequent distinct accepts assign **`1`, `2`, …** monotonically. Duplicate **`client_message_id`** (R3) echoes the **`seq`** from the original acceptance without advancing the counter.

---

## Specifier details for implementers

Concrete names and shapes below are **normative for task 6.0** so an implementer (or agent) does not need to infer structure from publisher-vs-observer prose alone.

### `GuyosWireInboundOutcome::PublishHandled`

```rust
PublishHandled {
    /// Always sent to the publishing connection only (ADR "Publisher vs observers").
    publisher_ack: PublishAck,
    /// Fan-out payload for observers only when `Some`; hub must not send to publisher.
    observer_chat: Option<WireChatMessage>,
}
```

Semantics:

| `PublishDecision` from ledger | `publisher_ack` | `observer_chat` |
| ----------------------------- | --------------- | ---------------- |
| **`FirstAcceptance { seq }`** | **`PublishAck`** with that **`seq`**, echoed **`client_message_id`**, task 6.0 **`server_timestamp: None`** | **`Some(WireChatMessage)`** with same **`seq`**, **`client_message_id`**, **`text`** from inbound **`Publish`**; **`server_timestamp`** / **`sender_endpoint_id`** **`None`** (Question 3) |
| **`IdempotentReplay { seq }`** | Same, echo **`seq`** (R3) | **`None`** — no observer fan-out |

### `PublishDecision` (ledger → dispatch)

Enumerated **enum**; observer suppression does **not** need a separate boolean (**replay** implies no fan-out):

```rust
pub enum PublishDecision {
    /// First acceptance of this `client_message_id` in this room: assign next `seq` (from 0 — Question 4).
    FirstAcceptance { seq: u64 },
    /// Idempotent retry (R3): echo stored `seq`; do not advance ordering cursor; no observer fan-out.
    IdempotentReplay { seq: u64 },
}
```

Both variants carry the **authoritative** **`seq`** for building **`PublishAck`** and, on first acceptance, **`WireChatMessage`**.

### `RoomPublishLedger::accept_or_replay`

**Parameters:** Only **`room_id`** and **`client_message_id`** participate in D2/R3/O1. **`text`** is **not** passed to the ledger; include **`text`** only when constructing **`WireChatMessage`** from the inbound **`Publish`** after **`FirstAcceptance`**. (If a client retries with the same **`client_message_id`** and different **`text`**, R3 still applies: same **`seq`**, no new observer payload; the first acceptance’s line remains authoritative for the room.)

**Return type:** **`PublishDecision`** — **infallible** for task 6.0 **`RoomPublishState`** (same spirit as a pure in-memory transition). This **differs** from **`AttachTicketVerifier`**, which returns **`Result`** because ticket verification can fail; ledger **`accept_or_replay`** for a **valid** UUID and known room is a deterministic state update. If a future persistent ledger needs I/O errors, handle them **outside** this trait (adapter wraps and maps to **`error`** frames) **or** extend with a fallible associated type in a later revision.

**Normative signature (task 6.0):**

```rust
pub trait RoomPublishLedger {
    fn accept_or_replay(
        &mut self,
        room_id: &str,
        client_message_id: &uuid::Uuid,
    ) -> PublishDecision;
}
```

(`uuid::Uuid` matches existing **`Publish`** / codec types.)

### Ledger ownership and multi-room sharding

The domain **only** receives **`ledger: &mut dyn RoomPublishLedger`**. [ADR 0003](../adr/0003-wire-protocol-and-compatibility.md) **Implementation freedom** allows either approach:

1. **Registry:** One **`RoomPublishState`** holds internal **`HashMap<String, …>`** (or equivalent) keyed by **`room_id`**; pass **`&mut RoomPublishLedger`** for that single struct into **`handle_client_message`** on every dispatch.
2. **Per-room shard:** Hub stores **`HashMap<room_id, Shard>`**; each **`Shard`** implements **`RoomPublishLedger`** for **one** room; dispatch selects **`&mut shard`** for the connection’s **`Attached.room_id`**.

The hub contract remains: **one logical ledger shard per active `room_id`, shared by all streams in that room.**

---

## Task 6.0 implementation outline (domain only)

1. Add the **`PublishHandled`** variant to **`GuyosWireInboundOutcome`** with fields **`publisher_ack`** and **`observer_chat`** exactly as in **[Specifier details for implementers](#specifier-details-for-implementers)**.
2. Implement pure domain **`handle_publish`** logic (dedicated function or inline in the `Attached` arm) that:
   - Validates the canonical UUID (M1), including reuse of existing decode validation where the publish has already been decoded to `GuyosWireV1Message::Publish`.
   - Delegates room-scoped deduplication and **`seq`** assignment to **`RoomPublishLedger::accept_or_replay`** (implementation backed by **`RoomPublishState`** in tests; see **Room publish ledger (Question 2)**).
   - Maps **`PublishDecision`** to **`PublishHandled`**: always emit **`PublishAck`**; include **`WireChatMessage`** only on first acceptance (O1, R3). Use **`None`** for **`server_timestamp`** and **`sender_endpoint_id`** per **TS1, S2, and task 6.0 scope (Question 3)**.
3. Extend **`handle_client_message`** with **`ledger: &mut dyn RoomPublishLedger`** and, in the **`Attached`** arm, invoke the publish logic so well-formed **`Publish`** messages return **`GuyosWireInboundOutcome::PublishHandled { … }`**.
4. Document the hub contract explicitly in the variant’s doc comment, citing ADR 0003 **Publisher vs observers**, **R3**, and **D2** as needed.
5. Add focused unit tests that assert both the publisher ack and the optional observer chat (or **`None`** on duplicate) using only in-memory values.
