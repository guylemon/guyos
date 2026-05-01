# guyos_core spike refactor task list (hexagonal architecture)

This checklist consolidates action items from:

- Source documents: `guyos_core_hexagonal_architecture_recommendation.docx`, `guyos_core_spike_code_review.docx`
- Text extractions (preserved in-repo): `[docs/reference/hexagonal-architecture-recommendation.md](docs/reference/hexagonal-architecture-recommendation.md)`, `[docs/reference/spike-code-review.md](docs/reference/spike-code-review.md)`

Goal: refactor (or re-implement) the `guyos_core` spike into a maintainable, testable, multi-frontend Rust library with a stable, Swift-friendly UniFFI surface.

**Normative decisions:** Locked rules and rationale live in [Architecture Decision Records](docs/adr/) linked below. This file is the **living execution checklist** (what to do, suggested order, done-when criteria). **If an adopted ADR ever disagrees with this checklist, follow the ADR** and update the checklist to match.

---

## Guiding decisions (lock these in early)

- **[ADR 0001 — Repository and package strategy](docs/adr/0001-repository-and-package-strategy.md)** (rewrite vs evolve-in-place, branch/packaging, spike disposition)
  - Done when: ADR is accepted and the workspace matches it (single-package path; one `guyos_core` crate in-tree, no crates.io release planned).
- **[ADR 0002 — Hexagonal boundaries and ownership](docs/adr/0002-hexagonal-boundaries-and-ownership.md)** (layers, dependency rules, lifecycle/supervision)
  - Done when: ADR documents layers (domain/application/ports/adapters), what is allowed to depend on what, and how background tasks are supervised and shut down.
- **[ADR 0003 — Wire protocol and compatibility](docs/adr/0003-wire-protocol-and-compatibility.md)** (messages, tickets, versioning policy)
  - Done when: ADR specifies message/ticket formats, versioning approach, and compatibility policy (e.g., “minor versions remain backward compatible for tickets”).

---

## Phase 0 — Repository & module structure (composition first)

- **Create the hexagonal module layout**
  - Target layout (adapt as needed):
    - `src/domain/`
    - `src/ports/`
    - `src/application/`
    - `src/adapters/driver/` (UniFFI, CLI)
    - `src/adapters/driven/` (iroh-gossip, OpenAI-compatible LLM)
    - `src/infrastructure/` (DI, config, shared runtime wiring)
    - `src/error.rs` (AppError)
  - Done when: modules compile and the public crate surface is intentionally re-exported from `src/lib.rs`.
- **Define the “composition root”**
  - Done when: there is exactly one place that wires concrete adapters into application services (e.g. `infrastructure/di.rs`), and driver adapters/CLI call into that.
- **Thin CLI entrypoint**
  - Done when: `src/main.rs` contains only arg parsing + calling application service APIs; no business rules, no LLM orchestration logic living only in CLI.

---

## Phase 1 — Domain layer (pure, testable, no external crates)

- **Extract domain entities/value objects**
  - Candidates: `Message`, `MessageBody`, `Ticket`, `Name`, `EndpointId` (newtype), “history message” types, message identifiers.
  - Done when: domain types are free of iroh/uniffi/reqwest/clap dependencies and can be unit tested without tokio.
- **Add invariants via constructors**
  - Add validations such as:
    - message length cap
    - required fields
    - nonce/id uniqueness rules (where applicable)
  - Done when: invalid inputs cannot be created without an explicit `unsafe`/test-only escape hatch.
- **Separate “domain types” from “wire/DTO types” (if needed)**
  - If serde is only used for wire, move serialization into adapters/DTOs instead of domain.
  - Done when: domain layer is format-agnostic (or, if serde is retained, it is explicitly justified and kept minimal).
- **Domain error model**
  - Done when: `DomainError` is a structured enum (no “stringly typed” variants), implements `Error + Display`, and is used by constructors/invariants.

---

## Phase 2 — Ports (contracts / traits)

