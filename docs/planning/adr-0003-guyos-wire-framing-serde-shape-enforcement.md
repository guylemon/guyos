**Normative Sources Governing the Work**

All required message shapes, key sets, optionality, and serialization constraints are enumerated in ADR 0003 under the following sections:  
- “Message typing (T1)” and “Appendix A — Normative v1 message shapes” (the v1 `type` summary table and the “Required keys per `type`” specification);  
- “`seq` in JSON (N2)”;  
- “Forward compatibility (P1)”;  
- “Message typing (T1)” and “Flat `snake_case` for `type` and keys (K1)”;  
- “Serialization and framing (S1, F1)” (UTF-8 JSON bodies);  
- The closed error taxonomy in “Errors (E1)” and Appendix A (for malformed or invalid messages).

**Resolved implementation decisions**

These choices are locked for the implementation recipe; update this section as the grill-me session resolves further branches.

1. **Message errors vs session errors** — Semantic failures (malformed JSON, unknown `type`, missing required keys, wrong JSON value kinds, invalid `seq` string, non-canonical UUID, invalid RFC 3339, etc.) are reported by a **new domain-level error enum** aligned with ADR E1 / Appendix A portable codes. **`GuyosWireV1SessionError` is not extended**; it remains the framing and transport boundary only (`FrameTooLarge`, `UnexpectedEof`, `InvalidUtf8`, `Io`). Callers compose `Result` from the session port with decode/encode `Result` from the message layer.

2. **Inbound decode strategy** — **Object-first:** parse the payload as a JSON object into `serde_json::Map<String, Value>` (or an object `Value`), dispatch on the `type` string, then validate required keys and value shapes with explicit checks. Unknown keys are tolerated (P1) by reading only the normative key set. **Outbound** may use `serde` `Serialize` on domain types where convenient, provided emission still satisfies K1 and N2 (e.g. custom serialization so `seq` is always a decimal string, never a JSON number).

3. **Domain type layout** — Use a **single tagged enum** (name at implementer discretion, e.g. `GuyosWireV1Message`) with **one struct payload per wire `type`** (`attach`, `attach_ack`, `publish`, `publish_ack`, `chat_message`, `detach`, `keepalive`, `error`). The wire `error` shape is a normal variant of this enum alongside the others.

4. **Module placement and visibility** — Implement in **`guyos_core::domain::guyos_wire_v1`** (e.g. `src/domain/guyos_wire_v1.rs`; split into a `guyos_wire_v1/` tree only if size warrants). Declare the module from `domain/mod.rs` as **`pub(crate)`**. Do **not** add wire v1 symbols to the crate-root stable re-export surface in this task.

5. **RFC 3339 timestamps in domain structs** — Fields defined by ADR 0003 as RFC 3339 (e.g. optional **`server_timestamp`**) are represented as **`Option<time::OffsetDateTime>`**. Add **`time`** as a **direct** dependency of **`guyos_core`** for parsing inbound strings and formatting outbound strings in a normative way.

6. **Message-layer error enum shape** — Use **one Rust variant per portable error code** surfaced at this layer (names aligned with ADR E1 / Appendix A, e.g. `MalformedJson`, `UnknownMessageType { … }`). Add **tuple or struct fields** on a variant when tests or diagnostics need context (unknown `type` string, missing key name, etc.). Do **not** introduce a separate parallel “coarse category” enum plus `portable_code()` for this task.

7. **`error` message `details` field** — When present on the normative **`error`** shape, represent **`details`** as **`Option<serde_json::Map<String, serde_json::Value>>`**. On decode, require a JSON **object** for `details`; if the wire value is a non-object JSON value, fail through the message-layer error enum with the ADR-appropriate portable code (per Appendix A / E1). Higher layers may read known keys and ignore unknown nested keys for forward compatibility inside `details`, consistent with P1 spirit.

8. **Typed frame handling API shape** — Expose **synchronous** **`&str` / `String` ↔ `GuyosWireV1Message`** decode and encode entry points only. **Do not** add async helpers that take **`GuyosWireV1Session`** in this module. Callers (and unit tests) compose **`read_application_frame` → decode** and **encode → `write_application_frame`**. **`max_frame_bytes`** and the single-stream invariant remain enforced only at the existing framing/session boundary; the codec does not duplicate async session I/O.

