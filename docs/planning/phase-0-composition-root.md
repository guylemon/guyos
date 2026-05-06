# Phase 0 — Composition root

## Intent

Promote the stub **`infrastructure/`** layer into the **single production composition site** (per [ADR 0002](../adr/0002-hexagonal-boundaries-and-ownership.md)): bind **`IrohGossipRelayBackend`** (the real **operation-level** outbound adapter) into **`ClientChatService`**, expose behavior through the **`HubClientSession`** inbound port, and keep the UniFFI **`Chat`** type as a **thin shell** that forwards to a **concrete** `Arc<ClientChatService<IrohGossipRelayBackend>>`.

This slice is **wiring and layer honesty**, not lifecycle hardening (receive loop remains **equivalent `tokio::spawn`**; **supervised tasks / explicit shutdown** stay Phase 3 per checklist) and not port object-safety polish (**no `dyn` inbound**, **no `async-trait` dependency** until Phase 2 tightening).

**Primary references:** [ADR 0002](../adr/0002-hexagonal-boundaries-and-ownership.md), [`GUYOS_CORE_SPIKE_REFACTOR_TASKS.md`](../../GUYOS_CORE_SPIKE_REFACTOR_TASKS.md), prior layout runbook [`phase-0-module-layout.md`](phase-0-module-layout.md). If this plan conflicts with an adopted ADR, **follow the ADR** and update this document.

---

## Crate anchor

Paths are relative to the **repository root** unless prefixed with **`guyos_core/`**.

```bash
cargo build --manifest-path guyos_core/Cargo.toml
cargo test --manifest-path guyos_core/Cargo.toml
```

---

## Naming (stable identifiers)

| Symbol | Layer | Role |
| --- | --- | --- |
| **`HubClientSession`** | `ports/inbound/` | Canonical inbound port trait (methods grow over time; **generic** `impl` only in Phase 0—**no `dyn`**) |
| **`ClientChatService<B>`** | `application/` | Application service; **`B: RelayChatBackend + Send + Sync`** (bounds as needed) |
| **`RelayChatBackend`** | `ports/outbound/` | **Operation-level** outbound port (not the final ADR 0003 transport port; **shape evolves** in Phase 2+) |
| **`IrohGossipRelayBackend`** | `adapters/outbound/` | Concrete **`RelayChatBackend`** for today’s iroh-gossip spike |
| **`FakeRelayChatBackend`** | `ports/outbound/` | **`#[cfg(test)]`** test double implementing **`RelayChatBackend`** |
| **`ChatMessage`** | `domain/` | Shared message DTO; **single** UniFFI `Record` definition lives here for Phase 0 (see **Domain note** below) |

**Production bootstrap** (exact spelling is a convention; pick one name and use it consistently, e.g. **`wire_chat_for_clients`**): a **`pub(crate)`** function in **`infrastructure/`** that builds **`IrohGossipRelayBackend`**, constructs **`ClientChatService<IrohGossipRelayBackend>`**, and returns **`Chat`** (or the pieces `Chat` needs). **UniFFI `Chat::new`** and **CLI** SHALL delegate to this path for production graphs.

---

## Domain note (`ChatMessage`)

**Phase 0 decision:** keep **one** **`ChatMessage`** in **`domain/`** with **`#[derive(uniffi::Record)]`** (or equivalent export) so **`application`** never imports **`adapters`**.

**Follow-on:** if stricter “domain has zero UniFFI” purity is desired later, split into **`domain::ChatMessage`** + **`adapters/inbound` FFI mirror** in a subsequent phase and update re-exports/bindings—called out here so the team does not treat the Phase 0 shortcut as permanent law.

---

## In scope

- **`RelayChatBackend`**: **operation-level** async methods that match today’s spike flow (bind/join topic, send, attach receive path, etc.—**implementation detail** of the exact method set, but **not** low-level iroh primitives scattered through **`application`**).
- **`IrohGossipRelayBackend`**: real implementation, **only** referenced from **`infrastructure`** for production wiring (plus its own module under **`adapters/outbound/`**).
- **`HubClientSession`**: trait + **`impl<B: …> HubClientSession for ClientChatService<B>`** so **`FakeRelayChatBackend`** and **`IrohGossipRelayBackend`** both exercise the **same** façade in unit tests.
- **`ClientChatService`**: owns **all** state previously held in **`ChatInner`** + the **`mpsc`** plumbing for **`next_message`**; owns **receive-loop `tokio::spawn`**, **behavior unchanged** from today (Phase 3 upgrades ownership).
- **`Chat`**: holds **`Arc<ClientChatService<IrohGossipRelayBackend>>`** (**concrete**, **no `dyn`**); forwards UniFFI methods; **`Chat::new` → infrastructure wiring** for production.
- **`infrastructure::wire_chat_for_clients`** (or equivalent): **exactly one** documented production composition function; tests may construct **`ClientChatService::<FakeRelayChatBackend>::…`** **without** going through **`infrastructure`** (not a second production root).
- **Architecture smoke test** (required): fails if **`application/`** or **`ports/`** source files reference **`iroh`**, **`iroh_gossip`**, or **`IrohGossipRelayBackend`** (tune patterns as needed; **zero** or near-zero exceptions). Satisfies checklist “**verifiable by … architecture/check test**.”
- **`FakeRelayChatBackend`**: under **`ports/outbound/`**, included only with **`#[cfg(test)]`**.

