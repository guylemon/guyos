# ADR 0004: Ticket profiles and reference profile v1

## Status

Accepted

## Context

[ADR 0003](0003-wire-protocol-and-compatibility.md) defines the **client ↔ hub** wire protocol, including `attach.ticket` as an **opaque UTF-8 string** and the error codes `**ticket_decode_failed`** vs `**invalid_ticket`**. It deliberately leaves **concrete ticket bytes** to **ticket profiles** (server-side selection; **TV2** — no separate ticket-format version on the wire).

This ADR specifies:

1. A **normative framework** that every ticket profile must follow (naming, registry, decode/validate ordering, error mapping, outputs).
2. A **normative reference profile** named `**guyos.reference_v1`** so implementations and tests have a **portable** baseline.
3. **Non-normative** appendices for operators and for illustrative second profiles.

Companion artifacts (normative where stated):

- `[docs/ticket-profile-registry.json](../ticket-profile-registry.json)`
- `[docs/fixtures/0004-ticket-profile-reference-v1.json](../fixtures/0004-ticket-profile-reference-v1.json)`

## Decision

### Normative framework (all profiles)

#### Profile identifier grammar and registry (**R2**, **U1**)

Profile identifiers are opaque strings that **MUST** conform to this grammar (ABNF):

```abnf
profile-id = profile-first 0*63(profile-next)
profile-first = ALPHA / DIGIT
profile-next  = ALPHA / DIGIT / "." / "_" / "-"
```

**Constraints (normative prose, in addition to the grammar):**

- Length is **1–64** characters inclusive (the `0*63` repetition applies after the first character).
- Identifiers **MUST NOT** begin or end with `.`, `_`, or `-`.
- Identifiers are **case-sensitive**; new registrations **SHOULD** use **lowercase** ASCII only.

**Allocation rules (normative):**

- The prefix `guyos.` is reserved for profiles defined in this specification family (this ADR and successors).
- Other identifiers **SHOULD** use reverse-DNS or vendor naming to reduce collision risk.

**Registry file (normative):**

The authoritative list of registered `profile_id` values, their **stability** classification, human **title**, and **defining_spec** pointer is maintained at:

`docs/ticket-profile-registry.json` (repository root–relative path).

That JSON file is **part of the normative specification**. Implementations **MUST** treat any `profile_id` not present in the **current** registry file as **unregistered** for purposes of **wire-visible profile identification** (see **Future wire coupling (`profile_hint`)**).

**Registry row shape (R-ops):** each object in the `profiles` array **MUST** contain:


| Field           | Meaning                                                                                                                |
| --------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `profile_id`    | Identifier matching `**profile-id`** grammar and allocation rules.                                                     |
| `stability`     | One of `normative`, `experimental`, `deprecated` (string).                                                             |
| `defining_spec` | Repository-relative pointer to the defining text (typically `docs/adr/0004-ticket-profiles-and-reference-profile.md`). |
| `title`         | Short human-readable title.                                                                                            |


**Forward compatibility:** decoders of the registry file **MUST** ignore unknown object keys at the top level and within each profile object (same spirit as ADR 0003 **P1**).

**Conformance testing note (normative intent, non-blocking for deployments):** portable conformance vectors **MUST** target rows with `stability: "normative"` unless a test explicitly targets experimental work. The `experimental` value is **informational for operators** and does not obligate hubs to implement those profiles.

#### Single active profile per listener (**M1**)

For a given hub **listener** (or other deployment boundary the product defines), configuration **MUST** designate **exactly one** registered `profile_id` as the **active** decoder for `attach.ticket`. The hub **MUST NOT** attempt multiple registered profiles in priority order on that listener for v1.

Deployment-private ticket formats that do **not** use registered wire-visible identifiers remain possible **only** by **configuration-only** selection (no unregistered `profile_id` strings on the wire).

#### Required outputs: `room_id` (**byte-for-byte** semantics)

