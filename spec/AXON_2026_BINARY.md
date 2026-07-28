# AXON 2026 -- Binary Encoding ("Binary AXON")

**Companion specification  /  working draft (revision 2)**
**Reference codec:** `axon.binary2026` (`encode`/`decode`) in pyaxon 0.11.0a13, covering the full value model with canonical output, shared/cyclic encoding, and a decoder. Revision 2 closes the findings of `AUDIT_AXON2026_BINARY_12-07-2026_1104`: the reference encoder now produces Canonical Binary AXON (Section 6), implements shared-reference and cyclic encoding (Section 5.12) with a depth guard, resolves links by kind, and ships an inverse decoder (Section 7).
**Depends on:** AXON 2026 language spec (value model Section 2, canonical form Section 14, interop Section 14.7, graph profile Section 10, resource limits Section 16). Where this document says "the value model," it means the semantic value model of Section 2.2 -- never the text surface syntax.

Binary AXON is a **compact, self-describing, canonicalisable binary serialisation of the AXON 2026 value model**. It is to AXON what CBOR/MessagePack are to JSON: the same values, in bytes rather than text, for size-sensitive, high-throughput, and content-addressed use. Every value expressible in text AXON has exactly one Binary AXON shape, and decoding reproduces the *identical* semantic value -- including the distinctions text AXON is careful about (tuple vs list, ordered map vs map, unit node vs empty-tuple-bodied node, `Z` vs `+00:00`, decimal scale, shared and cyclic structure).

All examples in this document are **real bytes** produced by a reference encoder over values parsed by the pyaxon 0.11.0a13 reference implementation, and independently confirmed to be well-formed CBOR.

---

## 1. Design: a CBOR profile

Binary AXON **is CBOR** (RFC 8949): every Binary AXON document is a single well-formed CBOR data item, and every conforming CBOR decoder can read its structure. This is a deliberate reuse decision, consistent with the rest of AXON 2026 (which builds canonical form on dCBOR + JCS rather than reinventing them):

- **Self-description for free.** CBOR's major types already carry integers, byte/text strings, arrays, maps, floats, and tags with lengths -- no bespoke framing to specify or get wrong.
- **Instant tooling.** Any CBOR or DAG-CBOR library decodes the structure today; AXON-specific kinds ride on CBOR *tags*, which unknown decoders surface as opaque tagged values rather than failing.
- **Determinism is a solved problem.** Core Deterministic CBOR (dCBOR / `draft-mcnally-deterministic-cbor`) gives canonical integers, floats, and map ordering; Section 6 layers AXON's canonical rules (Section 14) on top for a stable, CID-ready form.
- **A clean interop seam.** The encoding aligns with the Section 14.7 AXON<->CBOR mappings and with DAG-CBOR conventions (tag 42 CID, tag 258 set), so Binary AXON and the wider CBOR ecosystem interoperate.

Where CBOR already distinguishes a kind, Binary AXON uses the native representation; where AXON has a kind CBOR does not natively separate, Binary AXON uses a **tag**. The AXON tag block is provisional (private-use, pending registration) and MUST be treated as normative-by-value within this edition.

---

## 2. Tag registry

**Standard CBOR tags reused (normative):**

| Tag | Meaning | Used for |
|----:|---|---|
| 2 / 3 | positive / negative bignum | integers outside 64-bit |
| 4 | decimal fraction `[exponent, mantissa]` | finite exact decimals |
| 28 / 29 | shared value mark / reference | graphs, shared and cyclic structure |
| 32 | URI | `Link` (URI) |
| 42 | content identifier (CID) | `Link` (content-addressed) |
| 258 | set | `Set` |

**Provisional AXON tags (this edition; private-use, pending registration):**

| Tag | Name | Payload |
|----:|---|---|
| 722 | `axon-magic` | edition marker after the CBOR magic prefix (Section 3) |
| 720 | `axon-node` | `[body_kind, name, payload]` (Section 5.11) |
| 724 | `axon-tuple` | array of elements (Section 5.8) |
| 725 | `axon-omap` | array of `[key, value]` pairs, order significant (Section 5.9) |
| 727 | `axon-temporal` | array, kind-tagged (Section 5.7) |
| 731 | `axon-decimal-special` | `0`=+∞, `1`=-∞, `2`=NaN (Section 5.4) |

