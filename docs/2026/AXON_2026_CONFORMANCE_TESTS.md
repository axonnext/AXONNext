# AXON 2026 -- Conformance Test Vectors

**Purpose.** Executable acceptance criteria for AXON 2026 implementations. The first executable consumer is now the original Python project (`axon2.v2026`); ports follow only after the Python edition stabilises.

**Verification status.** The historical reader has been compiled and executed on Python 3.12 with Cython 0.29.37. Baseline expectations below are governed by `PYAXON_BASELINE_EMPIRICAL.md`; earlier source-only predictions are withdrawn where that report disagrees. Core vectors are implemented in `lib/axon/test/test_v2026.py` and currently pass.

**Notation.** Expected values are written in AXON itself; `ERR:<id>` means the Section 17.2 category; `=> n values` describes a stream. "Core" = `core` profile; "compat" = the Section 15.3 flag noted. Kinds are spelled where the surface is ambiguous (`Set{...}`, `Map{...}`, `List[...]`, `OMap[...]`, `Tuple(...)`, `Node`, `Str`, `Bytes`, `Date/Time/DateTime`).

---

## 1. Trivia, separators, discards (Section 1.2-1.4, Section 3.8-3.9)

| id | input | Core 2026 | baseline (runtime-verified) |
|---|---|---|---|
| T01 | `[1 2 3]` | `List[1 2 3]` | same |
| T02 | `[1, 2, 3,]` | `List[1 2 3]` | same (comma trivia) |
| T03 | `[,1]` | ERR:unexpected-token | lax/err (sep loop) |
| T04 | `[1,,2]` | ERR:unexpected-token | lax/err |
| T05 | `# c\n1` | `1` | same |
| T06 ! | `#_foo\n1` | discard consumes `foo` (unit node) => `1` | **comment** to EOL => `1` -- the one byte-identical divergence (Section 3.9); `compat.hash-underscore-comment` restores |
| T07 | `[1 #_ 2 3]` | `List[1 3]` | `#_ 2 3]` is a comment => ERR:unexpected-end |
| T08 | `{a:1 #_ b:2 c:3}` | `Map{a:1 c:3}` (whole pair elided) | comment => ERR |
| T09 | `#_ #_ 1 2 3` | => 1 value: `3` | comment line |

## 2. Containers and empties (Section 2.6, Section 3)

| id | input | Core 2026 | baseline |
|---|---|---|---|
| C01 | `[]` | empty `List` | same (`get_list_value` `]` fast-path) |
| C02 | `[:]` | empty `OMap` | same (`:`+`]` branch) |
| C03 | `{}` | empty `Map` | same (`}` fast-path) |
| C04 | `∅` | empty `Set` | same (dispatch `'∅' -> set()`) |
| C05 | `{1 2 3}` | `Set{1 2 3}` | same (`get_dict_value` non-KeyVal first => set) |
| C06 | `{a:1 b:2}` | `Map{a:1 b:2}` | same |
| C07 | `[a:1 b:2]` | `OMap[a:1 b:2]` | same |
| C08 | `{^12:30 ^14:00}` | `Set{Time Time}` -- caret atoms are opaque to recognition (Section 3.5) | caret parses in 0.9 too; the bare-shape `{12:30 14:00}` is `compat.bare-temporals` (same set) |
| C09 | `[^12:30 ^14:00]` | `List[Time Time]` | same; bare shapes under `compat.bare-temporals` |
| C10 | `{a:1 2}` | ERR:unexpected-token (mixing after Map recognition) | loop expects KeyVal => err |
| C11 | `{1 b:2}` | ERR:unexpected-token (mixing after Set recognition) | value loop => err |
| C12 | `{:}` | ERR:unexpected-token (Section 3.2 -- no such literal) | no `:` branch in `get_dict_value` => err |
| C13 | `(1 2)` | `Tuple(1 2)` | same |
| C14 | `(a:1)` | ERR:unexpected-token (Section 3.4) | value-context colon => `error_unexpected_keyval` |
| C15 ! | `{1 2 2}` | ERR:duplicate-set-element (Section 12.2) | silent dedup => `{1 2}` (Python set); `compat.set-dedup` |
| C16 ! | `{a:1 a:2}` | ERR:duplicate-key (Section 12.1) | last-wins => `{a:2}`; `compat.last-wins` |

## 3. Numbers (Section 1.6, Section 5)

