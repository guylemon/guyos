# Planning: Reference ticket profile (`guyos.reference_v1`) — ADR 0004

## References

- **Normative spec:** [`docs/adr/0004-ticket-profiles-and-reference-profile.md`](../adr/0004-ticket-profiles-and-reference-profile.md)
- **Golden vectors:** [`docs/fixtures/0004-ticket-profile-reference-v1.json`](../fixtures/0004-ticket-profile-reference-v1.json)
- **Registry (context):** [`docs/ticket-profile-registry.json`](../ticket-profile-registry.json)
- **Wire errors (taxonomy):** ADR 0003 — `ticket_decode_failed` vs `invalid_ticket` for ticket-specific failures

## Goal

Implement **types and logic** for the **reference ticket profile** so that:

1. **Binary layout** — Encode and decode the **57-octet** `ticket_binary = body || signature` where `body = version (1) || room_id (16) || expires_unix (8 BE)` and `signature` is **32** octets of **HMAC-SHA256** over `PREFIX || body` per ADR **K3**, **D1**, and issuer steps.
2. **Wire form** — **PAD-strict** `BASE64URL_NOPAD` of `ticket_binary` (RFC 4648 base64url, no `=`, no whitespace, alphabet enforced before decode).
3. **Hub verification** — Apply the **ordered checklist** exactly (early vs late error codes as specified): PAD → decode → length **57** → **version == 0x01** → **constant-time** MAC compare → **inclusive** expiry (`0` = no expiry; else valid iff `now <= expires_unix`).
4. **Outputs** — On success, derive **canonical `room_id` string** (RFC 4122 / 9562 **lowercase** hyphenated 8-4-4-4-12) from the **16** octets **without** UUID version/variant checks (**C3**). Use the **`uuid`** crate **only** to format bytes to the hyphenated string (e.g. `Uuid::from_bytes` + display); **do not** add semantic UUID validation beyond what the ADR allows (Question 6).
5. **Errors** — Map every failure to **`ticket_decode_failed`** or **`invalid_ticket`** per the checklist (no extra portable codes for these steps).
6. **Conformance** — Automated test that loads the fixture JSON, runs **every** vector with the documented `key_hex` / `now_unix`, and asserts **`expect`** (`ok` + `room_id`, or `error` + `code`) exactly.

## Explicitly out of scope

- **Protocol wiring:** No changes to **`attach` / `attach.ticket` / `attach_ack`**, hub session handlers, JSON-RPC or wire framing, **`ChatError`**, or UniFFI surfaces for this milestone. Decoding and encoding exist as **library logic** only until a later change explicitly connects them to the wire.

## Repository anchor

- Primary implementation target today: **`guyos_core/`** (Rust 2024, UniFFI; see [`docs/planning/phase-0-module-layout.md`](phase-0-module-layout.md) for layer conventions).
- **Errors for this profile:** A dedicated **`ReferenceTicketError`** (or equivalent name) with two variants aligned to **`ticket_decode_failed`** and **`invalid_ticket`**. **`ChatError` is unchanged** (decision **B**).

## Implementation outline

### Constants and layout

- **`PREFIX`:** 16 octets — UTF-8 of `guyos-ticket-v1` + `0x00` (verify length in tests).
- **`VERSION_V1`:** `0x01`.
- **`TICKET_BINARY_LEN`:** `57` (`25` body + `32` signature).

### Public API (proposed shape — refine during implementation)

- **`pub const REFERENCE_TICKET_PROFILE_ID: &str = "guyos.reference_v1"`** — registry / future wire alignment (Question 12); **no** protocol wiring in this milestone.
- **`encode_reference_v1(k_mac: &[u8; 32], room_id: &[u8; 16], expires_unix: u64) -> String`** — issuer path; PAD-strict base64url output.
- **`decode_reference_v1(ticket: &str, k_mac: &[u8; 32], now_unix: u64) -> Result<String, ReferenceTicketError>`** — hub path; **`String`** is canonical **`room_id`** on success. Use **`uuid`** internally only to format bytes → hyphenated lowercase text (Question 7).

### `ReferenceTicketError` (Question 8)

- Variants **`TicketDecodeFailed`** and **`InvalidTicket`** — PascalCase required by Rust; each corresponds **one-to-one** to the portable codes **`ticket_decode_failed`** and **`invalid_ticket`**.
- Expose **`fn portable_code(&self) -> &'static str`** (or equivalent) returning those **exact** wire strings (for conformance assertions and future protocol wiring). **`ChatError`** may be refactored later for clarity; it stays **unchanged** in this milestone.

Internal helpers: PAD-strict scan, base64url decode (length after decode), split `body` / `signature`, HMAC recompute, **`subtle::ConstantTimeEq`** (or equivalent) on the **32** digest octets, expiry check.

### Dependency notes

