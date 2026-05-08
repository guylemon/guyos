This task requires implementation of the attach negotiation rules, effective-minor computation, single-attach-per-stream invariant, and attach-acknowledgement construction as pure domain logic, plus validation of integer bounds and production of the exact error codes defined for unsupported majors and malformed attach payloads (ADR 0003, sections “Protocol versioning (V1, H2)”, “Attach (join / negotiation)”, “Attach acknowledgement (A1)”, and Appendix A).

The message shapes (`Attach`, `AttachAck`), JSON codec, and framing port from previous work, together with the reference ticket verifier, are already available. The following discrete work items remain:

- Develop domain-level attach handling: a **session dispatch** type (see **Planning decisions**) that owns cold-start vs attached branching, delegating successful **`Attach`** handling to a handshake path that accepts an `Attach` payload, a **`GuyosWireAttachPolicy`** (or equivalent) carrying **per-major hub ceilings**, advertised **`attach_ack` limits**, and **supported-major** logic (see **Planning decisions**), and an injected **ticket verifier** (see **Planning decisions**), runs the attach through the **shared attach preamble validator**, then applies major-policy, computes the effective negotiated minor as `min(client_minor, hub_minor)`, and either constructs a correctly populated `AttachAck` (echoing `room_id` from successful ticket verification, the negotiated minor, and the session limits) or returns the precise error variant required by the specification.

- Integrate the completed ticket verification logic so that a valid `attach.ticket` yields the canonical `room_id` string used for routing; map ticket-specific failures (`ticket_decode_failed`, `invalid_ticket`) and version-negotiation failures (`protocol_major_unsupported`) to the corresponding portable error codes while preserving the validation ordering mandated by ADR 0003 (semantic checks before ticket decode).

- Enforce the single-successful-attach-per-stream invariant at the domain level: any `attach` received after a prior successful `attach_ack` on the same session must be rejected with `invalid_attach` (via **session attach dispatch** — **Planning decisions**).

- Implement cold-start handling for messages arriving before the first successful `attach_ack`: `detach` and `keepalive` are treated as no-ops; all other client-to-hub messages (including a second `attach`) produce the appropriate error (`invalid_attach` or `attach_required` per the closed set in Appendix A), centralized in the same **session attach dispatch** (**Planning decisions**).

- Add isolated domain tests that exercise the negotiation arithmetic, bound violations, unsupported-major path, malformed-attach shapes, ticket-failure mapping, single-attach rejection, and pre-attach message handling, using only the framing port and JSON codec already delivered.

## Planning decisions

**Implementation guidance:** Record substantive design choices in this section as they are agreed; treat it as the checklist for implementers and automation so behavior stays aligned with ADR 0003 without re-deriving forks from chat history. **Normative message codes, validation order, and cold-start rules remain authoritative in [`docs/adr/0003-wire-protocol-and-compatibility.md`](../adr/0003-wire-protocol-and-compatibility.md)** — do not mirror Appendix A as a second checklist here.

### Shared attach preamble validator

**Decision:** Implement attach **semantic preamble** validation — the ADR 0003 phase that runs **after** UTF-8 JSON parses as an object with `type = attach` and **before** unsupported-major rejection and **before** ticket decode: required keys, field types, and u31 bounds for attach protocol integers (failures → `invalid_attach`). Implement that preamble as **shared domain logic**, invoked from both the JSON decode path for `attach` and the attach handshake coordinator. The coordinator then applies major policy (`protocol_major_unsupported`), then ticket verification, per ADR ordering.

**Rationale:** Keeps Appendix A ordering and portable codes aligned whether `Attach` is produced by `decode_guyos_wire_v1_json`, constructed in unit tests, or introduced through a future entrypoint; avoids copying normative rules into two places.

**Implementation consequence:** The codec must delegate to this shared preamble (or share one underlying helper) rather than owning those checks alone; the coordinator calls the same preamble before major-policy, ticket verification, and ack construction. Hand-built or partially validated `Attach` values are **not** trusted until they pass the preamble.

### Hub attach policy (`GuyosWireAttachPolicy`)

**Decision:** Model hub-side negotiation and advertised session limits as one domain type (working name **`GuyosWireAttachPolicy`**, or **`GuyosWireHubAttachConfig`** if the name becomes overloaded) passed into attach handling. It includes at minimum:

- **Per-`protocol_major` hub minor ceiling `H`:** expose **`hub_protocol_minor_ceiling(major: u32) -> Option<u32>`** (or equivalent) returning **`Some(H)`** when the hub implements that major and **`None`** when it does not. Use **`H`** from **`Some`** with **`min(C, H)`** for **`attach_ack.server_protocol_minor`**. A **v1-only** hub supplies **`Some(H)`** only for major **`1`** (or whichever major it implements) and **`None`** for all others.
- **Supported-major membership** must stay **consistent** with **`hub_protocol_minor_ceiling`**: if **`supports_major(major)`** exists, it matches **`hub_protocol_minor_ceiling(major).is_some()`**. Reject unsupported majors with **`protocol_major_unsupported`** only **after** attach preamble validation (**ADR 0003**).

