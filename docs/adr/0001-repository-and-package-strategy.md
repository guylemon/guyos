# ADR 0001: Repository and package strategy

## Status

Accepted

## Context

The `guyos_core` spike validated iroh-gossip, UniFFI, and Swift binding generation. It is exploratory and has no external dependents. The product is a **personal tool**; the crate is **not** intended for release on crates.io. `build-ios.sh` references spike-era paths and can be updated when the crate layout changes. Planning context also lives in [hexagonal architecture recommendation](../reference/hexagonal-architecture-recommendation.md), [spike code review](../reference/spike-code-review.md), git history, and the living checklist [GUYOS_CORE_SPIKE_REFACTOR_TASKS.md](../../GUYOS_CORE_SPIKE_REFACTOR_TASKS.md).

## Decision

- Re-implement as a **hexagonal layout** by replacing spike sources **inside** the existing [`guyos_core`](../../guyos_core/Cargo.toml) crate: keep the same package name, library name, and `guyos_core/` directory. **Do not** add a temporary crate rename or a second package for migration; use a branch for the work and rely on git history (and optional tags) if you need a pointer to the pre-refactor tree.
- Use a **single-package** branch workflow (no side-by-side spike and replacement crates in the workspace).
- **No parity gate** between spike and replacement; spike may be removed from the tree once the replacement builds and tooling is updated.
- Keep a **single workspace crate** named `guyos_core` (no long-term dual crate names in this repo); there is no crates.io publication requirement.

## Consequences

- Spike code disappears from HEAD after cutover; behavioral detail remains in reference docs and git history.
- `build-ios.sh` (and similar) must be pointed at the new crate layout once; no ongoing dual-target maintenance.