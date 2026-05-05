# Phase 0 — Hexagonal module layout

## Intent

Introduce the **ADR 0002 directory and module structure** under `guyos_core`, then **relocate** the existing UniFFI **`Chat` / `ChatMessage`** implementation into **`adapters/inbound/`** while preserving **`guyos_core::…` paths** and Swift bindings. This item is **layout + one colocated move**, not full hexagonal decomposition (no new inbound port trait or composition root yet).

**Primary references:** [`docs/adr/0002-hexagonal-boundaries-and-ownership.md`](../adr/0002-hexagonal-boundaries-and-ownership.md), [`GUYOS_CORE_SPIKE_REFACTOR_TASKS.md`](../../GUYOS_CORE_SPIKE_REFACTOR_TASKS.md). If this plan conflicts with an adopted ADR, **follow the ADR** and update this document.

---

## Crate anchor

All paths below are relative to the **repository root** unless prefixed with **`guyos_core/`**.

The Rust crate root is **`guyos_core/`** (standalone package, not a workspace member). Use:

```bash
cargo build --manifest-path guyos_core/Cargo.toml
cargo test --manifest-path guyos_core/Cargo.toml
```

---

## In scope

- **Phase A:** Full ADR layer directories and `pub(crate)` module graph; **no** behavioral change; **no** moving the chat implementation off `guyos_core/src/lib.rs` yet.
- **Phase B:** Move **`Chat`**, **`ChatMessage`**, and **all code only used by that UniFFI object** into `guyos_core/src/adapters/inbound/chat.rs`; shrink `lib.rs` to module wiring, **`//!` crate docs**, **`uniffi::setup_scaffolding!()`**, and **named** `pub use` re-exports only.
- **Bindgen:** If Phase B (or any edit) changes UniFFI exports or scaffolding, regenerate and commit Swift bindings per **§ UniFFI bindgen exit gate** below.

## Out of scope (this item)

- Moving **`mod llm`** / `llm.rs` into the library (stays on the binary / `main.rs`).
- Moving **`ChatError`** or **`pub type Result<T>`** out of `guyos_core/src/error.rs` (stay there per ADR; re-export from `lib.rs`).
- Introducing the **canonical inbound port trait**, an **application service** type, or **real** wiring in **`infrastructure/`** (stubs only).
- Splitting **`Chat`** into thin adapter + domain/ports beyond **file placement** (adapter may stay monolithic for now).
- **`pub use …::*`** at the crate facade (use **explicit** `pub use` per symbol).
- Repo-wide policy for explicit re-exports in *other* crates (see **Future work**).

---

## Stable public surface (`lib.rs`)

After Phase B, external callers rely on **named** re-exports from `lib.rs` only, for example:

- `Chat`, `ChatMessage`
- `ChatError`, `Result` (alias)

Do **not** promote `guyos_core::adapters::…` paths unless you deliberately change policy; layer modules stay **`pub(crate)`** or private until promoted.

---

## Runbook

### Phase A — Skeleton (module tree only)

**Goal:** `guyos_core` builds with the **full ADR layout** wired from `lib.rs`, while **all existing implementation—including UniFFI types—remains in `lib.rs`**.

#### Create / modify

| Action | Path |
| --- | --- |
| Create | `guyos_core/src/domain/mod.rs` |
| Create | `guyos_core/src/ports/mod.rs` |
| Create | `guyos_core/src/ports/inbound/mod.rs` |
| Create | `guyos_core/src/ports/outbound/mod.rs` |
| Create | `guyos_core/src/application/mod.rs` |
| Create | `guyos_core/src/adapters/mod.rs` |
| Create | `guyos_core/src/adapters/inbound/mod.rs` |
| Create | `guyos_core/src/adapters/outbound/mod.rs` |
| Create | `guyos_core/src/infrastructure/mod.rs` |
| Modify | `guyos_core/src/lib.rs` — add **`pub(crate)` module declarations** for the tree above only; **do not** move chat/domain logic yet |

**Do not** add `mod chat;` under `adapters/inbound/` until Phase B (or add it only when starting Phase B in the same branch—prefer **Phase A completes with inbound `mod.rs` empty or comment-only** so skeleton stays behavior-identical).