**ADR alignment:** ADR 0003 defines **`H`** as the largest **`protocol_minor`** the hub supports **for that `protocol_major`** (not a single global ceiling across majors). It does **not** require hubs to implement multiple majors; a hub may support **one** major and reject others with **`protocol_major_unsupported`**.

- **`max_frame_bytes`** and **`max_message_bytes`** for **`attach_ack`** (**ADR 0003** — limits echoed to the client).
- Optional **`keepalive_interval_seconds`** (`Option<u32>`) for the hint field on **`attach_ack`**.

Construct it at hub bootstrap (or per listener) from deployment configuration; validate field ranges per ADR (e.g. u31 where applicable); tests construct alternate instances without recompiling.

**Rationale:** Matches normative **`min(C, H)`** wording per major; avoids refactoring when a second major ships with a different ceiling.

**Implementation consequence:** The attach handshake resolves **`H`** as **`hub_protocol_minor_ceiling(attach.protocol_major)`**; **`None`** yields **`protocol_major_unsupported`** (after preamble). **`AttachAck`** is built from policy + canonical **`room_id`** + **`attach.protocol_major`** + **`server_protocol_minor` = min(C, H)**. No separate limits-only type unless a future deployment needs independent composition.

### Injected ticket verifier (`AttachTicketVerifier`)

**Decision:** Provide ticket verification to the attach handshake via a narrow injected abstraction (working name **`AttachTicketVerifier`** — e.g. a small trait or `Fn(&str) -> Result<String, ReferenceTicketError>`). The coordinator maps **`ReferenceTicketError`** to portable codes (`ticket_decode_failed` / `invalid_ticket`) and preserves ADR ordering relative to preamble and major checks; it does **not** hard-code **`k_mac`**, clock, or profile selection. Hub adapters wire the verifier to **`reference_ticket_v1::decode_reference_v1`** (or successors) with configured key material and **`now_unix`**.

**Rationale:** Keeps cryptography and wall-clock policy at the deployment boundary; pure negotiation tests use stubs or deterministic verifiers without threading secrets through domain APIs.

**Implementation consequence:** Integration tests that need real ticket bytes compose **`encode_reference_v1`** + **`decode_reference_v1`** in the adapter layer or pass a closure that calls **`decode_reference_v1`**.

### Session attach phase and inbound dispatch (`GuyosWireSession`)

**Decision:** Implement cold-start behavior, **`attach_required`** versus no-op **`detach`** / **`keepalive`**, a **second `attach`** after **`attach_ack`** (**`invalid_attach`**), and related Appendix A branching with **one domain session state type** (working name **`GuyosWireSession`** or **`WireSessionAttachState`**) that records whether a successful attach has completed (e.g. **`AwaitingAttach`** vs **`Attached`**) and exposes a **single entrypoint** for decoded client→hub **`GuyosWireV1Message`** values — for example **`handle_client_message(state, msg, policy, verifier) -> (UpdatedState, GuyosWireInboundOutcome)`** (see **Inbound dispatch outcome**) — instead of splitting phase logic across adapter code paths.

**Rationale:** Concentrates normative pre-attach vs post-attach ordering and error codes; transports remain thin.

**Implementation consequence:** The attach handshake (preamble, **`GuyosWireAttachPolicy`**, **`AttachTicketVerifier`**, **`AttachAck`** construction) runs inside this dispatch when an **`Attach`** is handled in the awaiting phase; other inbound kinds follow ADR cold-start rules from the same implementation. Prefer **pure** state transitions (no I/O inside domain types). The adapter interprets **`GuyosWireInboundOutcome`**: encode and frame **`Reply`**, or skip outbound write on **`NoReply`**.

### Inbound dispatch outcome (`GuyosWireInboundOutcome`)

**Decision:** Model the result of **`handle_client_message`** as an explicit **`enum`** (working name **`GuyosWireInboundOutcome`**) rather than **`Option<GuyosWireV1Message>`** alone. At minimum include **`NoReply`** (state updated; no outbound application frame — e.g. cold-start **`detach`** / **`keepalive`** no-ops) and **`Reply(GuyosWireV1Message)`** for **`AttachAck`** or **`Error`** (and extend later for **`publish_ack`**, fan-out, etc.). **`NoReply`** must remain distinguishable from “forgot to produce **`attach_ack`**.”

**Rationale:** Makes hub responses explicit for tests and transports; avoids conflating intentional silence with omission bugs.

**Implementation consequence:** Wire framing and **`encode_guyos_wire_v1_json`** run **outside** domain code on **`Reply`** payloads only; **`NoReply`** yields no encoded body for that turn.

