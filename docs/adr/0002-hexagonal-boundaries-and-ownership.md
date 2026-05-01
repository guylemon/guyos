# ADR 0002: Hexagonal boundaries and ownership

## Status

Accepted

## Context

[ADR 0001](0001-repository-and-package-strategy.md) locks single-crate, evolve-in-place packaging. This ADR records the **normative** high-level hexagonal layout, dependency rules, layer boundaries, process and task ownership (including shutdown), and error boundaries for `guyos_core`. **Wire formats, tickets, and versioning policy** belong in [ADR 0003](0003-wire-protocol-and-compatibility.md). Execution detail stays in [GUYOS_CORE_SPIKE_REFACTOR_TASKS.md](../../GUYOS_CORE_SPIKE_REFACTOR_TASKS.md); **if this ADR and the checklist disagree, follow this ADR** and update the checklist.

The intended deployment includes a **QUIC hub**: iOS and CLI clients connect to a **daemon**; the chat plane for v1 is **relayed through the daemon**, not direct client-to-client sockets. **Normative rules below stay transport-agnostic** so boundaries remain stable if the QUIC implementation changes.

## Decision

### Normative source layout

The following layout is the **normative target** for refactor work in `guyos_core/src`. The current spike-era source tree may temporarily lag this layout while implementation catches up.

| Path | Responsibility |
| --- | --- |
| `src/domain/` | Pure domain model, invariants, value objects, and domain errors. |
| `src/ports/inbound/` | Trait surface that inbound adapters call: the canonical application API. |
| `src/ports/outbound/` | Trait surface the application requires from outside systems. |
| `src/application/` | Use-case orchestration, lifecycle ownership, supervision, and coordination across ports. |
| `src/adapters/inbound/` | Concrete entrypoint adapters that call inbound ports, such as platform bindings or CLI frontends. |
| `src/adapters/outbound/` | Concrete integrations that implement outbound ports, such as transport or LLM clients. |
| `src/infrastructure/` | Composition root and process bootstrap helpers that wire concrete adapters into application services. |
| `src/error.rs` | Shared structured internal error type and cross-layer source chains. |

Top-level layer paths and dependency direction are normative. Internal file names and most submodules are implementation details unless a boundary rule below depends on them.

`src/lib.rs` is a facade/module declaration and re-export surface only. It MAY contain crate-level setup required by library consumers, but MUST NOT be the place that wires concrete adapters into services. Concrete wiring belongs in `src/infrastructure/`.

Binary entrypoints MAY follow normal Rust conventions (`src/main.rs`, `src/bin/*`, or equivalent Cargo targets). Each entrypoint MUST remain thin: parse process-specific inputs, call the shared bootstrap path, and delegate behavior to the application API.

### Dependency direction and layers

- Dependencies **point inward** toward the domain. **Strict isolation:** `src/domain/` MUST NOT import `ports`, `application`, `adapters`, or `infrastructure`.
- **Ports** are split by direction:
  - **Inbound** ports describe what entrypoints call (the canonical application API).
  - **Outbound** ports describe what the **application** needs from the outside world (e.g. network transport to the hub, LLM).
- **Application** depends only on **domain** and **port traits** (plus allowed cross-cutting libraries such as `tracing` as specified below). It MUST NOT import concrete `adapters`.
- **Concrete adapter types** are wired **only** from `src/infrastructure/`: it is the **only** layer that imports outbound implementations and constructs the graph. **Unit tests** construct the application with mocks/fakes and need not use that module.

### Async and domain purity

- **Outbound** port traits are **async end-to-end** where I/O is required; **application** code orchestrates `async` work and owns supervision.
- The **domain** layer remains **synchronous and pure**: no `async`/`await`, no `Future`, no Tokio or runtime types in domain code. Domain types cross boundaries as ordinary values.
- **Inbound** ports are **async** where entrypoints require it (e.g. UniFFI + Swift `await`).

### Single inbound surface and bootstrap

- There is **one canonical inbound trait** (single application facade), implemented by the core service. **All inbound adapters** (UniFFI, CLI, etc.) are thin delegators to it unless a future requirement forces a deliberate split.
- Each **process** uses **one bootstrap path** (e.g. a single builder or factory in `infrastructure`) that constructs the application with concrete outbound adapters. **Do not** duplicate wiring in each inbound adapter.

### Task ownership, shutdown, and signals

- **Application** owns **use-case-scoped** background work (receive loops, relay workers, coordinated tasks): it supervises cancellation, join/drain, or equivalent, and exposes a **normative** `**shutdown()`** (or one clear lifecycle API) for deterministic teardown.
- **Outbound** adapters implement **cancellable** async ports and MUST NOT leave **untracked** background tasks that outlive the application service.
- `**Drop` is best-effort** only; **normative** cleanup is explicit shutdown.
- For the **daemon**, **bootstrap** maps **SIGTERM** / **SIGINT** (and similar) into the **same** shutdown path as interactive clients.

### Errors

- Errors are **layered**: **domain** uses structured `**DomainError`** (or equivalent); **application** uses a structured internal error type (e.g. `**AppError`** with `thiserror` and source chains).
- **Only inbound adapters** map internal failures into **stable, adapter-specific** surfaces (e.g. UniFFI error enums, CLI messaging). Internal error types MUST NOT leak across the inbound boundary.

### Observability

- **Domain** MUST NOT emit `**tracing`** (or other logging). **Application** and **adapters** MAY use structured logging for lifecycle and I/O.

### Crate entrypoints

- The **same** `guyos_core` crate MAY expose **multiple entrypoints** (library + several `[[bin]]` targets). Each entrypoint uses the **same bootstrap discipline** with **different config and adapter sets**. **Not every binary** must link UniFFI or include the daemon accept loop; **layer rules apply to every target**.

## Implementation notes (non-normative)

These choices **do not** replace port abstractions; they document the **first** implementation intended for this repo.

- **v1 daemon:** QUIC is implemented **via iroh** (ALPN, `Endpoint` / `Router`, custom protocol handler / streams as appropriate). **Custom messaging** framing and compatibility are specified under [ADR 0003](0003-wire-protocol-and-compatibility.md).
- Current likely adapter examples include QUIC transport, UniFFI, CLI, and LLM clients; exact adapter submodule names are not normative.
- **Future modes** (e.g. chat vs other product modes) evolve through **versioned wire messages** and **application use cases**, not by bypassing these boundaries.

## Consequences

- Enforcing the graph (reviews, optional lint checks) is feasible: domain and ports stay small and testable; **integration tests** may still construct real adapters without violating production import rules.
- **Hub deployment** implies **different** bootstrap configuration for **daemon** vs **client** binaries, while **one** set of layer rules applies everywhere.
- Swapping **QUIC** libraries (e.g. away from iroh transport) requires updating **adapter and bootstrap** code, not redrawing **hexagonal** rules, as long as **ports** remain the boundary.
- **ADR 0003** must define wire shapes and compatibility; this ADR intentionally does not.
