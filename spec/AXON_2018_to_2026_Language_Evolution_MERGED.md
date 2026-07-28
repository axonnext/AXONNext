# AXON 2018 -> 2026 Language Evolution

## Modernising the Original AXON Language Before Continuing the Rust Port

**Document status:** 1.0
**Basis:** **ground-truth inspection of the real `intellimath/pyaxon` source and example files** (cloned, parsed, and grepped directly), together with a survey of the 2025-26 data-format landscape.
**Primary purpose:** language evolution, not Rust implementation design.
**Baseline language:** original AXON / pyaxon (eXtended Object Notation).
**Corrections and additions:** three syntax claims were corrected against real pyaxon; three genuinely-2026 developments (LLM token-efficiency, content-addressing, deterministic-encoding standards) were added; several internal inconsistencies were resolved. All corrections are marked **[GROUND TRUTH]** or **[2026]**.

---

## Part 0 -- Ground Truth: What Real pyaxon Actually Does

This section is new and takes precedence over any conflicting claim elsewhere in the document. It comes from directly cloning `intellimath/pyaxon` and reading the lexer (`lib/axon/_loader.py`), the dumper (`lib/axon/_dumper.py`), and the hand-written `.axon` example files. Where the earlier research draft or the `serde_axon` proof-of-concept diverged from real AXON, the divergence is called out so that the 2026 spec extends the *actual* language rather than an implementation's inventions.

### 0.1 Corrections to the original research draft