### Wire error envelope assembly (`ErrorEnvelope`)

**Decision:** Construct **`GuyosWireV1Message::Error(ErrorEnvelope)`** inside the **domain** when returning **`GuyosWireInboundOutcome::Reply`** on failure paths (attach preamble, major policy, ticket verification, session-phase rules). Provide small helpers or match arms keyed by ADR Appendix A **`code`** strings; use **stable, minimal English `message`** text per **`code`** (wording is implementation-defined under the ADR). Omit **`details`** unless the spec or a specific diagnostic requires structured JSON **`details`**.

**Rationale:** Domain tests can **`encode_guyos_wire_v1_json`** complete hub frames without adapter-specific error mapping; transports only UTF-8 JSON-encode and frame **`Reply`** bodies.

**Implementation consequence:** Adapters **do not** map portable codes to **`ErrorEnvelope`** at the edge; they treat **`Reply`** like any outbound **`GuyosWireV1Message`**.

## Implementation recipe (compact)

Use this as an ordered checklist for implementers and agents. **Normative codes and ordering:** [`docs/adr/0003-wire-protocol-and-compatibility.md`](../adr/0003-wire-protocol-and-compatibility.md). **Architecture:** **Planning decisions** above.

1. **New domain module** — Add **`guyos_core/src/domain/guyos_wire_attach.rs`** (split into additional files only if the module exceeds maintainable size). Declare **`pub(crate) mod guyos_wire_attach;`** in **`guyos_core/src/domain/mod.rs`**.

2. **Policy and ports-injection types** — Implement **`GuyosWireAttachPolicy`** (per-major **`hub_protocol_minor_ceiling`**, **`max_frame_bytes`**, **`max_message_bytes`**, **`keepalive_interval_seconds`**). Define **`AttachTicketVerifier`** (trait or **`Fn`**) returning **`Result<String, ReferenceTicketError>`**.

3. **Preamble** — Implement **`validate_attach_preamble(&Attach) -> Result<(), …>`** encoding ADR semantic attach validation (**u31**, required shape). Map failures to the same pathway the codec uses for **`invalid_attach`** (see step 7).

4. **Handshake core** — Implement **`accept_attach(&Attach, &GuyosWireAttachPolicy, &dyn AttachTicketVerifier) -> …`** with fixed ordering: preamble → **`hub_protocol_minor_ceiling`** / **`protocol_major_unsupported`** → **`AttachTicketVerifier`** → build **`AttachAck`** (**`min(C, H)`**, echo **`protocol_major`**, limits from policy, **`room_id`** from verifier). Return structured success/failure suitable for **`GuyosWireInboundOutcome::Reply`**.

5. **Wire errors** — Add small **`GuyosWireV1Message::Error`** builders (one per attach-path **`code`** used) with stable **`message`** strings; keep **`ErrorEnvelope`** construction here per **Wire error envelope assembly**.

6. **Session dispatch** — Implement **`GuyosWireSession`** (phase: awaiting attach vs attached) and **`GuyosWireInboundOutcome`** (**`NoReply`** / **`Reply`**). Implement **`handle_client_message`** to route **`GuyosWireV1Message`** variants: cold-start **`detach`**/**`keepalive`** → **`NoReply`**; pre-attach **`publish`** / etc. → **`attach_required`** or **`invalid_attach`** per ADR; duplicate **`attach`** after success → **`invalid_attach`**; awaiting **`Attach`** → handshake from step 4.

7. **Codec integration** — Refactor **`decode_attach`** in **`guyos_wire_v1.rs`** to call the shared preamble validator so JSON decode and hand-built **`Attach`** share semantics. Leave **`malformed_json`** / **`unknown_message_type`** in the codec only (bytes not yet a valid **`Attach`**).

8. **Tests** — Add **`#[cfg(test)]`** tables (or **`guyos_core/tests/`** integration tests) covering negotiation **`min(C, H)`**, **`protocol_major_unsupported`**, preamble failures, ticket failure mapping, duplicate attach, and cold-start branches using **`FakeGuyosWireV1Session`** / **`encode`/`decode`** from existing patterns in **`guyos_wire_v1.rs`** tests.

9. **Adapter wiring (later task)** — **`GuyosWireV1Session`** implementations call **`decode_guyos_wire_v1_json`**, then **`handle_client_message`**, then **`encode_guyos_wire_v1_json`** + framing on **`Reply`** only; inject policy and verifier at hub bootstrap.

10. **Verification** — Walk [**ADR 0003** Appendix A](../adr/0003-wire-protocol-and-compatibility.md) attach-related rows against the implementation; do not duplicate them in this doc.

Completion of these items, followed by verification that every normative attach/attach-ack exchange and error path matches ADR 0003 Appendix A, will fully discharge task 5.0 while preserving the legacy protocol and remaining strictly inside the domain layer.