| id | input | Core 2026 | baseline |
|---|---|---|---|
| N01 | `-42` | `Int(-42)` | same |
| N02 | `+42` | ERR:invalid-number (Section 5.2 -- no `+` dispatch exists) | ERR (`error_unexpected_value`) |
| N03 ! | `007` | ERR:invalid-number (leading zero, [2026]) | `7` (no check in digit loop); `compat.lax-numbers` |
| N04 ! | `5.` | ERR:invalid-number ([2026]) | float `5.0` (post-point loop is `*`); `compat.lax-numbers` |
| N05 | `.5` | ERR:unexpected-token (no `.` dispatch) | ERR |
| N06 ! | `1_000` | `Int(1000)` ([2026] Section 5.6) | number ends at `1`; `_000` lexes as a *name* => `[1, Node _000]` -- silent-shape divergence: never emit separators to legacy (Section 19.2) |
| N07 ! | `1__0` | ERR:unexpected-token -- the Section 5.9 adjacency rule (number ends at `1`; `__0` is name-start-adjacent) | `[1, Node __0]` -- restored under `compatibility` |
| N08 | `1230D` == `1230d` | same `Decimal(1230)` (Section 1.6.3); `1230$` is ERR:unknown-constant | runtime confirms only `d/D`; `$` is not an ordinary decimal suffix |
| N09 | `-1.25E+6D` | `Decimal(-1250000)` | same |
| N10 | `∞ -∞ ?` | `Float(+inf) Float(-inf) Float(NaN)` | same |
| N11 | `∞D ?D ?$` | decimal inf / decimal NaN x2 | same (`create_decimal_*`) |
| N12 | `1e999` | Core: `Float(+inf)` (IEEE) or ERR under strict-overflow option (Section 5.3) | float('1e999') => inf |
| N13 | `[1-2]` | ERR:invalid-temporal -- Core's bare-shape **tripwire** with a migration hint (Section 5.1/Section 6.5); under `compat.bare-temporals` the full 0.9 commit raises the same category | ERR (`error_invalid_datetime`) |
| N14 | `[1 -2]` | `List[1 -2]` | same |
| N15 | `?$` / `∞$` | decimal NaN / decimal Infinity -- `$` is a **specials-only** decimal marker (Section 1.6.4) | same (runtime) |
| N16 | `1abc` / `10Dx` | ERR:unexpected-token (Section 5.9 adjacency) | `[1, Node abc]` / `[Decimal 10, Node x]` shapes |

## 4. Temporals (Section 6)

Caret inputs test Core semantics; bare inputs test the tripwire and `compat.bare-temporals` (abbrev. *cbt*).

| id | input | Core 2026 | baseline / compat |
|---|---|---|---|
| D01 | `^2012-12-31` | `Date` | same -- canonical 0.9 spelling |
| D02 | `^12:30:34.250` | `Time`, positional fraction (`.250` = 250 ms) | baseline reads a literal-us integer (`.250` = 250 us; caret path too) -- `compat.raw-fraction`; corrected defect |
| D03 | `^9:00` | `Time` (single-digit hour is baseline; canonical pads) | same |
| D04 ! | `^2012-1-5` | ERR:invalid-temporal (padding, [2026]) | up-to-N reader parses it; `compat.lax-temporals` |
| D05 ! | `^12:35+03` | ERR:invalid-temporal (`:MM` required, [2026]) | `+03:00`-equivalent (`to[1]=0`); `compat.lax-temporals` |
| D06 | `^12:35+03:00` | `Time+offset` | same |
| D07 ! | `^...T12:00+24:00` | ERR:invalid-temporal (<= `23:59`, [2026]) | accepted (<= 1440-minute check); `compat.lax-temporals` |
| D08/D09/D10 | `^12:60` / `^24:00` / `^2026-02-30` | ERR:invalid-temporal (Section 6.4) | builder raises |
| D11 | `^2012-12-31T12:30` | local `DateTime` (uppercase `T` only) | same |
| D12 ! | `^2012-12-31 ^12:30` | => 2 values: `Date`, `Time` -- a space never joins (Section 6.5) | same rule for the bare shapes in 0.9 |
| D13 ! | `^2012-12-31T09:35Z` | UTC `DateTime` ([2026] Section 6.7) | `Z` is **outside** the temporal (no `Z` branch) => datetime + unit node `Z` -- never emit `Z` to legacy (Section 19.2) |
| D14 ! | `20261-01-01` (*cbt*) | Core: N13 tripwire | digit-swallow => `date(2026,1,1)` -- silent misparse; migration = manual (Section 19.3) |
| D15 | `^2026-11-01T01:30-06:00[America/Edmonton]` | `tz-names`: zoned `DateTime`; Core: ERR:unsupported-profile-feature | temporal ends at the offset; the `[...]` then fails -- no silent misread |
| D16 | `duration("P1DT2H")` | well-known tag (Section 6.9) | ordinary node -- degrade-safe |
| D17 ! | `12:30` (bare) | ERR:invalid-temporal -- tripwire with migration hint | *cbt*: `Time`; runtime 0.9: `Time` |
| D18 | `^12:00:00.5` | `Time` 500 ms (positional) | baseline: 5 us -- the D02 defect at its sharpest |

