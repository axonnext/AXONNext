<div align="center">

# AXON Next

**eXtended Object Notation -- 2026 edition**

A human-readable data language that keeps the meaning JSON throws away:
exact decimals, real dates, sets and tuples, named records, binary, and a
deterministic canonical form -- with two verified 1:1 implementations.

[![spec](https://img.shields.io/badge/spec-AXON%202026-4c6ef5)](spec/AXON_2026_SPEC.md)
[![axonnext](https://img.shields.io/badge/axonnext-1.0.0-3776ab)](docs/guide.md)
[![serde_axon](https://img.shields.io/badge/serde__axon-1.0.0-dea584)](serde_axon)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license--lineage)

</div>

---

## Contents

- [What is AXON?](#what-is-axon)
- [AXON at a glance](#axon-at-a-glance)
- [Why not just JSON?](#why-not-just-json)
- [Feature highlights](#feature-highlights)
- [Language comparison](#language-comparison)
- [Documentation](#documentation)
- [Implementations](#implementations)
  - [axonnext (Python reference)](#axonnext-python-reference)
  - [serde_axon (Rust)](#serde_axon-rust)
- [Conformance profiles](#conformance-profiles)
- [Canonical form & content addressing](#canonical-form--content-addressing)
- [Internationalisation](#internationalisation)
- [Parity: one language, two implementations](#parity-one-language-two-implementations)
- [Built on AXON: FoodML](#built-on-axon-foodml)
- [Project layout](#project-layout)
- [Release](#release)
- [License & lineage](#license--lineage)

---

## What is AXON?

**AXON** (*eXtended Object Notation*) is a text-based format for interchanging
objects, documents, and data. It reads like cleaner JSON, but its value model is
much richer -- closer to CBOR's -- while staying human-readable and diff-friendly.

**AXON Next** is the **2026 edition** (AXON Next 1.0): a faithful, modernised
continuation of the original
[`intellimath/pyaxon`](https://github.com/intellimath/pyaxon) project, frozen at
specification revision 5 and shipped as **v1.0.0** with a normative Python
reference implementation and a parity-verified Rust crate.

## AXON at a glance

```axon
# A document header configures the parser and is not part of the data.
axon{edition:"2026"}

inventory/item{
  name:     "Canned beans"      # namespaced, named record -- the type is data
  quantity: 24                  # an integer, distinct from 24.0
  price:    19.99D              # an exact decimal, not a binary float
  updated:  ^2026-07-17T00:30:00-06:00   # a native temporal, nanosecond-capable
  tags:     {"pantry" "canned"} # a set -- unordered, distinct
}
```

- `inventory/item` is a **named, namespaced node** -- the type travels with the value.
- `24` is an integer; `24.0` would be a float; `19.99D` is an exact decimal.
- `^...` is a real temporal value, not a string that happens to look like a date.
- Commas are optional, keys rarely need quotes, and comments are allowed.

## Why not just JSON?

JSON is understood everywhere, and converting JSON -> AXON is trivial. The reverse
is lossy, because JSON has nowhere to put the things AXON preserves:

```jsonc
// JSON -- everything is a string, a number, or an anonymous object
{
  "type": "item",           // the "type" is a convention, not part of the model
  "quantity": 24,           // 24 and 24.0 are the same JSON number
  "price": 19.99,           // binary float -- 19.99 is not exactly representable
  "updated": "2026-07-17T00:30:00-06:00"  // a string, not a date
}
```

```axon
# AXON -- the distinctions are in the data itself
item{ quantity:24  price:19.99D  updated:^2026-07-17T00:30:00-06:00 }
```

For simple data AXON just feels like tidier JSON. Once you use its native types,
it becomes a human-readable blend of JSON, CBOR's value model, and a typed
document language -- while staying safe by construction (strict duplicate
detection, mandatory resource limits, a deterministic canonical form).

## Feature highlights

| | |
|---|---|
| **Rich scalars** | integers vs floats vs **exact decimals** (`19.99D`); native **temporals** (`^2026-07-17T...`) with nanosecond precision; `∞`, `-∞`, `?` (NaN) |
| **Real collections** | lists `[...]`, **tuples** `(...)`, unordered **maps** `{k:v}`, **ordered maps** `[k:v]`, and native **sets** `{...}` / `∅` |
| **Named nodes** | `Name{...}` is a tag: `geo/point{lat:53.5 lon:-113.5}`; unit, brace, tuple, and list bodies; `ns/name` namespacing |
| **Native binary** | `\|SGVsbG8=\|` closed-pipe base64 -- bytes, not a string pretending to be bytes |
| **Ergonomics** | comments (`#`), optional commas, raw strings (`r"..."`), the `#_` discard token, document headers |
| **Safety** | strict duplicate-key/-element rejection, mandatory parser resource limits, defined error categories |
| **Determinism** | **Canonical AXON** -- the same value always produces the same bytes (checksums, signatures, cache keys, CIDs) |
| **Graph profile** | `&label` / `*label` for shared and cyclic structure -- a property tree models can't express |
| **Streaming** | multiple top-level values with no enclosing array; read incrementally |
| **Lossless editing** | a concrete syntax tree (CST) preserves comments, whitespace, and formatting for editors and migration tools |
| **Internationalisation** | mandatory UTF-8, Unicode (UAX #31) identifiers, surrogate-free `\u{...}` escapes, bytewise-deterministic key ordering, and locale-neutral numbers -- [details](#internationalisation) |

Advanced features (decimals, graph references, CIDs, binary, sets, tuples,
document links) are available to the language but never forced -- a simple
document stays simple.

## Language comparison

How AXON 2026 relates to other data languages. A dash isn't a failure -- for many
formats it's a deliberate scope choice; it just marks what AXON carries that they
don't. (`Y` yes  /  `~` partial / by-convention / via-extension  /  `-` no.)

| Capability | AXON | JSON | YAML | TOML | XML | CBOR | EDN | KDL |
|---|:--:|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| Human-readable text | Y | Y | Y | Y | Y | - | Y | Y |
| Comments | Y | - | Y | Y | Y | - | Y | Y |
| Integer != float | Y | - | - | Y | - | Y | Y | ~ |
| Exact decimals | Y | - | - | - | - | Y | Y | - |
| Native temporals | Y | - | Y | Y | - | Y | Y | - |
| Sets | Y | - | Y | - | - | ~ | Y | - |
| Tuples (!= list) | Y | - | - | - | - | - | - | - |
| Ordered map (!= map) | Y | - | - | - | Y | Y | - | ~ |
| Named / tagged nodes | Y | - | ~ | - | Y | Y | Y | Y |
| Native binary | Y | - | Y | - | ~ | Y | - | - |
| Strict duplicate rejection | Y | - | ~ | Y | - | ~ | - | - |
| Deterministic canonical form | Y | ~ | - | - | Y | Y | - | - |
| Content addressing (CIDs) | Y | - | - | - | - | ~ | - | - |
| Graph / shared refs | Y | - | ~ | - | ~ | - | - | - |
| Streaming (multi top-level) | Y | ~ | Y | - | Y | Y | Y | Y |
| Compact binary encoding | Y | - | - | - | - | Y | - | - |

The full 19-format x 19-capability matrix, graded against the specification,
lives in [`comparison/axon_2026_comparison.html`](comparison/axon_2026_comparison.html).

## Documentation

- **[Guide](docs/guide.md)** -- the language: value
  model, worked examples, and what the 2026 edition adds (and why).
- **[API reference](docs/api.md)** -- the `axonnext`
  Python API, historical and 2026 (`Options`, the `*2026` family, canonical/CID,
  migration).
- **[Specification](spec/AXON_2026_SPEC.md)** -- the normative AXON 2026
  language spec (revision 5), with the [Binary AXON](spec/AXON_2026_BINARY.md)
  companion.
- **[Changelog](docs/changelog.rst)** -- release
  history through v1.0.0.
- **Original project** -- AXON and `pyaxon` began at
  [`intellimath/pyaxon`](https://github.com/intellimath/pyaxon) (Zaur Shibzukhov,
  MIT); AXON Next continues it. See [License & lineage](#license--lineage).

## Implementations

AXON Next ships **two** implementations, both at **v1.0.0**, verified to agree
1:1 (see [Parity](#parity-one-language-two-implementations)).

### axonnext (Python reference)

The normative reference implementation (Python 3.10-3.13). It defines the
behaviour everything else is measured against, and adds the 2026 engine
(`loads2026`/`dumps2026`/`canonical2026`) alongside the historical API.

```bash
pip install axonnext
```

```python
import axon2
from axon2 import loads2026, dumps2026, canonical2026

axon2.dumps([{"b": 2, "a": 1}])                  # legacy API
loads2026('point{label:"p" 10 20}')             # 2026 edition API
dumps2026([{"b": 2, "a": 1}], canonical=True)   # -> '{a:1 b:2}'
```

Package: https://pypi.org/project/axonnext/. Source: the
repository root (`lib/axon/`, `setup.py`, `docs/`). Gate: `351 tests OK`. The
reference also includes a lossless CST, a schema companion, the Binary AXON
codec, and an executable, language-neutral conformance registry.

### serde_axon (Rust)

A `no_std`-friendly Rust crate with Serde text support, a lossless CST, full
migration and editor/LSP helpers, AXON Schema 2026 validation, scientific and
secured-stream helpers, and compact/canonical Binary AXON codecs, plus
`#![forbid(unsafe_code)]` and full UAX #31 identifiers.

```toml
[dependencies]
serde_axon = "1.0"
```

```rust
let text = serde_axon::to_string(&value)?;              // serialize
let value: MyType = serde_axon::from_str(&text)?;       // deserialize
let canon = serde_axon::to_string_canonical(&value);    // Section 14 canonical form
```

Crate: https://crates.io/crates/serde_axon. Source:
[`serde_axon`](serde_axon). Gates: `build` / `clippy -D warnings` / `fmt` /
`test` all green, including a 91-case semantic differential suite, CST and
Schema fixtures, and a 118-case generated Binary/CID/migration/LSP/scientific/
stream oracle from axonnext.

## Conformance profiles

Core AXON is the always-on baseline. Everything richer is a profile a processor
opts into (spec Section 15):

| Profile | Adds |
|---|---|
| **Core** | every baseline value kind; strict by default |
| **Graph** | `&label` / `*label` anchors and references (shared & cyclic identity) |
| **Doc-Link** | `cid("...")` / `link("...")` produce content-addressed / URI `Link` values |
| **tz-names** | named IANA time zones (`^...[America/Edmonton]`) |
| **compat** | the documented legacy-pyaxon reader, for reading pre-2026 documents losslessly |

## Canonical form & content addressing

**Canonical AXON** (spec Section 14) maps every value to one deterministic byte
sequence: map keys sorted, set elements ordered by their canonical encodings,
decimals in normal form, floats in JCS/ECMAScript layout. That makes stable
checksums, signatures, cache keys, and **CIDs** possible -- the same value
produces the same CID in either implementation:

```
point{x:1 y:-2}  ->  bafkreiec5tzcwyhehbpob5yir5idmjfqf2qzpiiguoia4qt5fquhrbx4ym
```

## Internationalisation

Text is international by default, and AXON 2026 treats Unicode as a first-class
concern rather than an afterthought:

- **Strict UTF-8.** Source text must be well-formed UTF-8 (a sequence of Unicode
  scalar values); anything else is rejected outright as an `invalid-unicode`
  error. No encoding guessing, no silent replacement characters.
- **Unicode identifiers.** Bare names and keys follow the Unicode identifier
  rules (UAX #31 `XID_Start` / `XID_Continue`), so `café`, `naïve`, `Ω`, and
  keys written in Greek, Cyrillic, Arabic, or CJK scripts are all legal
  *unquoted*. ASCII-only naming is a portability lint, never a hard limit.
- **Literal Unicode strings.** String values are Unicode scalar sequences
  written as plain UTF-8 -- `"café"` needs no escaping. The only Unicode escape
  is the braced `\u{...}` form, and it *rejects* surrogate code points, so AXON
  carries none of JSON's `\uD800`-`\uDFFF` surrogate-pair hazards.
- **Deterministic ordering.** Canonical AXON sorts map keys by their
  UTF-8-encoded bytes, so a document with international keys has one stable,
  reproducible byte sequence -- and therefore one stable CID -- in every
  implementation.
- **Timezone-aware temporals.** Local, offset, and `Z` date-times are distinct
  values; an offset such as `-06:00` is never silently rewritten to UTC, so
  civil time survives round-trips across regions.
- **Locale-neutral numbers.** Numbers always use `.` as the decimal point with
  no digit grouping, so a document means the same thing in every locale -- and
  exact decimals (`19.99D`) sidestep the rounding that bites locale-formatted
  currency.

**One deliberate caveat.** Canonical AXON does *not* silently apply Unicode
normalisation (NFC) to string *values* -- it refuses to alter your data. The
letter e-acute written as a single code point (`U+00E9`) and as `e` followed by
a combining accent (`U+0301`) are therefore *different* values with *different*
CIDs. If you need content-address equality across sources, normalise to NFC
before encoding. (Bare-key *emissions* are NFC-normalised on output, and a lint
warns about confusable compositions.)

**Compared with other formats.** AXON keeps the good parts and drops the traps:

- **JSON** has no bare keys and escapes non-ASCII with 16-bit `\uXXXX` plus
  surrogate pairs -- a perennial source of Unicode bugs -- and defines no
  canonical form in its base specification (JCS / RFC 8785 is a separate
  standard).
- **YAML** is Unicode-capable but pays for it with implicit-typing ambiguity,
  and has no widely-used canonical form.
- **TOML** mandates UTF-8 (good) but restricts *bare* keys to ASCII; non-ASCII
  keys must be quoted.
- **XML** allows Unicode element names and defines a canonical form (C14N), but
  the format is heavy and C14N is notoriously intricate.
- **CBOR** carries UTF-8 text natively, and its deterministic profile (dCBOR) is
  exactly what Binary AXON builds its canonical form on.

## Parity: one language, two implementations

The two implementations are held to **byte-for-byte agreement**, not vibes. A
differential harness generates a conformance corpus *from* axonnext -- each case's
accept/reject decision, canonical bytes, and error category -- and asserts
serde_axon reaches the identical outcome. Canonical bytes are the
cross-implementation contract, so one comparison catches both value-model and
writer divergences. The corpora are regenerated from the reference and
byte-compared by one fixture check, so they cannot drift. Alongside the
semantic, CST, and Schema families, the cross-surface oracle locks Binary AXON,
CIDs/canonical verification, migration, LSP helpers, scientific arrays, and
secured stream envelopes.

## Built on AXON: FoodML

AXON isn't only self-consistent -- it carries a real application domain.
**FoodML 2.0** (FoodBank Markup Language) is a vendor-neutral data language for food-support systems (food
banks, community pantries) built **entirely on AXON**, and it is the first
real-world consumer of AXON Next:

- **Representation** -- every FoodML document is an AXON document (`.axn2`). The
  language uses no YAML or JSON anywhere.
- **Structure** -- FoodML's document contract is an **AXON Schema 2026**. The
  conditional and combinatorial rules AXON Schema does not express (exactly-one-of,
  at-least-one-of, unique items, ...) are enforced by a small native **semantic
  layer** over the AXON value model.
- **Determinism & identity** -- FoodML relies on canonical AXON for stable,
  content-addressable document identity.
- **Secure records** -- client records are stored at rest as encrypted `.foml`
  containers (AES-256-GCM-SIV) whose plaintext is AXON.

FoodML exercises the value model, AXON Schema, canonical form, and the safe
parser end to end, in a domain with real privacy and integrity requirements --
concrete evidence that "richer than JSON, safe by construction" holds up in
practice. (FoodML ships as its own repository: [`axonnext/FoodML`](https://github.com/axonnext/FoodML).)

## Project layout

> This working set is the full development record. A public release cuts it down
> to the language spec plus the two implementation repositories.

| Path | Contents |
|---|---|
| `lib/`, `bin/`, `docs/`, `examples/`, `setup.py`, ... | The **axonnext** package (Python reference) at the repo root -- matching the original pyaxon layout |
| [`serde_axon/`](serde_axon) | The `serde_axon` Rust crate |
| [`spec/`](spec) | Normative spec (`AXON_2026_SPEC.md`), Binary AXON companion, 2018->2026 evolution research |
| [`comparison/`](comparison) | The full format x capability comparison matrix |
| [`publish_kit/`](publish_kit) | `LICENSE`, `NOTICE`, and the publish checklist |
| `_dev/` | Development tooling, audits, archives, and proposals (not part of the package) |

## Release

**v1.0.0 -- the first stable release.** AXON Next v1 is the AXON 2026 edition
(specification revision 5, frozen), shipping two implementations held to
byte-for-byte parity:

- **axonnext 1.0.0** -- the normative Python reference (CPython 3.10-3.13);
  `pip install axonnext`. Adds the 2026 engine (`loads2026` / `dumps2026` /
  `canonical2026`) alongside the historical API.
- **serde_axon 1.0.0** -- the `no_std`-friendly Rust crate; `serde_axon = "1.0"`.

Everything in [Feature highlights](#feature-highlights) -- exact decimals, native
temporals, namespaced nodes, strict-by-default safety, canonical form + CIDs, and
the Graph / Doc-Link profiles -- is in this release, and the two implementations
are held in agreement by the differential suite (see
[Parity](#parity-one-language-two-implementations)).

**Release gates (green):** axonnext `351 tests OK`; serde_axon
`build` / `clippy -D warnings` / `fmt` / `test` / `no_std + alloc` / rustdoc,
including the 91-case semantic differential suite, the lossless-CST and Schema
reference fixtures, and the 118-case cross-surface oracle.

**Repository:** [github.com/axonnext/AXONNext](https://github.com/axonnext/AXONNext)
(private during staging). Publish steps: `publish_kit/PUBLISH_CHECKLIST.md`.

## License & lineage

AXON Next is **dual-licensed MIT OR Apache-2.0**, at your option -- covering both the
Python `axonnext` package and the `serde_axon` crate. It continues the original
MIT-licensed project, and intellimath's original MIT copyright notice is retained (which
is what permits the dual offering). The file extension is `.axn2`; the Rust crate keeps
the name `serde_axon`.

- The original **AXON** and **`pyaxon`** were created by **intellimath (Zaur
  Shibzukhov)** and remain the historical upstream
  ([GitHub](https://github.com/intellimath/pyaxon)). Their original MIT licence
  and copyright are retained in the derived tree.
- The **AXON 2026 / AXON Next** continuation builds on that work and is offered under
  **MIT OR Apache-2.0**, at [github.com/axonnext/AXONNext](https://github.com/axonnext/AXONNext).
  Its 2026 copyright is held by **Michael Lauzon** (`axonnext@gmail.com`), with
  intellimath's original notice retained as MIT requires -- see `NOTICE.md` and
  `publish_kit/`.
