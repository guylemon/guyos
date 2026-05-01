# guyos_core spike refactor task list (hexagonal architecture)

Goal: refactor (or re-implement) the `guyos_core` spike into a maintainable, testable, multi-frontend Rust library with a stable, Swift-friendly UniFFI surface.

**Scope**: The intent of this refactoring process is to maintain the spike functionality using a custom wire protocol within a well-structured software architecture that allows for testing and future extensibility. Features mentioned by the [product overview](docs/overview.md) that are not currently in the spike are out of the scope of this document. Enhancements or non-functional, cross-cutting concerns related to performance, security, logging, persistence, and deployment boundaries are out of scope.

**Product alignment (informative):** [`docs/overview.md`](docs/overview.md) describes the hub as the integration core (e.g. LLM and, over time, other tools), a **thin client API** exported to **iOS via UniFFI**, and **multiple clients at once** (e.g. phone and CLI) sharing room/session semantics **governed by the wire contract**. Normative rules for that relay plane and packaging live in the ADRs below—not in the overview.

**Normative decisions:** Locked rules and rationale live in [Architecture Decision Records](docs/adr/) linked below. This file is the **living execution checklist** (what to do, suggested order, done-when criteria). **If an adopted ADR ever disagrees with this checklist, follow the ADR** and update the checklist to match.

---

## Guiding decisions

- **[ADR 0001 — Repository and package strategy](docs/adr/0001-repository-and-package-strategy.md)** (rewrite vs evolve-in-place, branch/packaging, spike disposition)
- **[ADR 0002 — Hexagonal boundaries and ownership](docs/adr/0002-hexagonal-boundaries-and-ownership.md)** (layers, dependency rules, lifecycle/supervision)
- **[ADR 0003 — Wire protocol and compatibility](docs/adr/0003-wire-protocol-and-compatibility.md)** (messages, tickets, versioning policy)
- **[ADR 0004 — Ticket profiles and reference profile v1](docs/adr/0004-ticket-profiles-and-reference-profile.md)** (ticket profile framework, registry, reference HMAC profile, golden vectors)

---

## Implementation ordering (hybrid)

**Spine — [ADR 0002](docs/adr/0002-hexagonal-boundaries-and-ownership.md):** Keep the phased checklist in **layer order**—composition and module layout → pure domain → port traits → application services → concrete adapters—so dependency direction and test seams stay obvious.

**Parallel relay-plane track — [ADR 0003](docs/adr/0003-wire-protocol-and-compatibility.md) / [ADR 0004](docs/adr/0004-ticket-profiles-and-reference-profile.md):** As soon as domain and ports reflect the same concepts as the wire (`attach`, opaque ticket bytes, `room_id`, `seq`, `client_message_id`, closed v1 error codes), implement **F1 framing, JSON DTOs, and ticket profile decoding** plus **fixture-backed conformance tests**. Do this **before** investing in full FFI binding polish or treating the transport stack as the only proof of correctness. Transport adapters stay **thin**: they move bytes on the bound in [ADR 0003](docs/adr/0003-wire-protocol-and-compatibility.md) (**Q1**) and call shared encode/decode logic.

**Why:** [`docs/overview.md`](docs/overview.md) notes that accepted ADRs today concentrate on the **client ↔ hub relay plane** (QUIC, framing, tickets); broader hub capabilities can evolve and gain ADRs when interoperability must be pinned down.