A decoder that does not recognise a provisional AXON tag MUST preserve it as a tagged value (CBOR's standard fallback); it MUST NOT silently drop the tag and treat the payload as a bare array/text (doing so would corrupt a node into a list, a tuple into a list, etc.).

---

## 3. Framing and self-description

A Binary AXON **document** is exactly one CBOR data item (Section 5), which MAY itself be a value, a node, or -- for a multi-value AXON stream (Section 11 of the language spec) -- a CBOR array of items understood as a stream, or a CBOR sequence (RFC 8742) when the transport frames items. An optional **self-identifying prefix** MAY precede a document: CBOR tag 55799 (the standard "CBOR magic" `0xd9d9f7`), OPTIONALLY followed by the provisional tag `0xd9 0x02 0xd2` (`axon-magic`, 722) carrying an edition marker. The prefix is advisory; canonical Binary AXON (Section 6) omits it, because canonical bytes describe a value, not a container. No other framing is defined; CBOR's length prefixes make every item self-delimiting for streaming (Section 8).

---

## 4. Encoding overview

Every Section 2.2 value kind maps to exactly one encoding:

| AXON kind | Encoding | Example (hex) |
|---|---|---|
| Null | `0xf6` | `f6` |
| Bool | `0xf4` / `0xf5` | `f5` (true) |
| Int | CBOR major 0/1; bignum tag 2/3 beyond 64-bit | `182a` (=42) |
| Float | CBOR float; specials native | `fb400921f9f01b866e` (3.14159) |
| Decimal | tag 4 `[exp, mant]`; specials tag 731 | `c482211a000186c3` (1000.35) |
| String | CBOR text (major 3) | `65636166c3a9` ("café") |
| Bytes | CBOR byte string (major 2) | `4548656c6c6f` (b"Hello") |
| Date/Time/DateTime | tag 727 | see Section 5.7 |
| List | CBOR array (major 4) | `83010203` ([1 2 3]) |
| Tuple | tag 724 + array | `d902d483010203` ((1 2 3)) |
| Map | CBOR map (major 5) | `a2616101616202` ({a:1 b:2}) |
| OrderedMap | tag 725 + pair array | `d902d5828261610182616202` |
| Set | tag 258 + array | `d9010283010203` ({1 2 3}) |
| Node | tag 720 | see Section 5.11 |
| Reference/graph | tags 28/29 | see Section 5.12 |
| Link | tag 42 (CID) / tag 32 (URI) | see Section 5.13 |

These are the verified reference vectors; the full table with all kinds is the Appendix.

---

## 5. Encoding of the value model (normative)

### 5.1 Null and booleans
`Null` -> `0xf6`. `false` -> `0xf4`, `true` -> `0xf5` (CBOR simple values). One byte each.

### 5.2 Integers
A finite `Int` in `[-(2^64), 2^64-1]` is encoded as CBOR major type 0 (non-negative) or 1 (negative), shortest form (Section 6). Outside that range it is a **bignum**: tag 2 (non-negative) or tag 3 (negative, encoding `-1 - n`) wrapping the minimal big-endian byte string. `Int` is arbitrary-precision and MUST NOT be narrowed. Example: `123456789012345678901234567890` -> `c2 4d 018ee90ff6c373e0ee4e3f0ad2` (tag 2, 13-byte magnitude).

### 5.3 Floats
A `Float` is a CBOR floating-point value. The canonical form (Section 6) is the shortest of half/single/double that round-trips the binary64 value, per dCBOR. Specials: `+∞` -> `0xf9 7c00`, `-∞` -> `0xf9 fc00`, quiet NaN -> `0xf9 7e00` (the single canonical NaN, matching Section 5.5/Section 14 of the language spec). `-0.0` is preserved as distinct from `+0.0`. Example: `3.14159` -> `fb 400921f9f01b866e` (double; a canonical encoder MAY emit a shorter form iff it round-trips).

### 5.4 Decimals
A finite `Decimal` with sign, coefficient, and base-10 exponent is encoded as **tag 4** wrapping the two-element array `[exponent, mantissa]`, where `mantissa` is a signed integer (itself a bignum, Section 5.2, when large). The exponent preserves **scale**, so `1000.35D` -> `c4 82 21 1a000186c3` = tag 4 `[-2, 100035]`, and a scale-significant value keeps its trailing zeros via a smaller exponent. Decimal specials use **tag 731**: `∞D` -> `d902db 00`, `-∞D` -> tag 731 `1`, `?D` -> tag 731 `2`. (Tag 4's array cannot carry infinity/NaN, hence the dedicated tag.)

### 5.5 Strings
A `String` is a CBOR text string (major 3), UTF-8, length-prefixed. No escaping exists at the binary layer -- the bytes are the scalar values. `"café"` -> `65 636166c3a9`.

### 5.6 Bytes
`Bytes` is a CBOR byte string (major 2), length-prefixed -- the decoded bytes directly, with none of the base64 framing of the text form. `|SGVsbG8=|` -> `45 48656c6c6f`. This is the round-trip the text open-pipe form could not always achieve (Section 8 of the language spec); Binary AXON has no such defect.

### 5.7 Temporals
A temporal is **tag 727** wrapping a kind-tagged array, lossless over AXON's full temporal model (nanosecond precision; local vs offset vs zoned; the `Z`/`+00:00` distinction of Section 6.6):

- **Date** `[0, year, month, day]`. `^2026-07-12` -> `d902d7 84 00 1907ea 07 0c`.
- **Time** `[1, hour, minute, second, nanosecond, tz]`. `^12:00:00.5` -> `d902d7 86 01 0c 00 00 1a1dcd6500 f6` (500 ms as 500 000 000 ns; `tz = null` -> local). `^12:35:00+03:00` -> `... 18b4` (`tz = 180` minutes).
- **DateTime** `[2, y, m, d, h, mi, s, ns, tz]`. `^2026-07-12T09:35:00Z` -> `d902d7 89 02 1907ea 07 0c 09 1823 00 00 615a` where the final `615a`... decodes the `tz` slot.

The **`tz` slot** encodes the offset/zone distinction exactly: `null` = local (no offset), the text `"Z"` = UTC-designated (zulu), an integer = offset in minutes, and a two-element `[minutes, "Area/Zone"]` = a named IANA zone (the `tz-names` profile, Section 6.8). Trailing fractional-zero significance follows the decimal-scale rule when the scale-significant profile is active. Decoders MAY additionally accept CBOR tag 0 (RFC 3339 text) and tag 1 (epoch) on input, mapping them to offset datetimes; canonical output never uses them.

### 5.8 Lists and tuples
A `List` is a CBOR array (major 4): `[1 2 3]` -> `83 01 02 03`. A `Tuple` is **tag 724** wrapping a CBOR array, so it is never confused with a list on decode: `(1 2 3)` -> `d902d4 83 010203`. Element order is significant for both.

### 5.9 Maps and ordered maps
A `Map` is a CBOR map (major 5): `{a:1 b:2}` -> `a2 6161 01 6162 02`. An `OrderedMap` is **tag 725** wrapping a CBOR **array of `[key, value]` pairs**, making order part of the value (a plain CBOR map is semantically unordered): `[a:1 b:2]` -> `d902d5 82 82616101 82616202`. This preserves the language spec's ordered/unordered distinction (Section 2.4, Section 12.3). Duplicate keys are resolved per the active profile (Section 12) before encoding; canonical Binary AXON contains none.

### 5.10 Sets
A `Set` is **tag 258** (the registered CBOR set tag) wrapping a CBOR array of distinct elements: `{1 2 3}` -> `d90102 83 010203`. The empty set `∅` is tag 258 wrapping an empty array -- distinct from the empty list `[]` (`80`), empty map `{}` (`a0`), and empty ordered map `[:]` (tag 725 + `80`).

### 5.11 Nodes
A `Node` is **tag 720** wrapping the three-element array `[body_kind, name, payload]`:

- `body_kind`: `0`=unit, `1`=brace, `2`=tuple-body, `3`=list-body.
- `name`: a CBOR text string; a namespaced name is the text `"ns/name"` (Section 9.3).
- `payload`: for **unit**, `null`; for **brace**, the two-element array `[attributes_map, children_array]` (attributes as a CBOR map with text keys, children as a CBOR array); for **tuple-body**/**list-body**, the values array.

Examples: `Foo` (unit) -> `d902d0 83 00 63466f6f f6`; `point{x:1 y:-2}` (brace, two attrs, no children) -> `d902d0 83 01 65706f696e74 82 a2617801617921 80`; `Rgb(255 128 0)` (tuple body) -> `d902d0 83 02 635267 62 8318ff188000`. Because `body_kind` is explicit, `Rgb` (unit) and `Rgb()` (empty tuple body) encode differently, preserving Section 4.4.1.

### 5.12 References, graphs, and cycles (Graph profile)
Shared and cyclic structure uses the CBOR value-sharing tags: the **first** occurrence of a shared value is wrapped in **tag 28** ("mark shared"), and every later occurrence is **tag 29** ("reference") carrying the zero-based index of the marked value in encounter order. This expresses aliasing and cycles that a tree encoding cannot. `&1 [1 2]` used twice as `[*1 *1]` -> `83 d81c820102 d81d00 d81d00` = array of three: `tag28([1,2])`, `tag29(0)`, `tag29(0)`. Outside the Graph profile these tags MUST NOT appear. A decoder MUST reconstruct shared identity (the two `tag29(0)` items resolve to the *same* object as the `tag28` item), and MUST enforce the Section 16 reference/anchor limits.

### 5.13 Links (Document Link profile)
A `Link` is encoded by its kind: a **CID** link -> **tag 42** wrapping a byte string whose content is `0x00` followed by the binary CID (the DAG-CBOR convention), and a **URI** link -> **tag 32** wrapping the URI text. `link("https://example.com")` -> `d820 73 68747470733a2f2f6578616d706c652e636f6d`. CID links require a valid CIDv1 (the reference implementation validates raw/sha2-256). Because these are the same tags DAG-CBOR uses, Binary AXON links are readable as IPLD links.

---

## 6. Canonical Binary AXON

**Canonical Binary AXON** is the deterministic subset used for hashing, signing, content addresses (Section 10.7, CIDs), lockfiles, and byte-equality. It is **Core Deterministic CBOR** plus **AXON's canonical value rules (Section 14)**:

*From deterministic CBOR (dCBOR-aligned):*
1. Integers and lengths use the **shortest** encoding; no indefinite-length items.
2. Floats use the shortest of half/single/double that round-trips; the one canonical NaN is `0xf9 7e00`; `-0.0` is preserved.
3. CBOR map keys (the `Map` kind and node attribute maps) are sorted by their **encoded key bytes**, bytewise lexicographic.
4. No semantic-free tags (e.g., the Section 3 self-identifying prefix) are emitted.

*From AXON Section 14 (value-level):*
5. `Set` elements are ordered by their own canonical Binary AXON encodings, bytewise.
6. `Decimal` uses the Section 14 normal form (trailing-zero handling) reflected in the tag-4 `[exponent, mantissa]`.
7. `OrderedMap` and `Node` children keep semantic order (never reordered); node attribute maps sort as in rule 3.
8. Temporals are normalised per Section 6.10 (zero-padded fields are irrelevant at the binary layer; the `tz` slot is canonical -- `"Z"` stays `"Z"`, offsets are integer minutes).
9. Duplicate map/set members are impossible (resolved before encoding).

Same value => identical canonical bytes => identical hash/CID. A **canonical-verify** mode (mirroring Section 14.6) MUST be available: decode, re-encode canonically, and accept iff the bytes match. Canonical Binary AXON is the natural hashing target for AXON's content-addressed `Link`s, which is what makes `cid()` meaningful.

---

## 7. Round-trip guarantees

Three round-trips are normative:

- **Binary <-> value:** encode(decode(b)) = b for canonical b; decode(encode(v)) = v for every value v. Kind is preserved -- a decoder MUST reconstruct `Tuple` (not list), `OrderedMap` (not map), `Set`, the node body kind, `Link`, and the temporal offset/zone distinction.
- **Value <-> text <-> binary:** the semantic value obtained from parsing text AXON and the value obtained from decoding Binary AXON are equal (Section 2.9 equality) when they denote the same value; canonicalising either representation and mapping to the other yields the counterpart's canonical form.
- **Binary <-> CBOR ecosystem:** Binary AXON is valid CBOR, so generic CBOR tooling round-trips the bytes; AXON tags are preserved as tagged values by decoders that don't model them.

No lossy narrowing is permitted anywhere: arbitrary-precision ints/decimals, nanosecond temporals, `-0.0`, `Z`-vs-offset, and shared identity all survive. The reference `decode` demonstrates this: 24/24 reference vectors and the shared/cyclic cases (self-cycle, shared leaf across list and tuple, cyclic map) reconstruct with correct object identity.

---

## 8. Streaming and resource limits

Every item is self-delimiting (CBOR length prefixes), so Binary AXON streams value-by-value with bounded look-ahead; a stream is a CBOR sequence (RFC 8742) or an array understood as a stream (Section 3). Decoders MUST enforce the Section 16 limits -- nesting depth (default 128), item counts, string/bytes sizes, bignum/decimal magnitude, and Graph-profile anchor/reference counts -- per top-level item, and MUST reject indefinite-length items in canonical mode. Untrusted bytes cannot exhaust memory or stack before a value is produced; the shared-reference tags (Section 5.12) are the only construct that can express cycles, and they are bounded by the reference limit.

---

## 9. Conformance

A conforming Binary AXON implementation MUST: (a) produce well-formed CBOR for every value in the AXON value model; (b) encode each kind exactly as Section 4-Section 5 specify, including the AXON tags and the round-trip-critical distinctions of Section 7; (c) provide canonical encoding and canonical-verify per Section 6; (d) enforce Section 8 limits; and (e) preserve unknown tags on decode rather than corrupting structure. The **reference vectors** in the Appendix are the acceptance corpus -- an implementation MUST reproduce each canonical encoding byte-for-byte. All Appendix vectors were produced from values parsed by pyaxon 0.11.0a13, encoded by the reference codec `axon.binary2026`, and independently validated as well-formed CBOR; the codec's `decode` inverse round-trips all of them in both canonical and non-canonical modes.

---

## Appendix -- Reference vectors (verified)

Parsed by pyaxon 0.11.0a13, encoded by the reference encoder, confirmed well-formed CBOR. Hex is the complete item.

| AXON (text) | Binary AXON (hex) | Bytes |
|---|---|---:|
| `null` | `f6` | 1 |
| `true` | `f5` | 1 |
| `42` | `182a` | 2 |
| `123456789012345678901234567890` | `c24d018ee90ff6c373e0ee4e3f0ad2` | 15 |
| `1000.35D` | `c482211a000186c3` | 8 |
| `∞D` | `d902db00` | 4 |
| `3.14159` | `fb400921f9f01b866e` | 9 |
| `?` (NaN) | `f97e00` | 3 |
| `"café"` | `65636166c3a9` | 6 |
| `\|SGVsbG8=\|` | `4548656c6c6f` | 6 |
| `^2026-07-12` | `d902d784001907ea070c` | 10 |
| `^2026-07-12T09:35:00Z` | `d902d789021907ea070c0918230000615a` | 17 |
| `^12:35:00+03:00` | `d902d786010c1823000018b4` | 12 |
| `^12:00:00.5` | `d902d786010c00001a1dcd6500f6` | 14 |
| `[1 2 3]` | `83010203` | 4 |
| `(1 2 3)` | `d902d483010203` | 7 |
| `{a:1 b:2}` | `a2616101616202` | 7 |
| `[a:1 b:2]` | `d902d5828261610182616202` | 12 |
| `{1 2 3}` | `d9010283010203` | 7 |
| `point{x:1 y:-2}` | `d902d0830165706f696e7482a261780161792180` | 20 |
| `Rgb(255 128 0)` | `d902d08302635267628318ff188000` | 15 |
| `Foo` | `d902d0830063466f6ff6` | 10 |
| `link("https://example.com")` | `d8207368747470733a2f2f6578616d706c652e636f6d` | 22 |
| `&1 [1 2] ... [*1 *1]` | `83d81c820102d81d00d81d00` | 12 |

Size note: on typical structured data Binary AXON is smaller than canonical text (`{a:1 b:2}` 7 B vs 9 B; `[1 2 3 4 5]` 6 B vs 11 B; a UTC datetime 17 B vs 21 B), while a small brace node with two attributes runs slightly larger than its terse text form (20 B vs 15 B) -- the expected trade for explicit, self-describing structure. Wire size is minimised by canonical shortest-form encoding; further compression is a transport concern.

---

*Working draft. The provisional AXON tag block (720-731) is subject to registration; the encoding of the value model, the canonical rules, and the round-trip guarantees are stable. This document earns AXON 2026 a genuine "compact binary" capability: the same values as the text form, in self-describing, canonicalisable, CID-ready bytes.*