## 5. Strings and names (Section 7, Section 9)

| id | input | Core 2026 | baseline |
|---|---|---|---|
| S01 ! | `"a\nb"` (backslash-n in source) | `Str "a
b"` (Section 7.3 escape) | `Str "a\nb"` **literal backslash + n** (passthrough, Section 7.2); `compat.legacy-escapes`; migration doubles the backslash |
| S02 | `"a` 
 `b"` (real newline) | `Str "a
b"` (multiline is baseline) | same |
| S03 | CRLF inside a string | value contains LF (normalisation, Section 7.4) | same (`\r`/`\r\n -> \n`) |
| S04 ! | `"ab\` 
 `c"` (backslash-newline) | continuation => `Str "abc"` ([2026] Section 7.3) | **defective**: `'abab\c'` -- chunk duplication (Section 7.2); `compat.legacy-escapes` reproduces it; migration = manual |
| S05 | `` `x\y` `` | Core: ERR:invalid-escape (`\y`) | `Str "x\y"` (passthrough -- backtick is *not* raw) |
| S06 | `r"x\y"` | `Str "x\y"` ([2026] raw) | ERR (name `r` + `"` => `get_named` fails) -- free syntax space confirmed |
| S07 | `"\u{1F600}"` | `Str "😀"` ([2026]) | passthrough => `\u{1F600}` literally |
| S08 | `"\u{D800}"` | ERR:invalid-escape (surrogate) | passthrough |
| S09 | `'weird name'{x:1}` | `Node 'weird name'` | same (quoted node name) |
| S10 | `{'q':1}` | ERR:unexpected-token -- `'quoted'` is **not** a map key (Section 9.6) | `error_expected_key` |
| S11 | `étage{x:1}` | `Node` -- Unicode names are baseline (Section 9.1) | same (`isalpha`) |
| S12 | `true{x:1}` | ERR:unexpected-token (constant with body, Section 4.3) | runtime: `AxonError` -- the builder rejects a constant with a body |
| S13 | `geo/point{lat:1}` | `Node geo/point` ([2026] Section 9.3) | ERR (`/` after name) -- **not** degrade-safe (Section 19.2) |

## 6. Nodes (Section 4)

| id | input | Core 2026 | baseline |
|---|---|---|---|
| B01 | `greek {alpha:1}` | one `Node` -- same-line brace binds through spaces | same (`skip_spaces` then `{`) |
| B02 | `[Foo {a:1}]` | **one** element: `Node Foo{a:1}` (Section 4.3) | same |
| B03 | `[Foo, {a:1}]` | **two** elements: unit `Foo`, `Map` | baseline raises `AxonError`; comma does not terminate a pending name cleanly |
| B04 | `[Foo` 
 `{a:1}]` | two elements: unit `Node Foo`, then `Map` -- Core binding never crosses a newline | baseline binds across the newline; `compat.legacy-binding` restores that behaviour |
| B05 ! | `Rgb(1 2 3)` | `Node` tuple body ([2026] Section 4.5) | **silent `None`** then the tuple => `[None,(1,2,3)]` -- data corruption, not an error; `compat.legacy-binding` |
| B06 ! | `Tags["a"]` | `Node` list body ([2026]) | silent `None` then the list, as B05 |
| B11 ! | `person 5` | unit node, then `5` (Section 4.3) | `[None, 5]` -- the silent-`None` quirk; `compat.legacy-binding` |
| B12 | `[Foo bar]` | two unit nodes (Section 4.3) | ERR -- the quirk is position-dependent (errors inside a list) |
| B07 | `Rgb` vs `Rgb()` | distinct values (Unit vs empty tuple body, Section 4.4.1) | `Rgb` unit only |
| B08 | `tree{id:1 leaf{id:2 "A"}}` | attrs then children | same |
| B09 | `tree{"A" id:1}` | ERR:unexpected-token (pair after child, Section 4.4) | `get_named` colon error |
| B10 | name 
 *indented* `a: 1` | Core: unit node + separate content per context; `compat.indented-bodies`: bound body (Section 4.6) | indented body (`idn` machinery) |

## 7. Graph, constants, links (Section 9.4, Section 10)

