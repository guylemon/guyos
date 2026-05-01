# ADR 0001: Repository and package strategy

## Status

Accepted

## Context

The `guyos_core` Rust library spike validated iroh-gossip, UniFFI, and Swift binding generation. It was exploratory and has no external dependents.

The product is a **personal tool**; the crate is **not** intended for release on crates.io. `build-ios.sh` references spike-era paths and can be updated when the crate layout changes. Historical context lives in [hexagonal architecture recommendation](../reference/hexagonal-architecture-recommendation.md), [spike code review](../reference/spike-code-review.md). The living checklist [GUYOS_CORE_SPIKE_REFACTOR_TASKS.md](../../GUYOS_CORE_SPIKE_REFACTOR_TASKS.md) outlines the current state of implementation.

## Decision

- Re-implement `guyos_core` with a **hexagonal layout** by replacing or refactoring spike sources **inside** the existing `[guyos_core](../../guyos_core/Cargo.toml)` crate. Keep the same package name, library name, and `guyos_core/` directory. **Do not** add a temporary crate rename or a second package for migration.
- Replace `iroh-gossip` protocol usage with a lightweight custom wire protocol to be implemented over QUIC. The implementation MAY use the iroh crate ecosystem for connection and custom protocol ergonomics; however, the client-hub nature of the product does not warrant gossip protocol.
- Use a **single-package** branch workflow (no side-by-side spike or replacement crates in the workspace).
- **No parity gate** between spike and replacement; spike code may be removed from the tree as the replacement builds and tooling is updated.
- Keep a **single workspace crate** named `guyos_core` (no long-term dual crate names in this repo); there is no crates.io publication requirement.

## Consequences

- Spike code disappears from codebase gradually as implementation proceeds.
- `build-ios.sh` continues to work as implemented; no ongoing dual-target maintenance is required for FFI binding generation.
