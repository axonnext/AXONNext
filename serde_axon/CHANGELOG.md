# Changelog

All notable changes to `serde_axon` are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.1.0]

A minor release: it adds one public type, and everything else is a fix. The
fixes accumulated after 1.0.0 was published and had never been released, so this
is the first version to carry any of them.

### Added

- **`RawValue`** — arbitrary AXON carried through a typed structure without
  being interpreted.

  Serde's data model is narrower than AXON's. Crossing into it turns a decimal
  and a temporal into strings, a tuple and a set into sequences, and an ordered
  map and a tagged node into plain maps, which is why `Value` deliberately
  implements neither `Serialize` nor `Deserialize`. The cost of that decision
  fell on anyone needing a field of unknown shape inside an otherwise typed
  structure — an open extension point, a passthrough field, a format that must
  preserve what it does not recognise. Such a field previously forced the whole
  document to be handled untyped.

  `RawValue` holds the AXON source text of one value and returns it untouched:

  ```rust
  #[derive(Serialize, Deserialize)]
  struct Record {
      name: String,
      extra: RawValue,   // any AXON at all, kept exactly as written
  }
  ```

  Owned rather than borrowed. The `serde_json` equivalent is unsized so it can
  borrow from the input buffer, which needs a transparent-newtype cast through
  `unsafe`; this crate is `#![forbid(unsafe_code)]`, and borrowing would buy
  nothing because a value is always rendered from the parsed tree rather than
  sliced out of the source.

  The text is produced by this crate's own writer and read back by its own
  parser, so nothing crosses Serde's data model on the way through and the
  canonical bytes are unchanged. A `RawValue` cannot alter how a document
  compares against the reference implementation.

### Fixed

- **Writing a decimal with an extreme exponent could panic or exhaust memory.**

  A decimal is `mantissa * 10^exponent` and positional notation writes every
  place, so the exponent alone decides the output size. `write_decimal_digits`
  negated the exponent to obtain the scale, and `i32::MIN` has no negation:
  `1E-2147483648D` panicked in a debug build and, in release, wrapped back to
  `i32::MIN`, which `as usize` sign-extended into roughly eighteen quintillion —
  the number of zeroes the padding loop then attempted. In the other direction
  nothing overflowed at all: `1E2147483647D` is fourteen bytes of input asking
  for two gibibytes of output, an amplification of about 150,000,000 to one.

  The negation is now `unsigned_abs`, matching the fix `write_offset` has
  carried since audit M7 and the `checked_add` guard `decimal_numeric_parts` has
  carried since audit M10 — this site sits between them and was missed by both.
  A new writer limit refuses a decimal whose positional form would exceed one
  million digits, enforced in `validate_write` before anything is allocated,
  alongside the existing depth limit. Parsing is unchanged, so the accept/reject
  boundary against the Python reference has not moved.

- **`^..Z[Europe/London]` could not be represented and was rejected.** AXON §6.6
  keeps `Z` distinct from `+00:00`, and that distinction survives into a named
  zone, so `Zone::Named` now records which spelling was used.

- **Decimal zero took different CIDs depending on its scale.** `0D`, `0.0D` and
  `0.000D` are one value and must have one canonical spelling; the trailing-zero
  loop stopped while one digit remained, so a mantissa of `0` with a negative
  exponent kept its scale.

- **Signed decimal zero made the text↔binary round trip lossy.** AXON §5.2 has
  no signed integer zero, and Binary AXON encodes a decimal mantissa as a CBOR
  integer, where `-0D` and `0D` share the bytes `c4820000`.

- **Migration rewrote reserved barewords to `null`, silently changing values.**
  The tokenizer classifies `true`, `false` and `null` as name tokens, so the
  pending-legacy-name rule turned `true []` into `null []`. The quoted spelling
  `'true'` is a name and is deliberately unaffected.

- **Migration fused a bareword into its neighbour, losing data.** Only `(` and
  `[` were spaced, so `'quoted name'1_0[1 2]` became `null1_0[1 2]` and the
  `1_0` was lost from the migrated document.

- **Two parser diagnostics were wrong.** `3.5:` and `1d:` reported a stray token
  rather than the bare-temporal tripwire the constraint index requires, and
  `1d[]` reported `unsupported-profile-feature` because a speculative parse
  raised instead of yielding to the number path.

- **Schema `any_of` could be driven exponentially.** Alternatives are explored
  independently, so nested alternation costs 2^d evaluations for a value
  matching none of them. The cost now has its own budget, mirroring
  `MAX_ANY_OF_EVALUATIONS` in the Python reference.

- **Resolving a CST span was quadratic in document size.** Byte offsets are now
  resolved through a line-offset table built in one pass.

## [1.0.0] — 2026-07-28

First release.