A successful ticket decode **MUST** yield a **canonical room identifier string** used as ADR 0003 `**attach_ack.room_id`**: **opaque UTF-8** whose equality semantics are **byte-for-byte** after JSON unescape (per ADR 0003 **Sessions and routing**).

Each profile defines how that string is derived from validated ticket bytes. For `**guyos.reference_v1`**, see **Room identifier string (C3)** below.

#### Error code mapping (identical taxonomy for every profile)

Ticket profiles **MUST** map failures to ADR 0003’s portable meanings using **only** these codes for ticket-specific failures:


| Code                   | Meaning (portable)                                                                                                                                                                                                                                     |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `ticket_decode_failed` | Failure **before** the ticket can be interpreted far enough to apply the profile’s semantic validation rules (including shape, encoding, framing of the opaque string, and **early** structural checks). Same portable meaning as ADR 0003 Appendix A. |
| `invalid_ticket`       | The ticket was interpreted far enough to apply the profile’s semantic/cryptographic checks, but **failed** them (including expiry and MAC/signature verification failures). Same portable meaning as ADR 0003 Appendix A.                              |


Profiles **MUST** document an **ordered decode/validate checklist** with **mandatory** early vs late failure classification for each step (`**ticket_decode_failed`** vs `**invalid_ticket`**). Receivers **SHOULD** fail as early as the checklist permits (without weakening security checks).

#### Profile versioning (no wire impact)

Ticket profile specifications **MAY** evolve **additively** within the same `profile_id` by tightening validation, clarifying text, or adding **optional** claims **without** changing ADR 0003 `**protocol_major`**, provided the wire obligations of ADR 0003 remain satisfied.

**Breaking** changes to a profile’s byte layout or semantics **SHOULD** be published as a **new** registered `profile_id` (or a new major document revision with explicit migration guidance), not as a silent incompatible change to an existing id.

#### Constant-time MAC verification (**CT-MUST**)

For profiles that use symmetric MAC verification (including `**guyos.reference_v1`**), implementations **MUST** verify the MAC using a **constant-time** comparison primitive or an **approved cryptographic API** that provides equivalent resistance (for example `hmac.compare_digest` in Python, `crypto.timingSafeEqual` in Node.js, or `subtle.timingSafeEqual` on digest bytes in browsers). Implementations **MUST NOT** branch early on partial digest comparisons.

### Reference profile v1 (`guyos.reference_v1`)

This section defines the **normative** on-the-wire ticket format and processing rules for `**guyos.reference_v1`**, for **both issuers (signing)** and hubs (**verification**) — **B-both**.

#### Signing / verification key (**K3** normative core)

The reference profile uses a **32-octet** symmetric key `k_mac` for **HMAC-SHA256**.

- `k_mac` **MUST** consist of exactly **32 octets** of uniformly random secret material from a **CSPRNG**.
- The issuer and every hub that accepts tickets under this profile **MUST** share **identical** `k_mac` octets.
- Key distribution and rotation are **deployment concerns** and are **out of scope** for this ADR.

**No key derivation function is part of the normative profile.**

See **Appendix A — Recommended key derivation (non-normative)** for passphrase-based deployments.

#### HMAC domain separation (**D1**)

Define the 16-octet prefix `**PREFIX`** as the UTF-8 encoding of the 15 ASCII characters `**guyos-ticket-v1`**, immediately followed by a single NUL octet `**0x00`**.

```
PREFIX = UTF8("guyos-ticket-v1") || 0x00   ; exactly 16 octets
```

Let `body` be the 25-octet concatenation:

```text
body = version || room_id || expires_unix
```

- `version` — **1 octet**. For this profile revision, `**version` MUST be `0x01`**.
- `room_id` — **16 octets**. The raw UUID octet sequence (see **Room identifier string (C3)**).
- `expires_unix` — **8 octets**, big-endian unsigned integer. `**0` means no expiry**. Otherwise, **UTC Unix seconds** at which the ticket ceases to be valid (**T1**), compared against the hub’s current Unix time `now` in whole seconds: valid iff `now <= expires_unix` (inclusive boundary).