| Feature | Original draft claimed | `serde_axon` PoC | **Real pyaxon 0.9 [GROUND TRUTH -- runtime-verified]** |
|---|---|---|---|
| Date/time literals | `^2012-12-31` (caret-prefixed) | `^...` prefixed | **The draft was right and this document's first correction was wrong.** Changelog 0.9: temporals gained the `^` prefix; bare forms are the *deprecated pre-0.9* notation, still loadable in 0.9.x and slated for removal in 0.10. Runtime: `loads` accepts both; **`dumps` emits `^12:00`** -- caret is canonical original. |
| Decimal suffix | `D` only | `D` | **`d`/`D` only -- the draft and `grammar.ebnf` were right.** Runtime: `1230$` is an *error* (`$` routes to the constants path); `$` acts as a decimal marker **only on specials** (`?$` -> decimal NaN, `∞$` -> decimal Infinity). This document's earlier "`$` is a synonym" row misread the specials branch. Dumper preserves scale: `Decimal('1000.350')` -> `1000.350D`. |
| Binary literal | `^x"deadbeef"` (hex) | `^x"..."` (hex) | **Open-pipe MIME Base64, `=`-terminated -- no closing pipe** (docs, grammar, and runtime agree). The literal ends at the padding run; interior chars <= U+0020 are skipped (multiline by design). **Proven original defect:** payloads with `len % 3 == 0` produce no padding, so `dumps([b'ABC'])` -> `\|QUJD\n` **cannot be reloaded** ("MIME Base64 string is not finished"). Hex remains a PoC invention. |
| String forms | `"..."`, optional raw `r"..."` | `"..."` | `"..."` **and** backtick strings `` `...` `` (`elif ch == '\`': return self.get_string(ch)`). |
| Named-node special form | quoted keys only | -- | Single-quoted **names**: `'weird name'{...}` -- `'` introduces a *name* for a node, distinct from a `"..."` string value. |
| `∞` / `?` specials | float specials | `∞`, `-∞`, `?` | Float specials **plus decimal variants**: `∞D`, `?D`, `?$`, etc. (`∞`/`?` followed by `D`/`d`/`$` produce `create_decimal_inf`/`create_decimal_nan`). |
| Anchors / references | `&a Person{...}` / `*b` | omitted (Serde can't model graphs) | **Authentic and confirmed**: `&label value` defines, `*label` references. Label may be an integer or a name. Example: `&1 { id:1 value:"A" }` ... `children: [*1 *2]`. |

### 0.2 Authoritative original-AXON atom syntax (from the lexer dispatch)

The atom dispatcher in `_loader.py` recognises, among others:

- Leading digit -> number; may become integer, float, decimal (`d`/`D`), or a deprecated bare temporal in the historical compatibility reader.
- `"` -> escaped string; `` ` `` -> backtick string; `'` -> single-quoted **name** (introduces a named node).
- `|` -> **base64 binary** literal -- opened by `|`, terminated by the `=` padding run (open form; Section 0.1).
- `∞`, `-∞`, `?` -> float infinity/NaN; with a trailing `D`/`d`/`$` -> the decimal-domain equivalents.
- `&label value` -> define a labelled (anchored) value; `*label` -> reference a labelled value.
- `$name` -> a registered constant reference. `$` is not an ordinary decimal suffix.
- `{ ... }` dict, `[ ... ]` list / ordered-dict, `( ... )` tuple, `name{...}` node bodies (same-line brace, or indented body in formatted style), comma optional throughout, `#` line comments. **Correction (deeper `get_named` inspection):** `name(...)` and `name[...]` node bodies do **not** exist in pyaxon -- after a name, only `{` (same line) or an indented body binds; `(`/`[` is an error. Tuple/list-bodied nodes are a serde_axon/2026 proposal, tagged **[2026]** in the spec. Additionally: node bodies are *attributes-then-children* (a `key:value` after the first positional child errors), and node attribute keys are bare or `'quoted'` names only.
- **Further ground-truth finds (deep loader inspection):** `∅` (U+2205) is the **empty-set literal** and `{V ... V}` (brace with non-pair first element) is a **set** -- pyaxon has a native set type; `[:]` is the **empty ordered-dict literal** (resolving the `[]` ambiguity the earlier draft agonised over -- `[]` is simply the empty list); `$name` is a **registered-constant reference** (parse-time lookup; undefined -> error), not a name form; bare names are **Unicode** (Python `isalpha`/`isalnum` classes), not ASCII; dotted names and `\n`/`\t`/`\u` escapes exist only as commented-out code; map/ordered-dict keys are bare names or `"strings"` (single-quoted names are *not* map keys -- those belong to node attributes only); and a top-level document is either a value stream or, if it opens with a `key: value` pair, a single ordered dict. The `$name` constant table ships **non-empty**: `$NaN`, `$NaND`, `$Inf`, `$NegInf` are registered by default (`true`/`false`/`null` appear only as commented-out entries). **Runtime resurrection:** the checkout was brought back to life on Python 3.12 -- pin `cython==0.29.37`, cythonize `odict.pyx` + `_objects.py` + `_loader.py` + `_dumper.py` from `lib/`, reconstruct the never-committed **`utils.h`** (eleven inline helpers/macros over the CPython C-API: `current_char`/`next_char`/`skip_char`/`get_chunk` as struct-field macros, plus the `c_*` conversions), and `gcc -O0 -fPIC -shared` each module. Every ground-truth claim in this document was then **runtime-verified** against the live 0.9 build; the corrections that verification forced (caret temporals, the `$` suffix, open-pipe binary and its round-trip defect) are folded into Section 0.1/Section 0.3 above.

### 0.3 Consequences for the 2026 spec (revised after runtime verification)

1. **Caret temporals are the original canonical form.** The author's own migration plan (changelog 0.9->0.10) deprecates bare temporals; AXON 2026 follows it: `^`-prefixed temporals are core, bare temporals live behind a compatibility flag with the author's own `dumps(loads(text))` recipe as the migration. The caret also dissolves the digits-then-`-`/`:` lexer commitment and its `[1-2]` footgun in core.
2. **Decimal suffixes are `d`/`D`; there is no numeric `$` suffix.** `$` is the constants sigil (`$Inf`) and a decimal marker on specials only (`?$`, `∞$`).
3. **Binary is open-pipe base64, `=`-terminated, no closing pipe** -- and carries a proven round-trip defect for `len % 3 == 0` payloads that the 2026 spec must fix explicitly rather than inherit or paper over.
4. **Backtick strings, `'name'` names, sets (`{V ...}`, `∅`), `[:]`, `$name` constants, and `&`/`*` references are all runtime-confirmed** and must be specified as baseline.
5. **Version provenance matters:** `grammar.ebnf` documents pre-0.8 AXON; `index.rst` + changelog document 0.8/0.9. The 2026 baseline is **pyaxon 0.9 as executed**, with the pre-0.9 forms as legacy input.

---

## Executive Summary

The original AXON language was already ahead of JSON in several important ways: a text format for interchanging objects, documents, and data; a deliberate blend of ideas from JSON, XML, and YAML; native date/time, decimal, and binary values; named/tagged nodes; cross-references; both compact and formatted styles; and comma removed as a mandatory separator. Real pyaxon confirms all of this and adds detail the secondary sources missed (caret-canonical temporals with the bare shapes as the deprecated pre-0.9 input, `d`/`D` decimals with a specials-only `$`, open-pipe base64 binary, backtick strings, single-quote node names, native sets, `$name` constants, and label-based `&`/`*` references).

AXON 2026 should therefore **not** be a trendy rewrite. It should preserve AXON's object-notation identity and bring its specification quality, interoperability rules, literal system, parser-safety model, canonical serialisation, and schema story up to modern expectations. The strongest modern formats are not the richest; they are the ones whose features are precisely specified and whose limitations are intentional.

Three developments since 2018 were **absent from the original research draft** and are the main additions of this merge:

- **[2026] LLM token-efficiency (TOON).** A format explicitly designed to feed structured data to language models with far fewer tokens appeared *because* JSON tokenises wastefully. AXON's comma-free, whitespace-separated design is unusually well-positioned here, which motivates an optional columnar form for homogeneous records -- with a firm caveat (below) about when it actually helps.
- **[2026] Content-addressing / linked data (IPLD, CIDs, DASL).** Content-addressed, self-certifying data went mainstream across IPFS, Filecoin, and ATProto. This reframes AXON's `*label` reference as potentially more than intra-document: a link can point at a content identifier. Crucially, **references and canonical form are the same problem** -- you cannot content-address without deterministic bytes.
- **[2026] Deterministic-encoding standards (dCBOR, JCS, CDDL).** Canonical serialisation is no longer "nice to have"; it is under active IETF standardisation. dCBOR and JCS (RFC 8785) have already litigated the exact edge cases (NaN canonicalisation, float/number reduction, key ordering) that a Canonical AXON profile must decide, and CDDL is a direct precedent for a companion schema language.

The recommended AXON 2026 direction (updated):

1. Keep named/tagged nodes as the core identity of AXON.
2. Keep comma-free containers, with comma accepted as optional whitespace.
3. Retain compact and formatted styles; define their equivalence precisely.
4. Preserve native decimal (`d`/`D` in, `D` out), temporal (**`^`-prefixed** canonical, bare as legacy input), and binary (open-pipe legacy, closed-pipe 2026) values as first-class, and make decimal/temporal genuinely lossless in the value model.
5. Distinguish semantic values from presentation trivia.
6. Add a **Canonical AXON** profile, modelled on dCBOR + JCS, for hashing, signing, deterministic tests, and content-addressing.
7. Add a lossless concrete-syntax-tree (CST) model for tooling.
8. Add a semantic value model for application use.
9. Specify duplicate-key and ordered/unordered map behaviour explicitly.
10. Treat anchors/references (`&`/`*`) as an advanced graph profile, optionally carrying content-addressed links.
11. Add parser resource limits to the spec.
12. Define schema/validation as a companion layer (à la CDDL / CUE), not core notation.
13. Consider an **optional columnar form** for homogeneous arrays (TOON-style), opt-in per block.
14. Avoid indentation-sensitive semantics and YAML-style implicit typing.
15. Do **not** turn AXON into a Turing-complete or constraint-solving language.

Note the last point aligns with the direction already set for this project: extend the *notation*, do not build a different (config-programming) language.

---

## 1. Scope and Non-Scope

This document is about the AXON language itself. Rust matters only because the `serde_axon` proof-of-concept exposes where modern type systems and serializer frameworks press on AXON's model; AXON's semantics must not be reduced to Serde's data model. It is a research and design study answering: *if AXON's 2018-era language were evolved using the lessons of data and configuration languages through 2026, what belongs in AXON, what stays out, and what should be optional or profile-based?*

Features are judged on seven criteria: faithfulness to AXON's object-notation philosophy; human readability; machine determinism; losslessness; security; interoperability; and long-term value beyond current fashion.

---

## 2. Original AXON Baseline

AXON is *eXtended Object Notation*: a simple text format for interchanging objects, documents, and data, explicitly combining the best of JSON, XML, and YAML. Its stated objectives include overcoming JSON's lack of date/time, decimal, and binary; representing complex data with cross-references natively; native named/tagged structures; both JSON-style and YAML-style formatting; removing comma as a mandatory separator; and staying relatively simple.

**[GROUND TRUTH]** The real composition rules (confirmed against pyaxon):

```text
list         [ V ... V ]
tuple        ( V ... V )
dict         { K:V ... K:V }
ordered dict [ K:V ... K:V ]
node         N { N:V ... N:V  V ... V }        # N may also be 'quoted name'
```

Authentic literal forms include:

```axon
[1 3.14 3.25D ∞ -∞ ?]                 # int, float, decimal, float-inf, -inf, NaN
(true ^12:00 ^2001-12-31 ^2001-12-31T12:00)  # caret temporals -- 0.9 canonical; bare = pre-0.9 legacy input
{alpha:1 beta:2 "other chars":4}       # bare + quoted keys
greek {alpha:123 beta:212}             # named node
1230D                                  # exact decimal; d/D are accepted
|SGVsbG8=                              # base64 binary -- open form, "="-terminated, no closing pipe in 0.9
&1 { id:1 value:"A" }  ... children:[*1 *2]  # anchor / reference by label
```

This tells us: AXON's type system was intended to be richer than JSON; whitespace separation was deliberate; nodes carry both named fields and positional children; and AXON had graph/object ambition, not just map/list ambition.

### 2.1 Original strengths to preserve

| Original feature | Why it matters | 2026 status |
|---|---|---|
| Named/tagged nodes | AXON's identity and object orientation | Preserve; specify precisely |
| Native decimals (`d`/`D`) | Avoids float loss in money/science | Preserve; make lossless mandatory; `D` canonical (`$` is constants/specials only) |
| Native temporals (**`^`-prefixed**) | Avoids stringly-typed dates | Preserve caret canonical (0.9); bare shapes = legacy input via compat |
| Binary (open-pipe base64) | Avoids base64-as-string ambiguity | Preserve; 2026 adds the closing pipe that fixes the `%3==0` round-trip defect |
| Comma-free containers | Less punctuation noise | Preserve |
| Compact + formatted styles | Machines and humans | Preserve; define equivalence |
| Cross-references (`&`/`*`) | Object graphs, not just trees | Preserve as optional profile |
| Backtick strings / `'name'` | Ergonomic raw text / odd names | Preserve; fold into string/identifier rules |
| Relative simplicity | Prevents YAML sprawl | Treat as a hard constraint |

---

## 3. Evidence from the `serde_axon` Crate

The crate is a Serde data-format binding implementing a pragmatic compact core of AXON. Its README maps Rust/Serde values to AXON (structs->named nodes, `Vec`->`[...]`, tuples->`(...)`, maps->`{...}`, enum variants->bare/bodied nodes, etc.). Its in-memory `Value`/`NodeBody` model is compact and practical.

Usefully, it exposes exactly the pressure points the language spec must resolve -- and, per Part 0, it also **invented** two spellings (`^`-prefixed temporals, `^x"..."` hex bytes) that are *not* original AXON. Treat those as PoC conveniences, not language decisions.

### 3.1 What `serde_axon` proves

Structs serialise naturally as named nodes; enums as bare/bodied nodes; whitespace separation and optional commas work cleanly; byte literals can be compact; document streams are useful; a nesting limit is easy to enforce. This supports keeping AXON's core syntax.

### 3.2 What `serde_axon` exposes (must be fixed at the language-spec level)

1. **Lossless decimals.** Mapping `1000.35D` to `f64` defeats a core reason AXON exists. Use an exact decimal representation.
2. **Typed temporals.** Date, time, datetime, offset, and duration need distinct parse results, not strings.
3. **A graph model** separate from the tree model for `&`/`*` (Serde can't express shared/cyclic references, but AXON should not lose the ambition).
4. **Two internal models**: a semantic value tree and a concrete syntax tree (for formatters, linters, comment-preserving rewrites).

---

## 4. Language Evolution Since 2018: What Actually Changed

Developments fall into several directions:

- **Ergonomic:** comments, trailing commas, raw/multiline strings, numeric separators.
- **Semantic:** native temporals, lossless decimals, schema languages, constraints, tagged values, validation.
- **Operational:** canonical serialisation, deterministic encoding, reproducible builds, signing, supply-chain security, streaming parsers, DoS resistance.
- **Cautionary:** YAML's complexity, implicit-typing surprises, unsafe deserialisation, implementation divergence.
- **[2026] LLM-facing:** token-efficient encodings for feeding structured data to language models (see Section 9.6).
- **[2026] Content-addressed / linked data:** self-certifying data identified by content hash (see Section 10.5).

The lesson: *more features* is not automatically better. The strongest modern formats are the ones whose features are precisely specified and whose limits are intentional.

---

## 5. JSON5: Ergonomics Without a Richer Data Model

JSON5 adds comments, trailing commas, unquoted identifier keys, single-quoted strings, line-continuation strings, extra number forms, hex numbers, `Infinity`, `-Infinity`, and `NaN`. Its lesson is *which* JSON complaints humans hit most. AXON already solves several differently (no mandatory commas, named structures, native specials).

**Adopt:** comments as first-class (semantic-trivia, preserved in the CST profile); commas as optional whitespace-equivalent separators that never create empty elements; a formal bare-name grammar.

**Reject/constrain:** extra string spellings unless presentation-only with a single canonical form; leading/trailing-point numbers (`.5`, `5.`) rejected in canonical syntax; hex *integers* deferred (note this is distinct from AXON's `|base64|` binary, which is real).

---

## 6. TOML: Strong Literal Discipline

TOML 1.0 shows how much value a modest, precisely-specified format delivers: typed scalars, arrays, tables, inline tables, and dates/times with optional offsets. Its lesson is **discipline** -- it does not try to become a programming language.

**Adopt:** strict ISO-8601 / RFC-3339-compatible temporal rules (bare dates/times parsed only when unambiguous; offsets preserved); numeric separators (`_` between digits, never adjacent to sign/point/exponent/suffix; omitted in canonical output); raw and multiline strings **only if** specifiable more simply than YAML.

**Do not copy:** TOML tables (AXON nodes are more natural) or dotted-key structure creation (collides with names/paths; defer).

---

## 7. YAML: The Warning Sign

YAML is visually light but the clearest warning against over-expanding AXON. Its own model usefully distinguishes presentation stream, serialization tree, representation graph, and native construction -- but its practical complexity (many scalar styles, indentation-sensitive structure, anchors/aliases/tags/directives, implicit-typing history, implementation divergence) is a cautionary tale.

**Adopt carefully:** the layered model -- source text, tokens, concrete syntax tree, semantic value tree, optional graph, native objects. Support references only in a named graph profile, never in the mandatory safe core.

**Reject:** indentation-sensitive semantics (keep explicit delimiters canonical); implicit typing (`yes`/`no`/`on`/`off`/date-like surprises); many string block styles; unsafe construction by default (parsers must not instantiate arbitrary application objects; construction is opt-in and type-registered).

---

## 8. KDL: Node-Oriented Design as Independent Confirmation

KDL is node-oriented -- names, ordered arguments, properties, and children, with argument order preserved, property order not assumed, and a slashdash to comment out nodes/properties. It independently validates AXON's instincts (named nodes are readable; positional args and named props coexist; child nodes suit documents; node-level disabling helps editing). AXON should study KDL, not become it.

**Adopt conceptually:** an explicit ordered/unordered split -- positional children, lists, tuples, and ordered dicts preserve order; plain dicts make no semantic ordering guarantee unless a profile says so; CST tools may still preserve original order. Consider a block/disabled-node comment mechanism only if it stays lexical trivia.

**Avoid:** silent rightmost-wins property override. AXON should make duplicate handling explicit by profile (strict = error; lossless = preserve in CST; compatibility = documented last-wins/first-wins/multi-map).

---

## 9. CUE (and the config-language cluster): Validation Belongs *Beside* AXON

CUE is a strongly-typed, constraint-based language whose defining idea is that **types are values and values are types**, composing by unification. It is deliberately restricted (not Turing-complete). **[2026]** It sits in a now-crowded config-as-code cluster: alongside CUE are **Pkl** (Apple, 2024 -- an object-oriented, Turing-complete config language with rich validation and codegen), **Nickel**, **Dhall**, and **KCL**, which share the JSON data model with first-class validation, gradual typing, and late-bound overriding.

The lesson for AXON is the *demand* for validation -- not a mandate to absorb constraints, unification, imports, or code generation into the core. Doing so would forfeit AXON's simplicity and, in effect, build a different language.

### 9.1 Recommendation: companion schema language

Reserve **AXON Schema** as a companion specification. Core AXON answers *"what value does this text denote?"*; AXON Schema answers *"is this value valid for this interface?"* -- required/optional fields, types, numeric ranges, string patterns, enum variants, tuple arity, element types, node names, graph/reference constraints, and canonicalisation requirements. CBOR's **CDDL** (Section 10) and CUE are the reference points. Keeping validation separate preserves AXON as a simple interchange format while letting serious applications enforce contracts.

### 9.6 [2026] LLM Token-Efficiency (TOON) -- *new section*

A 2025-born format, **TOON (Token-Oriented Object Notation)**, is a compact, lossless encoding of the JSON data model built specifically to feed structured data to language models with fewer tokens. It combines YAML-style indentation for nested objects with a **CSV-style tabular layout for uniform arrays**, declaring field headers and array lengths once. Reported savings are roughly 30-60% fewer tokens than JSON on uniform arrays (~40% in mixed benchmarks) at comparable or better model accuracy, with spec implementations across TypeScript, Python, Go, Rust, and .NET.

Why this matters for AXON: AXON is *already* comma-free and whitespace-separated -- the properties TOON adopts. This motivates an **optional columnar form** for homogeneous records, e.g.:

```axon
lines: #[sku qty price]
  "A-1" 2 19.99D
  "B-7" 1 960.37D
```

**Firm caveat (the nuance TOON's own benchmarks establish):** the win exists *only* for uniform arrays of records. For deeply nested or non-uniform data, plain JSON/AXON is often more token-efficient, and a columnar layout adds overhead. Therefore any AXON columnar form must be **opt-in per homogeneous block**, never a core layout change, and never the canonical form. (Also note: models are not trained on either TOON or AXON, so both pay a small in-context-learning tax -- an argument for keeping the columnar form simple and well-documented.)

Scope judgement: TOON is a *re-encoding* of JSON's data model, not a language evolution per se, so this belongs as an optional profile plus a paragraph in Section 4 -- not as a reshaping of the core.

---

## 9A. EDN, MessagePack, BSON, and HJSON -- Tags, Interop, and One Reject

Four more formats were evaluated. The finding: EDN is a rich source, MessagePack and BSON are valuable as type/interop lessons (both are binary, so nothing to borrow as *syntax*), and HJSON is mostly a cautionary *reject*.

### 9A.1 The cross-cutting insight: the tag/extension pattern

EDN's tagged literals (`#uuid`, `#inst`, namespaced `#myapp/Person`), MessagePack's ext types (with a **reserved timestamp ext**), BSON's binary subtypes, and CBOR's tags all converge on one idea: a registered way to attach a type the base model lacks. **AXON's `Name{...}` nodes already embody this mechanism.** So the reusable lesson is not "add tags" -- it is **discipline around tags**: (a) namespacing to avoid collision (EDN's `namespace/tag` form; cf. the `geo/point{...}` idea -- slash, not colon, since a name followed by `:` is pair syntax in AXON), and (b) a small **registry of well-known tags** (at minimum `uuid` and `instant`) so independent AXON parsers -- and other formats -- agree on their meaning. This is motivated four times over and is the single most important takeaway of this section.

### 9A.2 EDN -- the richest source

- **`#_` discard form** elides the *next* element (`[a #_ b c]` -> `[a c]`). It is EDN's analogue of KDL's slashdash (Section 8), giving the "node-level disabling" idea two independent precedents. **Adopt as lexical trivia.**
- **`M` and `N` suffixes** confirm AXON's choices: `M` is EDN's exact decimal (like AXON's `D`), `N` its arbitrary-precision integer. EDN is precedent that a *text* notation can carry arbitrary-precision numbers via a suffix -- exactly AXON's decimal model.
- **First-class set literal `#{...}`** -- **superseded by ground truth**: pyaxon already *has* a native set type, written `{V ... V}` (brace container whose first element is a bare value, not a pair) with `∅` as the empty set. EDN's `#{...}` is therefore unnecessary; the keeper is EDN's confirmation that text notations want a set kind, which AXON turns out to have had all along. (`serde_axon` flattening sets to sequences was a binding limitation, now doubly so.)
- Commas-as-whitespace AXON already shares. **Skip** EDN keywords (`:kw`) and character literals (`\c`): named nodes and bare-name keys already cover the symbolic role, and AXON needs no char type.

### 9A.3 MessagePack -- interop, not notation

Binary, so no syntax to borrow. Two keepers: its **reserved timestamp extension** (seconds + nanoseconds since epoch) is the binary form of a canonical instant type, reinforcing the "canonical UTC timestamp subtype" note in Section 12.3; and its **`bin` vs `str` split** confirms AXON's bytes-!=-text separation (`|base64|` vs strings) is standard practice. **Action:** define a mapping of AXON values <-> MessagePack ext / timestamp / bin for interop.

### 9A.4 BSON -- a type catalogue and a binary idea

Also binary. Lessons: **binary subtypes** (BSON tags a blob as generic / UUID / encrypted / ...) suggest AXON could optionally let a binary literal carry a subtype tag, or wrap it in a node (`UUID(|...|)`); **decimal128** is the bounded 128-bit decimal databases use, so **Canonical AXON should define a mapping to/from it** (AXON's arbitrary-precision decimal is the superset); and BSON's ordered documents plus its int32/int64 distinction reinforce our ordered-map handling and make our deliberate choice to keep integer *width* out of the surface syntax a conscious one (BSON puts width in the wire format -- the counter-example that means we owe an interop mapping, not a syntax change).

### 9A.5 HJSON -- mostly reject

Its safe features (comments, optional/trailing commas, unquoted keys, triple-quoted multiline) AXON already has. Its signature feature -- **quoteless strings** running to end of line -- is exactly the ambiguity trap this document rejects for YAML implicit typing (Section 7): a bareword could collide with a name, a number, or a bare temporal. **Explicit reject**, with the safe parts noted as already-present.

### 9A.6 What earns a place

1. **Tag namespacing + a small well-known-tag registry** (`uuid`, `instant`) -- EDN / MessagePack / CBOR convergence; rides on AXON's existing node mechanism.
2. **An EDN `#_`-style discard/elide form** -- matches KDL slashdash; lexical trivia.
3. **Canonical interop mappings**: AXON decimal <-> decimal128; AXON instant <-> MessagePack/CBOR timestamp ext; AXON bytes <-> BSON binary subtypes / MessagePack `bin`.
4. **Optional (profile-gated)**: first-class set literal `#{...}` (EDN); binary subtype tag (BSON).
5. **Explicit reject**: HJSON quoteless strings -- same ambiguity ground as YAML implicit typing.

### 9B. XSON -- Convergent Architecture, Two Keepers

**XSON** (eXtensible Syntax Object Notation, Bryan Ford, <https://bford.info/draft/xson/>) is an early, openly incomplete draft (placeholder links, unresolved "how?" notes), so it is an *idea source*, not a spec to track. Its architecture: every element has a universal **syntax-tagged form `tag[content]`** with optional shorthands; a document begins with a header `xson[imports]` declaring which syntax extensions it uses; a **minXSON** kernel (strings + arrays only) underlies JSON, which is "just" minXSON plus the `null`/`bool`/`number`/`object` extensions; and encoders **re-decode their own shorthand output** to detect cross-extension ambiguity, falling back to the tagged form on collision.

**Convergent validation.** `tag[content]` is structurally what AXON nodes already are -- independent confirmation (alongside KDL, Section 8) that named-tag-plus-body is the right primitive. minXSON's extension layering is the same instinct as AXON 2026's profiles (Section 28).

**Keepers:**
1. **Braced, variable-length Unicode escapes** -- XSON adopts Swift-style `\u[hex]` precisely to kill JSON's surrogate-pair escapes and the `\u`/`\U` split. AXON 2026 Section 7 should adopt the Rust/Swift spelling `\u{hex}` (1-6 hex digits, any scalar value, surrogate code points forbidden). Directly relevant ground truth: pyaxon's own `\u` escape path is *commented out* in the lexer, and unsupported surrogate-pair escapes were a real `serde_axon` audit finding -- so the escape story is genuinely unfinished in the baseline and needs this.
2. **Document-declared extensions** -- the `xson[imports]` header is the interesting one: the *document* states what it needs, instead of the parser being configured out-of-band. Adopt as a **candidate** for Section 15/Section 28: an optional AXON header declaring edition + required profiles, decided when the profiles section is written.

**Rejects:**
- *Encoder collision self-check* -- clever, but it exists because XSON's open extension set makes shorthand ambiguity unavoidable. AXON's grammar resolves ambiguity in-language (e.g. the `[...]` recognition rule); no ambiguous shorthands exist to collide, so the mechanism is unnecessary.
- *Rational numbers (`a/b`)* -- exact fractions add normalisation/equality/canonicalisation cost that decimals + arbitrary-precision integers already cover for the financial/scientific cases AXON targets. Also note `1/2` would sit lexically beside `ns/name` namespacing -- separable (digits vs name-start), but not worth the cognitive overlap. Rejected.
- *`0x`/`0o`/`0b` integer extensions* -- consistent with JSON5; AXON keeps radix literals **deferred** (Section 5), with XSON now a second precedent if ever added.

---

## 10. CBOR and Deterministic Encoding: Why Canonical AXON Matters

CBOR is binary, not a human notation, but its evolution is directly relevant because it is where deterministic encoding was worked out. RFC 8949 already notes that protocols need explicit rules for number representation and map-key ordering, and must specify how decoders handle invalid/unexpected data.

**[2026] This is now under active IETF standardisation**, and it materially strengthens the case for a Canonical AXON profile:

- **`draft-ietf-cbor-serialization`** defines three serializations -- *ordinary*, *deterministic*, and *general* -- clarifying that general serialisation is inherently non-deterministic and that determinism is an explicit, opt-in mode.
- **dCBOR (`draft-mcnally-deterministic-cbor`, draft-17)** is a dedicated deterministic profile whose single goal is determinism/non-malleability. It has **already decided the edge cases AXON must decide**: reduce all NaN values to the canonical half-width quiet NaN `0x7e00`; reduce reducible floats (`12.0` -> `12`); and reject non-preferred encodings on decode.
- **JCS (RFC 8785)** is the JSON analogue: sort object keys by UTF-16 code-unit order and emit the shortest decimal that round-trips a double -- used precisely for signatures, content-addressed storage, and cache keys.
- **CDDL** is CBOR's companion schema language -- a direct precedent for AXON Schema (Section 9.1).
- **EverCBOR** (formally verified CBOR emitting safe Rust, with the first machine-checked non-malleability proof of deterministic encoding) is a template for the rigour a Canonical AXON spec could aspire to.

The takeaway: **do not design Canonical AXON from first principles.** Model it on dCBOR + JCS, which already answered NaN canonicalisation, number reduction, and key ordering. Human-readable formats are increasingly used in lockfiles, signed metadata, reproducible tests, cache keys, and distributed systems -- "pretty close" serialisation is not enough.

### 10.1 Canonical AXON profile (draft)

Canonical AXON should specify: UTF-8 only; a Unicode-normalisation policy for identifiers (and a documented decision *not* to normalise string values); one spelling each for `true`/`false`/`null`; decimal output as `D` (never `d`/`$`); decimal/temporal normalisation rules; **base64 canonicalisation** for binary (case, padding, no interior whitespace); map-key ordering (à la JCS/dCBOR); no comments; no optional commas; no numeric separators; a single string delimiter (`"..."`, not backtick) with defined escaping; and a decision on `∞`/NaN (dCBOR canonicalises rather than forbids -- recommended over the earlier draft's "may forbid").

Canonical AXON is the stable target for signing, hashing, reproducibility, and equality -- not necessarily the prettiest human form.

### 10.5 [2026] Content-Addressing and Linked Data -- *new section*

The biggest thing that happened to *references* between 2018 and 2026 is that **content-addressing went mainstream**. **IPLD** (InterPlanetary Linked Data) is now the shared data model for the self-certifying, content-addressable web across IPFS, Filecoin, and ATProto/Bluesky, with a streamlined subset, **DASL** (Data Addressable Structures and Links), and a consolidated Rust ecosystem (`ipld-core`, superseding the deprecated `libipld`; DAG-CBOR / DAG-JSON as the canonical codecs). W3C Verifiable Credentials and DIDs reached Recommendation status in the same period.

This reframes AXON's `*label` reference (Section 18). Today there are *two* kinds of reference:

1. **Intra-document anchor** -- the existing `&label` / `*label`, pointing within one document (this is what pyaxon implements).
2. **[2026] Cross-document content-addressed link** -- a reference that points at a **content identifier (CID)** or URI, so the target lives in another document and is verifiable by hash.

The original research draft treats references purely as (1) and misses (2) entirely. And the two new sections are coupled: **you cannot content-address without a canonical serialisation** -- the "same data must yield the same CID" property (which, in practice, has *not* held across implementations) is exactly the determinism problem Section 10.1 solves. So a "Document Link" profile that carries CIDs depends on Canonical AXON. DASL + `ipld-core` (not the full IPLD stack) is the right, lean target for the Rust port if this profile is pursued.

---

## 11. Numeric System Modernisation

Numbers are where AXON 2026 must be stronger than both JSON and the PoC crate. JSON has one number grammar but no universal numeric semantics; many implementations parse everything as IEEE-754 doubles, losing precision. AXON's original decimals (`10D`, `1000.35D`, `-1.25E+6D`) exist precisely to avoid this.

**[GROUND TRUTH] Decimal spelling.** Runtime verification proves real pyaxon accepts `d` and `D`; an ordinary `$` suffix is rejected. `$` is accepted only for decimal specials. Canonical output uses `D`.

### 11.1 Required numeric categories

| Category | Example | Semantic requirement |
|---|---:|---|
| Signed integer | `-42` | Arbitrary precision or specified minimum range |
| Unsigned integer | `42` | Non-negative integer; no forced float |
| Decimal | `1000.35D` / `1000.35d` | Exact base-10, lossless; canonical `D` |
| Binary float | `1.5e-17` | IEEE-style approximate value |
| +Infinity | `∞` (or `∞D` for decimal-inf) | Domain-specific special |
| -Infinity | `-∞` | Domain-specific special |
| NaN / unknown | `?` (or `?D`/`?$` for decimal-nan) | Must be defined precisely |

**Note -- a Serde-ism to resist:** the earlier draft distinguished "unsigned `42`" from "signed `-42`" by the presence of a sign. That bakes Rust's `i64`/`u64` split into the *surface notation*, which contradicts the document's own "don't let Serde define AXON" principle. A notation should treat `42` as simply *integer* and let the schema/target type decide signedness.

### 11.2 Decimal design

Preserve sign, coefficient digits, scale/exponent, and (for round-tripping) whether exponent notation was used. `1000.350D` and `1000.35D` may compare equal *as decimal values* but differ in the CST. Canonical decimal output: no leading `+`; no unnecessary leading zeroes (except before the point); trailing fractional zeroes only if a scale-preservation profile requires; uppercase `D`; normalised exponent if exponent form is allowed.

### 11.3 Numeric separators

Adopt `_` between digits (`1_000_000`, `3.141_592D`, `1.602_176_634e-19`): only between digits; never adjacent to sign/point/exponent/suffix/boundaries; ignored in the semantic value; preserved in the CST; omitted in canonical output.

### 11.4 Special values

`∞`, `-∞`, `?` are float specials; with a `D`/`d`/`$` suffix they are the decimal-domain equivalents (**[GROUND TRUTH]**). Keep `∞`/`-∞` in the float (and decimal) profiles. Define `?` strictly as IEEE quiet NaN, **not** as generic unknown/missing (`null` already denotes absence). For canonical/equality/signing contexts, follow dCBOR: **canonicalise NaN to a single bit pattern** (`0x7e00`) rather than the earlier draft's ambiguous "output `?` but apps may forbid it" -- canonicalising the bytes keeps the encoding stable even though NaN != NaN.

---

## 12. Temporal System Modernisation

**[GROUND TRUTH, runtime-verified -- this section's original claim was wrong.]** Caret-prefixed temporals (`^12:00`) are the **0.9 canonical notation**: changelog 0.9 introduced the prefix "in order to be more explicit", the live dumper emits `^...` even for bare-loaded values, and the bare shapes (`12:00`, `2012-12-01` -- still visible in the repo's older example files) are the *deprecated pre-0.9* input, loadable in 0.9.x and scheduled by the author for removal in 0.10. The `^` in the earliest research draft and in `serde_axon` therefore matched the original after all; this document's "correction" of them is retracted. The 2026 baseline is caret temporals, with bare shapes as compatibility input.

### 12.1 Temporal categories

| Type | Canonical (0.9 / 2026) | Meaning |
|---|---|---|
| Date | `^2012-12-31` | Calendar date, no time |
| Time | `^12:30:34` | Local time, no date |
| Time+offset | `^12:35+03:00` | Time plus offset |
| Local datetime | `^2012-12-31T12:30` | Date+time, no offset |
| Offset datetime | `^2012-12-31T12:35+03:00` | Timestamp with explicit offset |
| UTC datetime | `^2012-12-31T09:35Z` | UTC timestamp -- **[2026]**: ground truth shows no `Z` branch in pyaxon's `get_tzinfo`; baseline offsets are signed numeric only |
| Duration | candidate | Requires separate design |

**On the prefix:** the earlier hand-wringing here about "adding" a caret is dissolved by the changelog -- the prefix *is* original, and the author's own migration recipe (`dumps(loads(text))`) converts old documents. AXON 2026 completes that migration: Core is caret-only; the bare shapes and their number-lexer commit live behind `compat.bare-temporals`, which also carries the `[1-2]` footgun byte-faithfully.

### 12.2 Strictness

Temporal parsing must be strict: `2026-02-30` -> error; `12:61:00` -> error; `24:00:00` -> decide explicitly (forbid or define).

### 12.3 Offset preservation (and a gap the draft missed)

Do not normalise offset datetimes to UTC in the semantic model (only in canonical output). `2026-07-08T12:00-06:00` carries different presentation/meaning than `2026-07-08T18:00Z` even at the same instant. Semantic model preserves date, time, offset, fractional precision, and zone marker.

**[2026] Named time zones -- a tradeoff the earlier draft did not consider.** The draft uses numeric offsets only. An offset loses the DST *rule*: for future-dated events (`2026-...` recurring appointments), a named IANA zone like `[America/Edmonton]` preserves the rule an offset cannot. The cost is a dependency on the IANA tz database and its update cadence. Recommendation: keep offsets in the core; consider named-zone datetimes as an optional temporal extension for future-dated/recurring use cases, clearly marked as carrying a tz-database dependency.

---

## 13. Strings and Text

**[GROUND TRUTH]** Real AXON has both `"..."` escaped strings **and** backtick `` `...` `` strings (a raw/alternate form), plus single-quoted **names** `'...'` that introduce a named node (not a string value).

### 13.1 Required string forms

1. Basic escaped: `"abc абв 中文"` -- **ground-truth caveat:** pyaxon's real escape set is only `\<delimiter>`; `\n`/`\r`/`\t`/`\u` are commented out in the lexer, any other `\x` passes the backslash through literally, and the backslash-newline "continuation" is **defective at runtime** (it duplicates the preceding chunk -- `"ab\
c"` -> `'abab\c'`). A real escape table is a **[2026]** definition (spec Section 7).
2. Multiline: **superseded** -- ground truth shows every baseline string (`"..."` and backtick) is already multiline with CR/CRLF->LF normalisation, so a triple-quoted form is redundant and is not adopted.
3. Backtick (**not** raw -- same lexer, alternate delimiter): `` `C:\Users\Name\file.txt` `` works today precisely because backslashes pass through; a true raw form is the [2026] `r"..."`/`r#"..."#` addition (spec Section 7).
4. Optional Rust-style raw `r"..."` / `r#"..."#` -- adopted as **[2026]**; the syntax space is free (a name followed by `"` is an error in baseline).

### 13.2 Avoid string-style sprawl

Do not copy YAML's many scalar modes. Keep a small set (escaped single-line, escaped multiline, backtick raw). Canonical output uses double-quoted escaped strings only (backtick is a human-input convenience), unless multiline canonical text is explicitly allowed.

### 13.3 Unicode

Require UTF-8 source; string values are Unicode scalar sequences; invalid UTF-8 is a parse error. Do **not** normalise string values (they may be binary-ish or user content). For identifiers, define allowed characters and consider NFC only for canonical output.

---

## 14. Identifiers, Names, and Keys

AXON's named nodes need a clear name grammar. **[GROUND TRUTH]** note that `'quoted'` names and `$name` are real constructs, so the grammar must reserve their meaning.

### 14.1 Proposed bare-name grammar

**Superseded by ground truth:** pyaxon's `get_name` uses Python's Unicode `isalpha`/`isalnum` classes plus `_`, so baseline names are **Unicode**, not ASCII. The spec (Section 9) therefore specifies names precisely via UAX #31 (`XID_Start`/`XID_Continue`  union  `_`) as a [2026] clarification of the lax baseline classes, with ASCII-only demoted to a portability lint. The ASCII-first grammar below is retained only as the historical proposal:

Conservative ASCII-first for interoperability:

```ebnf
name_start = ASCII_ALPHA | "_" ;
name_char  = ASCII_ALPHA | ASCII_DIGIT | "_" ;
name       = name_start , { name_char } ;
```

Keys needing other characters are quoted (`{"has space":1 "kebab-case":2}`) or single-quoted where a *name* is required.

### 14.2 Recommendation

Bare names conservative; quoted keys support all string keys; Unicode identifiers an optional profile, not required in the first modernised core; canonical output quotes keys that are not valid bare names.

---

## 15. Containers and Separators

AXON's optional-comma design is defining. Modern rule: whitespace separates adjacent values; comma may appear between values/pairs as optional trivia; comma never creates an empty element; consecutive commas are errors outside a permissive compatibility profile; canonical output never emits commas.

Valid: `[1 2 3]`, `[1, 2, 3]`, `[1, 2, 3,]`, `{a:1 b:2}`, `{a:1, b:2}`.
Invalid (strict): `[1,,2]`, `[,1]`, `{a:1,, b:2}`.

---

## 16. Maps, Ordered Maps, and Duplicate Keys

Original AXON distinguishes `{K:V ...}` (dict) from `[K:V ...]` (ordered dict). This must be specified precisely.

### 16.1 Proposed semantic model

| Form | Type | Ordering |
|---|---|---|
| `{K:V ...}` | map/dict | unordered by default |
| `[K:V ...]` | ordered map | order-preserving |
| `[V ...]` | list | order-preserving |
| `(V ...)` | tuple | order-preserving, arity by schema/type |
| `name{...}` | node | mixed named fields + ordered children |

### 16.2 Duplicate keys

Strict semantic parse: error. Lossless parse: preserve duplicate pairs in source order and report. Compatibility parse: documented last-wins/first-wins/multi-map. Canonical output: duplicates forbidden. This prevents silent data loss.

---

## 17. Nodes: The Heart of AXON

Nodes distinguish AXON from JSON/TOML/most config formats: typed objects (`Point{x:1 y:-2}`), enum variants (`Refunded{amount:19.99D reason:"damaged"}`), document structure (`tree{id:1 leaf{id:2 "AAA"}}`), positional records (`Rgb(255 128 0)`), and -- **[GROUND TRUTH]** -- nodes whose *name* is single-quoted for odd characters (`'weird name'{...}`).

### 17.1 Proposed node model

A node has: a name (bare or `'quoted'`); optional annotations (defer unless needed); zero+ named fields; zero+ positional children; an optional body form; CST trivia. Syntax: `Name` and `Name{field:value child child}` are **[BASELINE]**; `Name(value)` and `Name[value value]` are **[2026]** additions (motivated by Serde tuple-variant mapping; not in pyaxon).

### 17.2 Field/child ordering

The spec must say whether a node body is one ordered stream or split collections. Recommendation: the CST preserves exact mixed order; the semantic node exposes an ordered-entries stream plus named-field and positional-child views; duplicate-field rules follow Section 16; canonical output orders fields per canonical map rules, then children in semantic order (unless the node type requires preserving mixed order).

---

## 18. References, Anchors, and Graphs

**[GROUND TRUTH]** Confirmed authentic: `&label value` defines a labelled value; `*label` references it; a label may be an integer or a name. Example:

```axon
&1 { id:1 value:"A" }
&2 { id:2 value:"B" }
{ id:3 children:[*1 *2] }
```

`serde_axon` omits this because Serde cannot model shared/cyclic graphs -- a binding limitation, not a language one. References become a profile.

### 18.2 Proposed graph profiles

- **Tree Profile:** no anchors/references; safe default.
- **Graph Profile:** `&`/`*` intra-document anchors allowed (the real pyaxon semantics).
- **[2026] Document Link Profile:** `*` (or a distinct form) points at a **CID/URI** -- a cross-document, content-addressed link (see Section 10.5). Depends on Canonical AXON for the "same data -> same CID" property.

### 18.3 Safety rules

Graph parsing must: detect undefined references and duplicate anchors; define forward-reference behaviour, cycles, and a maximum anchor count; define whether anchors are semantic or serialization-only; and prevent anchor-expansion attacks (billion-laughs). Safe parsers may reject the graph profile unless explicitly enabled.

---

## 19. Binary Data

**[GROUND TRUTH, runtime-verified] Original AXON binary is *open-pipe* MIME Base64** -- opened by `|`, terminated by the `=` padding run, with **no closing pipe anywhere in 0.9** (grammar, prose docs, loader, and dumper agree). The `^x"..."` hex form in the earlier draft and in `serde_axon` is an invention. The open form carries a proven defect: payloads with `len % 3 == 0` (and empty bytes) produce no padding, so the 0.9 dumper emits literals its own loader rejects.

### 19.1 Binary literal forms

- **Legacy (existing):** `|SGVsbG8=` -- open-pipe, `=`-terminated. Loadable under the compatibility flag; unwritable for the defect payloads above.
- **Core [2026]:** `|SGVsbG8=|` -- a required closing pipe terminates the literal, padding becomes optional on input, and every payload is expressible; canonical output is closed and padded.
- **Optional [2026] add-on:** a hex form remains *deferred*; if ever admitted it is an extra spelling only -- "hex only" is rejected.

Canonicalisation: closed form, defined padding/case, no interior whitespace. Binary is bytes, not text.

---

## 20. Comments and Documentation

**[GROUND TRUTH]** `#` line comments are real. AXON 2026: keep `#` in core; add block comments only if easy to nest or explicitly non-nesting; do not add multiple comment syntaxes casually; comments are trivia in semantic parsing; the lossless CST preserves comments and attachment positions; canonical output drops comments.

---

## 21. Document Streams

pyaxon parses multiple top-level values from one input. Streams suit logs, event streams, append-only files, batch import/export, external-framed network data, and test fixtures.

```ebnf
stream = ws , [ value , { ws , value } ] , ws ;
```

A stream is *not* a list: two top-level `Event{...}` values are a stream of two, not one list of two. The spec should define whether inter-value comments are allowed, whether trailing garbage is an error, whether an empty stream is valid, and how partial input is reported. Streaming parsers should yield values incrementally with per-value memory limits.

---

## 22. Parser Security and Resource Limits

Hostile input is normal. `serde_axon` caps nesting at 128 (matching `serde_json`) to prevent stack exhaustion -- and this is not hypothetical: the same class of bug was the MEDIUM finding in the crate's audit. AXON 2026 should put limits in the spec (or a required implementation profile): max nesting depth, document size, string length, binary length, map entries, node children, anchor count, reference depth, numeric-literal length, temporal-literal length, and comment length.

Recommended defaults: nesting depth 128; numeric-literal length 1 KiB unless an arbitrary-precision parser needs more; anchor count disabled in the tree profile; total input size caller-configurable. The spec need not fix identical memory limits everywhere, but it must require limits to *exist* and be documented.

---

## 23. Error Handling

Define error *categories* rather than one generic failure: lexical, invalid escape, invalid Unicode, invalid number, invalid decimal, invalid temporal, unexpected token, unclosed container, duplicate key, unknown reference, duplicate anchor, resource-limit exceeded, unsupported-profile feature, trailing input, semantic-construction error. Each error carries byte offset, line, column, category, a short message, and optionally expected tokens and a recovery hint -- this matters for editors and tooling.

---

## 24. Semantic Model vs Concrete Syntax Model

Define two models explicitly.

**Semantic value model** (application parsers): `Null, Bool, Integer, Decimal, Float, String, Bytes, Date, Time, DateTime, List, Tuple, Map, OrderedMap, Node, Reference(graph profile)` -- discards comments and most formatting.

**Concrete syntax tree** (tools): tokens, whitespace, comments, original literal spelling (incl. `d`/`$` vs `D`, backtick vs `"..."`, `|base64|`), delimiters, optional commas, field order, mixed node-entry order, string-delimiter style, numeric-separator placement, and source spans -- enabling formatters, linters, migration tools, doc generators, editor plugins, and comment-preserving rewrites.

This CST/AST split is the single most important architectural recommendation; without it, AXON tools either lose comments or invent incompatible private ASTs.

---

## 25. Canonical AXON

See Section 10.1 for the dCBOR/JCS-modelled rules. Canonical AXON is an official serialisation profile for hashing, signing, reproducible builds, lockfiles, snapshot tests, semantic equality, and cache keys -- not necessarily the prettiest human form. It: uses UTF-8 and compact style; emits no comments, no optional commas, minimal delimiters, and minimal whitespace; emits `"..."` strings with minimal escaping; emits binary as canonical base64; `true`/`false`/`null`; decimals with `D`; sorts unordered map keys (JCS/dCBOR-style) while preserving list/tuple/ordered-map/child order; forbids duplicate keys; canonicalises `∞`/NaN; and forbids graph references unless canonical graph rules are defined. Map-key ordering is the hardest part -- do not rely on host-language map order.

---

## 26. Formatting Profile

Retain both styles. Compact: `Order{id:1024 paid:true customer:Customer{name:"Alex" tier:"gold"}}` -- ideal for generated output, logs, tests, small values, and the canonical profile. Formatted: the same with indentation and line breaks -- ideal for configuration, documentation, hand-edited files, and large objects. **Formatting must not change meaning** except where whitespace is needed to separate tokens; AXON is not indentation-sensitive in the YAML sense.

---

## 27. Schema and Validation

AXON needs a schema story, but schema is not core syntax. Users expect editor validation, completion, documentation, required/optional fields, type checking, version migration, API compatibility, and safe deserialisation -- as JSON Schema, CUE, Dhall, **CDDL**, and typed config systems all show.

Reserve **AXON Schema** as a companion layer (sketch: `schema Person { name: String  age: Int{min:0}  email: Optional[String] }`), designed properly before adoption. Core parser does not evaluate schemas; schema tools consume AXON values; schema may define canonicalisation requirements and version-migration hints. CDDL (CBOR's companion) and CUE are the models.

---

## 28. Compatibility Profiles

| Profile | Purpose | Features |
|---|---|---|
| Core Tree | Safe default interchange | No references, no schema, no construction |
| Human Config | Hand-written files | Comments, multiline/backtick strings, permissive commas |
| Lossless CST | Tools | Preserve trivia and source spans |
| Canonical | Deterministic output | For hashes/signatures/CIDs |
| Graph | Object graphs | `&`/`*` anchors |
| Document Link | Linked data | CID/URI references (depends on Canonical) |
| Columnar | LLM/token efficiency | Opt-in tabular blocks for uniform arrays |
| Stream | Logs/events | Multiple top-level values |
| Compatibility | Migration | Accept `d`/`$` decimals, backtick strings, legacy spellings |

Profiles let AXON grow without forcing every parser to implement everything.

---

## 29. Backwards Compatibility With Original AXON

**Preserve:** compact node syntax; list/tuple/dict syntax; decimal suffixes `d`/`D`; caret temporals with bare forms in compatibility mode; Base64 binary; backtick strings and quoted names; special values; comma-free containers; formatted style; and graph references.

**Clarify/constrain:** duplicate keys; ordered-dict semantics; mixed node field/child ordering; date/time grammar; reference semantics; comment forms; Unicode identifiers.

**Deprecate only with a migration path:** ambiguous old syntax is accepted in compatibility mode and rewritten to Core spelling (for example bare temporals gain `^`).

---

## 30. Recommendations Matrix (updated)

| Feature | Decision | Reason |
|---|---|---|
| Named nodes | Adopt/preserve | Core AXON identity |
| Compact + formatted styles | Adopt/preserve | Original goal; already works |
| Mandatory commas | Reject | Opposes AXON |
| Optional commas | Adopt | Editing compatibility, low cost |
| `#` comments | Adopt | Human need; real in pyaxon |
| Lossless decimals | Adopt | Original goal |
| **Decimal suffixes `d`/`D` (in), `D` (out)** | **Adopt [GROUND TRUTH]** | Runtime verification rejects an ordinary `$` suffix |
| Decimal-as-f64 | Reject | Loses data; defeats AXON's purpose |
| **Caret temporals `^...`** | **Adopt/preserve [GROUND TRUTH, runtime]** | 0.9 canonical (changelog + live dumper); the earlier bare-first ruling is retracted |
| Bare temporals | Compatibility input only (`compat.bare-temporals`) | Pre-0.9 legacy; the author's own 0.10 plan, completed |
| **Binary: open-pipe legacy, closed-pipe Core** | **Adopt [GROUND TRUTH, runtime]** | 0.9 has no closer and cannot round-trip `%3==0`/empty payloads; the 2026 closer fixes it |
| Hex binary / "hex only" | Reject "hex only"; hex stays deferred | Original is base64; hex-only breaks AXON |
| Numeric no-adjacency rule (Section 5.9 spec) | Adopt [2026] | `1__0`/`1abc` diagnose instead of silently lexing as int+name |
| Header enables required profiles (D-2) | Adopt | Consumed first value; error at the header, never downstream |
| Backtick strings / `'name'` | Adopt/preserve | Real pyaxon forms |
| Signed-vs-unsigned by sign | Reject | Serde-ism leaking into notation |
| Numeric separators `_` | Adopt | Proven readability |
| Raw/multiline strings | Adopt cautiously | Real config pain; keep set small |
| Many YAML string styles | Reject | Too complex |
| Implicit typing / indentation semantics | Reject | Unsafe/fragile |
| Anchors/references `&`/`*` | Adopt as graph profile | Original goal; real semantics |
| **[2026] CID/URI links** | Adopt as Document Link profile | Content-addressing went mainstream; depends on Canonical |
| Arbitrary object construction | Reject by default | Security risk |
| Schema in core | Reject | Keep notation simple |
| Companion schema (AXON Schema) | Adopt later | Needed for tooling; model on CDDL/CUE |
| **Canonical profile (dCBOR/JCS-modelled)** | Adopt | Deterministic requirement; standards exist |
| **[2026] Columnar form (TOON-style)** | Adopt as opt-in profile | Token efficiency for uniform arrays only |
| Lossless CST | Adopt | Required for tooling |
| Parser resource limits | Adopt | Security |
| Duplicate-key silent overwrite | Reject | Data-loss risk |
| Document streams | Adopt | Useful; prototyped |
| Serde as language model | Reject | Useful binding, insufficient semantics |
| Tag namespacing + well-known-tag registry (uuid/instant) | Adopt | EDN/MessagePack/CBOR convergence; rides on nodes |
| EDN `#_` discard form | Adopt | Lexical trivia; matches KDL slashdash |
| Interop mappings (decimal<->decimal128, instant<->MsgPack/CBOR ts, bytes<->BSON subtypes/MsgPack bin) | Adopt (Canonical) | Concrete cross-format compatibility |
| Set literal | **Baseline** (`{V ...}` / `∅` -- ground truth) | pyaxon has a native set type; EDN `#{...}` unnecessary |
| Binary subtype tag (BSON) | Optional | Or wrap as `UUID(\|...\|)` node |
| HJSON quoteless strings | Reject | Same ambiguity trap as YAML implicit typing |
| XSON `\u{...}` braced escapes | Adopt (Section 13 strings) | Swift/Rust precedent; kills surrogate pairs; pyaxon's own `\u` path is commented out |
| XSON document-declared extensions header | Candidate (Section 28 profiles) | Document states its required edition/profiles |
| XSON encoder collision self-check | Reject | AXON's grammar leaves no ambiguous shorthands to collide |
| Rationals `a/b` (XSON) | Reject | Decimals + BigInt cover the need; canonicalisation cost |

---

## 31. Draft AXON 2026 Value Model

```text
Value =
    Null
  | Bool(Boolean)
  | Int(BigInt | bounded Int with declared range)
  | UInt(BigUInt | bounded UInt with declared range)
  | Decimal(DecimalValue)          # from d / D; canonical D
  | Float(FloatValue)              # incl. ∞ / -∞ / NaN
  | String(UnicodeString)          # from "..." or backtick `...`
  | Bytes(ByteSequence)            # from |base64|
  | Date(DateValue)                # bare
  | Time(TimeValue)                # bare
  | DateTime(DateTimeValue)        # bare; offset or Z preserved
  | List(Vec<Value>)
  | Tuple(Vec<Value>)
  | Map(Vec<Pair>, unordered, duplicate policy enforced)
  | OrderedMap(Vec<Pair>)
  | Node(NodeValue)                # name may be bare or 'quoted'
  | Reference(ReferenceId)         # graph profile: &/* by label
  | Link(Cid | Uri)                # [2026] document-link profile
```

`NodeValue = { name, body: Unit | Tuple | List | Map | MixedEntries }`.
CST node: `{ kind, span, tokens, trivia_before, trivia_after, children, semantic_hint }`.

---

## 32. Draft Grammar Direction (illustrative, not final)

```ebnf
stream      = ws , [ value , { ws , value } ] , ws ;
value       = node | scalar | list | tuple | map | ordered_map
            | anchor | reference | binary ;

node        = ( name | quoted_name ) , [ node_body ] ;
node_body   = tuple | list | map ;

list        = "[" , ws , [ value , { sep , value } , [ sep ] ] , ws , "]" ;
tuple       = "(" , ws , [ value , { sep , value } , [ sep ] ] , ws , ")" ;
map         = "{" , ws , [ pair_or_child , { sep , pair_or_child } , [ sep ] ] , ws , "}" ;
ordered_map = "[" , ws , pair , { sep , pair } , [ sep ] , ws , "]" ;

anchor      = "&" , label , ws , value ;
reference   = "*" , label ;
binary      = "|" , base64_body , "|" ;

pair        = key , ws , ":" , ws , value ;
key         = name | string | quoted_name ;
sep         = ws , [ "," , ws ] ;
ws          = { whitespace | comment } ;
```

The `[V ...]` list vs `[K:V ...]` ordered-map ambiguity is real (disambiguated by whether the first element is followed by `:`) -- and **ground truth resolves the empty case**: pyaxon defines `[:]` as the empty ordered dict, so `[]` is unambiguously the empty list. Keep original syntax for compatibility and define ordered-map recognition precisely.

---

## 33. How This Affects the Rust Port Later

Not a Rust design doc, but implications: the Rust port should avoid a single overly-simple `Value` enum that loses AXON semantics. At minimum: `Value` (semantic), `Cst` (lossless), `DecimalValue` (exact), `TemporalValue` (date/time/datetime), `NodeValue`, `GraphValue` (optional), `ParseOptions` (limits/profiles), `SerializeOptions` (compact/formatted/canonical). Serde is *one layer on top*, not the definition. For Serde specifically: lossless decimals need newtype adapters; temporals need adapters/feature integrations; references cannot be represented generally; sets need custom handling; map-key restrictions must be explicit; enum/node mapping remains a strength. **[2026]** If the Document Link profile is pursued, target DASL + `ipld-core` (not full IPLD). **[2026 additions to the port requirements]:** the core crate is `no_std + alloc` (arbitrary-precision ints/decimals and strings need the heap; nothing needs an OS), with a `std` feature gating io adapters and the `tz-names` tzdb; the serde binding documents an explicit mapping for all four enum representations (externally tagged falls out of nodes naturally; internal/adjacent tagging get defined map-with-tag-key spellings; untagged is best-effort as everywhere); and a PyO3 wrap-back into Python is a sanctioned post-0.2 follow-on, not a roadmap item.

---

## 34. Final Research Conclusion

The original AXON language does not need replacing -- it needs to be completed, tightened, and modernised. The strongest path from 2018 to 2026:

1. Keep AXON's object-centred identity and nodes as first-class syntax.
2. Preserve compact and formatted styles.
3. Make decimals (`d`/`D`/`$` in, `D` out) and **bare** temporals genuinely lossless.
4. Keep binary as `|base64|`; treat hex as an optional add-on, not the core.
5. Clarify maps, ordered maps, node entries, and duplicates.
6. Add official parsing profiles.
7. Add Canonical AXON, modelled on dCBOR + JCS.
8. Add lossless CST support for tooling.
9. Treat graph references (`&`/`*`) as opt-in, optionally carrying content-addressed links.
10. Keep schema separate (model on CDDL/CUE).
11. Consider an opt-in columnar form for token efficiency, uniform arrays only.
12. Reject YAML-style ambiguity and Serde-driven surface semantics.
13. Do not let Serde limitations -- or config-language envy -- redefine the language.

AXON's original design already anticipated many modern JSON complaints. The job now is not to bolt on eight years of random features, but to absorb eight years of lessons: humans need comments and readable literals; machines need deterministic semantics (now with real standards to follow); security needs resource limits; tools need lossless syntax trees; applications need schemas; and serious data formats must preserve numbers, dates, bytes, and object identity -- and, increasingly, be content-addressable -- without pretending everything is a string.

---

## Bibliography and Research Sources

**Original language + ground truth**
1. `intellimath/pyaxon` -- source (`lib/axon/_loader.py`, `_dumper.py`) and `.axon` example files, cloned and inspected directly. **[GROUND TRUTH]**
2. AXON 0.8β documentation, pyaxon ReadTheDocs. <https://pyaxon.readthedocs.io/en/latest/>
3. pyaxon, PyPI. <https://pypi.org/project/pyaxon/>
4. `serde_axon` 0.1.0 -- proof-of-concept Rust crate (inspected locally).

**Comparative formats**
5. JSON5 specification. <https://spec.json5.org/>
6. TOML v1.0.0. <https://toml.io/en/v1.0.0>
7. YAML 1.2.2. <https://yaml.org/spec/1.2.2/>
8. KDL Document Language. <https://kdl.dev/spec/>
9. CUE Language Specification. <https://cuelang.org/docs/reference/spec/>
10. Pkl (Apple) -- configuration-as-code language. <https://pkl-lang.org/> / <https://github.com/apple/pkl>
11. Nickel <https://nickel-lang.org/>, Dhall <https://dhall-lang.org/>.
11a. EDN (Extensible Data Notation). <https://github.com/edn-format/edn>
11b. MessagePack -- spec incl. ext/timestamp types. <https://github.com/msgpack/msgpack/blob/master/spec.md>
11c. BSON -- binary subtypes and decimal128. <https://bsonspec.org/>
11d. HJSON. <https://hjson.github.io/>
11e. XSON (eXtensible Syntax Object Notation), Bryan Ford -- early draft. <https://bford.info/draft/xson/>

**[2026] Deterministic encoding, LLM efficiency, linked data**
12. RFC 8949 (CBOR). <https://www.rfc-editor.org/info/rfc8949/>
13. `draft-ietf-cbor-serialization` -- CBOR Serialization and Determinism (IETF, 2026).
14. `draft-mcnally-deterministic-cbor` (dCBOR, draft-17, 2026).
15. RFC 8785 -- JSON Canonicalization Scheme (JCS).
16. CDDL (RFC 8610) -- Concise Data Definition Language.
17. EverCBOR / PulseParse -- formally verified CBOR with a non-malleability proof (2025).
18. TOON (Token-Oriented Object Notation) -- spec, benchmarks, SDKs. <https://toonformat.dev/> / <https://github.com/toon-format/toon>
19. IPLD / DASL / content addressing -- IPFS Foundation (2025 review, 2026 preview); `ipld-core` (Rust).
20. W3C Verifiable Credentials Data Model; W3C Decentralized Identifiers (DIDs) v1.0.
