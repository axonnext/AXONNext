# Formal Grammar and Parser Generation

**Status:** Proposed Ecosystem Roadmap for AXON 2026
**Inspiration:** LLLPG (Loyc LL(k) Parser Generator), ANTLR

## Motivation
Currently, parsers for AXON 2026 (such as the reference `pyaxon` Python library and the `serde_axon` Rust crate) use hand-written recursive-descent parsers. While hand-written parsers allow for highly optimised, zero-allocation memory management and custom error recovery, they are laborious to write and hard to maintain across dozens of programming languages.

To spur widespread adoption, the AXON ecosystem needs a formal machine-readable grammar that allows developers to auto-generate parsers in C++, Java, Go, TypeScript, and others, using modern tools like LLLPG or ANTLR4.

## The Strategy

### 1. Define an EBNF / ANTLR4 Grammar File
The first step is to codify the AXON 2026 spec into a strict `axon.g4` (ANTLR) or `.ecs` (LLLPG) grammar file. 

This grammar must explicitly model:
- The context-sensitive "no adjacency" rules for numeric literals (e.g., `5x` is invalid, `5` and `x` must be separated).
- Maximal-munch semantics for temporal caret values (`^2026-07-15`).
- The four node styles (Unit, Brace, Tuple, List) and their precise nesting rules.
- Multiline backtick string parsing rules.

### 2. AST Generation Hooks
A generated parser is only as good as the tree it builds. The grammar must define hooks that map directly to the `Value` structures defined in the reference implementations (e.g., `Value::Node`, `Value::Map`, `Value::Decimal`).

### 3. Distributing the Tooling
Once the grammar is finalised and published under the `01_specs` directory:
- We will generate a base JavaScript/TypeScript parser as a proof-of-concept.
- Encourage community adoption for JVM (Java/Kotlin/GSON bindings) and Go.
- Maintain the hand-written parsers for primary environments (Rust `no_std`, C-extensions in Python) where extreme performance boundaries exist, but rely on generated parsers for rapid ecosystem expansion.