- **Define outbound port: `NetworkPort`**
  - Must cover: `open`, `join`, `broadcast/send`, `subscribe` (or stream/channel of incoming messages), plus any necessary metadata methods (my endpoint id, peers count).
  - Done when: application logic can be compiled and unit-tested with a mock `NetworkPort`.
- **Define outbound port: `LlmPort`**
  - Must cover: non-stream and/or stream reply generation, plus configuration inputs (model, base url, etc.).
  - Done when: LLM relay logic can be tested with a fake `LlmPort` (no HTTP).
- **Define inbound port / use-case surface**
  - Expose a stable application API (e.g. `ChatUseCases`) that driver adapters (UniFFI/CLI) call.
  - Done when: driver adapters are thin delegators with almost no logic.

---

## Phase 3 — Application layer (use cases, orchestration, lifecycle)

- **Implement `ChatService` (use cases)**
  - Move spike logic for `open/join/send/next_message` here.
  - Done when: `ChatService` depends only on domain + ports; no iroh/uniffi/reqwest/clap usage.
- **Implement a supervised task model**
  - Use `JoinSet`, a “supervisor” object, or a dedicated background worker owned by the service.
  - Done when: all spawned tasks are tracked and can be cleanly shut down.
- **Add explicit shutdown**
  - Provide `shutdown()` and/or implement `Drop` to signal cancellation and await task termination where appropriate.
  - Done when: Ctrl-C / app exit does not leak tasks, and Swift can release the `Chat` object without leaving background work running indefinitely.
- **Fix coarse locking and contention**
  - Replace “one big mutex for everything” with more granular state or `RwLock` where appropriate.
  - Done when: receiving messages does not require serializing unrelated API calls under a single lock.
- **Make “connection state & diagnostics” queryable**
  - Add APIs like `my_endpoint_id()`, `connected_peers()`, `is_joined()`, etc.
  - Done when: Swift/CLI can render connection status without parsing logs.

---

## Phase 4 — Driven adapters (outbound implementations)

### iroh-gossip adapter

- **Move iroh setup + router/gossip code into `adapters/driven/iroh/`**
  - Done when: iroh types are not referenced from application/domain code.
- **Implement `NetworkPort` for `IrohGossipAdapter`**
  - Done when: application tests can swap the real adapter for a mock.
- **Handle AboutMe / name resolution ordering**
  - Improve name resolution race handling (message may arrive before AboutMe).
  - Done when: behavior is deterministic (e.g., cache updates, delayed name resolution, or message update semantics are defined).

### OpenAI-compatible LLM adapter

- **Move all reqwest + SSE parsing into `adapters/driven/openai/`**
  - Done when: application layer can request an LLM reply solely via `LlmPort`.
- **Replace or harden SSE implementation**
  - Option A: adopt a mature SSE library (preferred).
  - Option B: keep custom parser but fully support multiline `data:` and resilience.
  - Done when: streaming is robust against common SSE variations and malformed chunks are handled intentionally.
- **Add client hardening**
  - timeouts
  - connection pooling config
  - optional retries/backoff
  - optional auth headers (Bearer)
  - Done when: adapter configuration supports real deployments beyond local Ollama.

---

## Phase 5 — Driver adapters (inbound implementations)

### UniFFI (Swift binding)

- **Move UniFFI object(s) into `adapters/driver/uniffi/`**
  - Done when: UniFFI macros and exported async methods are isolated to this module.
- **Keep the pull-based API (`next_message`)**
  - Done when: Swift consumes via `while let msg = await chat.nextMessage()` and no callback-based interface is required.
- **Eliminate LLM UI leakage**
  - Remove the “(partial) ” prefix hack.
  - Done when: partial streaming is represented as either:
    - a distinct message type (e.g. `ChatEvent::Partial { id, text, done }`), or
    - an explicit boolean/enum field exposed over UniFFI.