| id | input | Core (`graph` where relevant) | baseline |
|---|---|---|---|
| G01 | `&1 {id:1} *1` (stream) | shared identity across the stream (Section 10.2) | same (`labeled_objects` on Loader) |
| G02 ! | `*9` (unbound) | ERR:unknown-reference | **undefined sentinel** (`dict.get(..., c_undefined)`); `compat.undefined-ref-sentinel` |
| G03 ! | `&1 1 &1 2` | ERR:duplicate-anchor | silent rebind |
| G04 ! | `&1 {v:*1}` | one-node **cycle** ([2026] pre-binding, Section 10.3) | `v` = sentinel (label bound *after* value) |
| G05 | `&x [1] *x` | label may be a name | same (`get_label`: alnum union _) |
| G06 | `*1` outside `graph` | ERR:unsupported-profile-feature | n/a (always on) |
| G07 | `$Inf $NegInf $NaN $NaND` | +∞, -∞, NaN, decimal-NaN -- the four default constants (Section 9.4) | same (`c_constants`) |
| G08 | `$pi` | ERR:unknown-constant | `Undefined name 'pi'` |
| G09 | `cid("bafy...")` | `doc-link`: `Link`; else ordinary node | ordinary node -- degrade-safe |

## 8. Streams and documents (Section 11)

| id | input | Core 2026 | baseline |
|---|---|---|---|
| X01 | `Event{a:1} Event{a:2}` | => 2 values (a stream is not a list) | `loads` => list of 2 |
| X02 | `a: 1` 
 `b: 2` | **attribute document** => one `OMap[a:1 b:2]` (Section 11.1) | `load()` KeyVal branch => one odict |
| X03 | `a: 1` 
 `2` | ERR:unexpected-token (mixing forms) | odict loop expects pairs => err |
| X04 | *(empty / trivia-only)* | => 0 values | `[]` |
| X05 | `axon{edition:"2026" require:["graph"]} &1 {v:*1}` | header **consumed and enabling** (0.11.0a3, D-2) => one value, a self-cycle | ordinary node -- degrade-safe |
| X06 | `...require:["nope"]` | ERR:unsupported-profile-feature **at the header** | ordinary node |
| X07 | header + `Options(allow_header_profiles=False)` | ERR:unsupported-profile-feature at the header, naming the refused ids (Section 11.4) | n/a |

## 9. Binary (Section 8)

| id | input | Core 2026 | baseline |
|---|---|---|---|
| Y01 | `\|SGVsbG8=\|` | `Bytes "Hello"` | baseline uses the open-pipe form `\|SGVsbG8=` and has no closer |
| Y02 ! | `\|SGVsbG8\|` (unpadded) | `Bytes "Hello"` -- closing pipe is the 2026 terminator | baseline rejects it; its own dumper emits some unpadded payloads that its loader cannot read |
| Y03 | `\|SGVs bG8=\|` | interior Section 1.2 whitespace skipped | any <= U+0020 skipped (broader) |
| Y04 | `\|SGVsbG8==X\|` | ERR:invalid-binary | padding-run return leaves `X\|` dangling -- quirk documented Section 8.2 |
| Y05 ! | `\|SGVsbG8=` (open form) | ERR:unexpected-end -- Core requires the closing pipe (Section 8.1); `compat.legacy-binary` => `b'Hello'` | `b'Hello'` -- the only baseline spelling |
| Y06 ! | round-trip `b'ABC'` / `b''` | closed form round-trips (`\|QUJD\|`, `\|\|`) | dumper emits `\|QUJD` / bare `\|` -- **unreadable by its own loader** (Section 8.2) |

## 10. Canonical spot-checks (Section 14)

| id | value (any spelling) | canonical bytes |
|---|---|---|
| K01 | `1000.350d` | `1000.35D` |
| K02 | `{b:2 a:1}` | `{a:1 b:2}` (UTF-8 bytewise keys) |
| K03 | `{2 1}` | `{1 2}` (element canonical-byte order) |
| K04 | `^9:00` | `^09:00:00` |
| K05 | `12:35+03:00` vs `15:35Z`-era instants | distinct values => distinct bytes (no normalisation, Section 6.6) |
| K06 | shared `&`-value used twice | `&1 ...` at first use, `*1` after (Section 14.5) |
| K07 | `-0.0` | `-0.0` (kinds stay split -- documented dCBOR divergence) |
| K08 | attr-document input | canonicalises as its `OMap` value: `[a:1 b:2]` |

---

*Coverage note.* Every ! row is a normative baseline divergence with its `compat.*` flag or Section 19 migration named; together they are the acceptance gate for the `compat` bundle. The complete executable registry is `conformance/normative_vectors.json`: it includes independent vectors for every compatibility flag, every Section 16 resource category, Section 17 UTF-8 byte spans, CST expected-token/fix-it recovery, schemas, migration, canonical graphs, and the full RFC 8785 Appendix B table.