The HMAC input is:

```text
mac_message = PREFIX || body
signature     = HMAC-SHA256(k_mac, mac_message)
```

`signature` is **32 octets** (raw HMAC output).

The **decoded ticket binary** is:

```text
ticket_binary = body || signature     ; exactly 57 octets
```

#### Wire encoding (**PAD-strict**)

The opaque `attach.ticket` string is:

```text
ticket = BASE64URL_NOPAD(ticket_binary)
```

**BASE64URL_NOPAD** means **RFC 4648** base64url alphabet `**A–Z` `a–z` `0–9` `-` `_`** with **no** `=` padding octets.

Implementations **MUST** reject tickets that contain any character outside that alphabet, `**=` padding**, or **ASCII whitespace**, with `**ticket_decode_failed`** (before or as part of base64 decoding, as long as the outcome code is stable).

#### Room identifier string (**C3**)

Let `room_id` be the 16-octet UUID payload. The canonical `**room_id` string** is the **RFC 4122 / RFC 9562** textual UUID form: **lowercase** hexadecimal with hyphens (`8-4-4-4-12`), with field layout derived from those 16 octets as a **128-bit UUID**. Implementations **MUST NOT** impose UUID version/variant semantic constraints in v1 (**C3**); formatting alone is normative.

#### Hub verification checklist (normative order)

Given opaque UTF-8 `ticket`, octet string `k_mac`, and hub **Unix time** `now` in **whole seconds**:

1. **Alphabet and padding (**`PAD-strict`**) —** if any character is outside the base64url set or `=` appears → `**ticket_decode_failed`**.
2. **Base64url decode —** if decoding fails → `**ticket_decode_failed`**.
3. **Length —** if decoded length is not **57** → `**ticket_decode_failed`**.
4. **Version —** if `version != 0x01` → `**ticket_decode_failed`**.
5. **MAC —** recompute `HMAC-SHA256(k_mac, PREFIX || body)` where `body` is the first 25 octets; compare to `signature` using **CT-MUST**. On mismatch → `**invalid_ticket`**.
6. **Expiry —** if `expires_unix != 0` and `now > expires_unix` → `**invalid_ticket`**.
7. **Success —** derive `**room_id` string** per **Room identifier string (C3)** from the 16-octet `room_id` field.

#### Issuer signing steps (normative)

Given `k_mac`, 16-octet UUID `room_id`, and optional expiry `expires_unix` (**uint64**, `0` means none):

1. Set `version = 0x01`.
2. Form `body = version || room_id || BE_uint64(expires_unix)`.
3. Compute `signature = HMAC-SHA256(k_mac, PREFIX || body)`.
4. Emit `ticket = BASE64URL_NOPAD(body || signature)`.

#### Issuer / hub time (**IS-guide**)

Normatively, `**expires_unix`** (when non-zero) is interpreted as **UTC Unix seconds** on the hub. This ADR does **not** mandate how issuers obtain time.

**Non-normative:** see **Appendix B — Issuer time guidance**.

### Future wire coupling (`profile_hint`) (**D2**, **H-B**, **X1**, **D-free**)

A **future minor revision** of ADR 0003 may introduce an **optional** top-level `attach` field, tentatively named `**profile_hint`**, whose value is a `**profile_id` string** from `[docs/ticket-profile-registry.json](../ticket-profile-registry.json)`.

Until that ADR revision is accepted:

- Conforming clients **MUST NOT** send `profile_hint`.
- Hubs **MUST** ignore unknown `attach` keys for **P1** compatibility if a premature client sends the field anyway.

After acceptance (normative split):