- **Base64url:** Crate already includes **`data-encoding`**. **PAD-strict (Question 5):** run an **explicit byte scan** first (allowed base64url alphabet only; reject `=` and ASCII whitespace), then **`BASE64URL_NOPAD::decode`**, so step 1 vs step 2 of the ADR checklist map cleanly to **`ticket_decode_failed`**.
- **HMAC-SHA256:** **`hmac`** + **`sha2`** (RustCrypto) + **`subtle`** for constant-time digest equality (**CT-MUST**). Decision **Question 3**.
- **`uuid`:** Add for **C3** hyphenated lowercase formatting from raw 16 octets only (Question 6); use **`default-features = false`** (or the minimal feature set) if feasible so **`serde`** is not pulled in unless needed elsewhere.

### Module placement

- **Implementation file:** **`guyos_core/src/domain/reference_ticket_v1.rs`**, wired from **`domain/mod.rs`** (Question 9). **Stable `pub` API:** named **`pub use`** from **`lib.rs`** for **`REFERENCE_TICKET_PROFILE_ID`**, `encode_reference_v1`, `decode_reference_v1`, and `ReferenceTicketError`. UniFFI / chat adapter stays **untouched** per out-of-scope rules above.

### Conformance test

- **Integration test file:** **`guyos_core/tests/reference_ticket_v1_conformance.rs`** (Question 11).
- **`include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/fixtures/0004-ticket-profile-reference-v1.json"))`** (or path relative to crate after verifying **CARGO_MANIFEST_DIR** resolution from `guyos_core`).
- Parse with **`serde_json`**; iterate **`vectors`**; for each: decode `key_hex` using the **`hex`** crate as a **`[dev-dependencies]`** entry (Question 4); call decode (and optionally encode round-trip only where `expect.kind == "ok"` if you add cross-checks), assert **exact** `expect`.
- For **`expect.kind == "error"`**, assert **`err.portable_code()`** equals the fixture **`code`** string (primary / normative check) (Question 10). For **`ok`**, assert **`room_id`** string equality.
- Ignore unknown keys per **S2** / ADR forward-compatibility intent where applicable.

### Vectors coverage checklist (fixture file)

ADR **G3** minimum names — confirm fixture contains each: `ok_no_expiry`, `ok_with_expiry`, `ok_expiry_boundary`, `expired`, `bad_signature`, `wrong_version`, `wrong_length_short`, `wrong_length_long`, `invalid_base64url_padding`, `invalid_base64url_alphabet`, `truncated_base64url` (current JSON matches this set).

## Decision log (grill-me)

Decisions below are filled in as the implementation interview proceeds.

| Topic | Decision | Rationale |
| --- | --- | --- |
| Portable error representation in Rust | **B** — `ReferenceTicketError` only; **no** `ChatError` / UniFFI change | Build ticket logic without coupling to the wire or Swift until wiring is an explicit milestone. |
| Protocol / attach wiring | **Out of scope** | User requirement: implement types and logic only. |
| Crate public surface for ticket API | **`pub`** — explicit re-exports from `lib.rs` | Matches phase-0 named re-export style; integration conformance tests exercise the same surface future wire code will call. |
| Crypto stack for HMAC / CT compare | **RustCrypto** — `hmac` + `sha2` + `subtle` | Pure Rust, auditable; no `ring` for this path (Question 3). |
| Fixture `key_hex` in tests | **`hex` as `[dev-dependencies]`** | Small standard helper; production code stays free of hex parsing (Question 4). |
| PAD-strict step 1 | **Explicit scan**, then base64url decode | Matches ADR checklist order; stable `ticket_decode_failed` for alphabet / padding / whitespace (Question 5). |
| Canonical `room_id` (**C3**) | **`uuid`** crate for formatting | User choice; use `Uuid::from_bytes` (or equivalent) + hyphenated lowercase string — **no** extra version/variant gating beyond ADR (Question 6). |
| Decode success type | **`String`** (`room_id`); `uuid` **internal** only | Matches ADR wire output and fixture `expect.room_id`; callers are not forced to use `Uuid` in their API (Question 7). |
| `ReferenceTicketError` shape | **`TicketDecodeFailed`** / **`InvalidTicket`** + **`portable_code()`** → exact ADR strings | Names track wire codes (Rust PascalCase); exact codes for tests / future glue; **`ChatError`** refactor deferred (Question 8). |
| Source layout | **`domain/reference_ticket_v1.rs`** | Single cohesive module; defer `domain/ticket/` until a second profile exists (Question 9). |
| Conformance error assertions | **`portable_code()`** vs fixture `code` (primary) | Locks exact ADR strings; resilient if variant names change (Question 10). |
| Conformance integration test file | **`tests/reference_ticket_v1_conformance.rs`** | Clear scope naming; not ADR-number-only (Question 11). |
| `profile_id` literal in public API | **`pub const REFERENCE_TICKET_PROFILE_ID`** | Single source for registry string; no attach / `profile_hint` wiring yet (Question 12). |

## Grill-me status

Design branches for this milestone are **resolved** in the decision log above. **Next step:** implement per **Exit criteria** and keep **protocol wiring** explicitly out of scope.

## Exit criteria

- All checklist steps match ADR order and error mapping.
- MAC verification satisfies **CT-MUST**.
- Conformance test passes against the committed fixture file with **no** vector skipped.
- No normative behavior contradicted by ADR 0004 or the fixture’s documented `expect` outcomes.
