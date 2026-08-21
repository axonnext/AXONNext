# serde_axon

[![crates.io](https://img.shields.io/crates/v/serde_axon.svg)](https://crates.io/crates/serde_axon)
[![docs.rs](https://img.shields.io/docsrs/serde_axon)](https://docs.rs/serde_axon)
[![license](https://img.shields.io/crates/l/serde_axon.svg)](#license)

[Serde](https://serde.rs) support for **AXON 2026** -- a human-readable text data
format that keeps the meaning JSON throws away: exact decimals, native temporals,
sets and tuples, named/tagged nodes, native binary, a graph profile for shared
and cyclic structure, and a deterministic canonical form for stable checksums,
signatures, and content identifiers (CIDs).

`serde_axon` is the Rust implementation of AXON Next, verified **1:1** against the
`pyaxon` reference by a differential conformance suite. It is `#![no_std]`-friendly
and contains **no `unsafe` code** (`#![forbid(unsafe_code)]`).

```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Point { x: i32, y: i32 }

let p = Point { x: 1, y: -2 };
let text = serde_axon::to_string(&p)?;              // "{x:1 y:-2}"
let back: Point = serde_axon::from_str(&text)?;
assert_eq!(p, back);
```

## Installation

```toml
[dependencies]
serde_axon = "1.1.0"
serde = { version = "1.0", features = ["derive"] }
```

Or:

```sh
cargo add serde_axon serde --features serde/derive
```

## Why AXON?

For simple data, AXON reads like tidier JSON. Once you use its native types, it
becomes a human-readable blend of JSON, CBOR's value model, and a typed document
language -- while staying safe by construction:

```axon
inventory/item{
  name:     "Canned beans"      # a named, namespaced node -- the type is data
  quantity: 24                  # an integer, distinct from 24.0
  price:    19.99D              # an exact decimal, not a binary float
  updated:  ^2026-07-17T00:30:00-06:00   # a native temporal
  tags:     {"pantry" "canned"} # a set -- unordered, distinct
}
```

## What's in the crate

- **Parser** -- a hand-written recursive-descent parser for AXON 2026, covering the
  Core profile plus the Graph, Document Link, `tz-names`, and `compat`
  (legacy-reader) profiles, with mandatory Section 16 resource limits.
- **Value model** -- [`Value`] represents every AXON kind losslessly, preserving
  the distinctions AXON is careful about: tuple vs list, ordered map vs map, node
  body style, `Z` vs an explicit offset, decimal scale, and graph anchors/refs.
- **Writer** -- compact output and **Canonical AXON** (Section 14): sorted map keys,
  set elements ordered by canonical bytes, decimal normal form, JCS/ECMAScript
  float layout, and whole-stream graph relabelling -- so the same value yields
  the same bytes, and the same CID, as any conforming implementation.
- **Lossless CST** -- byte-addressed tokens and arena nodes preserve comments and
  whitespace, attach diagnostics and repairs, expose semantic traces, migrate
  legacy indentation, and support deterministic formatting.
- **Migration** -- deterministic legacy-to-Core canonical rewrites plus
  semantics-proven selective rewrites with structured events, UTF-8 byte edits,
  and explicit canonical fallback.
- **Editor/LSP helpers** -- transport-neutral diagnostics, hover information,
  formatting edits, and graph-aware document symbols over the lossless CST.
- **Binary AXON** -- compact and canonical CBOR-profile codecs for the complete
  AXON value model, including graph identity and CID-ready canonical bytes.
- **AXON Schema 2026** -- schema/node normalisation, a governed local module
  registry, structured diagnostics, resource limits, and safe regular-expression
  patterns with no network resolution.
- **Scientific and secured streams** -- checked `Array{shape type data order}`
  layouts and a fail-stop `StreamStart`/`StreamEnd` envelope validator.
- **Serde `Serializer` / `Deserializer`** -- anything deriving `Serialize` /
  `Deserialize` round-trips through AXON, with Rust enum variants mapping onto
  AXON's tagged-node model.
- **`RawValue`** -- carries arbitrary AXON through a typed structure untouched,
  for open extension points and passthrough fields whose shape is not known in
  advance.

## Usage

### Serialise & deserialise

```rust
let text = serde_axon::to_string(&value)?;          // compact
let value: MyType = serde_axon::from_str(&text)?;   // typed
```

### Canonical form & CIDs

```rust
// Deterministic Section 14 bytes -- stable across implementations, suitable for
// checksums, signatures, cache keys, and content identifiers.
let canonical = serde_axon::to_string_canonical(&value);
```

### Untyped values

```rust
use serde_axon::{from_str_value, Value};

let v: Value = from_str_value("point{x:1 y:-2}")?;
```

### Raw values

Serde's data model is narrower than AXON's, which is why [`Value`] implements
neither `Serialize` nor `Deserialize`. When one field of an otherwise typed
structure holds AXON of unknown shape -- an open extension point, a passthrough
field, a format that must preserve what it does not recognise -- `RawValue`
carries it through verbatim:

```rust
use serde::{Serialize, Deserialize};
use serde_axon::RawValue;

#[derive(Serialize, Deserialize)]
struct Record {
    name:  String,
    extra: RawValue,   // any AXON at all, kept exactly as written
}
```

The text is written by this crate's own writer and read back by its own parser,
so nothing crosses Serde's data model in transit and the canonical bytes are
unchanged -- a `RawValue` cannot move how a document compares against the
reference implementation. It is owned rather than borrowed: the `serde_json`
equivalent is unsized so it can borrow from the input buffer, which needs an
`unsafe` transparent-newtype cast, and this crate is `#![forbid(unsafe_code)]`.

### Streaming (multiple top-level values)

```rust
for value in serde_axon::iter_stream("1 2 3") {
    let value = value?;   // yields 1, 2, 3 one at a time
}
```

### Lossless syntax trees

```rust
use serde_axon::{format_cst, parse_cst, Options};

let source = "point{ x:1 # retained\n y:2 }";
let document = parse_cst(source, &Options::default())?;
assert_eq!(document.render(), source);

let formatted = format_cst(source, 2, &Options::default())?;
```

### Migration and editor tooling

```rust
use serde_axon::{diagnostics, migrate_selective, Options};

let migrated = migrate_selective("# keep\n007\n")?;
assert_eq!(migrated.text, "# keep\n7\n");
assert_eq!(migrated.mode.as_str(), "selective");

let issues = diagnostics("[1,,2]", &Options::default())?;
assert_eq!(issues[0].code, "unexpected-token");
```

### Binary AXON

```rust
let bytes = serde_axon::binary::encode_canonical(&value)?;
let decoded = serde_axon::binary::decode(&bytes)?;
```

### AXON Schema 2026

```rust
use serde_axon::{from_str_value, load_schema2026, validation_report2026};

let value = from_str_value(r#"{name:"Alex" age:27}"#)?;
let schema = load_schema2026(
    r#"{kind:"map" required:["name"] properties:{name:{kind:"string" min_length:1}}}"#,
)?;
let report = validation_report2026(&value, &schema);
assert!(report.valid());
```

The `_with` variants accept a local `SchemaRegistry2026` and explicit resource
controls. Set `ValidationOptions::scale_significant` when validating a value
parsed with that profile so `const`/`enum` retain its distinct equality rules.

### Profiles via `Options`

```rust
use serde_axon::{from_str_value_with, Options};

// Graph profile: &label / *label for shared and cyclic structure.
let opts = Options { graph: true, ..Options::default() };
let v = from_str_value_with("[&a point{x:1} *a]", opts)?;
let tree = v.resolve_refs()?;   // expand references into a plain tree

// Document Link profile: cid("...") / link("...") become validated Link values.
let dl = Options { doc_link: true, ..Options::default() };

// compat: the documented legacy-pyaxon reader, for pre-2026 documents.
let legacy = Options::compat();
```

`Options` mirrors the reference implementation field-for-field: the profile
switches, the `compat.*` legacy flags, and the Section 16 resource limits.

## Rust <-> AXON mapping

| Rust | AXON |
|---|---|
| `bool` / integers / floats / `String` | scalar |
| `&[u8]` / `Vec<u8>` (via `serialize_bytes`) | binary `\|...\|` |
| `Option::None` / `()` / unit struct | `null` |
| struct / map | map `{ k: v ... }` |
| `Vec<T>` / slice | list `[ ... ]` |
| tuple / tuple struct | tuple `( ... )` |
| unit enum variant | unit node `Variant` |
| newtype / tuple enum variant | `Variant( ... )` |
| struct enum variant | `Variant{ ... }` |
| `RawValue` | any AXON value, carried verbatim |

Enum variants use **external tagging** onto AXON nodes; internal/adjacent tagging
via `#[serde(tag = ...)]` works through the normal Serde attributes and lands as
ordinary maps.

## Conformance profiles

| Profile | Adds |
|---|---|
| **Core** | every baseline value kind; strict by default |
| **Graph** | `&label` / `*label` anchors and references (shared & cyclic identity) |
| **Doc-Link** | `cid("...")` / `link("...")` produce content-addressed / URI `Link` values |
| **tz-names** | named IANA time zones (`^...[America/Edmonton]`) |
| **compat** | the documented legacy-pyaxon reader, for reading pre-2026 documents |

## `no_std`

The text and binary codecs, value model, Serde adapters, lossless CST,
migration/LSP helpers, schema validator, scientific arrays, and stream
envelopes are `#![no_std]` + `alloc`, so the crate runs on embedded and WASM
targets:

```toml
serde_axon = { version = "1.1.0", default-features = false, features = ["alloc"] }
```

| feature | default | effect |
|---|:---:|---|
| `std` | Y | `std::error::Error` for `Error`, std-dependent conveniences |
| `alloc` | (implied) | the core; always required |

## Parity & conformance

`serde_axon` is held to **byte-for-byte agreement** with the `pyaxon` reference.
A differential harness generates a corpus *from* pyaxon -- each case's
accept/reject decision, canonical bytes, and error category -- and asserts
serde_axon reaches the identical outcome (`tests/differential.rs`). Because
canonical bytes are the cross-implementation contract, one comparison catches
both value-model and writer divergences, and the corpus is regenerated from the
reference so it cannot drift. The semantic corpus currently has 91 cases; a
companion CST fixture covers 60 tokenizer cases, 13 complete documents, 15
indentation migrations, and 5 formatter cases. The Schema oracle adds 42
normalisation cases, 16 identifier cases, 6 configurations, 6 registry
scenarios, 127 validation cases, and 70 safe-regex cases, all generated from
the Python reference. The normative oracle executes 125 requirement-bound AXON
vectors plus all 26 RFC 8785 Appendix B cases. A fifth generated oracle adds
172 Binary AXON, CID, canonical-verification, text/profile, incremental,
migration, LSP, scientific-array, and secured-stream checks.
`tests/check_fixtures.py` regenerates and byte-compares all five fixture
families. `tests/conformance.rs`, `tests/robustness.rs`,
`tests/roundtrip.rs`, `tests/decimal_expansion.rs`, `tests/raw_value.rs`, and
`tests/temporal_round_trip.rs` add adversarial hardening, writer resource
limits, raw-value passthrough, temporal round-trips, and Serde round-trips. The
full suite passes, including `no_std + alloc` and rustdoc, and the crate is
`cargo fmt`- and `cargo clippy -D warnings`-clean.

Identifier classification uses the full UAX #31 tables via `unicode-ident`.

## Minimum supported Rust version

Rust **1.96** (edition 2024).

## Scope

This crate implements text AXON 2026 through Serde, including native `|base64|`
binary values; a lossless text CST for editor and migration tooling; and
**Binary AXON**, the compact CBOR profile of the same value model. It also
implements AXON Schema 2026 normalisation, local registry governance, and
validation, full canonical/selective migration, transport-neutral LSP helpers,
checked scientific arrays, and fail-stop stream envelopes. The binary codec
provides ordinary and canonical, CID-ready whole-document encodings.

## AXON 2026

AXON is *eXtended Object Notation*. AXON Next is its 2026 edition -- a
continuation of the original [`intellimath/pyaxon`](https://github.com/intellimath/pyaxon)
(Zaur Shibzukhov, MIT). `serde_axon` is an independent, clean-room implementation
written from the AXON 2026 specification and verified against the reference; it
contains no `pyaxon` source.

## License

Licensed under either of **MIT** ([LICENSE-MIT](LICENSE-MIT)) or **Apache-2.0**
([LICENSE-APACHE](LICENSE-APACHE)) at your option.