#### Steps

1. Add the files above with **minimal** `mod.rs` contents (may be empty aside from brief comments).
2. In `lib.rs`, declare the layer modules with **`pub(crate)`** visibility as needed so inner modules resolve; follow usual Rust nested `mod` patterns (`adapters` re-exports `inbound` / `outbound`, etc.).
3. **Do not** change `main.rs`, `llm.rs`, `error.rs`, `uniffi-bindgen.rs`, or Cargo `[[bin]]` / `[lib]` settings unless required for compilation.

#### Verify (Phase A)

```bash
cargo build --manifest-path guyos_core/Cargo.toml
cargo test --manifest-path guyos_core/Cargo.toml
```

Expect **no** change to UniFFI Swift output from this phase alone (no bindgen step required if you truly only added empty modules and `lib.rs` wiring).

---

### Phase B — Relocate UniFFI chat into `adapters/inbound`

**Goal:** Implementation of **`Chat` / `ChatMessage`** (and **only** code on their dependency fan-in from current `lib.rs`, e.g. `ChatInner`, `Message` / `Ticket`, gossip/iroh glue) lives in **`guyos_core/src/adapters/inbound/chat.rs`**. `lib.rs` becomes facade + scaffolding + **explicit** `pub use`. **`error.rs` unchanged.**

#### Create / modify

| Action | Path |
| --- | --- |
| Modify | `guyos_core/src/adapters/inbound/mod.rs` — add `mod chat;` and any **pub(crate)** re-exports needed for `lib.rs` |
| Create or heavily modify | `guyos_core/src/adapters/inbound/chat.rs` — receives the moved implementation |
| Modify | `guyos_core/src/lib.rs` — remove moved code; keep `mod` graph, `uniffi::setup_scaffolding!()`, **named** `pub use`, add **`//!` crate documentation** (stable re-exports, internal layers unstable, pointers to ADR 0002 + this doc) |
| Leave | `guyos_core/src/error.rs` (still defines `ChatError` and UniFFI error surface) |
| Leave | `guyos_core/src/main.rs`, `llm.rs`, **binary-only** `mod llm` |

**Rule of thumb:** If a type or function is **only** there to serve the current UniFFI **`Chat`** implementation, move it with **`chat.rs`**. If in doubt, keep it collocated with `Chat` for this item rather than inventing `domain/` types.

#### Steps

1. Add `chat.rs` and move the **UniFFI record/object** implementation and helpers from **`lib.rs`**.
2. Adjust imports (`use crate::…`, `use super::…`, `crate::error::ChatError`, etc.) until the crate compiles.
3. Add **`//!` on `lib.rs`** per **Out of scope**: document stable re-exports and point to ADR + this runbook.
4. Expose stable API via **explicit** lines such as `pub use crate::adapters::inbound::Chat;` — **no** glob re-exports.
5. If **`#[uniffi::…]`** surface, proc-macro exports, or **`uniffi::setup_scaffolding!()`** placement changed, run **§ UniFFI bindgen exit gate** before finishing.

#### Verify (Phase B)

```bash
cargo build --manifest-path guyos_core/Cargo.toml
cargo test --manifest-path guyos_core/Cargo.toml
```

**If UniFFI exports or scaffolding changed:**

```bash
./build-ios.sh
```

Then ensure **`GuyOSClient/Sources/GuyOSClient/guyos_core.swift`** is **committed** if generator output differs (no stray uncommitted binding drift).

---

## UniFFI bindgen exit gate (ongoing)

Apply when changing **any** of:

- `#[uniffi::export]`, `#[derive(uniffi::…)]`, or equivalent export surface
- Crate-level UniFFI scaffolding (`uniffi::setup_scaffolding!()` or similar)
- Anything that can alter generated Swift **checksums** or ABI

**Procedure:** from repo root, run `./build-ios.sh` (builds `guyos_core`, runs bindgen, copies Swift). For a lighter loop, mirror the bindgen invocation from that script against `guyos_core/target/debug/libguyos_core.dylib` after `cargo build`, then align **`GuyOSClient/Sources/GuyOSClient/guyos_core.swift`** with the result.

