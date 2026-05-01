# GuyOS — system overview

This document is **informative**. It sketches what lives in this repository, how the main pieces relate, and where **normative** engineering rules are written. When anything here disagrees with an adopted Architecture Decision Record (ADR), **follow the ADR** and update this overview—or the refactor checklist—for consistency.

GuyOS is a **personal tooling** workspace. The **mental model** for development is below; concrete APIs and wire formats remain governed by the normative links at the end of this file.

## Product shape (informative)

- **Hub as core:** The hub runs the integrations, e.g., connections to **local or remote LLM providers**, a **custom external spaced-repetition flashcard program**, and other services or tools as needs grow. It exposes a **thin client API** oriented toward those capabilities.
- **UniFFI surface:** The build generates **UniFFI bindings** from that Rust API so **iOS** platform code does not reimplement hub protocols by hand.
- **Hub clients:** The hub is used by both an **iOS application** (through UniFFI) and a **CLI** (through the same `guyos_core` stack: typically binary entrypoints that share the library’s bootstrap discipline). **Multiple clients may be connected at once** (for example **phone and CLI simultaneously**); the hub’s room/session semantics and wire contract govern how messages and state are shared across those connections.
- **Initial use cases (examples, not a closed list):** **LLM chat**, and **LLM-assisted spaced repetition learning** (helping the user study via the external SRS tool the hub integrates with). Use cases may be driven from either client surface.

Today’s accepted ADRs focus heavily on the **client ↔ hub relay plane** (QUIC, framing, tickets). Broader “what the hub does” beyond that plane will evolve with product work and may gain additional ADRs when behavior needs to be nailed down for compatibility.

## Components (in repository)

Known parts appear first, with pointers into the tree. Paths are repository-relative.

| Component | Role |
| --- | --- |
| **[`guyos_core/`](../guyos_core/)** | Rust **hub** crate: protocol and application logic; library surface and process entrypoints per [`guyos_core/Cargo.toml`](../guyos_core/Cargo.toml) (including **CLI** and hub/daemon-style targets); **`uniffi-bindgen`** for Swift-facing FFI from the thin client API. |
| **[`GuyOSClient/`](../GuyOSClient/)** | Swift package that consumes a **`guyos_core`** UniFFI build via the **`guyos_coreFFI`** binary target (XCFramework checked in at `GuyOSClient/GuyOSClient.xcframework`). |
| **[`GuyOS/`](../GuyOS/)** | SwiftUI **iOS** application that builds on **GuyOSClient**, intended to orchestrate multiple hub-backed use cases over time alongside any **CLI** sessions attached to the same hub. |

## Planned or out-of-tree components (stubs)

The full application may include responsibilities not yet documented as separate trees here. Examples of placeholders—expand or replace as the system grows:

- **Ticket issuance or operator tooling** — how join tickets are minted and distributed relative to the hub’s configured ticket profile.
- **Deployment and security composition** — TLS trust, network exposure, and access control around the hub (called out as deployment scope in the wire ADR).
- **Additional clients or shells** — platforms or UIs beyond the current iOS app and in-crate **CLI** / daemon entrypoints aligned with the hub.
- **External SRS / flashcard product** — the standalone program the hub integrates with (not necessarily in this repo); document endpoints, trust boundary, and sync semantics when they stabilize.
- **Automation and release** — CI, packaging, and distribution workflows not yet centralized in this doc.

When a component gains a stable home in the repo, add it to **Components (in repository)** with a one-line role and an optional link to its own README or ADR.

## Normative pointers (`guyos_core`)

Implementation and compatibility rules for the Rust core and its wire contract live in ADRs and the living checklist. **Do not** treat this overview as a specification.

| Artifact | What it defines |
| --- | --- |
| [ADR 0001 — Repository and package strategy](adr/0001-repository-and-package-strategy.md) | Single-crate evolution, no migration crate; spike may be replaced in place. |
| [ADR 0002 — Hexagonal boundaries and ownership](adr/0002-hexagonal-boundaries-and-ownership.md) | Layer layout, dependency direction, async vs pure domain, shutdown, errors, observability boundaries. |
| [ADR 0003 — Wire protocol and compatibility](adr/0003-wire-protocol-and-compatibility.md) | QUIC binding, framing, JSON message shapes, versioning, hub-visible behavior, limits. |
| [ADR 0004 — Ticket profiles and reference profile](adr/0004-ticket-profiles-and-reference-profile.md) | Ticket profile framework, registry, reference HMAC profile, golden vectors. |
| [`GUYOS_CORE_SPIKE_REFACTOR_TASKS.md`](../GUYOS_CORE_SPIKE_REFACTOR_TASKS.md) | Execution checklist for the spike refactor; **if it conflicts with an adopted ADR, follow the ADR** and update the checklist. |

Companion data (normative where the ADRs say so): [`docs/ticket-profile-registry.json`](ticket-profile-registry.json), [`docs/fixtures/0004-ticket-profile-reference-v1.json`](fixtures/0004-ticket-profile-reference-v1.json).

Future ADRs may cover other subsystems; this section should list them as they are accepted.