## Out of scope (this item)

- Replacing fire-and-forget spawns with **supervised tasks**, **explicit shutdown**, or **join/cancel registries** (Phase 3).
- **`dyn HubClientSession`**, **`async-trait`**, and full **object-safe** port polish (Phase 2 hardening track).
- Splitting **`ChatMessage`** into domain vs pure-FFI DTO (explicitly **deferred**; see Domain note).
- ADR 0003 **codec / ticket** extraction (parallel track; outbound port remains **spike-shaped**).
- Moving **`llm`** into the library crate or wiring LLM through **`infrastructure`** (still **binary-local** per prior Phase 0 item).

---

## Dependency rules (enforced by review + smoke test)

- **`domain/`**: no imports of **`ports` / `application` / `adapters` / `infrastructure`** (existing ADR rule).
- **`application/`**: **`domain`**, **`ports`** only—**no** `iroh` / **`IrohGossipRelayBackend`** strings (smoke test).
- **`ports/`**: **no** concrete **`IrohGossipRelayBackend`** (smoke test / review); **`FakeRelayChatBackend`** only under **`cfg(test)`**.
- **`adapters/outbound/`**: may depend on **`ports`**, **`domain`**, **`error`**, iroh crates as needed.
- **`infrastructure/`**: **only** production layer that **constructs** **`IrohGossipRelayBackend`** and binds it into **`ClientChatService`**, then **`Chat`**.

---

## Runbook (suggested implementation order)

Phases below can be **one PR or a short chain**; keep each step compiling and tested.

### Phase A — `ChatMessage` + re-exports

1. Move **`ChatMessage`** into **`domain/`** with UniFFI derive unchanged at the **type-definition** site.
2. **`lib.rs`**: **named** `pub use` for **`ChatMessage`** from **`domain`** (same stable path as today if possible: **`guyos_core::ChatMessage`**).
3. Update inbound **`chat.rs`** imports.
4. Run verify commands; if UniFFI output changes, run **§ UniFFI bindgen exit gate**.

### Phase B — Outbound port + iroh adapter

1. Add **`RelayChatBackend`** (async trait, **operation-level** methods) in **`ports/outbound/`**.
2. Add **`IrohGossipRelayBackend`** in **`adapters/outbound/`**; move **iroh/gossip orchestration** that **`ClientChatService`** will need out of **`chat.rs`** into this adapter **without** behavior changes.
3. Add **`#[cfg(test)] mod fake_relay_chat_backend;`** (or equivalent) under **`ports/outbound/`** with **`FakeRelayChatBackend`**.

### Phase C — Inbound port + application service

1. Define **`HubClientSession`** in **`ports/inbound/`** mirroring UniFFI-relevant operations (**`open`**, **`join`**, **`send`**, **`next_message`**—adjust to match actual extraction).
2. Implement **`ClientChatService<B>`** in **`application/`**:
   - holds **`B`** (typically **`Arc<B>`** or owned `B`, per ergonomics),
   - owns prior **`ChatInner`** + channel state,
   - calls **`RelayChatBackend`** for network operations,
   - preserves **`tokio::spawn`** for receive path (**document TODO → Phase 3**).
3. **`impl<B: RelayChatBackend + Send + Sync + …> HubClientSession for ClientChatService<B>`** (bounds as required by **`RelayChatBackend`**).

### Phase D — Thin `Chat` + composition root

1. Replace **`Chat`** internals with **`Arc<ClientChatService<IrohGossipRelayBackend>>`** (or equivalent handle).
2. Implement **`infrastructure::wire_chat_for_clients(name) -> Chat`** (or **`&`**/`Arc` pattern if **`Chat` remains an owned UniFFI object—match UniFFI constructor rules**).
3. **`Chat::new`**: **only** calls **`crate::infrastructure::wire_chat_for_clients`** (or a `pub(crate)` alias), **no** direct **`IrohGossipRelayBackend::new`** in **`adapters/inbound`**.

