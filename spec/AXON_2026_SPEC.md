# AXON 2026 -- Language Specification

**Status:** revision 5 final specification -- semantic surface frozen on 2026-07-11. Corrections that do not change denoted values, canonical bytes, profiles, or error categories may be published as errata; semantic changes require a future AXON edition. Empirical baseline corrections take precedence over older source-derived wording.
**Relationship to prior art:** AXON 2026 is an **edition** of AXON (eXtended Object Notation) that extends the language as actually implemented by `intellimath/pyaxon`. Every construct in this document is either (a) present in real pyaxon, or (b) a clearly-marked 2026 addition. Nothing here silently redefines an existing AXON construct.
**Companion documents:** `AXON_2018_to_2026_Language_Evolution_MERGED.md` contains rationale and research. This specification and `conformance/normative_vectors.json` are the normative language artefacts; the latter assigns stable requirement and vector identifiers for implementation-independent testing.

---

## Normative Conventions

The key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** are used per their ordinary RFC 2119 sense. Grammar is given in EBNF. Two provenance tags appear throughout:

- **[BASELINE]** -- the construct exists in real pyaxon and is preserved unchanged.
- **[2026]** -- a new addition introduced by this edition, gated behind a profile unless stated otherwise.

Where a construct is preserved but its behaviour is being *specified more precisely* than pyaxon documents it, the text says **[BASELINE, clarified]**.

---

## Contents (order frozen at completion)

1. Lexical Foundation  /  2. Values and the Semantic Value Model  /  3. Containers  /  4. Nodes  /  5. Numbers  /  6. Temporals  /  7. Strings and Text  /  8. Binary  /  9. Identifiers, Names, Keys, and Constants  /  10. References, Graphs, and Links  /  11. Documents and Streams  /  12. Duplicates and Ordering  /  13. The Concrete Syntax Tree  /  14. Canonical AXON  /  15. Conformance Profiles  /  16. Parser Resource Limits  /  17. Errors  /  18. AXON Schema (Companion)  /  19. Backwards Compatibility and Migration  /  Appendix A -- Collected Formal Grammar  /  Appendix B -- Normative Conformance Registry  /  Appendix C -- Internationalisation (informative).

The per-section grammar fragments are authoritative alongside Appendix A. Appendix B defines the machine-readable conformance registry.

---

## 1. Lexical Foundation

This section defines how AXON 2026 source text is decomposed into tokens, and specifies every atomic literal. Container and node *structure* is deferred to Sections 3-4; this section covers only the lexical layer.

### 1.1 Source text and character set

**[BASELINE, clarified]** AXON 2026 source text **MUST** be a sequence of Unicode scalar values encoded as UTF-8. A parser **MUST** reject input that is not well-formed UTF-8 with an `invalid-unicode` lexical error (Section 17). String values and names are sequences of Unicode scalar values. Neither the lexical layer nor Canonical AXON applies Unicode normalisation; visually identical but differently composed sequences remain different semantic values (Sections 7, 9, and 14).

### 1.2 Whitespace and value separation

**[BASELINE]** The space (U+0020), horizontal tab (U+0009), carriage return (U+000D), and line feed (U+000A) are whitespace. Whitespace separates adjacent tokens where a separation is otherwise ambiguous and is **not** otherwise semantically significant. AXON 2026 **MUST NOT** be indentation-sensitive: indentation and line breaks never change the denoted value except by separating tokens.

```ebnf
ws        = { ws_char | comment } ;
ws_char   = " " | "\t" | "\r" | "\n" ;
```

### 1.3 Separators (comma is optional)

**[BASELINE]** Inside containers, items are separated by whitespace. A comma **MAY** appear between items or key/value pairs as optional separator trivia. A comma **MUST NOT** produce an empty element. In the Core Tree profile, two or more consecutive commas, a leading comma, or a comma with no following item (other than a single permitted trailing comma) are `unexpected-token` errors; a Compatibility profile **MAY** relax this but **MUST** document the relaxation. Canonical AXON (Section 14) **MUST NOT** emit commas.

```ebnf
sep       = ws , [ "," , ws ] ;
```

### 1.4 Comments

**[BASELINE]** A comment begins with `#` and extends to the end of the line. Comments are lexical trivia: they **MUST NOT** affect the denoted value. A Lossless-CST parser (Section 13) **MUST** preserve comment text and attachment position; a semantic parser **MUST** discard them; Canonical AXON **MUST NOT** emit them.

```ebnf
comment   = "#" , { any_char_except_newline } ;
```

A block-comment form is **not part of AXON 2026**. A future AXON edition may add one only if nesting behaviour is completely defined.

### 1.5 Token categories

The lexer dispatches on the first non-whitespace character of an atom:

| Leading character(s) | Token | Section |
|---|---|---|
| `^` | **temporal** (`^2012-12-31`, `^12:30`) -- the 0.9 canonical opener | 1.8, 6 |
| digit `0`-`9`, `-` | number -> integer, float, **decimal**; deprecated **bare temporal** shapes only under `compat.bare-temporals` | 1.6, 5.1 |
| `∞`, `-∞`, `?` | float special (or decimal special with `d`/`D`/`$` suffix) | 1.6.4 |
| `"` | escaped string | 1.9 |
| `` ` `` | backtick string | 1.9 |
| `'` | single-quoted **name** (introduces a named node) | 1.10 |
| `\|` | base64 binary literal | 1.11 |
| `&` | anchor definition (`&label value`) | 1.12 |
| `*` | reference (`*label`) | 1.12 |
| `∅` | empty-set literal **[BASELINE]** | 2.6 |
| `$` | constant reference (`$name`) -- resolves a registered constant | 9 |
| bare name-start | bare name / node name / bareword key | 1.10 |
| `{` `[` `(` | container open (see Sections 3-4) | -- |

> `$` introduces a registered constant (`$Inf`). It is not an ordinary decimal suffix. Historical `$` decimal spellings in earlier drafts are retracted; `$` remains accepted only on numeric specials such as `?$`.

### 1.6 Numeric tokens (lexical form)

Full numeric *semantics* are in Section 5; this defines the lexical shape.

```ebnf
digit        = "0".."9" ;
digits       = digit , { digit | "_" } ;        (* "_" is a [2026] separator -- Section 5.6 *)
neg          = "-" ;                             (* a leading "+" is not AXON -- Section 5.2 *)
exp_sign     = "+" | "-" ;                       (* signs in exponents are [BASELINE] *)
integer      = [ neg ] , digits ;
frac         = "." , digits ;
exp          = ( "e" | "E" ) , [ exp_sign ] , digits ;
float        = [ neg ] , digits , ( frac , [ exp ] | exp ) ;
```