- **Expose cancellation / responsiveness strategy**
  - If needed, add a cancel token or a `shutdown()` that unblocks `next_message()`.
  - Done when: Swift can stop listening without relying on process lifetime.

### CLI

- **Move all LLM relay ownership into the library/application**
  - Done when: the CLI does not maintain its own history/abort logic; it just enables the feature and renders events.

---

## Error handling & observability (cross-cutting)

- **Introduce `AppError` using `thiserror`**
  - Keep rich error sources internally; stringify only at the FFI boundary.
  - Done when: errors retain causal chains (`source`) and mapping into UniFFI error types is centralized.
- **Centralize error mapping for UniFFI**
  - Done when: UniFFI surface exposes a stable, Swift-friendly `ChatError` while internal errors keep context.
- **Add structured logging (`tracing`)**
  - Done when: major lifecycle events (open/join/subscribe/send/receive/llm start/llm stop) emit structured logs and can be enabled/disabled via config.

---

## Protocol, compatibility, and security hardening

- **Add protocol versioning**
  - Include a version field in `Ticket` and `Message`.
  - Done when: old tickets/messages can be rejected gracefully with a clear error, and a compatibility story is documented.
- **Add message authenticity (optional but recommended)**
  - Sign messages (e.g., ed25519 using iroh identity keys) and include `verified` on received messages.
  - Done when: name spoofing is detectably prevented (or explicitly out of scope with documented threat model).
- **Add validation + limits**
  - message size caps
  - rate limiting/spam controls (at least basic)
  - Done when: malformed/oversized inputs are rejected before heavy processing.
- **Revisit serialization format**
  - If/when needed: move from JSON to a compact binary (postcard/bincode) for gossip efficiency.
  - Done when: format choice is documented, benchmarked, and versioned.

---

## Testing strategy (unit + integration)

- **Unit tests for application layer with mocked ports**
  - Use `mockall` or manual fakes.
  - Done when: `ChatService` behavior (send/receive ordering, history trimming, error mapping) is covered without networking.
- **Property tests for serialization**
  - Roundtrip ticket/message encoding/decoding.
  - Done when: regressions in compatibility are caught automatically.
- **Integration tests for real iroh gossip**
  - Spin up multiple instances in-process (e.g., 3 peers) and assert message delivery.
  - Done when: open→join→send works reliably in CI.
- **LLM adapter tests**
  - Mock server for SSE streaming and edge cases (CRLF, multiline, partial frames).
  - Done when: adapter behavior is stable and does not regress.

---

## Performance & resource management

- **Avoid repeated history cloning**
  - Replace per-message full clones with shared immutable history snapshots, bounded buffers, or incremental prompting.
  - Done when: LLM relay overhead is bounded and measurable.
- **Backpressure and buffering**
  - Ensure message channels have intentional bounds and behavior when consumers lag.
  - Done when: memory does not grow unbounded and behavior is documented.

---

## Documentation & maintenance

- **Write module-level docs (“where to add what”)**
  - Done when: contributors can find where to implement a new domain rule vs a new adapter without reading the whole codebase.
- **Extract spike learnings into docs**
  - Ticket format, message shapes, AboutMe protocol, UniFFI pull-based contract.
  - Done when: v2 behavior is specified independently of spike implementation.
- **Keep spike as a reference implementation**
  - Option: move spike into `examples/` or keep on a dedicated branch.
  - Done when: there’s a stable reference for manual parity testing during refactor.

---

## Suggested implementation order (pragmatic)

- **Step 1**: module skeleton + composition root + thin CLI
- **Step 2**: domain extraction + `DomainError` + invariants
- **Step 3**: ports + application `ChatService` (fully unit-tested with mocks)
- **Step 4**: iroh adapter implementing `NetworkPort`
- **Step 5**: UniFFI adapter delegating to application (keep pull API)
- **Step 6**: LLM adapter + relay service + partial event model (no string prefix)
- **Step 7**: integration tests + lifecycle/shutdown polish

