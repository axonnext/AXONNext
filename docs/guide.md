# AXON -- Guide

*AXON 2026 edition (AXON Next). The reference implementation is `axonnext`.*

AXON is **eXtended Object Notation** -- a simple, text-based format for
interchanging objects, documents, and data. It reads like cleaner JSON, keeps
the meaning JSON throws away, and combines:

- the **simplicity** of JSON,
- the **extensibility** of XML,
- the **readability** of YAML.

AXON was created to overcome real gaps in JSON:

- no native date/time, decimal, or binary data;
- no way to represent complex data with cross-references natively;
- no native support for named/tagged structures (typed data, document
  elements) where XML would otherwise be reached for;
- commas mandatory as item separators.

...while staying about as simple as JSON, and supporting both a JSON/C brace style
and a YAML/Python indented style.

> The original API (`loads` / `dumps`) is unchanged. The 2026 engine is reached
> through the explicit `*2026` API (`loads2026` / `dumps2026` / `canonical2026`)
> -- see [API reference](api.md). This guide covers the language; the
> [AXON 2026 edition](#axon-2026-edition) section covers what the edition adds
> and why.

---

## The value model

AXON composes values from atomic values by a handful of container rules:

| Kind | Rule | Example |
|---|---|---|
| **list** | `[ V ... V ]` | `[1 3.14 3.25D ∞ -∞ ?]` |
| **tuple** | `( V ... V )` | `(true ^12:00 ^2001-12-31 ^2001-12-31T12:00)` |
| **set** | `{ V ... V }` | `{"a" "b" "c" "d"}` |
| **map** (dict) | `{ K:V ... K:V }` | `{alpha:1 beta:2 "other chars":4}` |
| **ordered map** | `[ K:V ... K:V ]` | `[alpha:1 beta:2 gamma:3]` |
| **node** | `N{ N:V ... N:V  V ... V }` | `tree{id:1 leaf{id:2 "AAA"} leaf{id:3 "BBB"}}` |

where **N** is a *name*, **K** a *key*, and **V** a *value*.

### Atomic values

| Type | Examples |
|---|---|
| integer | `0`  `-1`  `17` |
| float | `3.1428`  `1.5e-17` |
| decimal (exact) | `10D`  `1000.35D`  `-1.25E+6D` |
| boolean | `true`  `false` |
| null | `null` |
| string | `"abc абв 中文本"` (Unicode; multi-line allowed) |
| date | `^2012-12-31` |
| time | `^12:30:34`  `^12:35:12.000120`  `^12:35+03` |
| datetime | `^2012-12-31T12:30`  `^2012-12-31T12:35+03` |
| binary | `\|QVhPTiBpcyBlWHRlbmRlZA==\|` (closed-pipe base64) |
| numeric specials | `∞`  `-∞`  `?` (NaN) -- and decimal forms `∞D` `-∞D` `?D` |

### A worked example

The same document in three equivalent forms.

**Indented (YAML/Python) form:**

```axon
axon
  name: "AXON is eXtended Object Notation"
  short_name: "AXON"
  atomic_values
    int: [0 -1 17]
    decimal: [10D 1000.35D -1.25E+6D]
    string: "abc абв 中文本"
    datetime: [^2012-12-31T12:30 ^2012-12-31T12:35+03]
    binary: |QVhPTiBpcyBlWHRlbmRlZA==|
  complex_values
    list: ["one" "two" "three"]
    dict: {one:1 two:2 three:3}
    tuple: ("nodes" "edges")
    set: {"a" "b" "c"}
    node: person
      name: "Alex"
      age: 32
```

**Compact form:**

```axon
axon{name:"AXON is eXtended Object Notation" short_name:"AXON"
atomic_values{int:[0 -1 17] decimal:[10D 1000.35D -1.25E+6D] string:"abc абв 中文本"
datetime:[^2012-12-31T12:30 ^2012-12-31T12:35+03] binary:|QVhPTiBpcyBlWHRlbmRlZA==|}
complex_values{list:["one" "two" "three"] dict:{one:1 two:2 three:3}
tuple:("nodes" "edges") set:{"a" "b" "c"} node:person{name:"Alex" age:32}}}
```

Both denote the same values.

---

## AXON 2026 edition

AXON Next is the **2026 edition** of AXON: a faithful continuation of the
original `pyaxon` that keeps everything above and adds a modern, safe,
deterministic layer. It is frozen at specification revision 5 and shipped as
**v1.0.0**, with a Python reference (`axonnext`) and a parity-verified Rust crate
(`serde_axon`).

The rest of this section is the **what and why** -- each thing the edition adds,
and the problem it solves.

### Exact, unambiguous numbers

**What.** Integers, floats, and arbitrary-precision **decimals** (`19.99D`) are
distinct kinds; `∞`, `-∞`, and `?` (NaN) are first-class, in both float and
decimal domains.

**Why.** JSON has one "number" type backed by binary floating point, so `19.99`
is not exactly representable and `24` vs `24.0` is lost. Money, measurements,
and identifiers need exactness and need integers to stay integers -- so AXON
keeps the distinction in the data itself.

### Native temporals

**What.** Dates, times, and datetimes are real values (`^2026-07-17T00:30:00-06:00`)
with nanosecond precision, and named IANA zones under the `tz-names` profile.

**Why.** In JSON a timestamp is a string that merely looks like a date; every
consumer re-parses it, and formats drift. A native temporal is validated once,
at parse time, and can't be a "2026-02-30".

### Named, namespaced nodes

**What.** `Name{...}` attaches a type to a value, and names may be namespaced as
`ns/name` -- e.g., `geo/point{lat:53.5 lon:-113.5}`.

**Why.** In JSON the "type" of an object is a `"type"` field by convention, and
independent vocabularies collide. Making the type part of the data, with
namespacing, lets independent schemas compose without a central registry -- the
lesson EDN tagged literals, CBOR tags, and XML namespaces each converged on.

### Strict safety by default

**What.** Duplicate map keys and duplicate set elements are **rejected**, and
every parser enforces mandatory resource limits (depth, length, counts).

**Why.** JSON leaves duplicate keys undefined -- different parsers keep the
first, keep the last, or reject -- so `{"count":24,"count":30}` silently loses
data. AXON makes that an error, and the resource limits make a parser safe to
point at untrusted input.

### Deterministic Canonical AXON

**What.** Every value maps to exactly one canonical byte sequence -- sorted map
keys, set elements ordered by their canonical bytes, decimals in normal form,
floats in JCS/ECMAScript layout.

**Why.** Determinism is what makes checksums, signatures, cache keys, and
**content identifiers (CIDs)** possible: the same value always produces the same
bytes, and therefore the same CID, in any conforming implementation. Ordinary
JSON needs an extra convention (JCS) to get close.

### Graph profile -- shared and cyclic structure

**What.** `&label value` binds a label; `*label` refers to that same value.
Under the Graph profile a document can express sharing and cycles.

**Why.** A tree model literally cannot represent two fields pointing at the *same*
object, or a structure that refers back to itself. The Graph profile makes that
representable when you need it -- and stays off by default when you don't.

### Document Link profile

**What.** `cid("bafy...")` and `link("https://...")` produce content-addressed and
URI `Link` values (validated).

**Why.** Content addressing and hyperlinks are first-class in a data language
built for reproducible, distributed data -- with zero new syntax (they're
ordinary nodes, so non-profile parsers degrade gracefully).

### Ergonomics

**What.** Comments (`#`), optional commas, raw strings (`r"..."`), the `#_`
discard token, and `axon{edition:"2026"}` document headers.

**Why.** Config and human-edited data want comments and forgiving punctuation;
raw strings avoid escape thickets; the discard token lets you comment out a
value structurally; the header lets a document declare the edition and required
profiles up front.

### Internationalisation

**What.** Source text is mandatory, strictly-validated UTF-8; bare names and keys
follow the Unicode identifier rules (UAX #31), so `café`, `Ω`, or keys written in
Cyrillic, Arabic, or CJK scripts are legal unquoted. String values are Unicode
scalars written literally, with a single braced `\u{...}` escape that rejects
surrogate code points. Canonical AXON sorts keys by their UTF-8 bytes, and
temporals keep local / offset / `Z` distinct.

**Why.** International text should be ordinary, not a special case. Strict UTF-8
removes encoding guesswork; Unicode identifiers let a schema name fields in the
author's own language; the surrogate-free escape sidesteps JSON's
`\uD800`-`\uDFFF` footgun; bytewise key ordering gives an international document
one stable CID everywhere; and locale-neutral numbers (always `.`, no digit
grouping) plus exact decimals mean a figure reads the same in every locale.
Compared with the field: JSON escapes non-ASCII as 16-bit surrogate pairs and has
no built-in canonical form; TOML mandates UTF-8 but keeps *bare* keys ASCII-only;
YAML is Unicode-capable but ambiguous; XML has Unicode names and C14N, but both
the format and its canonicaliser are heavy.

**Caveat.** Canonical AXON does not silently NFC-normalise string *values* -- it
will not alter your data -- so the same text in two different Unicode
compositions is two different values (and two different CIDs). Normalise to NFC
upstream if you need cross-source equality; bare-key emissions are NFC-normalised
on output, and a lint warns about confusable names.

### And more

Streaming (multiple top-level values, read incrementally), a **lossless concrete
syntax tree** (preserves comments and formatting for editors and migration
tools), an **AXON Schema** companion, a compact **Binary AXON** encoding, and a
**`compat`** reader that loads pre-2026 documents losslessly -- so nothing that
worked before stops working.

---

## Conformance profiles

Core AXON is the always-on baseline; richer behaviour is opt-in.

| Profile | Adds |
|---|---|
| **Core** | every baseline value kind; strict by default |
| **Graph** | `&label` / `*label` anchors and references |
| **Doc-Link** | `cid(...)` / `link(...)` produce `Link` values |
| **tz-names** | named IANA time zones |
| **compat** | the documented legacy-pyaxon reader, for pre-2026 documents |

---

## Implementations

- **`axonnext`** -- the Python reference (this package), 3.10-3.13. Defines the
  behaviour everything else is measured against.
- **`serde_axon`** -- a `no_std`-friendly Rust crate, verified **1:1** with
  axonnext by a differential conformance suite.

Both are at **v1.0.0**.

## Quick start

```python
import axon2
from axon2 import loads2026, dumps2026, canonical2026, Options

axon2.dumps([{"b": 2, "a": 1}])                  # historical API
loads2026('point{label:"p" 10 20}')             # 2026 edition API
dumps2026([{"b": 2, "a": 1}], canonical=True)   # -> '{a:1 b:2}'
```

See the [API reference](api.md) for the full surface and the
[changelog](changelog.rst) for release history.

---

## In practice: FoodML

**FoodML 2.0** (FoodBank Markup Language) -- a vendor-neutral data language for food-support systems (food
banks, community pantries) -- is built entirely on AXON: every document is AXON
(`.axn2`), its structural contract is an **AXON Schema 2026**, its conditional
rules run in a small native semantic layer over the AXON value model, and
encrypted-at-rest client records use a `.foml` container whose plaintext is AXON.
It is the first real-world consumer of AXON Next -- concrete evidence that the
value model, AXON Schema, and canonical form hold up in a domain with real
privacy and integrity requirements.

---

## Lineage

AXON and `pyaxon` were originally created by **intellimath (Zaur Shibzukhov)**
and released under the MIT licence; the historical upstream remains at
[github.com/intellimath/pyaxon](https://github.com/intellimath/pyaxon). The
AXON 2026 / AXON Next continuation builds on that work and is dual-licensed
MIT OR Apache-2.0, retaining intellimath's original MIT copyright.