#### 1.6.1 Integers
**[BASELINE, clarified]** An integer literal is an optional **minus** followed by decimal digits; a leading `+` is not AXON (ground truth: pyaxon's atom dispatch has no `+` branch). AXON 2026 **MUST NOT** encode signedness in the surface syntax: `42` is simply an integer; a target type or schema decides whether it is stored as signed or unsigned (Section 5).

#### 1.6.2 Floats
**[BASELINE]** A float literal has a fractional part, an exponent, or both. Leading-point (`.5`) and trailing-point (`5.`) forms are `invalid-number` errors in the Core Tree profile and **MUST NOT** be emitted by Canonical AXON. Provenance: pyaxon's lexer **tolerates** the trailing-point form (`5.` -- the post-point digit loop is zero-or-more), so Core's rejection of it is a **[2026]** restriction and the Compatibility profile **MAY** accept it; the leading-point form has no baseline support (`.` never dispatches to the number lexer) and is rejected in all profiles.

#### 1.6.3 Decimals
**[BASELINE, runtime-verified]** A decimal literal is a numeric form immediately followed by `d` or `D` -- **and only those two**. (An earlier draft of this spec listed `$` as a third synonym; live 0.9 falsifies that: `1230$` is an *error* -- `$` belongs to constants (Section 9.4) and to the decimal **specials** only, Section 1.6.4.)

```ebnf
dec_suffix   = "d" | "D" ;
decimal      = ( integer | float ) , dec_suffix ;
```

Examples: `10D`, `1000.35D`, `-1.25E+6D`, `1230d`. A conforming parser **MUST** parse decimals into an exact base-10 value (Section 5); mapping a decimal to a binary float is non-conforming. **Canonical output uses `D`**; `d` is accepted on input only.

#### 1.6.4 Numeric specials
**[BASELINE, runtime-verified]** `∞` (U+221E) and `-∞` denote floating-point positive/negative infinity; `?` denotes a floating-point quiet NaN. Each **MAY** be immediately followed by a decimal-special suffix to denote the decimal-domain equivalent -- and here (only here) `$` is valid alongside `d`/`D`:

```ebnf
special          = ( "∞" | "-∞" | "?" ) , [ special_suffix ] ;
special_suffix   = "d" | "D" | "$" ;
```

Runtime: `?$` -> decimal NaN, `∞$` -> decimal Infinity, `?D` -> decimal NaN. Section 5 fixes their precise meaning and canonical treatment (canonical specials use `D`).

### 1.7 `$` is not a numeric suffix

**[GROUND TRUTH, runtime-verified]** An earlier draft treated `1230$` as a decimal. Live 0.9 rejects it: after digits, `$` is never consumed as a suffix -- atom-leading `$` begins a constant reference (Section 9.4), and `$` appears numerically only inside the specials of Section 1.6.4. Applications that need currency **MUST** model it explicitly (e.g., `Money{amount:1230D currency:"CAD"}`).

### 1.8 Temporal tokens (lexical form)

**[BASELINE, runtime-verified -- inverting this spec's earlier ruling]** The canonical original temporal form is **caret-prefixed**: `^2012-12-31`, `^12:30`, `^2012-12-31T12:35+03`. Changelog 0.9 introduced the `^` prefix; **bare** temporals are the *deprecated pre-0.9* notation, which 0.9.x still loads (via the number lexer's `-`/`:` backtrack) and which the author scheduled for removal in 0.10. Live confirmation: `loads` accepts both spellings; `dumps` emits `^...`. AXON 2026 completes the author's migration: **Core accepts caret temporals only**; bare temporals are accepted under `compat.bare-temporals` (Section 15.3) with `dumps(loads(text))` -- the author's own recipe -- as the migration. Section 6 is the authority on grammar and strictness; Section 5.1 covers what the caret does to the number lexer.

```ebnf
temporal     = "^" , ( datetime | date | time ) ;     (* core -- shapes and strictness: Section 6.2 *)
```

Bare (unprefixed) shapes are recognised only under `compat.bare-temporals`, exactly as 0.9.x recognised them. The `Z` offset suffix remains **[2026]** -- absent from `get_tzinfo` in the baseline regardless of prefix.

### 1.9 String tokens

**[BASELINE]** Two string delimiters exist:

- `"..."` -- an **escaped** string; supports escape sequences (Section 7) and **MAY** span lines. A triple-quoted `"""..."""` multiline form is defined in Section 7.
- `` `...` `` -- a **backtick** string; an alternate (raw-style) form suited to regexes, paths, and shell/SQL fragments.

```ebnf
string       = escaped_string | backtick_string ;
escaped_string   = '"' , { escaped_char } , '"' ;
backtick_string  = "`" , { backtick_char } , "`" ;
```

A string is a **value**. Escape processing, the multiline form, and canonicalisation (Canonical AXON emits `"..."` only) are specified in Section 7.

### 1.10 Names

**[BASELINE, clarified]** A name may appear (a) as a **node name** preceding a body, or (b) as a **bareword key**. Ground truth: pyaxon's `get_name` accepts **Unicode** -- any character in Python's `isalpha` class (plus `_`) starts a name, and `isalnum` (plus `_`) continues it -- so baseline names were never ASCII-only. AXON 2026 specifies these lax classes precisely via UAX #31:

```ebnf
name         = name_start , { name_char } ;
name_start   = XID_Start | "_" ;                      (* [2026] precise form of the baseline classes *)
name_char    = XID_Continue | "_" ;
quoted_name  = "'" , { quoted_name_char } , "'" ;     (* single-quoted, for characters bare names exclude *)
```

A `quoted_name` introduces a **name** (not a string value). A `$`-prefixed atom is a **constant reference**, not a name form -- Section 9.4. (A dotted-name form exists only as commented-out baseline code and is not AXON.) ASCII-only naming is a portability *lint*, not grammar. Names are compared and emitted by exact scalar sequence; a lint may recommend NFC, but canonical output never changes name identity (Sections 9, 14). Key domains differ by container -- the authority is the consolidated table in Section 9.6.

### 1.11 Binary literals

**[BASELINE + 2026 correction]** Historical AXON uses an opening pipe followed by MIME Base64 and has no closing delimiter. Runtime verification found that its dumper can emit unpadded Base64 which its loader cannot terminate, breaking round trips. AXON 2026 therefore adds a required closing pipe:

```ebnf
binary_2026  = "|" , { base64_char | ws_char } , "|" ;
binary_legacy = "|" , { base64_char | ws_char } ;  (* compatibility reader only *)
base64_char  = ascii_alpha | digit | "+" | "/" | "=" ;
```

Interior whitespace **MAY** appear between base64 characters and is ignored. The literal denotes the decoded byte sequence -- a **bytes** value, distinct from text. Example: `|SGVsbG8=|` denotes the five bytes of `Hello`. The closing pipe is **[2026]**, not baseline punctuation.

**[2026]** A hexadecimal binary form **MAY** be added later as an *additional* spelling for short, human-inspectable values. It is **not** in this baseline. "Hex only" is explicitly rejected: the base64 form **MUST** always be accepted, because it is the real AXON binary literal.

### 1.12 Anchors and references (lexical form)

**[BASELINE]** Object identity uses labelled anchors and references:

```ebnf
label        = integer | name ;
anchor       = "&" , label , ws , value ;   (* defines: binds label -> value *)
reference    = "*" , label ;                (* uses: resolves to the bound value *)
```

`&1 { ... }` binds label `1` to the following value; `*1` resolves to it. A label may be an integer or a name. Anchors/references belong to the **Graph** profile (Section 10); the Core Tree profile **MUST** reject them unless the Graph profile is enabled. Undefined references, duplicate anchors, cycles, forward references, anchor-count limits, and anchor-expansion protection are specified in Section 10.

### 1.13 Lexical summary

An AXON 2026 token stream is a sequence of: whitespace/comment trivia; scalar atoms (integer, float, decimal, numeric special, temporal, string, binary, `null`/`true`/`false` barewords -- Section 2); names (bare, quoted, `$`-prefixed); anchor/reference markers (`&`/`*` with labels); and container/node delimiters (`{` `}` `[` `]` `(` `)`, Sections 3-4). The closed binary terminator and other explicitly tagged additions are [2026]; block comments and hexadecimal binary literals are excluded from this edition.

---

## 2. Values and the Semantic Value Model

Section 1 defined how source text becomes tokens. This section defines *what those tokens denote* -- the **value domain** of AXON 2026 -- and the **semantic value model**, the abstract shape an application parser produces. Lexical and syntactic detail for each value kind lives in its own later section; this section fixes the domain, the profile boundaries, value equality, and the well-known-tag mechanism.

### 2.1 Two models: semantic vs concrete

**[BASELINE, clarified]** A conforming AXON 2026 processor distinguishes two representations of the same document:

- The **semantic value model** (this section): the abstract value an application consumes. It discards comments, whitespace, optional commas, original literal spelling, and delimiter style.
- The **concrete syntax tree** (Section 13): a lossless representation retaining all of the above, for formatters, linters, and comment-preserving tools.

A processor **MUST NOT** require the CST in order to produce a semantic value. Content that is presentation-only (Section 2.9) **MUST NOT** appear in the semantic model.

### 2.2 The value domain

**[BASELINE]** unless tagged. Every AXON 2026 document denotes a value drawn from this domain:

| Value kind | Origin | Profile | Section |
|---|---|---|---|
| `Null` | `null` | core | 2.3 |
| `Bool` | `true` / `false` | core | 2.3 |
| `Int` | integer literal | core | 5 |
| `Float` | float literal, `∞`/`-∞`/`?` | core | 5 |
| `Decimal` | `...d` / `...D`, decimal specials | core | 5 |
| `String` | `"..."` / `` `...` `` | core | 7 |
| `Bytes` | `\|base64\|` | core | 8 |
| `Date` | `^` date literal | core | 6 |
| `Time` | `^` time literal | core | 6 |
| `DateTime` | `^` datetime literal | core | 6 |
| `List` | `[V ...]` | core | 3 |
| `Tuple` | `(V ...)` | core | 3 |
| `Map` | `{K:V ...}` (unordered) | core | 3, 12 |
| `OrderedMap` | `[K:V ...]` (ordered) | core | 3, 12 |
| `Node` | `Name{...}` / unit `Name` **[BASELINE]**; `Name(...)` / `Name[...]` **[2026]** | core | 4 |
| `Reference` | `&label` / `*label` | **Graph** | 10 |
| `Set` | `∅` / `{V ...}` **[BASELINE]** | core | 2.6 |
| `Link` **[2026]** | CID or URI | **Document Link** | 10 |

Core-profile processors **MUST** support every value kind marked `core`. Kinds marked with a profile name **MUST** be rejected (with an `unsupported-profile-feature` error, Section 17) unless that profile is enabled.

### 2.3 Scalar constants: `null`, `true`, `false`

**[BASELINE]** The barewords `null`, `true`, and `false` denote `Null` and the two `Bool` values. They are reserved: a bare name equal to `null`, `true`, or `false` **MUST** be interpreted as the corresponding constant, never as a node name or bareword key. To use one of these as a *key*, it **MUST** be quoted (`"null"`). Numeric, string, binary, and temporal scalars are specified in Sections 5-8; nothing in those sections adds a value kind beyond those listed in 2.2.

### 2.4 Composite values

**[BASELINE]** `List`, `Tuple`, `Map`, `OrderedMap`, and `Node` are the composite kinds (Sections 3-4). Ordering is normative: `List`, `Tuple`, `OrderedMap`, and a `Node`'s positional children preserve order; a `Map` makes **no** semantic ordering guarantee unless a profile states otherwise (Section 12). Duplicate-key policy is per-profile (Section 12). A `Node` carries a name (bare or quoted, Section 1.10) and a body that is unit, positional, or keyed (Section 4).

### 2.5 Reference values (Graph profile)

**[BASELINE]** `&label value` binds a label to a value; `*label` denotes that same value (shared identity, potentially cyclic). In the semantic model a `Reference` **MUST** resolve to the identical value object bound by its anchor, so that shared and cyclic structure is representable -- a property Serde-style tree models cannot express (Section 10, and see Section 33 of the research doc). Outside the Graph profile, `&`/`*` are errors (Section 1.12).

### 2.6 Set values

**[BASELINE]** -- a correction of every prior draft, including this project's own research: pyaxon has a **native set type**. `∅` (U+2205) is the empty set, and a brace container whose first element is a bare value (not a pair) is a set: `{1 2 3}`. Full container rules are in Section 3; the EDN-style `#{...}` spelling proposed earlier is withdrawn as redundant. A `Set` is an unordered collection of distinct values; distinctness uses value equality (2.9). A processor **MUST NOT** silently coerce a `Set` to a `List` (the `serde_axon` proof-of-concept flattened sets to sequences -- a binding limitation, not the language). Duplicate elements: baseline deduplicates silently; Core policy is fixed in Section 12. Canonical element ordering is fixed in Section 14.

### 2.7 Well-known tags and the tag registry [2026]

**[2026]** AXON needs no separate "tagged literal" syntax because a **`Node` already is a tag**: `Name{...}` attaches the type `Name` to a body. To make this interoperable -- the lesson converged on by EDN tagged literals, MessagePack ext, BSON subtypes, and CBOR tags -- AXON 2026 defines two rules:

1. **Namespacing.** A node name **MAY** be namespaced as `ns/name` (e.g., `geo/point{lat:53.5 lon:-113.5}`) so independent vocabularies compose without collision. The separator is `/`, following EDN -- **not** `:`, because a name followed by `:` is pair/key syntax in AXON (ground truth: pyaxon's `get_named` errors on a colon at value position; see Section 4.7). A processor that does not recognise a namespace **MUST** still parse the node structurally.
2. **A registry of well-known tags.** This edition reserves an initial set whose meaning is fixed so that independent processors -- and mappings to other formats -- agree:

   | Well-known node | Body | Meaning |
   |---|---|---|
   | `uuid(...)` or `uuid{...}` | a string or `\|base64\|` | an RFC 4122 UUID |
   | `instant(...)` | a UTC datetime or epoch value | an absolute point in time (maps to MessagePack/CBOR timestamp) |
   | `duration(...)` | an ISO-8601 duration string, e.g., `duration("P1DT2H")` | a time span -- there is deliberately **no bare duration literal**, because `P1DT2H` lexes as a *name* (Section 1.10) and would collide with unit nodes (Section 6.9) |
   | `cid(...)` | a CID string | a content-addressed `Link` under the Document Link profile (Section 10.7); an ordinary node otherwise -- graceful degradation |
   | `link(...)` | a URI string | a URI `Link` under the Document Link profile (Section 10.7); an ordinary node otherwise |
   | `axon{...}` | edition/profile fields | the optional document header (Section 11.7) when it is the first value of a stream; an ordinary node anywhere else |
   | `grid{...}` | `{cols:[...strings...] rows:[...values...]}` with `len(rows) == 0 (mod len(cols))` | arity-framed tabular records, row-major -- the token-efficiency answer to TOON-style columnar data (Section 15.4) with **zero new syntax**: consumers that recognise the tag read a sequence of records; everything else sees an ordinary node |
   | `grid{...}` | `grid{cols:[sku qty price] rows:["A-1" 2 19.99D "B-7" 1 960.37D]}` | a columnar table: `cols` names the columns (unit nodes or strings); `rows` is a **flat** list whose length is a multiple of the column count, framed by arity -- never by newlines. Tools MAY expand it to a list of maps. This tag *is* the token-efficiency answer (Section 15.4) |

   Additional well-known tags **MAY** be registered by future editions. A processor **MUST NOT** assign application-specific meaning to a registered well-known tag, and **SHOULD** surface unknown tags to the application rather than discarding them.

Well-known tags are the anchor for the interop mappings in Section 14 (AXON `Decimal` <-> decimal128; `instant` <-> MessagePack/CBOR timestamp ext; `Bytes` <-> BSON binary subtypes / MessagePack `bin`).

### 2.8 Link values [2026]

**[2026]** A `Link` denotes a **cross-document, content-addressed reference** -- a content identifier (CID) or URI -- as distinct from the intra-document `Reference` of Section 2.5. It exists only in the Document Link profile and **depends on Canonical AXON** (Section 14): a content address is only meaningful if the referent has a deterministic serialisation. The concrete surface syntax for a `Link` is specified in Section 10; this section only fixes that it is a distinct value kind, not a subtype of `Reference` or `String`.

### 2.9 Value equality and what is *not* a value

**[BASELINE, clarified]** Two semantic values are equal when they are of the same kind and:

- `Null`, `Bool`: trivially by value.
- `Int`, `Float`, `Decimal`: by **numeric value**, within their kind. `1000.350D` and `1000.35D` compare equal as decimal values while remaining distinct in the CST. `Float` NaN follows IEEE semantics; Canonical AXON uses one NaN encoding.
- `String`, `Bytes`: by exact scalar/byte sequence.
- `Date`, `Time`, `DateTime`: by the fields they preserve, including offset/zone (an offset datetime and a `Z` datetime at the same instant are **not** the same value unless normalised -- Section 6).
- `List`, `Tuple`, `OrderedMap`, `Node` children: element-wise and order-sensitive.
- `Map`, `Set`: as unordered collections.

The following are **presentation trivia** and **MUST NOT** affect a value or its equality: comments; whitespace and line breaks; optional commas; the choice of decimal suffix (`d`/`D`); the choice of string delimiter (`"..."` vs backtick); numeric separators (`_`); interior whitespace inside a `|base64|` literal; and compact-vs-formatted style. All are preserved in the CST.

### 2.10 Reference semantic-value shape (Rust-facing)

Informative -- the semantic value model maps to a shape of roughly this form (normative field names may be refined in Section 33 alignment):

```text
Value =
    Null
  | Bool(bool)
  | Int(IntValue)                  # arbitrary-precision or bounded (Section 5)
  | Float(FloatValue)              # incl. +/-∞, NaN
  | Decimal(DecimalValue)          # exact base-10 (Section 5)
  | Str(String)                    # from "..." or `...`
  | Bytes(Vec<u8>)                 # from |base64|
  | Date(DateValue)                # bare
  | Time(TimeValue)                # bare; offset optional
  | DateTime(DateTimeValue)        # bare; offset or Z preserved
  | List(Vec<Value>)
  | Tuple(Vec<Value>)
  | Map(Vec<(Value, Value)>)       # unordered semantics; dup policy per profile
  | OrderedMap(Vec<(Value, Value)>)
  | Node(NodeValue)                # name may be bare or quoted; namespaced ns:name
  | Reference(ReferenceId)         # Graph profile
  | Set(Vec<Value>)                 # [BASELINE] core; unordered, distinct (∅ / {V ...})
  | Link(LinkValue)                # [2026] Document Link profile; CID | URI
```

`Reference` is a resolved-identity handle, not a copy (Section 2.5). `Set` and `Link` are absent from a Core-profile processor's `Value`. This shape is the semantic model only; the lossless `Cst` node type is defined in Section 13.

---

## 3. Containers

This section specifies the five plain container forms -- list, tuple, map (dict), ordered map (ordered dict), and **set** -- their grammar, their disambiguation rules, and separator behaviour. Nodes (`Name{...}` etc.) are containers too, but their body rules differ enough to warrant their own section (Section 4). Duplicate-key *policy* is per-profile and lives in Section 12; this section defines only which shapes are well-formed.

### 3.1 Container kinds

**[BASELINE]**

| Syntax | Value kind | Ordering | Elements |
|---|---|---|---|
| `[V ...]` | `List` | order-preserving | values |
| `(V ...)` | `Tuple` | order-preserving | values |
| `{K:V ...}` | `Map` | unordered (semantic) | key:value pairs only |
| `[K:V ...]` | `OrderedMap` | order-preserving | key:value pairs only |
| `{V ...}` | `Set` | unordered | values; distinct |

All five are **[BASELINE]** pyaxon forms (the set forms were recovered by deep loader inspection -- `get_dict_value` builds a Python `set` when the first element is not a pair, and `∅` is the empty set). Elements are heterogeneous at the notation level; a schema (Section 18) may constrain them.

### 3.2 Grammar

```ebnf
list         = "[" , ws , [ value , { sep , value } , [ sep ] ] , ws , "]" ;
tuple        = "(" , ws , [ value , { sep , value } , [ sep ] ] , ws , ")" ;
map          = "{" , ws , [ pair  , { sep , pair  } , [ sep ] ] , ws , "}" ;
ordered_map  = "[" , ws , ( ":" | pair , { sep , pair } , [ sep ] ) , ws , "]" ;
set          = "∅"
             | "{" , ws , value , { sep , value } , [ sep ] , ws , "}" ;

pair         = map_key , ws , ":" , ws , value ;
map_key      = name | string ;        (* 'quoted' names are NOT map keys -- node attrs only; Section 9.6 *)
sep          = ws , [ "," , ws ] ;
```

The empty forms are exact and **[BASELINE]**: `[]` empty list, `[:]` empty ordered map, `{}` empty map, `∅` empty set. (`{:}` does not exist.)

### 3.3 Lists

**[BASELINE]** A list is zero or more values in `[...]`, order-preserving. `[1 3.14 3.25D ∞ -∞ ?]` is a six-element list. Elements are separated per Section 1.3 (whitespace; comma optional). A pair (`k:v`) appearing in a list that has already been recognised as a *list* (3.6.1) is an `unexpected-token` error -- pairs and bare values **MUST NOT** be mixed inside `[...]`.

### 3.4 Tuples

**[BASELINE, clarified]** A tuple is zero or more values in `(...)`, order-preserving. Structurally a tuple behaves like a list; the distinction is **kind**: a `Tuple` connotes a fixed-arity positional record (arity checked by schema or target type, not by the parser), while a `List` connotes a sequence. A conforming parser **MUST** preserve the distinction (they are different value kinds, Section 2.2) and **MUST NOT** silently coerce one to the other. Pairs inside `(...)` are an `unexpected-token` error in this baseline.

### 3.5 Maps and sets -- the `{...}` disambiguation

**[BASELINE, clarified]** Braces are shared by maps and sets, disambiguated exactly like `[...]` (3.6.1): after `{` and leading trivia, if the first complete atom is a legal `map_key` followed by `:`, the container is a `Map` and every element **MUST** be a pair; otherwise it is a `Set` and every element **MUST** be a bare value. Mixing after recognition is an `unexpected-token` error. `{}` is the empty `Map`; the empty `Set` is `∅` (there is no `{:}`). `{alpha:1 beta:2}` is a map; `{1 2 3}` is a set; `{12:30 14:00}` is a **set of times** (the temporal-atom rule of 3.6.1 applies identically). Mixed named-plus-positional content is a *node body* feature (Section 4), not a brace feature. Map keys are per 3.7; semantic `Map` ordering per Section 2.4; duplicates per Section 12.

### 3.6 Ordered maps and the `[...]` disambiguation

**[BASELINE, clarified]** `[K:V ...]` is an ordered map -- order-preserving pairs. Because lists and ordered maps share the `[...]` delimiters, recognition must be precise.

#### 3.6.1 Recognition rule

After consuming `[` and leading trivia, the parser lexes the **first complete atom** (maximal munch, Section 1). If that atom is a legal key (bare name, quoted name, or string) **and** the next non-trivia token is `:`, the container is an `OrderedMap` and every subsequent element **MUST** be a pair. Otherwise it is a `List` and every subsequent element **MUST** be a bare value. Mixing after recognition is an `unexpected-token` error.

Two consequences worth stating explicitly:

- **Temporal literals never trigger ordered-map recognition.** `[12:30 14:00]` is a two-element `List` of times, *not* an ordered map with key `12`, because the lexer's maximal munch produces the atom `12:30` (a time) before the recogniser ever sees a `:`. The colon inside a temporal literal is part of the atom, not a pair separator.
- **Only key-legal atoms can open an ordered map.** `[*1: x]` is an error, not an ordered map: a reference is not a key (3.7).

#### 3.6.2 The empty case -- resolved by the baseline

`[]` denotes the empty `List`; **`[:]` denotes the empty `OrderedMap`** -- both **[BASELINE]** (ground truth: `get_list_value` returns an empty odict for `[:]`). The ambiguity the research draft worried over never existed; the recognition rule composes cleanly with these literals (no first pair -> list, and the explicit `[:]` covers the pairless ordered map).

#### 3.6.3 Streaming note

Recognition requires lookahead of exactly one atom plus one token past it. Streaming parsers **MUST** buffer at most that much before deciding the container kind; this bound is intentional and is why more permissive mixing is rejected.

### 3.7 Keys

**[BASELINE, clarified]** A map/ordered-map key is a bare `name` or a `"..."` string -- ground truth: the pair readers (`get_keyval_dict` and kin) accept exactly those two forms, so **`'quoted'` names are not map keys** (they are node-attribute keys only, Section 4.4.2). Keys are not general values: numbers, temporals, binary, containers, nodes, references, and the constants `null`/`true`/`false` (Section 2.3) are not keys. In the semantic model a bare-name key and a string key with identical text denote the **same key** -- spelling is CST trivia (Section 13); Canonical AXON picks one spelling per key (Section 14). The consolidated key-domain table is Section 9.6.

### 3.8 Separators inside containers

**[BASELINE]** Restating Section 1.3 normatively for containers: whitespace separates elements; a single comma **MAY** appear between elements and a single trailing comma **MAY** precede the closing delimiter; commas never create empty elements; leading commas and consecutive commas are errors in the Core Tree profile. Canonical AXON emits no commas. Valid: `[1 2 3]`, `[1, 2, 3]`, `[1, 2, 3,]`, `{a:1 b:2}`, `{a:1, b:2,}`. Invalid (Core): `[,1]`, `[1,,2]`, `{a:1,, b:2}`.

### 3.9 Element elision -- the discard form [2026]

**[2026]** The token `#_` (discard, borrowed from EDN; sibling of KDL's slashdash) elides the **next complete value** wherever a value may appear -- in lists, tuples, map/ordered-map *pairs*, node bodies, and top-level streams (Section 11):

```axon
[1 #_ 2 3]              # => [1 3]
{a:1 #_ b:2 c:3}        # => {a:1 c:3}   (discard before a pair elides the whole pair)
(#_ "skipped" 42)       # => (42)
```

Rules: `#_` **MUST** be followed by one complete, well-formed value (or pair, when in pair position), which is fully parsed and then excluded from the semantic model; discards compose (`#_ #_ a b` elides both `a` and `b`); the elided text **MUST** still satisfy all lexical and structural rules and **counts against resource limits** (Section 16); the CST preserves the discard and its operand; Canonical AXON **MUST NOT** emit `#_`. Lexical note: `#_` is distinguishable from a `#` comment by the immediately following `_` -- a comment is `#` followed by anything else, per Section 1.4; this refines Section 1.4 without changing any existing document's meaning (no baseline document contains `#_` followed by a value in element position, since `#_...` to end-of-line was previously a comment -- for that reason, parsers in the **Compatibility profile** (Section 15) **MAY** treat `#_` as a comment opener to preserve byte-exact legacy behaviour, and MUST document the choice).

### 3.10 Nesting

**[BASELINE]** Containers nest arbitrarily (`[{a:[1 (2 3)]}]`), subject to the mandatory depth limit of Section 16 (default 128). Nothing about container kind changes with depth.

---

## 4. Nodes

Nodes are AXON's identity: a **name** attached to a **body**, giving typed objects (`Point{x:1 y:-2}`), enum-like variants, and document structure (`tree{id:1 leaf{id:2 "AAA"}}`). This section specifies node forms, the name-body binding rule, body content and ordering, and namespaced names. Provenance is tagged with unusual care here because deep inspection of pyaxon's `get_named`/`get_complex_value`/`get_attributes` corrected two earlier claims (see 4.1 and 4.7).

### 4.1 Node forms and provenance

| Form | Example | Provenance | Notes |
|---|---|---|---|
| Unit node | `Foo` | **[BASELINE]** | bare name, no body |
| Brace body (same line) | `greek {alpha:123 beta:212}` | **[BASELINE]** | `{` may be preceded by spaces/tabs on the same line |
| Indented body | name, newline, deeper-indented content | **[BASELINE, constrained]** | pyaxon's formatted style; accepted only in the Formatted-Input/Compatibility profile (4.6) |
| Tuple body | `Rgb(255 128 0)` | **[2026]** | positional record; not in pyaxon |
| List body | `Tags["a" "b"]` | **[2026]** | sequence-bodied node; not in pyaxon |

**Correction of record:** earlier drafts (including this project's own ground-truth summary) listed `Name(...)`/`Name[...]` as baseline. Direct inspection shows pyaxon's `get_named` accepts only a same-line `{` or an indented body after a name; `(` or `[` there is an error, and no repository example uses those forms. They are therefore **[2026]** additions to this edition -- motivated by Serde tuple-variant mapping and positional records -- not preserved syntax. They are part of the AXON 2026 core grammar (unambiguous and LL(1)-clean), but a writer targeting legacy pyaxon **MUST NOT** emit them.

### 4.2 Grammar (delimiter mode)

```ebnf
node        = node_name , [ hs , node_body ] ;
node_name   = name | quoted_name | namespaced_name ;      (* 4.8 *)
node_body   = brace_body | tuple_body | list_body ;

brace_body  = "{" , ws , [ attributes ] , [ children ] , ws , "}" ;
attributes  = attr , { sep , attr } ;
attr        = attr_key , ws , ":" , ws , value ;
attr_key    = name | quoted_name ;                        (* narrower than map keys -- 4.4.2 *)
children    = value , { sep , value } , [ sep ] ;

tuple_body  = "(" , ws , [ value , { sep , value } , [ sep ] ] , ws , ")" ;   (* [2026] *)
list_body   = "[" , ws , [ value , { sep , value } , [ sep ] ] , ws , "]" ;   (* [2026] *)

hs          = { " " | "\t" } ;                            (* horizontal whitespace only *)
```

### 4.3 Name-body binding

**[2026, replacing a defective baseline]** In Core, a body binds to a name **only if its opening delimiter (`{`, `(`, `[`) appears on the same line as the name**, separated by nothing or by horizontal whitespace. **Any other outcome makes the name a unit node** and parsing simply continues: a newline, a comma (an ordinary separator), or any non-opening token all yield the unit node -- never an error, never a discarded value.

```axon
greek {alpha:1}      # one node                       [BASELINE behaviour, kept]
[Foo {a:1}]          # ONE element: Foo{a:1}          [BASELINE behaviour, kept]
[Foo, {a:1}]         # Core: TWO elements             (baseline: AxonError)
[Foo
 {a:1}]              # Core: TWO elements             (baseline: ONE -- binding crosses the newline)
person 5             # Core: unit node, then 5        (baseline: silent None, then 5)
```

**The baseline reality this replaces [runtime-verified]:** 0.9's binding logic crosses newlines inside containers (the indentation machinery leaking through), raises an error on a comma after a pending name, and -- worst -- yields a **silent `None`** when a name is followed on the same line by anything other than `{` (`Rgb(1 2 3)` -> `[None, (1,2,3)]`; `person 5` -> `[None, 5]`), a data-corrupting quirk, not a diagnostic. All three behaviours are reproduced together under `compat.legacy-binding` (Section 15.3) for byte-faithful legacy reads; none of them is something a Core document can rely on, so no meaning is silently changed -- corrupted or erroring inputs become well-defined ones.

Trivia note: Section 1.2 already grants that whitespace "separates tokens"; commas and newlines here delimit *elements* in exactly that sense and never alter an element's value (Section 2.9 stands).

**Reserved barewords.** `true`, `false`, and `null` are never node names (Section 2.3). Runtime: the baseline builder rejects a constant with a body, and AXON 2026 keeps that as `unexpected-token` (`true{x:1}`); `True{x:1}` is an ordinary node named `True`.

### 4.4 Brace bodies: attributes, then children

**[BASELINE, clarified]** A brace body consists of **zero or more attributes followed by zero or more positional children** -- in that order. This matches pyaxon exactly: `get_complex_value` parses an attribute block, then a values block, and a `key:` encountered after the first positional child is an error (`get_named` raises on the colon).

```axon
tree {
  id: 1                 # attributes first
  name: "root"
  leaf{id:2 "AAA"}      # children after -- any values, incl. named nodes
  leaf{id:3 "BBB"}
}
```

Rules: an attribute after the first child is an `unexpected-token` error in Core; free interleaving is **not** part of this edition's semantic model (the CST, Section 13, still records exact source positions for tooling). Duplicate attribute keys follow Section 12. Attribute order: semantically unordered like `Map` entries (Section 2.4) unless a profile states otherwise; children are order-preserving.

#### 4.4.1 The semantic node shape

A `Node` value exposes: its name; its attributes (a map view, duplicate policy per Section 12); and its children (an ordered sequence). Body kind is preserved: `Unit`, `Brace{attrs, children}`, and **[2026]** `Tuple(values)` / `List(values)` are distinct -- `Rgb` (unit) and `Rgb()` (empty tuple body) are different values, and a processor **MUST NOT** conflate them.

#### 4.4.2 Attribute keys are narrower than map keys

**[BASELINE]** Inside a brace body, an attribute key is a bare `name` or a `'quoted'` name -- **not** a `"..."` string. (Ground truth: `get_attributes` recognises only those two key forms; a `"..."` token there begins the *children* block as a string child.) This differs deliberately from plain maps, where string keys are legal (Section 3.7). Canonical AXON and formatters **MUST** respect the distinction rather than "normalising" a string key into a node attribute or vice versa.

### 4.5 Tuple and list bodies [2026]

**[2026]** `Name(...)` denotes a positional record (arity checked by schema/target type, not the parser); `Name[...]` denotes a named sequence. Both follow the same-line binding rule (4.3), the same separators (Section 1.3/3.8), and contain **values only** -- a pair inside either is an `unexpected-token` error. Rationale: these forms give enum tuple variants and compact records (`Rgb(255 128 0)`) a first-class spelling instead of forcing `Rgb{0:255 ...}` workarounds, and they map 1:1 onto Serde's variant kinds. Writers targeting legacy pyaxon rewrite `Name(a b)` as `Name{...}` per their schema or decline to emit.

### 4.6 Indented bodies (formatted style) [BASELINE, constrained]

**[BASELINE]** pyaxon's formatted style binds a body across a newline by **indentation**: after `name` + newline, content indented deeper than the name is the body; content at the same or lesser indentation makes the name a unit node; inconsistent indentation is an error. This is real, load-bearing pyaxon behaviour (the `idn`/`idn0` machinery), and AXON 2026 does not pretend otherwise -- but it *is* indentation-sensitive semantics, which this edition excludes from the core (research doc Section 7/Section 26).

Disposition: indented bodies are accepted **only** in the **Formatted-Input / Compatibility profile** (Section 15). Core Tree parsers **MUST** reject them (the unit-node reading of 4.3 applies, and stray deeper-indented values are ordinary stream/container content or errors by context). Canonical AXON **MUST NOT** emit them. A conforming formatter **MUST** be able to rewrite indented bodies to brace bodies losslessly -- the sanctioned migration path (research doc Section 29).

### 4.7 Why namespaces use `/` and not `:`

**[2026]** Namespaced names are `ns/name` (Section 2.7), e.g., `geo/point{lat:53.5 lon:-113.5}`. The separator is `/` following EDN's `namespace/tag`. A colon is impossible: ground truth shows `get_named` raises `error_unexpected_keyval` when a name is followed by `:` at value position -- colon *is* pair syntax in AXON, and `{geo:point{...}}` must keep meaning "key `geo`, value node `point{...}`". `/` collides with nothing (bare names exclude it; base64 `/` occurs only inside `|...|`). One namespace segment in this edition (`a/b/c` is reserved/invalid). Unknown namespaces parse structurally (Section 2.7); resolution is the application's or schema's concern.

### 4.8 Node name domain

A node name is: a bare `name` (Section 1.10); a `'quoted'` name (for characters bare names exclude -- **[BASELINE]**); or a `ns/name` namespaced name (**[2026]**). Reserved barewords are excluded (4.3). The `$name` construct is a distinct atom whose meaning is specified in Section 9, not a node-name form. Well-known tags (`uuid`, `instant`, Section 2.7) are ordinary nodes whose names are registered; they use these same forms.

### 4.9 Pointers

Duplicate attribute keys and ordering guarantees: Section 12. Lossless preservation of body layout, indentation style, and attribute order: Section 13. Canonical attribute ordering and body rendering: Section 14. Anchors on nodes (`&label Name{...}`): Section 10 -- the anchor binds the node value; nothing about the node changes.

---

## 5. Numbers

This section fixes the three numeric value kinds -- `Int`, `Float`, `Decimal` -- their syntax, semantics, special values, the [2026] digit-separator addition, and what is deferred or rejected. Lexical shapes were sketched in Section 1.6; this section is the authority and refines two of Section 1.6's claims with ground truth from pyaxon's `get_number`.

### 5.1 Numeric kinds and the shared lexer entry

| Kind | Literals | Provenance |
|---|---|---|
| `Int` | `0`, `42`, `-42` | **[BASELINE]** |
| `Float` | `3.14`, `-1.5e-17`, `1E6`, `∞`, `-∞`, `?` | **[BASELINE]** (exponent signs included) |
| `Decimal` | `10D`, `1000.35d`, `-1.25E+6D`, `∞D`, `?D` | **[BASELINE]** -- `d`/`D`; `$` only on specials |

**Ground truth -- the temporal gateway, now compat-scoped.** In pyaxon, `get_number` consumes digits and then *branches*: a following `-` backtracks into a **date/datetime** parse; a following `:` backtracks into a **time** parse. That behaviour is how the deprecated **bare** temporals load, and it is preserved verbatim under `compat.bare-temporals` -- including its footgun: in that mode `[1-2]` is an `invalid-temporal` error (runtime: "Invalid datetime"), never "an int then something", so legacy writers separate negatives (`[1 -2]`).

**In Core the gateway is gone**: temporals are announced by `^` (Section 1.8), so digits followed by `-` or `:` never open a temporal parse. But the bytes must not silently change meaning between editions, so Core treats digits **immediately** followed by `-` or `:` (no intervening trivia) as `invalid-temporal` with a migration hint ("write `[1 -2]`, or `^...` for a temporal") -- the same category 0.9 raises and exactly what 0.11 implements. A byte sequence never changes value silently between the baseline and Core -- it either keeps its meaning or becomes a diagnosed error.

### 5.2 Integers

**Syntax [BASELINE, clarified]:** optional minus, then digits (Section 1.6). A leading `+` is not AXON (no `+` dispatch exists in the baseline lexer) and is an `invalid-number` error in all profiles.

**Leading zeros:** pyaxon's digit loop performs no leading-zero check, so the baseline *tolerates* `007`. AXON 2026 Core **rejects** a leading zero (`invalid-number`) except for the single digit `0` -- a **[2026]** restriction adopted for JSON-family interoperability and canonical uniqueness. The Compatibility profile **MAY** accept and normalise them; Canonical AXON **MUST NOT** emit them. `-0` as an integer denotes `0` (there is no signed integer zero); Canonical emits `0`.

**Semantics:** the semantic `Int` is **arbitrary-precision**. A conforming parser **MUST NOT** silently wrap, truncate, or round an integer literal; if an implementation exposes bounded integer bindings (i64/u64/...), a literal outside the binding's range is a **binding-time error**, never a mutated value. (Precedent: EDN's `N` arbitrary-precision suffix shows text notations carry big integers fine -- AXON needs no suffix because `Int` is simply unbounded.) Width and signedness live in schemas and target types, never in the surface syntax (Section 1.6.1); BSON's int32/int64 split is a *wire-format* concern and is handled by the interop mappings (Section 14), not by AXON literals.

### 5.3 Floats

**Syntax [BASELINE]:** digits with a fractional part, an exponent, or both; exponent signs `+`/`-` are baseline (`1.602e-19`, `-1.25E+6`). Point-form edge cases are per Section 1.6.2: trailing-point is a tolerated-baseline form rejected by Core **[2026 restriction]**; leading-point is rejected everywhere.

**Semantics:** a `Float` is an IEEE-754 **binary64** value (implementations **MAY** parse into wider formats but **MUST** preserve at least binary64 behaviour). Parsing rounds to nearest per IEEE-754; a finite literal whose value exceeds binary64 range yields `+/-∞` per IEEE semantics, except that a strict profile **MAY** treat overflow-to-infinity as `invalid-number`. Applications needing exactness use `Decimal`. Signed zero: `-0.0` is a distinct `Float` value and **MUST** be preserved in the semantic model; its canonical rendering is decided in Section 14 (dCBOR reduces `-0.0` -- flagged there as an explicit decision point). `12` and `12.0` are different values of different kinds (`Int` vs `Float`); dCBOR-style numeric reduction is a canonical/interop concern (Section 14), never a parse-time unification.

### 5.4 Decimals

**Syntax [BASELINE]:** `(integer | float)` immediately followed by `d` or `D`; `D` is canonical. Exponent forms are baseline (`-1.25E+6D`).

**Semantics:** a `Decimal` is an exact base-10 value modelled as *(sign, arbitrary-precision coefficient, exponent)*. Losslessness is mandatory. Equality is decimal-value equality: `1000.35D` = `1000.350D`; suffix case and trailing-zero scale are CST trivia unless a scale-significant profile is enabled.

**Decimal specials [BASELINE]:** `∞D`, `-∞D`, `?D` (and `d`/`$` variants) are the decimal-domain infinity/NaN -- ground truth: `create_decimal_inf`/`create_decimal_nan`. They mirror the float specials' semantics within the decimal domain.

### 5.5 Special values

**[BASELINE]** `∞` (U+221E) and `-∞` are the IEEE infinities. `?` is strictly the IEEE **quiet NaN** -- it is *not* a generic "unknown/missing" marker; absence is `null` (Section 2.3). One text spelling exists per special. NaN **payloads** and signalling NaNs are **not representable** in AXON text: every NaN reads as the single quiet NaN and writes as `?`. Semantically NaN != NaN (Section 2.9); determinism is an encoding property -- Canonical AXON emits the stable spelling `?`, and binary interop mappings canonicalise to dCBOR's `0x7e00` (Section 14). ASCII spellings exist **[BASELINE]** via the default constant registry (Section 9.4): `$Inf`, `$NegInf`, `$NaN`, and `$NaND` (decimal NaN -- note the baseline asymmetry: no decimal-infinity constant is registered). Canonical output still uses the Unicode spellings.

### 5.6 Numeric separators [2026]

**[2026]** -- verified absent from the baseline lexer (no `_` handling anywhere in `get_number`). An underscore **MAY** appear **between two digits** of any digit run in an integer, float, or decimal literal:

```axon
1_000_000      3.141_592D      1.602_176_634e-19
```

Constraints: never leading or trailing in a digit run; never adjacent to the minus, the point, `e`/`E`, an exponent sign, or a decimal suffix; **never inside temporal literals** (temporals are distinct atoms, Section 1.8/Section 6). A doubled underscore cannot occur *inside* a literal at all -- `1__0` ends the number at `1` and then fails the adjacency rule (Section 5.9). Separators are trivia: ignored in the semantic value, preserved in the CST, **MUST NOT** appear in Canonical AXON. Violations are `invalid-number`.

### 5.7 Excluded and rejected

- **Radix literals** (`0x`/`0o`/`0b`): excluded from AXON 2026. A future edition may reconsider integers-only, profile-gated forms.
- **Rationals** (`a/b`, XSON): **rejected** (Section 9B of the research doc) -- decimals plus arbitrary-precision integers cover the exactness need; rationals import normalisation/equality/canonicalisation cost and sit confusingly beside `ns/name` namespacing.
- **Width/sign suffixes** (`8080u16`-style): **rejected** -- a Rust-port concern that must not leak into the notation (research doc Section 30).

### 5.8 Canonical and interop summary (pointers)

Fixed here, detailed in Section 14: canonical `Int` has no `+`, no leading zeros, no separators, and `-0` -> `0`; canonical `Float` uses the **shortest round-tripping decimal** (JCS precedent) with specials spelled `∞`/`-∞`/`?` and the `-0.0` decision recorded in Section 14; canonical `Decimal` uses `D` and the Section 14 normal form. Interop mappings (Section 14): `Decimal` <-> decimal128; `Int` <-> CBOR/MessagePack integer widths with overflow rules; `Float` specials <-> dCBOR canonical NaN `0x7e00`.

### 5.9 Numeric adjacency [2026]

A numeric literal **MUST NOT** be immediately followed by a name-start character or a digit; the violation is `unexpected-token` ("a numeric literal must be separated from a following name or digit"). This closes the D-1 review finding: without it, `1__0` and `1abc` silently lex as an integer plus a *name* (`[1, __0]`) -- a shape no author intends. The digits-then-`-`/`:` case has its own sharper diagnosis (Section 5.1). Under `compat.legacy-binding`-era reads the baseline shape is reproduced (`compatibility` restores `[1, Node __0]`); Core diagnoses. Implemented and tested in 0.11.0a3.

---

## 6. Temporals

Temporal literals are **caret-prefixed** (`^...`, Section 1.8) -- the 0.9 canonical notation, which the dumper itself emits; the deprecated bare shapes load only under `compat.bare-temporals`. This section is the authority on grammar, strictness, fraction semantics, kinds, and the [2026] additions (the `Z` suffix, extended fractional precision, named zones, and the `duration` tag). Ground truth throughout is the runtime-verified 0.9 build (`get_date`, `get_time`, `get_time_offset`, `get_tzinfo`) plus the changelog's 0.9->0.10 migration plan, which this edition completes.

### 6.1 Temporal kinds

| Kind | Core spelling | Provenance |
|---|---|---|
| `Date` | `^2012-12-31` | **[BASELINE 0.9]** -- bare `2012-12-31` is pre-0.9, `compat.bare-temporals` |
| `Time` (local) | `^12:00`, `^9:00`, `^12:30:34.250` | **[BASELINE 0.9]** -- single-digit hour is baseline; fraction semantics: 6.3 |
| `Time` + offset | `^12:35+03:00` | **[BASELINE 0.9]** -- `+03` short form is `compat.lax-temporals` |
| `DateTime` (local) | `^2012-12-31T12:30` | **[BASELINE 0.9]** |
| `DateTime` + offset | `^2012-12-31T12:35+03:00` | **[BASELINE 0.9]** |
| `DateTime` + `Z` | `^2012-12-31T09:35Z` | **[2026]** -- no `Z` branch exists in the baseline regardless of prefix (Section 1.8) |

### 6.2 Grammar (authoritative; supersedes the Section 1.8 sketch)

```ebnf
temporal   = "^" , ( datetime | date | time ) ;  (* Core -- Section 1.8; bare shapes only under compat.bare-temporals *)
date       = year , "-" , month , "-" , day ;
time       = hour , ":" , minute , [ ":" , second , [ "." , fraction ] ] , [ offset ] ;
datetime   = date , "T" , time ;                (* uppercase T only -- [BASELINE] *)

year       = digit4 ;                            (* exactly four -- Core; see 6.3 *)
month      = digit2 ;   day = digit2 ;
hour       = digit1_2 ;                          (* 1-2 digits -- [BASELINE] *)
minute     = digit2 ;   second = digit2 ;
fraction   = digit1_9 ;                          (* 1-6 [BASELINE]; 7-9 [2026] *)
offset     = zulu | ( "+" | "-" ) , digit2 , ":" , digit2 ;
zulu       = "Z" ;                               (* [2026] *)
```

### 6.3 Baseline laxness vs Core strictness

The baseline lexer's digit reader (`try_get_int`) accepts *up to* N digits -- and on overflow silently consumes one extra digit -- so the baseline tolerates unpadded fields (`2012-1-5`), short years, and related sloppiness. AXON 2026 Core tightens this; every tightening is a **[2026] restriction** the Compatibility profile may relax:

| Rule | Core | Baseline reality |
|---|---|---|
| Year | exactly 4 digits | up to 4, extra digit swallowed |
| Month/day | exactly 2, zero-padded | 1-2 digits |
| Hour | 1-2 digits (**[BASELINE]**, kept; canonical pads) | 1-2 digits |
| Minute/second | exactly 2 | 1-2 digits |
| Fraction | 1-9 digits (**[2026]** beyond 6) | 1-6, 7th swallowed |
| Notation | `^`-prefixed only | bare shapes load too -- `compat.bare-temporals` |
| Offset | sign **required**, `:MM` **required**, magnitude <= `23:59` | signless-digit quirk branch; `:MM` optional (`+03` legal); the lexer admits `24:00`, but the baseline value builder rejects it, so compatibility also rejects it |
| Fraction **meaning** | positional decimal fraction (ISO 8601) | **literal integer count of us** -- `.5` = 5 us, on the caret path too; `compat.raw-fraction` |

**Fraction semantics [2026 fix over a runtime-proven defect].** The baseline reads the fraction digits as a raw microsecond integer (`^12:00:00.5` -> 5 us; `.50` -> 50 us); only exactly-six-digit fractions mean what ISO says, and the dumper's always-six-digit padding is what kept the ecosystem self-consistent while hiding the defect. Core reads fractions **positionally**, 1-9 digits, over a **nanosecond** value model (`^12:00:00.5` -> 500 ms); `compat.raw-fraction` reproduces the literal-us reading for byte-faithful legacy loads, and the Section 19.3 migration pads to six digits *before* reinterpretation -- at six digits the two readings coincide, so the rewrite is value-preserving. Legacy consumers truncate digits 7-9 (their model is us) -- an interop note, never a licence to round silently in a 2026 parser. Trailing fraction zeros remain CST trivia (mirroring decimal scale, Section 5.4); the scale-significant profile MAY make them semantic.

### 6.4 Validation

**[BASELINE]** Calendar and clock validity are enforced (in the baseline, by the builder; in AXON 2026, normatively at parse): `2026-02-30` -> `invalid-temporal`; minutes/seconds <= 59; hour <= 23 (`24:00:00` is forbidden -- decided, not left open); **no leap seconds** -- `:60` is rejected everywhere, a deliberate divergence from RFC 3339 that matches the baseline value model's inability to represent them. Offsets beyond the Core magnitude are `invalid-temporal`.

### 6.5 The bare-shape tripwire and the space rule

Core has no number->temporal gateway (Section 1.8, Section 5.1). Two normative consequences:

- **Tripwire:** digits immediately followed by `-` or `:` are `invalid-temporal` with a migration hint -- the same category 0.9 raised -- so `[1-2]` and `[12:99]` are never silently "an int then something" in either edition. Under `compat.bare-temporals` the full 0.9 commit applies and the bare shapes parse (with all their Section 6.3 laxities).
- **A space never joins a date and a time.** `^2012-12-31 ^12:30` is a `Date` followed by a `Time` -- two values (Section 11 / container elements) -- never a datetime; the same held for the bare shapes in 0.9. Only uppercase `T` forms a datetime. Writers converting from RFC 3339's space-separated or lowercase-`t` variants **MUST** normalise to `T`.

### 6.6 Kinds, offsets, and equality

Local and offset temporals are **distinct kinds of value content**; a parser **MUST NOT** assume UTC for a local datetime, and **MUST NOT** normalise an offset (`12:00-06:00` is not rewritten to `18:00Z` -- research Section 12.3). Equality is field-wise per Section 2.9: same instant at different offsets -> **different values**. `Z` [2026] is preserved as a marker distinct from `+00:00`. Instant-comparison across offsets is an application/canonical operation (Section 14), not value equality.

### 6.7 The `Z` suffix [2026]

Adopted for RFC 3339 interop: `Z` after a time denotes UTC. Ground truth makes the legacy behaviour explicit: in baseline pyaxon a trailing `Z` is *outside* the temporal (it would lex as a following name), so emitting `Z` targets 2026 parsers only; writers targeting legacy emit `+00:00`.

### 6.8 Named time zones [2026, profile-gated]

The research question (Section 12.3) is settled: **offsets stay core; named IANA zones are an optional extension** under the `tz-names` profile (Section 15), using the RFC 9557 suffix form attached with **no whitespace**:

```axon
2026-11-01T01:30-06:00[America/Edmonton]
```

Rules: the bracket suffix is part of the temporal atom only under the profile (otherwise the temporal ends and `[` opens a list -- which then fails to parse, so there is no silent misread); the numeric offset remains **required** alongside the zone; an offset/zone mismatch for that instant is `invalid-temporal`; RFC 9557 criticality flags are excluded from this edition; implementations declare their tzdb version. Rationale: a named zone carries the DST *rule* an offset cannot -- the future-dated-events case -- at the cost of a tz-database dependency, which is exactly why it is not core.

### 6.9 Durations [2026]

There is **no bare duration literal**: `P1DT2H` lexes as a *name* (Section 1.10) and would collide with unit nodes -- a ground-truthed impossibility, not a style choice. Durations use the well-known tag: `duration("P1DT2H")` (ISO-8601 duration string; registry Section 2.7). Validation of the inner string is the tag's semantics, not the lexer's.

### 6.10 Canonical and interop pointers

Fixed in Section 14: canonical temporals are **`^`-prefixed**, zero-pad every field (including hour), always emit seconds, carry a positional fraction with no trailing zeros (and a bare `.` never appears), render offsets as `+/-HH:MM`, and keep `Z` as `Z`. Interop: `instant(...)` <-> MessagePack/CBOR timestamp ext; local temporals map to tagged strings in formats lacking the kind.

---

## 7. Strings and Text

Ground truth reshaped this section: pyaxon lexes `"..."`, backtick, **and** `'...'` through one function (`get_string`), so the baseline escape story is far thinner -- and stranger -- than any prior draft assumed. AXON 2026 keeps the baseline's good properties (native multiline, delimiter flexibility) and defines a real escape system as **[2026]**.

### 7.1 Forms

**[BASELINE]** Two string-*value* delimiters: `"..."` and `` `...` ``. They are the **same construct with alternate delimiters** -- the backtick is *not* a raw string in the baseline (same lexer, same backslash handling); its practical use is holding unescaped `"`. Both span lines natively. Delimiter choice is CST trivia (Section 2.9). (`'...'` shares the lexer but produces a **name**, not a string -- Section 1.10/Section 9.)

### 7.2 The baseline escape reality [GROUND TRUTH, runtime-verified]

The baseline has exactly **one working** escape -- `\<active-delimiter>` yields the delimiter character -- plus one **defective** one: backslash-newline was meant as a line continuation, but at runtime it duplicates the preceding chunk and keeps the backslash (`"ab\` 
 `c"` -> `'abab\c'`). Everything else -- `\n`, `\r`, `\t`, `\u` -- is **commented out in the lexer**, and any other `\x` emits a literal backslash and leaves `x` as content (backslash passthrough). That passthrough is why `` `C:\Users\file.txt` `` round-trips today. A modern escape table therefore cannot be called "preserved"; it is defined fresh below, and the continuation is *redefined*, not inherited.

### 7.3 The AXON 2026 escape table [2026]

In `"..."` and backtick strings, Core recognises exactly:

| Escape | Meaning |
|---|---|
| `\\` | backslash |
| `\"`, `` \` ``, `\'` | the three quote characters (escapable in **any** string, regardless of delimiter) |
| `\n`, `\r`, `\t` | LF, CR, HT |
| `\u{H...}` | 1-6 hex digits: any Unicode scalar value; surrogate code points -> `invalid-escape` |
| `\<newline>` | line continuation -- **[2026]**, giving the semantics the defective baseline lacked (7.2) |

Any other `\x` is an `invalid-escape` error in Core -- a **[2026] restriction** over the baseline's passthrough, which `compat.legacy-escapes` reproduces (documented, including the continuation defect) for legacy files. There is deliberately **no** 4-hex `\uXXXX` form and **no** surrogate-pair escapes (the exact serde_axon audit finding): the braced form (XSON/Swift/Rust precedent, research Section 9B) is the only Unicode escape.

### 7.4 Newlines

**[BASELINE]** Literal newlines are legal in all strings, and CR / CRLF **normalise to LF** in the value -- retained as normative (cross-platform determinism). To embed a real CR, use `\r` or `\u{D}` [2026]. Continuation semantics per 7.3 -- the baseline's version is the documented defect of 7.2, never something to rely on.

### 7.5 Raw strings [2026]

`r"..."`, `r#"..."#`, `r##"..."##` ... (Rust rules: the closing quote must be followed by the same number of `#`). No escapes, no continuation; newline normalisation (7.4) still applies; may span lines. The syntax space is verifiably free: in the baseline, a name followed by `"` is an error, so no legacy document changes meaning.

### 7.6 No triple-quoted form

Rejected as redundant -- baseline strings are already multiline (7.1/7.4). This supersedes the research draft's `"""..."""` proposal.

### 7.7 Content rules

String values are Unicode scalar sequences (Section 1.1); no normalisation is applied to values. Core forbids **unescaped** C0 controls other than HT, LF, and CR-before-normalisation -- a **[2026] restriction** (the baseline chunks raw bytes through); Compatibility accepts them. Strings are never implicitly typed: no string ever becomes a number, temporal, or boolean by inspection.

### 7.8 Canonical pointer

Canonical AXON emits `"..."` only, single-logical-line (LF as `\n`), with the minimal escape set: `\\`, `\"`, `\n`, `\r`, `\t`, and `\u{...}` for remaining C0 controls; all other scalars appear literally as UTF-8. Details in Section 14.

---

## 8. Binary

### 8.1 Two spellings, one value kind

Binary denotes `Bytes` -- never text; equality is bytewise (Section 2.9). Two spellings exist:

- **Legacy open form [BASELINE, runtime-verified]:** `|SGVsbG8=` -- opened by `|`, body of standard-alphabet base64 with any characters <= U+0020 skipped (multiline by design), and terminated **only by the `=` padding run**. There is **no closing pipe anywhere in 0.9** -- grammar, prose docs, loader, and dumper all agree. Accepted only under `compat.legacy-binary`.
- **Core closed form [2026]:** `|SGVsbG8=|` -- the closing `|` terminates the literal; padding is optional on input (a body length == 1 mod 4 after whitespace-stripping is `invalid-binary`); interior skipped characters narrow to Section 1.2 whitespace. This is the only form Core accepts (`|SGVsbG8=` in Core is `unexpected-end`: "binary literal requires a closing pipe").

### 8.2 Why the closed form exists -- the baseline's round-trip defects [runtime-verified]

Because the open form can end only at a padding run, **payloads that produce no padding are unwritable in it**: the 0.9 dumper emits `|QUJD` for `b'ABC'` (any `len % 3 == 0`) and a bare `|` for `b''` -- outputs **its own loader rejects** ("MIME Base64 string is not finished"). The closed form repairs both without changing any *loadable* legacy document's meaning; the Section 19.3 migration adds the closer, and Section 19.2 forbids writing the affected payloads to legacy targets at all.

### 8.3 Canonical form and what binary is not

Canonical AXON emits the **closed** form: standard alphabet, padded, no interior whitespace (Section 14.3). Hexadecimal spelling is excluded from this edition (Section 1.11); there is no subtype syntax -- a typed blob is a **node** wrapping the literal, e.g., `uuid(|...|)` (BSON-subtype lesson, research Section 9A.4). Size limits: Section 16. Interop (BSON binary subtypes, MessagePack `bin`): Section 14.

---

## 9. Identifiers, Names, Keys, and Constants

### 9.1 Bare names

**[BASELINE, clarified]** Bare names are **Unicode** (Section 1.10 ground truth) -- pyaxon's classes are Python's `isalpha`/`isalnum` plus `_`. AXON 2026 pins these lax classes to UAX #31: `name_start = XID_Start  union  {_}`, `name_char = XID_Continue  union  {_}`. The differences from the Python classes are edge-case-only and are a **[2026] clarification**; the Compatibility profile accepts the raw Python classes. ASCII-only is a portability lint, never grammar. `greek`, `_tmp`, `étage`, `имя` are all names. Reserved barewords: `null`, `true`, `false` (Section 2.3) -- never names.

### 9.2 Quoted names

**[BASELINE]** `'...'` introduces a *name* for characters bare names exclude (`'weird name'{...}`). Ground truth: quoted names go through the string lexer, so baseline quoted names are multiline with the Section 7.2 backslash behaviour; AXON 2026 applies the Section 7.3 escape table to them, and Core forbids **literal newlines inside names** as a **[2026]** sanity restriction (Compatibility accepts). A quoted name and a bare name with identical text are the **same name**.

### 9.3 Namespaced names [2026]

`ns/name` (Section 2.7, Section 4.7). Both segments are names per 9.1/9.2; exactly one `/`; the namespaced name is a distinct name whose text includes the slash (`geo/point` != `point`).

### 9.4 Constants: `$name`

**[BASELINE]** -- ground truth settled what `$name` is: a **registered-constant reference**. The parser resolves `name` against a constant table at parse time and substitutes the value; an unregistered name is an error (Core: `unknown-constant`). **The default table is not empty**: the baseline registers exactly four constants -- `$NaN`, `$NaND`, `$Inf`, `$NegInf` (float NaN, decimal NaN, +/-infinity; `true`/`false`/`null` appear in the same table only as commented-out code). AXON 2026 keeps those four as the normative default registry **[BASELINE]** -- they double as the ASCII spellings of `?`/`∞` (Section 5.5) -- and applications **MAY** register more. Constants are a *reader-side* facility: the CST preserves the `$name` spelling; the semantic model contains only the substituted value; Canonical AXON **MUST NOT** emit `$name` (it emits the value -- canonical output cannot depend on reader configuration). Security note: constants are lookup-only substitution -- no evaluation, no construction.

### 9.5 Semantic identity of names and keys

Names compare by exact scalar sequence -- **no Unicode normalisation** is applied when comparing or emitting them (two visually identical but differently-composed names are different names; a lint SHOULD warn). Canonical output quotes where required but never changes the scalar sequence (Section 14).

### 9.6 Consolidated key/name domains [GROUND TRUTH]

| Context | Bare name | `'quoted'` name | `"string"` |
|---|---|---|---|
| Map / OrderedMap key (Section 3.7) | Y | N | Y |
| Node attribute key (Section 4.4.2) | Y | Y | N |
| Node name (Section 4.8) | Y | Y | N |
| Top-level attribute key (Section 11.3) | Y | N | Y |

The asymmetry is baseline reality, preserved deliberately; formatters **MUST NOT** "normalise" across columns (e.g., rewriting a `"has space"` map key as a `'has space'` attribute-style key or vice versa).

---

## 10. References, Graphs, and Links

### 10.1 Syntax and labels

**[BASELINE]** `&label value` binds; `*label` references (Section 1.12). Ground truth: a label is one-or-more of the *name-continue* class (`alnum  union  {_}`, Unicode) -- so `1`, `42`, `a`, `n7`, and even `1x` are labels; a label is a token, not an integer (label `01` != label `1`).

### 10.2 Scope and resolution

**[BASELINE]** The label table is **stream-scoped**: it persists across all top-level values of one parse (ground truth: `labeled_objects` lives on the Loader) -- this is load-bearing (the cross-reference example depends on it) and is preserved. Resolution is **define-before-use**: ground truth shows `*label` performs an immediate lookup; in the baseline an unknown label silently yields an *undefined sentinel*. AXON 2026 Core replaces the sentinel with an `unknown-reference` error -- a **[2026] restriction**; the Compatibility profile documents the sentinel. Forward references are therefore invalid in Core (they were never *resolvable* in the baseline, only silently broken).

### 10.3 Duplicates and cycles

Baseline overwrites a re-bound label silently; Core makes a duplicate label a `duplicate-anchor` error -- **[2026] restriction**. Cycles: the baseline binds the label **after** parsing the value, so self-reference inside an anchored value hits the sentinel -- cycles were structurally impossible. AXON 2026's Graph profile binds the label **at the anchor site, before the body is parsed** -- a **[2026]** change that makes shared *and cyclic* structure expressible (`&1 {self: *1}` is a legal one-node cycle). This is flagged as a semantic change, not a clarification; no meaningful legacy document depended on the sentinel-inside-own-body behaviour.

### 10.4 Semantics and safety

A `Reference` resolves to the **identical value** (Section 2.5) -- aliasing, not copying. Consequently, the billion-laughs attack does not exist by construction: `*label` never expands text or duplicates memory. Remaining limits (anchor count, reference count) are in Section 16. Outside the Graph profile, `&`/`*` are `unsupported-profile-feature` errors (Section 1.12). Writer-side: emitting `&`/`*` for shared/cyclic values is established baseline practice (the dumper's `crossref` mode); deterministic label allocation is fixed in Section 14.11.

### 10.5 Anchors are values

`&label value` is transparent: the anchored expression denotes `value` itself (baseline: the anchor branch returns the value). An anchor may wrap any value, including nodes (`&cfg server{...}`), containers, and scalars.

### 10.6 What references are not

References are not paths, not queries, and not lazy: no `*a.b.c`, no cross-file resolution in the Graph profile, no side effects. Cross-*document* linking is the separate Document Link profile:

### 10.7 Document Links [2026]

Under the **Document Link profile**, the well-known tags `cid("bafy...")` and `link("https://...")` (Section 2.7) produce `Link` values (Section 2.8) -- content-addressed and URI references respectively. AXON-generated CIDs are CIDv1 values over canonical AXON bytes using the permanent `raw` multicodec (`0x55`), SHA2-256 multihash (`0x12`, 32-byte digest), and lowercase base32 multibase (`b`). Implementations MUST reject malformed or differently parameterised values when an AXON-generated CID is required. Design properties: **zero new syntax** (they are ordinary nodes, so legacy and non-profile parsers degrade gracefully to a structural node); the profile defines no fetching -- a `Link` is a value, and dereferencing is the application's concern.

---

## 11. Documents and Streams

### 11.1 Two document forms [GROUND TRUTH]

A parse unit (file, string) takes one of two shapes, chosen by its **first non-trivia construct**:

1. **Value stream** -- zero or more top-level values separated by trivia. `loads` of the baseline returns them as a sequence; `iload` yields them one at a time.
2. **Top-level attribute document** -- if the first construct is a `key : value` pair, the *entire* document is one **OrderedMap**: every subsequent construct **MUST** be a pair (mixing is an error). This is the config style (`application: "myapp"` ...) and is **[BASELINE]** (ground truth: `Loader.load` switches on whether the first item is a KeyVal).

Top-level attribute keys are bare names or `"strings"` (Section 9.6). The attribute form is an *input surface style*: semantically it denotes exactly the `OrderedMap` value; the CST records which style was used; Canonical AXON always emits the value-stream form (an attribute document canonicalises to one `[k:v ...]` value).

### 11.2 Stream semantics

**[BASELINE]** A stream is **not** a list: two top-level `Event{...}` values are two documents, not `[Event{} Event{}]`. Values are self-framing (atoms and balanced delimiters); no framing tokens exist or are needed -- NDJSON-style one-value-per-line is a *style*, not semantics. An empty (or trivia-only) input is a valid stream of zero values. Inter-value comments are trivia. The anchor table spans the stream (Section 10.2). Incremental parsing **MUST** be possible value-by-value with per-value resource limits (Section 16); trailing garbage is an error at the first offending token; EOF inside a value is the corresponding `unexpected-end` error with a span (Section 17).

### 11.3 Discards at top level

`#_` (Section 3.9) elides the next top-level value (or pair, in an attribute document) -- [2026], same rules.

### 11.4 The document header [2026]

Closing the XSON candidate (research Section 9B): if the **first value of a value stream** is a node named `axon`, it is the **document header**:

```axon
axon{edition:"2026" require:["graph" "tz-names"]}
```

Fields: `edition` (string); `require` (a list of profile ids). Semantics per the D-2 adjudication, implemented in 0.11.0a3: the parser **MUST** support every listed id (unknown -> `unsupported-profile-feature` **at the header**); supported feature ids are **enabled for this document** unless caller policy forbids header enabling, in which case the error is likewise raised at the header, naming the refused ids -- never silently downstream. The header is **consumed** (it is not a data value); a node named `axon` anywhere else -- and everywhere in legacy parsers -- is ordinary data, which is the graceful-degradation property that made this design win. Unknown *fields* are ignored (forward compatibility). The header never appears in an attribute document (its first construct is a pair by definition). Canonical AXON does not emit a header (canonical bytes describe values, not parser configuration); a signing scheme that wants the header covered simply includes it as a value.

---

## 12. Duplicates and Ordering

### 12.1 Duplicate keys

**[GROUND TRUTH]** The baseline silently **last-wins** everywhere (every pair reader does `mapping[key] = val`). AXON 2026:

| Profile | Map / OrderedMap / node attributes | Top-level attribute doc |
|---|---|---|
| Core | `duplicate-key` **error** [2026 restriction] | same |
| Lossless CST | all pairs preserved in order **and** a diagnostic reported | same |
| Compatibility | documented last-wins (baseline) | same |
| Canonical (output) | duplicates impossible | -- |

Duplicate detection uses semantic key identity (Section 3.7/Section 9.5): `{a:1 "a":2}` is a duplicate; differently-composed Unicode is not.

### 12.2 Duplicate set elements

Baseline deduplicates silently (Python set). Core raises `duplicate-set-element`; Compatibility silently deduplicates; the CST preserves the duplicate plus a diagnostic. `{1000.35D 1000.350D}` is a duplicate by decimal-value equality.

### 12.3 Ordering guarantees (consolidated)

| Construct | Semantic order |
|---|---|
| `List`, `Tuple`, `OrderedMap`, node children | preserved, significant |
| `Map`, node attributes, `Set` | none (CST preserves source order for tools) |
| Stream values | preserved, significant |

No profile may make `Map` iteration order semantic without saying so explicitly; programs needing ordered pairs use `[k:v ...]` -- that distinction existing *in the notation* is a core AXON advantage and the reason silent reordering by tools is forbidden outside canonicalisation.

---

## 13. The Concrete Syntax Tree

**Implementation:** `axon2.cst2026` provides byte-owning tokens, UTF-8 byte spans, line/column locations, exact rendering, semantic values when valid, retained diagnostics when invalid, and byte-span edits followed by reparsing.

### 13.1 Purpose

The CST is the **lossless** representation (Section 2.1): every byte of the source is owned by exactly one token or trivia piece, so `bytes -> CST -> bytes` is the identity. It exists for formatters, linters, migration rewriters, doc generators, and editors -- the tools that must preserve comments and style while changing content. A semantic parser never needs it; a tool that mutates documents always does.

### 13.2 Model

A CST is a tree of **nodes** (one kind per grammar production: document, stream-value slot, container, pair, node-name, body, atom ...) whose leaves are **tokens** with byte spans, each carrying **leading and trailing trivia** lists (whitespace runs, comments, commas, discard forms with their elided operands, continuation backslashes). Attachment rule: trivia binds to the *following* token, except trailing same-line trivia (e.g., an end-of-line comment after a value), which binds to the preceding token -- the rule that makes comment-preserving edits behave the way humans expect. Every semantic value parsed from a document, including nested and discarded values, **MUST** have an explicit trace containing its UTF-8 span and nearest owning CST node.

A recovering CST parser reports every independently recognisable structural error, not only the first semantic error. Where the grammar determines the next legal token class, the diagnostic **MUST** carry a context-specific expected-token set. A proposed fix is a non-overlapping UTF-8 byte edit `(start, end, replacement)`; applying it **MUST** produce exactly the advertised text, and a fix advertised as syntactic repair **MUST** remove that diagnostic when reparsed. A tool may omit a fix where author intent is ambiguous.

### 13.3 What the CST preserves (consolidated)

All Section 2.9 presentation trivia, plus everything the spec has declared trivia since: decimal-suffix spelling (`d` vs `D`) and scale; string delimiter choice, escape-vs-literal spellings, continuations, and raw-string hash counts; numeric separators; leading zeros / unpadded temporal fields (Compatibility inputs); the `Z`-vs-offset spelling; base64 interior whitespace and padding presence; key spelling per Section 9.6 column; node body style (brace vs indented -- the indented form is *representable only* in the CST, Section 4.6); attribute/child interleave positions (Section 4.4); anchor label spellings and `$name` constant spellings (Section 9.4, Section 10.1); the two document forms (Section 11.1); the header (Section 11.4); and comma/discard placement. Token classification **MUST** use the active profile: in particular, compatibility open-pipe binary ends at its padding run, and `compat.hash-underscore-comment` owns `#_...` as comment trivia rather than a discard.

### 13.4 Operations

Formatting, linting, and the Section 19 migrations are defined as CST transforms with two invariants: (a) untouched regions are byte-identical; (b) the semantic value is unchanged unless the transform's contract says otherwise (a migration's contract is "same value, 2026 spelling"). Rust-port note (informative): a rowan-style immutable green tree with red cursors fits this model exactly (research Section 24, Section 33).

---

## 14. Canonical AXON

### 14.1 Purpose and shape

Canonical AXON is a **function from a semantic value (or stream of values) to bytes**, for hashing, signing, CIDs (Section 10.7), lockfiles, snapshot tests, and equality-by-bytes. Same value => same bytes; different values => different bytes (non-malleability), modelled on dCBOR + JCS (research Section 10). Output is UTF-8; a single value occupies one logical line with **no newline anywhere** (strings escape theirs, Section 7.8) and no trailing newline; a stream is values joined by exactly one LF, no trailing LF.

### 14.2 Layout rules

No comments, commas, discards, headers, or `$name` spellings (Section 9.4, Section 11.4). Whitespace: exactly one space between adjacent tokens only where required for separation -- concretely, between container/body elements and between an attribute's value and the next attribute key; **no** space around `:`, after `{ [ (`, before `} ] )`, or between a node name and its body. Empty forms: `{}`, `[]`, `[:]`, `∅`, and node bodies `Name{}` / `Name()` / `Name[]` distinct from unit `Name` (Section 4.4.1).

### 14.3 Scalars

- **Constants:** `null`, `true`, `false`.
- **Int:** minimal decimal digits, `-` only for negatives, no leading zeros, no separators; integer `-0` is `0` (Section 5.2).
- **Float:** the shortest decimal string that round-trips the binary64 value (JCS / RFC 8785 number serialisation, transposed to AXON spelling: no leading `+`, lowercase `e`, no exponent leading zeros, and JCS's ordinary-decimal thresholds). If the JCS spelling contains neither a decimal point nor an exponent, append `.0` so loading preserves the Float kind. Specials are `∞`, `-∞`, `?`. **Decision:** `-0.0` canonicalises as `-0.0` -- AXON keeps Int/Float distinct (Section 5.3), so dCBOR's cross-kind reduction (-0->0, 12.0->12) does not apply; one spelling per *Float* value preserves non-malleability without collapsing kinds. All NaNs canonicalise to `?` (payloads unrepresentable, Section 5.5); binary mappings use dCBOR's `0x7e00`.
- **Decimal:** **positional-only** -- no exponent form. Strip trailing fractional zeros and insignificant leading zeros; suffix `D`; specials `∞D`, `-∞D`, `?D`. (Determinism over brevity: `1E100D` canonicalises to the written-out positional form; the scale-significant profile is incompatible with Canonical.)
- **Temporals:** per Section 6.10 -- the `^` prefix always present; every field zero-padded (including hour), seconds always present, positional fraction with no trailing zeros and no bare `.`, offsets as `+/-HH:MM`, `Z` stays `Z` (distinct value, Section 6.6). Named-zone suffixes (Section 6.8) are emitted verbatim after the offset when present.
- **Strings and names:** strings per Section 7.8 (always `"..."`, minimal escapes, NFC is not applied). Keys and names preserve their exact scalar sequence: emit each namespace segment bare when legal (Section 9.1), otherwise use the quoted form permitted by its context (Section 9.6) with Section 7.8-style escaping. No canonical spelling may normalise a name.
- **Bytes:** the **closed** form `|...|` -- standard alphabet, padded, no interior whitespace (Section 8.3).

### 14.4 Ordering of unordered collections

`Map` pairs and node attributes sort by their key's semantic text compared as **UTF-8 bytes, lexicographically** -- following modern deterministic-CBOR practice and deliberately diverging from JCS's UTF-16 order (AXON is UTF-8-native; the divergence is documented, not accidental). `Set` elements sort by their own canonical encodings compared as UTF-8 bytes (recursive; well-founded because elements canonicalise independently). `List`/`Tuple`/`OrderedMap`/children/stream order is semantic and untouched.

### 14.5 Graphs [Canonical + Graph]

Only values that are shared (reachable twice) or cyclic receive anchors. Labels are `1, 2, 3...` in first-encounter order of the canonical traversal (document order after 14.4 sorting); the first encounter emits `&n value`, every subsequent one `*n`. This makes graph canonicalisation deterministic; the Document Link profile then hashes the bytes for CIDs (Section 10.7).

### 14.6 Verification

A **canonical-verify** mode is required: parse in strictest Core, re-canonicalise, and accept iff bytes match. Anything non-canonical -- a comment, a `d` suffix, an unsorted key, an unpadded month -- fails verification even though it parses fine as ordinary AXON.

### 14.7 Interop mappings

| AXON | CBOR | MessagePack | BSON | Lossless requirement |
|---|---|---|---|---|
| `Int` | major 0/1 or bignum | integer | int32/int64 | overflow errors; never float coercion |
| `Float` | binary64 | float64 | double | canonical NaN follows Section 14.2 |
| `Decimal` | tag 4/exact decimal | registered extension | decimal128 | out-of-range errors; never silent rounding |
| `Bytes` / `uuid` | byte string / tag 37 | bin / registered UUID | binary / UUID subtype | byte identity; UUID is exactly 16 bytes |
| temporals | registered tag or text | extension or text | UTC datetime where unambiguous | zone, offset, or nanosecond loss requires explicit policy |
| list/tuple/set | array plus kind tag where needed | array/extension | array | a lossless binding preserves collection kind |
| maps | map | map | string-key document | no key coercion or duplicate collapse |
| `Node` | registered structured tag | extension/map | structured document | preserve name, style, attributes, and children |
| graphs | shared tags 28/29 | graph extension | graph document | tree-only targets reject sharing/cycles |

Round-tripping through a mapping advertised as lossless **MUST** reproduce canonical AXON bytes. Deliberately degrading bindings use separately named lossy APIs.

---

## 15. Conformance Profiles

### 15.1 Model

A **profile** is a named feature flag a parser may support and a document may require. The base is **Core** -- Sections 1-12 with every strict rule, all `[BASELINE]` constructs, and the `[2026]` core additions; it is always on. Profiles have stable kebab-case identifiers used by the document header's `require` list (Section 11.4) and by implementations' capability advertisements. A parser encountering a required profile it does not support, or a profile-gated construct outside its profile, raises `unsupported-profile-feature`. Profiles never relax the resource limits of Section 16.

### 15.2 Profile registry

| Id | Grants | Defined in | Interactions |
|---|---|---|---|
| `graph` | `&label` / `*label` with 2026 semantics (pre-binding; cycles) | Section 10 | composes with `canonical` via Section 14.5 |
| `doc-link` | `cid(...)` / `link(...)` produce `Link` values | Section 10.7 | producing/verifying CIDs requires `canonical` |
| `tz-names` | RFC 9557 `[Zone/Name]` suffixes | Section 6.8 | tzdb version must be declared |
| `scale-significant` | decimal scale and temporal fraction trailing zeros become semantic | Section 5.4, Section 6.3 | **incompatible with `canonical`** |
| `cst` | lossless concrete-syntax-tree access; recovering multi-error parse | Section 13, Section 12 | -- |
| `canonical` | canonical writer + canonical-verify mode | Section 14 | not meaningful in `require` except as "verify me" (`canonical-verify`) |
| `compat` | the legacy-fidelity bundle | 15.3 | input only -- see below |

*Implementation note (informative):* Core is deliberately amenable to structural-index ("simdjson-style") scanning -- delimiters are explicit, recognition lookahead is bounded to one atom (Section 3.6.3), and no semantics live in indentation or line structure. Every behaviour that forces sequential, backtracking parsing (indentation bodies, the bare-temporal commit, padding-run binary, escape passthrough) is a `compat.*` flag: the Core/compat split is the fast-path/slow-path split.

### 15.3 The Compatibility bundle

`compat` is a bundle of individually addressable relaxation flags, each reproducing a **documented** baseline behaviour. It exists to ingest legacy pyaxon documents byte-faithfully and to feed the Section 19 migrations. It **MUST NOT** be used to produce interchange output.

| Flag | Baseline behaviour restored | Section |
|---|---|---|
| `compat.bare-temporals` | pre-0.9 bare temporal shapes via the number-lexer commit -- including its `[1-2]` invalid-temporal footgun | 1.8, 5.1, 6 |
| `compat.raw-fraction` | fraction digits read as a literal microsecond integer (`.5` = 5 us) | 6.3 |
| `compat.legacy-binding` | newline-crossing name->body binding; error on a comma after a pending name; silent `None` for a name before a non-opener | 4.3 |
| `compat.indented-bodies` | indentation-bound node bodies | 4.6 |
| `compat.lax-numbers` | leading zeros; trailing-point floats | 5.2, 1.6.2 |
| `compat.lax-temporals` | unpadded fields; short years; `+/-HH` and signless offsets; digit-swallow lexer quirk; `24:00` remains a construction error exactly as in the baseline runtime | 6.3 |
| `compat.legacy-escapes` | backslash passthrough; delimiter-only escapes; the defective duplicating continuation | 7.2 |
| `compat.legacy-binary` | the open-pipe form: `=`-padding-run termination, no closing pipe, <= U+0020 skipped -- with the dumper's unpadded/empty outputs remaining unreadable, as in 0.9 | 8 |
| `compat.hash-underscore-comment` | `#_` opens a comment, not a discard | 3.9 |
| `compat.last-wins` | silent duplicate-key overwrite | 12.1 |
| `compat.set-dedup` | silent duplicate-set-element dedup | 12.2 |
| `compat.undefined-ref-sentinel` | unknown `*label` yields the undefined sentinel | 10.2 |
| `compat.python-name-classes` | raw `isalpha`/`isalnum` name classes | 9.1 |
| `compat.lax-strings` | unescaped C0 controls; newlines in quoted names | 7.7, 9.2 |

### 15.4 Dissolved profiles

Three profiles from the research roadmap dissolved during specification. **Set** and **Stream**: ground truth showed both are baseline core (Section 2.6, Section 11), so there is nothing to gate. **Columnar**: the research-era `#[cols]` sketch is **rejected** -- any `#`-prefixed opener silently reads as a *comment* in every legacy and core parser, which is silent data corruption, and TOON-style newline-framed rows would violate the no-newline-semantics rule (Section 1.2). The token-efficiency goal is met instead by the `grid{cols rows}` well-known tag (Section 2.7): arity-framed rows, zero new syntax, graceful degradation. Likewise "Human Config" from the research draft merged into Core (comments, multiline strings, optional commas are already core) plus `compat` for the rest.

### 15.5 Composition

Profiles compose freely except where the registry says otherwise (`scale-significant` x `canonical`). Enabling a profile never changes the meaning of a document that does not use its constructs -- the property that makes `require` lists honest.

---

## 16. Parser Resource Limits

### 16.1 Principle

Hostile input is normal input. A conforming parser **MUST** enforce limits in every category below, **MUST** document its defaults, and **SHOULD** make them configurable. Exceeding any limit raises `resource-limit-exceeded` (Section 17), deterministically (same input + same configuration => same outcome), before unbounded work occurs. The unbounded-recursion DoS was a real audit finding in `serde_axon` (M1); these limits are spec-level so no implementation re-learns it.

### 16.2 Categories and recommended defaults

| Category | Recommended default |
|---|---|
| Nesting depth (containers + node bodies + anchored values, combined) | 128 |
| Total input size / stream value count | caller-configured |
| String / quoted-name length | 16 MiB / 4 KiB |
| Bare-name, label, namespace-segment length | 1 KiB |
| Numeric-literal length (incl. separators) | 1 KiB |
| Temporal-literal length (incl. zone suffix) | 128 B |
| Decoded binary length | 16 MiB |
| Entries per map/ordered-map/set; attributes + children per node | 1 048 576 |
| Anchor count / reference count per stream (`graph`) | 65 536 each |
| Header `require` list length | 64 |
| Lossless-CST token count | 1 048 576 |

Discarded values (`#_`) count fully against every category (Section 3.9).

### 16.3 Hardening notes

References alias -- they cannot amplify memory (Section 10.4), so no expansion limit is needed beyond counts. Base64 whitespace-skipping is bounded by input length. **[2026]** Any interning or caching (the baseline interns names unboundedly via its name cache) **MUST** be size-bounded. Limits apply identically in canonical-verify mode, before verification.

---

## 17. Errors

### 17.1 Model

Every error carries: a stable **category id** (kebab-case, listed below -- ids are API and safe to match in tests; message text is not); a primary location representable as a zero-length or ranged **span** (0-based byte offsets; 1-based line; 1-based column counted in Unicode scalar values); a human message; optionally an expected-token set, recovery hint, and secondary span. Semantic parsing is **fail-fast** (first error terminates). A `cst` parser **MAY** recover and collect multiple diagnostics for tooling, provided each carries its own span. Errors involving two sites **SHOULD** carry a secondary span for the earlier site when the processing layer retains it.

### 17.2 Category registry

| Id | Raised by | Section |
|---|---|---|
| `invalid-unicode` | non-UTF-8 input | 1.1 |
| `unexpected-token` | structure violations: mixing after recognition, pair after child, comma abuse, stray input, constants with bodies | 3, 4, 11 |
| `unexpected-end` | EOF inside a value, container, string, or binary | 3, 7, 8, 11 |
| `invalid-number` | leading zeros, `+`, point forms, separator misuse, float overflow (strict) | 5 |
| `invalid-temporal` | failed commit after digits+`-`/`:`, calendar/clock violations, offset range, zone mismatch | 5.1, 6 |
| `invalid-escape` | unknown escape, surrogate `\u{...}` | 7.3 |
| `invalid-name` | newline in a quoted name (Core), bad namespace shape | 9.2, 9.3 |
| `invalid-binary` | bad alphabet, impossible length | 8 |
| `invalid-link` | malformed CID or non-absolute document-link URI | 10.5 |
| `duplicate-key` / `duplicate-set-element` / `duplicate-anchor` | Section 12.1 / Section 12.2 / Section 10.3 | -- |
| `unknown-reference` / `unknown-constant` | unresolved `*label` / `$name` | 10.2, 9.4 |
| `unsupported-profile-feature` | profile-gated construct without its profile; unsatisfiable `require` | 15.1, 11.4 |
| `resource-limit-exceeded` | Section 16 | -- |

`semantic-construction` errors (binding a parsed value to an application type -- e.g., an `Int` outside a bounded target, Section 5.2) are the binding layer's, deliberately outside this registry.

---

## 18. AXON Schema (Companion)

Core AXON answers *"what value does this text denote?"*; **AXON Schema** answers *"is this value valid for this interface?"*. It is a separate companion layer operating on semantic values and never executing code. The dialect identifier is `urn:axonnext:schema:2026`. The Python reference accepts mapping schemas and `schema{... field{...}}` documents and implements the listed constraints with structured paths. `$ref` supports local JSON-Pointer-style fragments and exact external identifiers supplied through an explicit registry; resolution never performs network access.

A schema-module identifier **MUST** be a non-empty absolute ASCII URI without a fragment. A registry **MUST** reject duplicate identifiers unless replacement is explicitly requested, **MUST** impose module/depth/node limits, and **MUST NOT** fetch or discover a missing module. A registry may be frozen; after freezing, mutation **MUST** fail, and accessors **MUST NOT** expose mutable aliases that can change registered modules. If a module declares `$id`, it **MUST** equal the identifier under which the module is registered.

Schema keyword shapes are governed: `properties` and `$defs` are string-keyed mappings of schemas; `prefix_items` is a sequence of schemas; `items`, `children`, and `attributes` are schemas; `required` is a duplicate-free string sequence; `kind` is a registered kind or non-empty sequence of registered kinds; length/count bounds are non-negative integers; `enum` is a sequence or set; and `pattern`, `key_pattern`, `$id`, and `$ref` are strings. Invalid shapes report `invalid-schema`, never raw host-language exceptions. JSON-Pointer-style indexes are non-negative decimal indexes within the target sequence; invalid escapes and out-of-range indexes report `unresolved-ref`.

Where `pattern` constrains a string *value*, **`key_pattern` constrains a mapping's keys**: every key of the validated mapping **MUST** match it, and a key that is not a string, or that does not match, reports `key-pattern`. It applies only to mappings and is ignored for every other kind, so a union schema may carry it alongside string keywords. It composes with `properties`, `required`, and `additional` — key matching is evaluated first, so a mapping whose keys are wrong reports `key-pattern` rather than only the downstream value issues. `key_pattern` uses the same governed regular-expression subset as `pattern`, and an unsafe or invalid expression reports `key-pattern` once rather than per key. It exists because key-shaped contracts — language-tag maps, publisher-namespaced extension maps, identifier-keyed resource maps — cannot otherwise be expressed, and pushing them into a host-language layer puts them outside the schema where implementations disagree.

Regular-expression matching uses a deliberately conservative linear-time subset. Nested quantified expressions, quantified alternation/lookaround/backreferences, multiple variable quantifiers on one accepted pattern, and unanchored variable quantifiers are rejected as `pattern` issues. This restriction is security-governed; implementations may use a proven linear-time regex engine to accept more spellings without changing successful-match semantics.

Validation diagnostics are aggregated deterministically by data path, then code, schema path, and message; exact duplicates are removed. Implementations **MUST** provide a configurable positive issue limit and report `issue-limit-exceeded` when additional issues are suppressed. Equivalent mappings with different insertion order therefore produce the same ordered diagnostic report.

**Registry governance** lives with this companion: additions to the well-known tags (Section 2.7), profile ids and `compat.*` flags (Section 15), and error categories (Section 17.2) require a spec revision; applications extend vocabulary through namespaced names (`ns/name`, Section 9.3), never by squatting on unregistered bare tags they hope stay unclaimed.

---

## 19. Backwards Compatibility and Migration

### 19.1 Reading legacy documents

The `compat` bundle (Section 15.3) is the normative legacy reader -- every flag reproduces a documented, ground-truthed pyaxon behaviour. Reading legacy is therefore lossless by construction; the interesting direction is the other two.

### 19.2 Writing for legacy consumers

A writer targeting baseline pyaxon **MUST** restrict itself to the `[BASELINE]` surface. Per-construct strategy:

| 2026 construct | Legacy strategy |
|---|---|
| `Name(...)` / `Name[...]` bodies (Section 4.5) | rewrite to `Name{...}` per schema, or refuse |
| `Z` (Section 6.7) | emit `+00:00` (semantic marker lost -- flag if `Z`-vs-offset distinction matters) |
| Escapes `\\ \n \r \t \u{...}` and raw strings (Section 7) | emit characters literally (strings are multiline, so LF/HT embed directly); a backslash that a legacy reader would treat as an escape or continuation (before the delimiter or a newline) has no legacy spelling in that position -- restructure or refuse |
| U+000D in a string value (Section 7.4) | **unrepresentable** (normalisation) -- refuse |
| Temporals | emit `^...` -- the 0.9 canonical spelling `dumps` itself uses; pre-0.9 readers are out of scope. Fractions: emit **exactly six** digits (the defect-masking pad, Section 6.3); digits 7-9 -> refuse, or truncate only under an explicit lossy flag |
| Bytes with `len % 3 == 0`, and empty bytes (Section 8.2) | **unrepresentable** in the open form -- refuse |
| Numeric separators (Section 5.6) | strip |
| `ns/name` (Section 9.3) | **not** degrade-safe (`/` after a name is a legacy error) -- rename or refuse |
| `cid(...)`, `link(...)`, `duration(...)`, `axon{...}`, `grid{...}`, `uuid(...)`, `instant(...)` | **degrade-safe** -- ordinary nodes to legacy readers, by design (Section 2.7) |
| Cycles (Section 10.3) | unrepresentable -- refuse; shared acyclic values are fine (`crossref`) |
| `#_` discards (Section 3.9) | never emit; drop the discard and its operand |
| Sets, `∅`, `[:]`, `$`/`d` decimals, backtick strings, `&`/`*` | baseline -- emit freely |

### 19.3 Migrating legacy documents to Core

Migrations are CST transforms (Section 13.4) with the contract *same value, Core spelling*. A conforming migrator applies, and reports each application of:

1. `#_` at a comment opener -> `# _` (the one place legacy and 2026 lexing diverge on identical bytes, Section 3.9);
2. indented node bodies -> brace bodies (Section 4.6);
3. **bare temporals -> `^...`** -- the author's own `dumps(loads(text))` recipe (changelog 0.9); then padding: `^2012-1-5` -> `^2012-01-05`, `+/-HH` -> `+/-HH:00`; a `24:00` offset or a digit-swallowed literal (`20261-...`) -> error, manual fix;
4. `007` -> `7`; `5.` -> `5.0`;
5. string escapes: every backslash not followed by the active delimiter or a newline -> `\\`; unescaped C0 controls (other than HT/LF) -> `\u{...}`; a backslash-newline continuation is decoded with the documented baseline duplication behaviour and then emitted with Core escapes, with a warning because the likely author intent is ambiguous;
6. duplicates: keep-last **with a warning** (or fail under `--strict`); duplicate set elements: drop with a warning;
7. unknown `*label` (the sentinel cases) -> error, manual fix;
8. quoted names containing literal newlines -> escape those scalar values as `\n` / `\r` in the Core quoted-name spelling and report the rewrite;
9. temporal fractions: pad to **exactly six digits before** positional reinterpretation -- at six digits the baseline's literal-us reading and Core's positional reading coincide, so the rewrite is value-preserving (Section 6.3); binary: add the closing `|` (Section 8).

Everything else -- `d` decimal suffixes, backtick strings, key spellings -- is already Core-legal; rewriting those is *style* (the canonical writer), not migration. An ordinary `$` numeric suffix is invalid and therefore cannot be migrated as a legal value.

A conforming migrator exposes both a deterministic canonical rewrite and a selective rewrite. Selective migration preserves every unaffected source byte and returns non-overlapping UTF-8 byte edits. It **MUST** compare the selectively rewritten document's canonical semantic result with compatibility parsing of the original; if equivalence cannot be proven, it **MUST** fail or fall back explicitly to the canonical rewrite. Silent semantic drift is never permitted.

### 19.4 Edition detection

New documents **SHOULD** open with the header (Section 11.4). Absent one, a reader's policy is: parse as Core; on failure, retry under `compat` and surface the Section 19.3 diagnostics. There is no content sniffing beyond that -- the grammars are close enough that guessing is worse than the two-pass rule.

### 19.5 Stability promise

Future editions extend AXON only through profile ids, `compat.*` flags, well-known tags, and new error categories under Section 18 governance -- never by silently redefining a construct. That is the same promise this document made to pyaxon in its front matter, kept forward.

---

## Appendix A -- Collected Formal Grammar

Normative consolidation of the per-section fragments (which remain the authorities for semantics and constraints; bracketed notes cite them). Terminals: `XID_Start`/`XID_Continue` per UAX #31; `ws_char` per Section 1.2; all literals are Unicode scalars in UTF-8 source (Section 1.1).

```ebnf
(* ------ documents ------ *)
document        = ws , [ attribute_document | value_stream ] , ws ;         (* Section 11.1 *)
value_stream    = stream_item , { ws , stream_item } ;
stream_item     = { "#_" , ws } , value ;                                   (* discard [2026] Section 3.9 *)
attribute_document                                                          (* iff first construct is a pair *)
                = doc_pair , { ws , doc_pair } ;
doc_pair        = { "#_" , ws } , pair ;

(* ------ values ------ *)
value           = constant | scalar | container | node
                | anchored | reference | const_ref ;
constant        = "null" | "true" | "false" ;                               (* Section 2.3 *)
scalar          = temporal | decimal | float | integer | special
                | string | raw_string | binary ;
container       = list | ordered_map | map | set | tuple | "∅" ;            (* ∅ = empty set Section 2.6 *)

list            = "[" , ws , [ seq ] , ws , "]" ;                           (* first item not key":" -- Section 3.6.1 *)
ordered_map     = "[" , ws , ( ":" | pairs ) , ws , "]" ;                   (* [:] = empty Section 3.6.2 *)
map             = "{" , ws , [ pairs ] , ws , "}" ;                         (* {} = empty map Section 3.5 *)
set             = "{" , ws , seq , ws , "}" ;                               (* first item not key":" -- Section 3.5 *)
tuple           = "(" , ws , [ seq ] , ws , ")" ;

seq             = seq_item , { sep , seq_item } , [ sep ] ;
seq_item        = { "#_" , ws } , value ;
pairs           = pair_item , { sep , pair_item } , [ sep ] ;
pair_item       = { "#_" , ws } , pair ;
pair            = map_key , ws , ":" , ws , value ;
map_key         = name | string ;                                           (* Section 3.7, Section 9.6 *)
sep             = ws , [ "," , ws ] ;                                       (* Section 1.3 *)

(* ------ nodes ------ *)
node            = node_name , [ hs , node_body ] ;                          (* same-line binding Section 4.3 *)
node_name       = ns_name | name | quoted_name ;                            (* Section 4.8 *)
ns_name         = name , "/" , name ;                                       (* [2026] Section 9.3 *)
node_body       = brace_body | node_tuple | node_list ;
brace_body      = "{" , ws , [ attrs ] , [ children ] , ws , "}" ;          (* attrs before children Section 4.4 *)
attrs           = attr , { sep , attr } ;
attr            = attr_key , ws , ":" , ws , value ;
attr_key        = name | quoted_name ;                                      (* Section 4.4.2 *)
children        = [ sep ] , seq ;
node_tuple      = "(" , ws , [ seq ] , ws , ")" ;                           (* [2026] Section 4.5 *)
node_list       = "[" , ws , [ seq ] , ws , "]" ;                           (* [2026] Section 4.5 *)

(* ------ graph / constants ------ *)
anchored        = "&" , label , ws , value ;                                (* graph Section 10 *)
reference       = "*" , label ;
label           = xid_cont_char , { xid_cont_char } ;                       (* Section 10.1 *)
const_ref       = "$" , name ;                                              (* Section 9.4 *)

(* ------ numbers (constraints in Section 5) ------ *)
integer         = [ "-" ] , digits ;
float           = [ "-" ] , digits , ( frac_part , [ exp ] | exp ) ;
frac_part       = "." , digits ;
exp             = ( "e" | "E" ) , [ "+" | "-" ] , digits ;
decimal         = ( integer | float ) , dec_suffix ;
dec_suffix      = "d" | "D" ;                                               (* empirical Section 1.6.3 *)
special         = ( "∞" | "-∞" | "?" ) , [ "d" | "D" | "$" ] ;           (* `$` exists for specials only *)
digits          = digit , { digit | "_" } ;                                 (* "_" [2026] Section 5.6 *)

(* ------ temporals (authority Section 6.2) ------ *)
temporal        = "^" , ( datetime | date | time ) ;                          (* Core; bare is compatibility-only *)
date            = digit4 , "-" , digit2 , "-" , digit2 ;
time            = digit1_2 , ":" , digit2 ,
                  [ ":" , digit2 , [ "." , digit1_9 ] ] , [ offset ] ;
datetime        = date , "T" , time , [ zone_suffix ] ;
offset          = "Z"                                                        (* [2026] Section 6.7 *)
                | ( "+" | "-" ) , digit2 , ":" , digit2 ;
zone_suffix     = "[" , zone_name , "]" ;                                    (* tz-names profile Section 6.8 *)

(* ------ strings / binary ------ *)
string          = '"' , { str_elem_dq } , '"'
                | "`" , { str_elem_bt } , "`" ;                             (* both multiline Section 7.1 *)
raw_string      = "r" , hashes , '"' , raw_content , '"' , hashes ;          (* [2026] Section 7.5; matched # counts *)
escape          = "\\" , ( "\\" | '"' | "`" | "'" | "n" | "r" | "t"
                | "u{" , hex1_6 , "}" | newline ) ;                          (* [2026] table Section 7.3 *)
binary          = binary_2026 | binary_legacy ;
binary_2026     = "|" , { b64_char | ws_char } , "|" ;                       (* Core -- Section 8.1 *)
binary_legacy   = "|" , { b64_char | le_ws } , pad_run ;                     (* compat.legacy-binary; ends at the "="-run *)

(* ------ names / trivia ------ *)
name            = ( XID_Start | "_" ) , { XID_Continue | "_" } ;             (* Section 9.1 *)
quoted_name     = "'" , { qname_elem } , "'" ;                               (* string-lexed Section 9.2 *)
ws              = { ws_char | comment } ;
hs              = { " " | "\t" } ;
comment         = "#" , [ not_us_nl , { not_nl } ] ;                         (* "#_" is the discard, Section 3.9 *)
```

**Constraint index** (grammar-adjacent rules the EBNF does not encode): recognition of `[...]`/`{...}` by first complete atom, temporal atoms opaque to it (Section 3.5, Section 3.6.1); in Core, digits immediately followed by `-` or `:` are the bare-shape tripwire -- `invalid-temporal` with a migration hint -- and the full number->temporal commit lives only under `compat.bare-temporals` (Section 5.1, Section 6.5); a numeric literal may not be immediately followed by a name-start or digit (Section 5.9); no leading zeros, no `+`, point-form and separator placement (Section 5.2-5.6); exact temporal digit counts, positional fractions, validity, `24:00` and leap seconds forbidden, zone-suffix whitespace-free (Section 6.3-6.8); a name binds only to a **same-line opening delimiter**, otherwise it is a unit node, and attrs precede children (Section 4.3-4.4); reserved barewords never names (Section 2.3); key domains per context (Section 9.6); newline normalisation and Core control-character rules (Section 7.4, Section 7.7); Core binary is the closed form, the open form is `compat.legacy-binary` (Section 8); profile gating (Section 15); resource limits (Section 16).

## Appendix B -- Normative Conformance Registry

`conformance/normative_vectors.json` is the machine-readable index for this specification. Revision 5 contains 131 stable requirement identifiers and 125 executable vectors. Every indexed requirement references at least one vector or an explicitly named external corpus; all 16 semantic error categories have a direct vector. `conformance/normative_clause_index.json` freezes all 64 RFC-2119-bearing source lines by section and SHA-256 fingerprint, maps every such section to executable requirements or an explicit non-executable disposition, and makes unregistered normative edits fail the release gate.

`conformance/fuzz_seeds.json` fixes the deterministic generated-value, mutation, malformed-input, and graph-isomorphism seeds. These are conformance evidence rather than new syntax. Implementations may use additional fuzzing, but they **MUST** pass the checked-in corpus without changing expected categories or canonical bytes.

## Appendix C -- Internationalisation (informative)

This appendix is informative. It gathers how AXON 2026 handles international text
and how that compares with other data languages; the normative rules live in
Section 1.1 (encoding), Section 7 (strings and text), Section 9 (identifiers,
names, keys), and Section 14 (Canonical AXON).

How AXON handles it:

- Encoding. Source text is well-formed UTF-8 -- a sequence of Unicode scalar
  values -- and input that is not well-formed UTF-8 is rejected as an
  `invalid-unicode` error (Section 1.1). There is no encoding guessing and no
  silent replacement character.
- Identifiers. Bare names and keys follow the Unicode identifier rules of UAX #31
  (`XID_Start` / `XID_Continue`), so keys in Latin-accented, Greek, Cyrillic,
  Arabic, or CJK scripts are legal unquoted (Section 9). ASCII-only naming is a
  portability lint, not a grammar limit.
- Strings. String values are Unicode scalar sequences written directly as UTF-8.
  The sole Unicode escape is the braced `\u{...}` form, which rejects surrogate
  code points (Section 7); there is no 16-bit `\uXXXX` form and no surrogate-pair
  escape, so the surrogate-pairing errors common to other formats cannot arise.
- Determinism. Canonical AXON orders map keys by their UTF-8-encoded bytes
  (Section 14), so a document with international keys has one stable byte
  sequence, and therefore one stable content identifier, across implementations.
- Temporals. Local, offset, and `Z` date-times are distinct values, and an offset
  is never rewritten to UTC (Section 6), so civil time is preserved across
  regions.
- Numbers. Numbers use `.` as the decimal point with no digit grouping, so a
  document reads identically in every locale, and exact decimals avoid
  locale-formatted rounding.
- Normalisation. Canonical AXON does not apply Unicode normalisation (NFC) to
  string values -- it does not alter data -- so the same text in two Unicode
  compositions is two distinct values with two distinct content identifiers
  (Sections 7.8, 9.5). Bare-key emissions are NFC-normalised on output, and a
  lint warns about confusable compositions. A producer that needs cross-source
  identity normalises to NFC before encoding.

Compared with other formats:

- JSON has no bare keys, escapes non-ASCII with 16-bit `\uXXXX` plus surrogate
  pairs, and defines no canonical form in its base standard (JCS / RFC 8785 is a
  separate specification).
- YAML is Unicode-capable but carries implicit-typing ambiguity and has no
  widely-used canonical form.
- TOML mandates UTF-8 but restricts bare keys to ASCII; non-ASCII keys are quoted.
- XML permits Unicode element names and defines Canonical XML (C14N), though both
  the format and the canonicaliser are heavy.
- CBOR carries UTF-8 text natively, and its deterministic profile (dCBOR) is the
  basis of Binary AXON's canonical form (see the Binary AXON companion).

A capability-by-capability comparison across data languages is maintained in
`comparison/axon_2026_comparison.html`.

---

*AXON 2026 final specification, revision 5 -- aligned with the pyaxon 0.11.0a12 Python reference implementation. Rust implementations consume this frozen specification and its language-neutral conformance corpus.*