**Done means:** the Swift file matches the committed generator output for that change set; Rust tests passing **alone** is insufficient if bindings would change.

This rule is also under **Guiding decisions** in [`GUYOS_CORE_SPIKE_REFACTOR_TASKS.md`](../../GUYOS_CORE_SPIKE_REFACTOR_TASKS.md).

---

## Consumers (unchanged expectations)

| Consumer | Expectation |
| --- | --- |
| `guyos_core` binary | Keeps **`use guyos_core::Chat`**; **`mod llm`** stays out of the library crate |
| UniFFI / Swift | Stable Rust paths via `lib.rs` re-exports; regenerate Swift when the export surface changes |
| `uniffi-bindgen` bin | **`guyos_core/uniffi-bindgen.rs`** / `Cargo.toml` **`[[bin]]`** unchanged unless bindgen workflow requires it |

---

## Done-when checklist

- [ ] Phase A verify commands pass; behavior unchanged from pre–Phase A.
- [ ] Phase B verify commands pass; `lib.rs` is facade-only for moved code; **`adapters/inbound/chat.rs`** holds the chat implementation.
- [ ] Stable API exposed **only** via **named** `pub use` on `lib.rs`.
- [ ] If UniFFI changed in Phase B: `./build-ios.sh` (or equivalent) run; `guyos_core.swift` committed if output changed.

---

## Follow-on commits log

| Date | Commit / PR | Scope | Notes |
| --- | --- | --- | --- |
| — | — | — | — |

---

## Future work (not this item)

- Codify **explicit re-exports only** (no `pub use …::*`) for **all workspace crates** in an ADR or contributor guide.

---

## Risks / notes

- Today’s flat layout (`lib.rs`, `main.rs`, `llm.rs`, `error.rs`, `uniffi-bindgen.rs`) must remain **Cargo- and UniFFI-valid** throughout.
- Later Phase 0 tasks (**composition root**, thin entrypoints) will flesh out **`infrastructure/`**; keep it stubby here.

---

## Appendix A — Target layout (ADR 0002)

| Path | Responsibility |
| --- | --- |
| `src/domain/` | Pure domain model, invariants, value objects, domain errors |
| `src/ports/inbound/` | Traits inbound adapters call (canonical application API) |
| `src/ports/outbound/` | Traits the application needs from outside systems |
| `src/application/` | Use-case orchestration, lifecycle, supervision |
| `src/adapters/inbound/` | FFI, CLI fronts, etc. |
| `src/adapters/outbound/` | Transport, LLM clients, etc. |
| `src/infrastructure/` | Composition root and bootstrap wiring |
| `src/error.rs` | Shared structured internal errors |

`src/lib.rs`: module graph, `uniffi::setup_scaffolding!()`, **named** re-exports — **not** the composition root (that is `infrastructure/` per ADR).

---

## Appendix B — Decision log (context)

| # | Topic | Decision |
| --- | --- | --- |
| 1 | Skeleton vs big-bang | **Skeleton first**, then small relocation commits. |
| 2 | Public API | **Backward-compatible** `lib.rs` **named** re-exports. |
| 3 | Layer visibility | **`pub(crate)` / private** modules; **only `lib.rs`** defines stable paths until promoted. |
| 4 | `llm` | **Binary-only** for this item. |
| 5 | First relocation | **`Chat` + `ChatMessage` + chat-only dependency fan** → `adapters/inbound/chat.rs`; **`error.rs` stays**. |
| 6 | `//!` timing | **Phase B** (facade real), not Phase A only. |
| 7–8 | Commits | **Phase A:** tree only, impl stays in `lib.rs`. **Phase B:** move + facade. |
| 9 | File layout | `adapters/inbound/mod.rs` + **`chat.rs`**. |
| 10 | Re-exports | **Explicit** symbols; **no** globs. |
| 11 | ADR vs item | **No** inbound port trait / application service / real **infrastructure** wiring yet. |
| 12 | Bindgen | **Regenerate + commit Swift** when UniFFI surface or scaffolding changes. |