9. **`Error` / `Display` for message-layer errors** — Implement **`std::fmt::Display`** and **`std::error::Error`** **manually** for the message-layer error enum (same policy as `GuyosWireV1SessionError`). **Do not** add **`thiserror`** or other helper crates for this surface.

10. **Unit test placement** — Colocate tests in a **`#[cfg(test)] mod …`** submodule at the bottom of **`domain/guyos_wire_v1.rs`** (same file as the primary implementation). Do not rely on crate-level integration tests in **`tests/`** for the required coverage in this task.

11. **`seq` decimal string grammar (N2)** — Implement **`seq`** parsing **exactly as specified in ADR 0003** if the ADR gives a concrete grammar or examples. If the ADR is **silent** on edge cases (leading zeros, whitespace, signs), use **strict canonical unsigned decimal:** the string must be **`"0"`** or match **`[1-9][0-9]*`** with no surrounding whitespace and no `+` prefix; otherwise return the message-layer error variant that maps to the ADR portable code for invalid `seq` / malformed numeric encoding.

12. **UUID wire strings (`client_message_id`, etc.)** — Follow **ADR 0003** if it defines allowed UUID string forms. If the ADR is **silent**, require **canonical UUID strings:** after **`Uuid::parse_str`**, accept only when **`wire == parsed.to_string()`** (hyphenated **8-4-4-4-12**, lowercase hex). Reject other parse-tolerant forms with the portable code the ADR assigns to invalid / non-canonical **`client_message_id`** (per Appendix A / E1).

13. **Top-level JSON kind** — After JSON parse succeeds, the payload must be a **JSON object** at the root. If the top-level value is an **array, string, number, boolean, or null**, fail with **`malformed_json`** (or the ADR portable code that corresponds to “invalid message document,” if named differently in Appendix A).

14. **Outbound JSON shape** — Encoded message JSON must be **minified**: **no insignificant whitespace** outside string values (typical `serde_json::to_string` output, or hand-built equivalent). Do not pretty-print outbound bodies.

**Concrete Deliverables**

1. **Domain Message Types**  
   Define Rust types as a **tagged enum with struct payloads** (see “Resolved implementation decisions”) that exactly mirror every normative v1 shape listed in ADR 0003 Appendix A: `attach`, `attach_ack`, `publish`, `publish_ack`, `chat_message`, `detach`, `keepalive`, and `error`. Each type must expose the precise required keys, optional fields (`server_timestamp`, `sender_endpoint_id`, `keepalive_interval_seconds`, `details` — see §7 for `details`), and correct Rust-native representations (e.g., `u64` for sequence numbers internally with **decimal string `seq` on the wire** per N2, `uuid::Uuid` for `client_message_id`, **`Option<time::OffsetDateTime>`** for RFC 3339 timestamps per resolved decision §5).

2. **Serialization and Deserialization Operations**  
   Provide (de)serialization functions or trait implementations that:  
   - Implement **decode** with the object-first `Map` / `Value` pipeline in “Resolved implementation decisions”; **encode** may use `serde` `Serialize` (or equivalent) if K1 and N2 remain guaranteed. **Encode must emit minified JSON** (§14).  
   - Emit and accept only flat `snake_case` keys and `type` values (K1);  
   - Enforce the exact set of required keys for each shape while ignoring unknown keys on decode (P1);  
   - Serialize the `seq` field as a decimal string, never as a JSON number (N2);  
   - Validate type correctness (integer ranges, UUID canonical form, RFC 3339 timestamps when present) and surface failures through the **domain message-layer error enum** (see “Resolved implementation decisions”), not `GuyosWireV1SessionError`.

3. **Typed Frame Handling Layer**  
   Implement **synchronous** operations (see §8) that convert between raw UTF-8 JSON **text** (`&str` / `String`) and typed domain messages, suitable for use **after** `GuyosWireV1Session::read_application_frame` and **before** `write_application_frame`. Do not introduce async session combinators in `domain::guyos_wire_v1`. The single-stream invariant and **`max_frame_bytes`** remain enforced only at the framing/session boundary already defined for task 3.0.

4. **Validation and Error Mapping**  
   Ensure that missing required keys, type mismatches, or invalid `seq` encoding produce well-defined errors that map cleanly onto the portable codes in ADR 0003 Appendix A (`malformed_json`, `unknown_message_type`, `invalid_client_message_id`, etc.). Unknown `type` values must be rejected at this layer (T1) while extra keys are tolerated (P1).