**Protocol sections below** (“Protocol, compatibility, and security hardening”) are a **compatibility roll-up**—not a signal to defer all wire/ticket work until the end. Prefer completing the relay-plane library surface (DTOs, tickets, golden tests) **alongside** early application work; see Phase 4 order and [Suggested implementation order](#suggested-implementation-order-pragmatic).

---

## Phase 0 — Repository & module structure (composition first)

- **Create the hexagonal module layout**
  - Organize the crate by layers and adapters as described in [ADR 0002](docs/adr/0002-hexagonal-boundaries-and-ownership.md): domain, ports (inbound/outbound), application services, inbound and outbound adapters (FFI, CLI, daemon entrypoints, transport, LLM), infrastructure/composition, and a single cross-cutting error surface. Exact directory names and file splits are a project convention—not prescribed here.
  - Done when: the crate builds, and the publicly supported API is exposed through an intentional, documented module boundary (reviewable without relying on a particular file name).
- **Define the “composition root”**
  - Done when: there is exactly one documented wiring/composition site that binds concrete adapters to application services; all inbound entrypoints and the CLI resolve dependencies through it (verifiable by module structure or an architecture/check test).
- **Thin process entrypoints**
  - Done when: binary entrypoints contain only bootstrap concerns (arguments, signals, wiring selection, calls into the application API); automated or manual review shows business rules, wire handling, and LLM orchestration are not entrypoint-only (e.g. covered by application/domain tests or forbidden-dep checks).

---

## Phase 1 — Domain layer (pure, synchronous, testable)

- **Extract domain entities/value objects**
  - Cover the wire-adjacent concepts the ADRs require (`room_id`, opaque tickets, sequencing, client message identity, protocol version, etc.); add product-facing value objects (e.g. display names) only where the domain needs them—names and fields follow ADR semantics, not the spike’s old types.
  - Done when: domain types have no dependencies on transport, FFI, HTTP client, CLI, or async-runtime crates; unit tests for domain types run without starting an async runtime.
- **Add invariants via constructors**
  - Add validations such as:
    - ADR 0003 message length cap (`max_message_bytes`)
    - required fields
    - canonical UUID `client_message_id`
    - `seq` as lossless u64 semantics where represented internally
  - Done when: invalid inputs cannot be constructed except through an explicit test-only or `unsafe` escape hatch; tests assert rejection for representative invalid cases.
- **Separate “domain types” from “wire/DTO types” (if needed)**
  - Keep wire serialization and DTO shapes in adapter or codec modules; domain stays encoding-agnostic unless you document a rare exception.
  - Done when: domain types do not imply a wire encoding; any retained serialization in domain is justified in code review and covered by tests showing domain remains independent of adapter DTOs.
- **Domain error model**
  - Done when: domain failures are represented as a structured, typed error taxonomy (no free-form string variants as the primary model); constructors and invariant violations surface those types; unit tests cover mapping from invalid inputs to expected variants.

---

## Phase 2 — Ports (contracts / traits)

- **Define outbound port: client network / transport contract**
  - Must cover client transport use cases: connect to daemon, attach with opaque ticket and protocol version, publish, receive `publish_ack` / `chat_message` / `error` events, detach/keepalive where needed, and expose connection diagnostics.
  - Done when: application logic compiles against the network port contract and passes unit tests using an in-memory fake or stub for that port (no real network stack).
- **Define the single canonical inbound port trait (application facade)**
  - All inbound adapters (UniFFI, CLI, daemon entrypoint) delegate to this one trait; the facade may internally route to hub or chat use cases. No additional public inbound traits without an explicit future ADR ([ADR 0002](docs/adr/0002-hexagonal-boundaries-and-ownership.md)).
  - Done when: review or lightweight adapter tests show each inbound adapter delegates behavioral work to this facade only; adapters contain mapping and host- or transport-appropriate glue; orchestration and domain rules stay in inner layers; wire/ticket semantics stay anchored in ADRs and shared codecs.
- **Define outbound port: LLM / model client contract**
  - Must cover: non-stream and/or stream reply generation, plus configuration inputs (model, base url, etc.).
  - Done when: LLM relay logic is covered by unit tests using a fake or stub for the LLM port, with no outbound HTTP in those tests.

---

## Phase 3 — Application layer (use cases, orchestration, lifecycle)

- **Implement client chat application service (use cases)**
  - Own client-side connect / attach / send / receive orchestration; method names and internals are up to the implementation as long as ports and ADRs are satisfied.
  - Done when: the client chat application service depends only on domain types and ports; unit tests with fakes and optional dependency/constraints checks show no direct coupling to transport, FFI, HTTP client, or CLI crates.
- **Implement daemon hub service**
  - Own room sessions, per-room `seq` assignment, fan-out, deduplication by `(room_id, client_message_id)`, publisher-only `publish_ack`, and no `chat_message` echo to the publisher.
  - Done when: hub behavior matches [ADR 0003](docs/adr/0003-wire-protocol-and-compatibility.md) and is verified through port fakes without a real network transport.
- **Multi-client, same room (product checkpoint)**
  - Matches [`docs/overview.md`](docs/overview.md): **multiple clients** (e.g. iOS via UniFFI and CLI) may attach to the **same** logical room; ADR 0003 governs what they observe on the wire.
  - Done when: unit tests with fakes cover at least **two** logical client connections in one `room_id` (publish/ack/fan-out/dedup paths); separate stack integration tests extend the same scenarios to the real network implementation.
- **Implement a supervised task model**
  - Own async or background work in a way that matches [ADR 0002](docs/adr/0002-hexagonal-boundaries-and-ownership.md) lifecycle expectations (single owner, explicit join/cancel path); choice of primitives is team discretion.
  - Done when: every concurrent unit of work started by the service is owned (registration or parent handle is explicit); shutdown or drop can await completion without orphaned tasks (verified by tests or deterministic teardown hooks).
- **Add explicit shutdown**
  - Expose a clear shutdown path and destructor semantics so hosted clients can release resources; exact API shape follows your async/runtime conventions.
  - Done when: interactive or process termination and releasing the FFI-hosted client object both end without leaked tasks or runaway background work (covered by shutdown tests and/or lifecycle checks).
- **Fix coarse locking and contention**
  - Reduce contention so unrelated operations are not serialized behind one global lock; structure is intentionally unspecified—optimize using guidance in [ADR 0002](docs/adr/0002-hexagonal-boundaries-and-ownership.md) and profiling.
  - Done when: receiving messages does not require serializing unrelated API calls under a single lock.
- **Make “connection state & diagnostics” queryable**
  - Add APIs like connection state, attached room id, negotiated protocol minor, keepalive hint, and transport endpoint diagnostics where available.
  - Done when: FFI consumers and the CLI can obtain connection status and diagnostics through typed APIs or structured data, without scraping logs.

---

## Phase 4 — Outbound adapters and protocol implementations

Do **ADR 0003** framing/DTOs and **ADR 0004** tickets **before** treating the QUIC transport stack as the main milestone: shared encode/decode should be unit- and fixture-tested independently of the network stack ([Implementation ordering (hybrid)](#implementation-ordering-hybrid)). Transport adapters stay thin: they move bytes on the wire per [ADR 0003](docs/adr/0003-wire-protocol-and-compatibility.md) (**Q1**) and delegate encoding/decoding to shared codec logic.

### ADR 0003 — Wire codecs (F1 + JSON)

- **Implement ADR 0003 framing and JSON DTOs**
  - Implement F1 framing and JSON DTO behavior per [ADR 0003](docs/adr/0003-wire-protocol-and-compatibility.md) (including **P1** dispatch and tolerance rules); field layouts and byte-level details are defined only in the ADR and fixtures—not duplicated here.
  - Done when: malformed JSON, frame limits, unknown type, attach-required, invalid UUID, and message size failures map to the closed v1 error codes required by [ADR 0003](docs/adr/0003-wire-protocol-and-compatibility.md); conformance fixtures or tests cover each failure class.
- **Add protocol versioning to attach flow**
  - Implement **H2** negotiation per ADR 0003; keep ticket format under **TV2** (no separate ticket-format version on the wire).
  - Done when: attach selects `protocol_major` / `protocol_minor` per [ADR 0003](docs/adr/0003-wire-protocol-and-compatibility.md); tests or fixtures prove the negotiated values and reject unsupported majors as specified—without introducing a per-message wire version or a separate ticket-format version field.

### ADR 0004 — Ticket profiles

- **Implement ticket profile support**
  - Satisfy **M1**, the reference profile, constant-time verification, and room derivation as specified in [ADR 0004](docs/adr/0004-ticket-profiles-and-reference-profile.md); profile ids, registry rows, and cryptographic details are authoritative in that ADR and its fixtures.
  - Done when: profile registry validation, reference-profile golden vectors, and `ticket_decode_failed` vs `invalid_ticket` mapping pass tests against the normative examples in [ADR 0004](docs/adr/0004-ticket-profiles-and-reference-profile.md). See also [Protocol, compatibility, and security hardening](#protocol-compatibility-and-security-hardening).

### Transport adapter (wire stack)

- **Isolate transport stack from inner layers**
  - Keep QUIC (or successor) stack types and routing hooks in the outbound/inbound transport adapters only; wire codec types stay shared and transport-agnostic.
  - Done when: transport-stack types and bindings exist only in the outbound/inbound transport adapters; application and domain compile and test without referencing them; superseded protocol dependencies are absent from the build as required by [ADR 0001](docs/adr/0001-repository-and-package-strategy.md) (verifiable in the manifest and dependency graph).
- **Implement the outbound network port for the client transport adapter**
  - Done when: integration or contract tests can substitute a fake network implementation for the production adapter while exercising the same network port contract as the rest of the codebase.
- **Implement daemon transport inbound adapter**
  - Enforce transport and stream usage per [ADR 0003](docs/adr/0003-wire-protocol-and-compatibility.md) (**Q1**, ALPN, 0-RTT policy, single-stream framing)—exact socket API is dictated by the stack you choose under [ADR 0001](docs/adr/0001-repository-and-package-strategy.md).
  - Done when: the adapter’s outputs on the wire are frame- and field-level equivalent to the codec DTOs from [ADR 0003](docs/adr/0003-wire-protocol-and-compatibility.md); tests show decoded frames reach daemon application use cases and responses re-encode without leaking transport details upward.

### Outbound LLM adapter (HTTP, streaming)

- **Confine HTTP client and streaming parse to the outbound LLM adapter**
  - Done when: the application obtains LLM behavior only through the LLM port; HTTP client and streaming-parse code live in the outbound adapter and are not required for application-layer unit tests.
- **Replace or harden stream (e.g. SSE-style) parsing**
  - Meet the stream framing and field behavior of the upstream HTTP API you integrate with; parser choice and dependencies are team decisions—cover edge cases with tests.
  - Done when: streaming parse accepts common edge cases (e.g. CRLF, multiline `data:`) and malformed chunks produce defined, logged, non-crashing outcomes; regression tests lock the behavior without mandating a particular parser implementation.
- **Add client hardening**
  - timeouts
  - connection pooling config
  - optional retries/backoff
  - optional auth headers / credentials as required by deployment
  - Done when: configuration covers timeouts, pooling, retries/backoff, and optional credentials at levels appropriate for remote deployments—not only loopback defaults; defaults and limits are documented and covered by adapter tests where feasible.

---

## Phase 5 — Inbound adapters

### FFI binding (e.g. UniFFI)

- **Confine generated binding objects to the inbound FFI adapter**
  - Done when: generated FFI shims and async-export surface are confined to the inbound FFI adapter; review shows no generated binding artifacts or export directives in domain, application, or wire codec modules.
- **Keep a pull-based consumer API for inbound events**
  - Done when: FFI consumers can drive the client with a pull-style loop over received events without a callback-based API; behavior is covered by binding-layer tests or contract examples.
- **Eliminate LLM UI leakage**
  - Represent partial vs final model output in the type model, not with ad-hoc string conventions.
  - Done when: partial streaming is represented as a first-class event shape (distinct type or explicit completion/continued flags) at the FFI boundary; tests prove no UI-oriented string prefixing is required to distinguish partial vs final text.
- **Expose cancellation / responsiveness strategy**
  - If needed, add cancellation or shutdown that unblocks pull-based receive paths.
  - Done when: FFI consumers can stop listening or tear down the client without relying on process exit; waiters unblock deterministically (verified by cancellation or shutdown tests).

### CLI

- **Move all LLM relay ownership into the library/application**
  - Done when: the CLI entrypoint forwards configuration and renders streamed events only; session history, retry/abort policy, and LLM orchestration live in application services and are covered by library tests—not CLI-only code.
- **Keep CLI bootstrap shared with other entrypoints**
  - Done when: CLI, daemon, and FFI-driven processes share one documented bootstrap path (composition + config) with only the adapter set and configuration differing; smoke or architecture checks prove duplicate wiring sites are absent.

---

## Error handling & observability (cross-cutting)

- **Introduce a typed application error surface**
  - Keep rich error sources internally; stringify only at the FFI boundary.
  - Done when: top-level errors compose from underlying failures with preserved causal chains; mapping into stable FFI-facing error types is implemented in one layer and covered by mapping tests.
- **Centralize error mapping at the FFI boundary**
  - Done when: the FFI surface exposes a small, versionable error taxonomy aligned with product needs, while internal errors retain diagnostic context; golden or snapshot tests can be used to prevent accidental breaking renames.
- **Add structured logging for operations and lifecycle**
  - Done when: major lifecycle events (connect/attach/publish/detach/keepalive/send/receive/LLM start/stop) emit structured, queryable log records; verbosity can be turned up or down via configuration without recompiling.

---

## Protocol, compatibility, and security hardening

Roll-up of compatibility obligations from [ADR 0003](docs/adr/0003-wire-protocol-and-compatibility.md) and [ADR 0004](docs/adr/0004-ticket-profiles-and-reference-profile.md). Prefer implementing these **in the same timeframe** as [Phase 4 — ADR 0003 / ADR 0004 subsections](#phase-4--outbound-adapters-and-protocol-implementations), not only at project tail.

- **Add protocol versioning**
  - Implement ADR 0003 `protocol_major` / `protocol_minor` negotiation on `attach`; do not add per-message version fields or a separate ticket-format version field.
  - Done when: unsupported majors, minor downgrade to `min(C, H)`, and unknown-key tolerance are covered by tests or fixtures per [ADR 0003](docs/adr/0003-wire-protocol-and-compatibility.md).
- **Implement ticket profile versioning outside the wire protocol**
  - Follow ADR 0004: profile ids are registry-backed, exactly one active profile is selected per listener, and breaking ticket byte changes use a new profile id rather than an ADR 0003 major bump unless wire behavior changes.
  - Done when: invalid profile ids fail fast at startup or load time where applicable, and conformance tests exercise every normative registry row referenced by the deployment docs for [ADR 0004](docs/adr/0004-ticket-profiles-and-reference-profile.md).
- **Document deployment security boundary**
  - ADR 0003 leaves TLS identity, pinning, authorization, and threat modeling out of normative v1.
  - Done when: deployment docs explicitly state what protects daemon access and whether message authenticity beyond hub authorization is in or out of scope.
- **Add validation + limits**
  - ADR 0003 frame/message size caps
  - canonical `client_message_id` validation
  - rate limiting/spam controls (implementation policy, not wire contract)
  - Done when: oversize frames, oversize messages, malformed UUIDs, and hostile inputs are rejected at parse/validation boundaries with measurable early-exit behavior (fixtures or benchmarks optional); policy controls are documented when rate limiting is enabled.
- **Revisit serialization format**
  - ADR 0003 v1 is UTF-8 JSON over F1 frames; non-JSON payloads or breaking message changes require a future major version.
  - Done when: any future binary/bulk format is documented as a new protocol extension instead of changing v1 silently.

---

## Testing strategy (unit + integration)

- **Unit tests for application layer with faked ports**
  - Use hand-written fakes, lightweight stubs, or generated test doubles—pick what keeps tests readable.
  - Done when: client chat orchestration and daemon hub behavior—send/receive ordering, deduplication, publisher ack vs observer fan-out, and wire error mapping—are covered by unit tests using fakes, with no real network I/O in those tests.
- **Protocol conformance tests**
  - Golden frames for every ADR 0003 `type`, frame length handling, unknown-key tolerance, required-key validation, `seq` decimal-string parsing, and closed v1 error codes.
  - Done when: client and daemon implementations can each be validated against the shared reference corpus independently (no cross-binary coupling required for conformance).
- **Ticket profile tests**
  - Validate the normative ticket profile registry and golden vectors for the active profile(s) per [ADR 0004](docs/adr/0004-ticket-profiles-and-reference-profile.md) (including boundary expiry and bad-signature cases); registry file location follows that ADR / repo docs.
  - Done when: automated tests against reference inputs detect unintended changes in [ADR 0003](docs/adr/0003-wire-protocol-and-compatibility.md) / [ADR 0004](docs/adr/0004-ticket-profiles-and-reference-profile.md) compatibility.
- **Integration tests for real QUIC daemon/client transport**
  - Spin up a daemon and **at least two** clients in-process; attach them to the **same** room via tickets (mirrors [`docs/overview.md`](docs/overview.md): e.g. CLI + future iOS paths sharing hub semantics). Publish from one client; assert publisher `publish_ack` and observer fan-out per ADR 0003.
  - Done when: end-to-end attach/publish/receive succeeds reliably in continuous integration using the primary transport stack, with at least two concurrent clients in one logical room, without depending on legacy gossip-era components.
- **LLM adapter tests**
  - Mock server for SSE streaming and edge cases (CRLF, multiline, partial frames).
  - Done when: adapter tests against a controllable HTTP test double show stable handling of streaming edge cases (line endings, multiline payload fields, truncated chunks); regressions fail CI.

---

## Performance & resource management

- **Avoid repeated history cloning**
  - Reduce copying on the LLM relay hot path while respecting [ADR 0003](docs/adr/0003-wire-protocol-and-compatibility.md) (hub history is non-normative policy); pick snapshot, buffer, or incremental strategies based on measurement.
  - Done when: hot-path cost for prompt assembly is bounded (documented strategy and representative benchmarks or profiling budgets), and upstream clients cannot rely on full wire-history replay because hub history policy remains non-normative under [ADR 0003](docs/adr/0003-wire-protocol-and-compatibility.md).
- **Backpressure and buffering**
  - Ensure message channels have intentional bounds and behavior when consumers lag.
  - Done when: memory does not grow unbounded and behavior is documented.

---

## Documentation & maintenance

- **Write module-level docs (“where to add what”)**
  - Done when: contributors can find where to implement a new domain rule vs a new adapter without reading the whole codebase.
- **Extract spike learnings into docs**
  - ADR 0003 message shapes, ADR 0004 ticket profile behavior, the pull-based FFI contract, and any intentionally dropped gossip/AboutMe behavior.
  - Done when: replacement behavior is specified independently of spike implementation.
- **Retire spike code without a parity gate**
  - ADR 0001 permits replacing/removing spike code in-tree as the new implementation lands; do not create a second crate or long-lived example solely for parity.
  - Done when: remaining spike references are either historical docs or explicitly marked non-normative.

---

## Suggested implementation order (pragmatic)

Follows [Implementation ordering (hybrid)](#implementation-ordering-hybrid): **ADR 0002** layer spine, with **ADR 0003** / **ADR 0004** relay-plane work **started early** (shared codecs + fixtures), **before** treating the FFI binding or QUIC stack as the sole proof of correctness.

- **Step 1**: Module skeleton, composition root, and thin binary entrypoints.
- **Step 2**: Domain extraction, domain error model, and invariants (aligned with ADR 0003 field semantics where they touch the model).
- **Step 3**: Port traits (network, LLM, single canonical inbound facade per [ADR 0002](docs/adr/0002-hexagonal-boundaries-and-ownership.md)) **and begin** ADR 0003 framing/DTOs and ADR 0004 ticket decode **in parallel** with stub or partial application wiring—iterate until fixtures pass.
- **Step 4**: Client application service and daemon hub service fully driven by **fakes**; include **multi-client / same-room** unit coverage; keep DTOs and hub logic aligned with normative JSON and ticket behavior.
- **Step 5**: Client and daemon transport adapters on shared wire codecs (**Q1**); retire superseded protocol paths per [ADR 0001](docs/adr/0001-repository-and-package-strategy.md).
- **Step 6**: Inbound FFI adapter delegating to the application; preserve pull-based event consumption.
- **Step 7**: LLM outbound adapter, relay, and partial event model (no string-prefix leakage).
- **Step 8**: Stack integration tests (transport stack, **≥2 clients, same room**), lifecycle/shutdown polish, cross-cutting errors/logging.