- **ADR 0003** owns wire-local rules: field presence, JSON types, ordering relative to other `attach` validation, and **P1** behavior.
- **This ADR** owns registry semantics: `profile_hint`, when present, **MUST** equal the hub’s configured active `**profile_id` exactly** (**strict matching**). If it does not, or if the value is **unregistered**, the hub **MUST** reject `**attach`** with `**error.code = invalid_attach`** (**X1**). Hubs **SHOULD** validate `**profile_hint`** (when present) **before** attempting `**ticket`** decode so failures do not masquerade as ticket decode errors.

`**error.details`:** no normative structured subclass keys for these failures (**D-free**); clients **MUST NOT** depend on `details`.

### Golden vectors (**G3**, **E2**, **S2**)

Normative vectors for `**guyos.reference_v1`** live in:

`[docs/fixtures/0004-ticket-profile-reference-v1.json](../fixtures/0004-ticket-profile-reference-v1.json)`

That file **MUST** use the wrapped object shape:

- `fixture_schema_version` (integer, starts at **1**)
- `profile_id` (**MUST** be `guyos.reference_v1` for this file)
- `vectors` (array)

**Schema evolution (`**S2`**):** bump `fixture_schema_version` when removing or redefining required fields or changing `expect` semantics. **New optional** keys may be added **without** a bump; parsers **MUST** ignore unknown keys on the wrapper and on each vector object.

**Minimum required vector `name`s (normative coverage):** the file **MUST** include at least one vector each for: `ok_no_expiry`, `ok_with_expiry`, `ok_expiry_boundary`, `expired`, `bad_signature`, `wrong_version`, `wrong_length_short`, `wrong_length_long`, `invalid_base64url_padding`, `invalid_base64url_alphabet`, `truncated_base64url`.

## Consequences

- Hubs and test harnesses can implement `**guyos.reference_v1`** independently of any particular issuer stack, with **byte-stable** golden vectors.
- **PAD-strict** base64url and the **57-octet** layout keep tickets canonical across languages.
- **TV2** remains true: ticket profile evolution can proceed without ADR 0003 `**protocol_major`** bumps **until** ticket handling changes would break wire semantics.
- Operators must still compose **TLS / authorization / pinning** separately (ADR 0003 **Sec1**).

## Open points

- **Future ADR 0003 minor:** finalize the `profile_hint` field name, attach ordering text, and acceptance criteria.
- **Additional profiles:** register new `profile_id` rows and define their checklists in this ADR family or linked documents.
- **Fixture-only keys:** optional observability fields may be added to vectors under **S2** rules.

---

## Appendix A — Recommended key derivation (non-normative)

For operators who prefer a human-memorable secret instead of raw 32-octet material, the following **HKDF-SHA256** construction is **recommended** (but **not required** for interoperability):

```text
k_mac = HKDF-SHA256(
    ikm  = passphrase encoded as UTF-8,
    salt = UTF-8("guyos-ticket-v1"),
    info = UTF-8("mac-key"),
    L    = 32
)
```

Implementations that follow this recipe can derive identical `k_mac` octets from the same passphrase, but **interoperability of tickets** still requires **all** participating hubs and issuers to agree on the **derived** `k_mac` (or to configure the same raw `k_mac` directly).

## Appendix B — Issuer time guidance (non-normative)

Issuers that set `**expires_unix != 0`** should use a **reliable UTC clock**, issue tickets with enough **margin** to account for hub/issuer skew under **T1** (no skew tolerance), and monitor for systematic clock drift.

## Appendix C — JWT profile sketch (non-normative)

A second profile might wrap a **minimal JWT** (fixed algorithm allow-list, small claim set, strict header rules). Such a profile would supply its own checklist and **MUST** still map failures to `**ticket_decode_failed`** vs `**invalid_ticket`** per the framework above. This ADR does **not** define that profile normatively.

## Appendix D — Choosing profiles (non-normative)

Use `**guyos.reference_v1`** when you want **minimal dependencies** and **HMAC-based** join tickets. Consider richer profiles (for example JWT-based) when integrating with existing identity issuers—at the cost of larger tickets and stricter parser discipline.