### Phase E — CLI + smoke test

1. **`main.rs`**: keep **`Chat::new`** (or delegate to the same **`wire_…`** if you prefer explicit calls—both satisfy “**through composition root**” **if** **`Chat::new` is documented as the thin entry**).
2. Add **`tests/layer_policy.rs`** (name flexible) implementing the **required** substring / path scans.
3. Add **`ClientChatService<FakeRelayChatBackend>`** unit tests under **`application/`** (or colocated **`#[cfg(test)]`**) proving facade + fake backend.

### Verify

```bash
cargo test --manifest-path guyos_core/Cargo.toml
```

**UniFFI bindgen exit gate:** run **`./build-ios.sh`** when exports, derives, or scaffolding change; commit **`GuyOSClient/Sources/GuyOSClient/guyos_core.swift`** if it drifts.

---

## UniFFI bindgen exit gate (ongoing)

Same rule as [`phase-0-module-layout.md`](phase-0-module-layout.md#uniffi-bindgen-exit-gate-ongoing): changing exported types, proc-macro exports, or scaffolding requires **regenerate + commit Swift**.

---

## Done-when checklist

- [ ] Exactly **one** documented **`pub(crate)`** production wiring function in **`infrastructure/`** builds **`IrohGossipRelayBackend` + `ClientChatService` + `Chat`**.
- [ ] **UniFFI `Chat::new`** and **CLI** use that wiring path (via **`Chat::new`** or explicit call—**document which**).
- [ ] **`ClientChatService` owns** all former **`ChatInner` + channel state**; **`Chat` is thin**.
- [ ] **`HubClientSession`** exists; **`ClientChatService<B>`** implements it **generically** (**no `dyn`**).
- [ ] **`RelayChatBackend` + `IrohGossipRelayBackend`** exist (**operation-level**).
- [ ] **`ChatMessage` lives in `domain/`** (single UniFFI definition for Phase 0).
- [ ] **Architecture smoke test** is **required** and passes on CI / locally.
- [ ] **`FakeRelayChatBackend`** lives under **`ports/outbound/`**, **`cfg(test)`** only.
- [ ] Receive loop spawning is **behavior-equivalent**; Phase 3 ownership work is **called out** as a **TODO**, not silently forgotten.

---

## Follow-on commits log

| Date | Commit / PR | Scope | Notes |
| --- | --- | --- | --- |
| — | — | — | — |

---

## Risks / notes

- **Concrete `Chat` → `ClientChatService<IrohGossipRelayBackend>`** couples the FFI shell to the iroh adapter **until Phase 2** introduces **`dyn`** / richer port contracts—**accepted** for Phase 0 scope.
- **Operation-level `RelayChatBackend`** will churn as ADR 0003 relay codecs land; plan for **adapter + trait method** updates, not a rewrite of **`infrastructure`**’s *role*.
- **`#[cfg(test)]` fakes are invisible to integration tests** that build the library as a normal dependency; keep **fast façade tests** as **unit tests** under **`src/`**, or introduce a **`test-utils` feature** later if integration coverage needs the fake.

---

## Appendix — Decision log (this planning session)

| # | Topic | Decision |
| --- | --- | --- |
| 1 | Application seam | **Minimal `ClientChatService` + inbound trait now** |
| 2 | Outbound shape | **`RelayChatBackend` + `IrohGossipRelayBackend`**, **operation-level** |
| 3 | Receive loop | **Equivalent `spawn`**, **Phase 3** for supervision |
| 4 | State ownership | **`ClientChatService` owns all**; **`Chat` thin** |
| 5 | Inbound naming | **`HubClientSession` + `ClientChatService`** |
| 6 | Outbound naming | **`RelayChatBackend` + `IrohGossipRelayBackend`** |
| 7 | Production wiring | **Single `infrastructure` function**; **tests** may skip it with **fakes** |
| 8 | `ChatMessage` | **Single type in `domain/`** + UniFFI; **strict split later** |
| 9 | Verification | **Required** automated layering smoke test |
| 10 | Fake placement | **`ports/outbound`**, **`cfg(test)`** |
| 11 | FFI coupling | **`Chat` concrete to `ClientChatService<IrohGossipRelayBackend>`**; **`dyn`/`async-trait` → Phase 2** |
| 12 | Inbound polymorphism | **`HubClientSession` trait now**, **`impl` generic over `B`**, **no `dyn`** |
