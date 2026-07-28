# API Reference

The `axon` package exposes two layers: the **historical API** (unchanged from the
original `pyaxon`) and the **AXON 2026 edition API** (the `*2026` family). Import
either from the top-level package:

```python
import axon2
from axon2 import loads2026, dumps2026, canonical2026, Options
```

See the [guide](guide.md) for the language itself.

---

## Historical API

The original interface, preserved unchanged for backwards compatibility.

### Loading

```python
axon2.loads(text, mode='safe', errto=None)
axon2.load(fd, mode='safe', errto=None, encoding='utf-8')
axon2.iloads(text, mode='safe', errto=None)          # iterator
axon2.iload(fd, mode='safe', errto=None, encoding='utf-8')   # iterator
```

Load values from text (`loads`/`iloads`) or a file/path (`load`/`iload`). `mode`
selects the object builder:

- `'safe'` -- build only built-in value kinds (default);
- `'strict'` -- use registered factories; unknown names become `undef`;
- `'mixed'` -- registered factories where available, else `'safe'`.

The `i*` variants yield values one at a time.

### Dumping

```python
axon2.dumps(vals, pretty=0, braces=0, sorted=0, hsize=0, crossref=0)
axon2.dump(fpath, val, pretty=0, braces=0, sorted=0, hsize=0, crossref=0, encoding='utf-8')
axon2.display(text, pretty=1, braces=0, sorted=0, hsize=0, crossref=0)
```

Serialise values to text (`dumps`), to a file (`dump`, written atomically), or
pretty-print reformatted AXON (`display`). Flags:

- `pretty` -- indented (YAML/Python) form instead of compact;
- `braces` -- use `{}` braces (JSON/C style) in the pretty form;
- `sorted` -- sort mapping keys;
- `crossref` -- emit references to shared objects instead of copies.

### Classes

- **`Loader`** -- the streaming reader behind `load`/`loads`.
- **`Dumper`** -- the writer behind `dump`/`dumps`.
- **`Node`** -- a named/tagged value: a `name`, optional attributes, and children.

### Factories (unsafe/strict mode)

```python
axon2.node(name, attrs=None, vals=None)   # construct a Node
axon2.reduce(type_)                        # register a value -> AXON reducer
axon2.factory(name)                        # register a name -> object builder
```

---

## AXON 2026 edition API

The 2026 engine. `Options` selects conformance profiles (Core, Graph, Doc-Link,
`tz-names`) and the `compat` legacy-reader bundle; the `*2026` functions mirror
the historical `load`/`dump` family.

### `Options`

```python
from axon2 import Options

Options()            # Core profile, strict, all safety on
Options(graph=True)  # enable the Graph profile
Options(doc_link=True, tz_names=True)
Options.compat()     # the documented legacy-pyaxon reader bundle
```

`Options` carries the profile switches, the `compat.*` legacy flags, and the
Section 16 resource limits (max depth, string/binary length, container counts, anchor/
reference counts, ...). Defaults are Core with every profile off and every safety
check on.

### Loading and dumping

```python
axon2.loads2026(text, *, options=None, constants=None)
axon2.load2026(source, *, options=None, constants=None, encoding='utf-8')
axon2.iloads2026(text, *, options=None, constants=None)          # iterator
axon2.iload2026(source, *, options=None, constants=None, encoding='utf-8')  # iterator
axon2.dumps2026(values, *, canonical=False, graph=False)
axon2.dump2026(path, values, *, canonical=False, graph=False, encoding='utf-8')
```

Parse (`loads2026`/`load2026`) or serialise (`dumps2026`/`dump2026`) under the
2026 semantics. Pass `canonical=True` to emit Canonical AXON; `graph=True` to
allow anchors/references in output. Files are written atomically.

### Canonical form and content identifiers

```python
axon2.canonical2026(value)            # -> bytes: the Section 14 canonical encoding
axon2.verify_canonical2026(text)      # -> bool: is this text already canonical?
axon2.cid2026(value)                  # -> str: CIDv1(raw, sha2-256) over canonical bytes
axon2.parse_cid2026(text)            # -> bytes: validate + decode a CID
axon2.verify_cid2026(cid, value)     # -> bool: does the CID match the value?
```

Canonical AXON is deterministic, so `cid2026` gives a stable content identifier:
the same value produces the same CID in any conforming implementation.

### Migration

```python
axon2.migrate2026(text, *, graph=False, selective=False)   # legacy text -> canonical 2026
axon2.migrate_selective2026(text, *, graph=False)          # selective variant
```

`migrate2026` reads a document with the `compat` bundle and re-emits it as
canonical 2026, self-checking semantic equivalence. The selective variant
(`selective=True`, or `migrate_selective2026`) rewrites through the lossless CST,
preserving surrounding comments and formatting rather than fully re-canonicalising.
Both return a string-compatible `MigrationResult` with structured `events`,
UTF-8 byte-addressed `edits`, and a `mode` of `canonical`, `selective`, or
`canonical-fallback`. `apply_migration_edits2026(source, result.edits)`
reproduces every selective result.

---

## Companion modules

Beyond the top-level API, the reference ships:

- **`axon2.cst2026`** -- a lossless concrete syntax tree (comments, whitespace,
  and formatting preserved) with byte-addressed repairs.
- **`axon2.schema2026`** -- the AXON Schema validator (kind, range, length,
  pattern, enumeration, required/additional properties, and more).
- **`axon2.binary2026`** -- the compact **Binary AXON** codec (canonical binary
  form).
- **`axon2.lsp2026`** -- transport-neutral `diagnostics2026`, `hover2026`,
  `formatting_edits2026`, and `document_symbols2026` helpers. Byte ranges are
  UTF-8; line/character positions are zero-based Unicode-scalar coordinates.
- **`axon2.scientific`** -- NumPy conversion for
  `Array{shape type data order}` nodes.
- **`axon2.stream`** -- fail-stop `StreamStart`/`StreamEnd` envelope validation
  with record-count checking.

---

## Exceptions

- **`Axon2026Error`** -- raised by the 2026 engine; carries a stable error
  category (see the specification, Section 17) and source position.
- **`AxonError`** -- raised by the historical loader/dumper.