5. **Unit Test Suite**  
   Supply automated tests (see §10 for placement) that cover:  
   - Round-trip serialization/deserialization for every normative shape, including optional fields and empty `text`;  
   - Rejection of payloads missing required keys or containing incorrect types;  
   - Tolerance of additional unknown keys;  
   - Correct decimal-string encoding of `seq` values across the full `u64` range;  
   - Error paths for malformed JSON and non-canonical `client_message_id` strings.  
   All tests must execute in isolation using only the domain types and the framing helpers already delivered in task 3.0.

**Scope Boundaries**

No changes are required to the transport interface, ticket profile, or legacy protocol paths. No application-level wiring, iroh concrete adapter, or end-to-end relay tests are included in this task; those remain deferred. The work stays strictly inside the domain layer, producing types and operations that higher protocol logic can consume without reference to any concrete transport.

**Completion Criterion**

This task is discharged when the typed message shapes, (de)serialization logic, and supporting tests compile cleanly against the existing framing contract and pass all unit tests, thereby furnishing a complete, ADR-aligned message layer ready for the attach negotiation, publish semantics, and domain conformance work that follow.

**Implementation recipe (for an AI agent)**

Use ADR 0003 (Appendix A, T1, K1, N1/N2 as applicable, P1, E1) as the normative checklist alongside the resolved decisions §1–§14 above.

1. **Dependencies** — Add **`time`** as a direct **`guyos_core`** dependency with whatever features are needed for **`OffsetDateTime`** parse + RFC 3339 format consistent with ADR 0003. Do **not** add **`thiserror`**.

2. **Module wiring** — Create **`guyos_core/src/domain/guyos_wire_v1.rs`**, declare **`pub(crate) mod guyos_wire_v1`** from **`domain/mod.rs`**. Do **not** re-export wire types from **`lib.rs`**.

3. **Types** — Define one **struct per wire shape** and a single **`GuyosWireV1Message`** (name flexible) **tagged enum** wrapping those structs (§3). Use **`u64`** for internal `seq`, **`Option<time::OffsetDateTime>`** for RFC 3339 fields (§5), **`uuid::Uuid`** for UUID fields, **`Option<serde_json::Map<…>>`** for **`details`** on **`error`** (§7). Opaque wire strings per ADR as **`String`** unless ADR prescribes otherwise.

4. **Message-layer errors** — Define **`GuyosWireV1MessageError`** (name flexible) with **one variant per portable code** used at this layer (§6). Implement **`Display`** and **`Error`** manually (§9).

5. **Decode path** — `serde_json::from_str` → require **root JSON object** (§13) → **`Map<String, Value>`** → read **`type`** as string → **unknown `type`** → appropriate variant (T1) → per-shape functions that **require** all keys ADR marks required, **tolerate** extra keys (P1), validate JSON **value kinds** strictly, parse **`seq`** per ADR with **§11** fallback rules, UUID fields per **§12**, timestamps via **`time`**, **`details`** as object-only JSON object when present (§7). Map every failure to the correct **Appendix A / E1** variant.

6. **Encode path** — Produce **minified** JSON only (§14). Emit **only** normative **`snake_case`** keys and string **`type`** values (K1). Serialize **`seq`** as a **decimal string**, never a JSON number (N2). Format timestamps per ADR; if silent, use a single consistent RFC 3339 profile compatible with **`OffsetDateTime`**.

7. **API surface** — Export **sync** functions such as **`decode_*(&str) -> Result<GuyosWireV1Message, GuyosWireV1MessageError>`** and **`encode_*(&GuyosWireV1Message) -> Result<String, GuyosWireV1MessageError>`** (exact names at implementer discretion). **No** async **`GuyosWireV1Session`** wrappers in this module (§8).

8. **Tests** — At the bottom of **`guyos_wire_v1.rs`**, add **`#[cfg(test)]`** coverage per deliverable 5: round-trip all shapes (including optionals and empty **`text`**), missing/wrong-type keys, unknown top-level keys ignored, **`seq`** string round-trip across **`u64`**, malformed JSON, non-canonical UUID strings, non-object **`details`** when exercised. Where framing is needed, compose **`FakeGuyosWireV1Session`** with **`read_application_frame` / `write_application_frame`** and the sync codec (§8, §10).

9. **Verify** — **`cargo test -p guyos_core`** (or workspace equivalent) passes with **no new** `GuyosWireV1SessionError` variants.
