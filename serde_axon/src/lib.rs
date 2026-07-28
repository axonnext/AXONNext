//! # serde_axon
//!
//! [Serde](https://serde.rs) support for **AXON 2026** -- a text data format that
//! evolves the original AXON object notation with exact decimals, native
//! temporals, native binary, sets, tagged nodes, references, a canonical form,
//! and (via the companion Binary AXON encoding) a compact binary representation.
//!
//! This crate provides:
//!
//! * a hand-written recursive-descent [`parser`] for AXON 2026 Core text,
//!   producing the [`Value`] model;
//! * a [`writer`] that renders [`Value`] back to compact or canonical AXON;
//! * a lossless [`cst`] for editing, diagnostics, migration, and formatting;
//! * full canonical/selective [`migration`] and transport-neutral [`lsp`]
//!   helpers over that CST;
//! * a [`binary`] codec for compact and canonical Binary AXON documents;
//! * [`schema`] normalisation, governed local registries, and structured AXON
//!   Schema 2026 validation;
//! * checked [`scientific`] arrays and fail-stop [`stream`] envelopes;
//! * a Serde [`Serializer`](ser) and [`Deserializer`](de) so any type deriving
//!   `Serialize` / `Deserialize` round-trips through AXON.
//!
//! ```
//! use serde::{Serialize, Deserialize};
//! # #[cfg(feature = "std")] {
//! #[derive(Serialize, Deserialize, PartialEq, Debug)]
//! struct Point { x: i32, y: i32 }
//!
//! let p = Point { x: 1, y: -2 };
//! let text = serde_axon::to_string(&p).unwrap();      // {x:1 y:-2}
//! let back: Point = serde_axon::from_str(&text).unwrap();
//! assert_eq!(p, back);
//! # }
//! ```
//!
//! ## `no_std`
//!
//! The text and binary codecs, value model, Serde adapters, lossless CST,
//! migration/LSP helpers, schema validator, scientific arrays, and stream
//! envelopes are `no_std` and require only `alloc`. The default `std` feature
//! adds `std::error::Error` for [`Error`] and std-dependent conveniences. Build
//! with `default-features = false, features = ["alloc"]` for embedded / WASM
//! targets.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

pub mod binary;
pub mod cid;
pub mod cst;
pub mod de;
pub mod error;
pub mod graph;
pub mod lsp;
pub mod migration;
pub mod parser;
pub mod regex;
pub mod schema;
pub mod ser;
pub mod value;
pub mod writer;

#[doc(inline)]
pub use crate::error::{Category, Error, Result};
#[doc(inline)]
pub use crate::graph::{
    GraphDocument, GraphId, GraphNode, GraphObject, GraphValue, from_str_graph,
    from_str_graph_with, resolve_graph, resolve_graph_stream,
};
#[doc(inline)]
pub use crate::parser::{Options, tzdb_version};
#[doc(inline)]
pub use crate::value::{
    Anchor, Date, DateTime, Decimal, Link, Node, NodeStyle, Temporal, Time, Value, Zone,
};
#[doc(inline)]
pub use crate::writer::WriteOptions;

// Content identifiers and canonical verification.
#[doc(inline)]
pub use crate::cid::{cid, parse_cid, verify_canonical, verify_cid};
// Lossless CST, diagnostics, edits, migration, and formatting.
#[doc(inline)]
pub use crate::cst::{
    CstDiagnostic, CstDocument, CstElement, CstFix, CstNode, CstNodeId, CstSemanticTrace,
    CstSemanticValue, CstToken, CstTokenId, Span, format_cst, migrate_indented, parse_cst,
};
// Transport-neutral editor/LSP helpers.
#[doc(inline)]
pub use crate::lsp::{
    DocumentSymbol, LspDiagnostic, LspFix, LspHover, LspTextEdit, SymbolPathSegment, diagnostics,
    document_symbols, formatting_edits, hover,
};
// Legacy-to-Core migration and byte-addressed selective edits.
#[doc(inline)]
pub use crate::migration::{
    MigrationEdit, MigrationEvent, MigrationMode, MigrationResult, apply_migration_edits, migrate,
    migrate_selective, migrate_selective_with_graph, migrate_with, migrate_with_graph,
};
// Schema normalisation, registry, validation, and diagnostics.
#[doc(inline)]
pub use crate::schema::{
    PathSegment, SCHEMA_DIALECT_2026, SchemaError, SchemaErrorKind, SchemaRegistry2026,
    SchemaValidationError, ValidationIssue, ValidationOptions, ValidationReport, assert_valid2026,
    load_schema2026, normalize_schema, normalize_schema_with, validate2026, validate2026_with,
    validation_report2026, validation_report2026_with,
};
// Serialisation entry points.
#[doc(inline)]
pub use crate::ser::{to_string, to_string_canonical, to_value};
// Deserialisation entry points.
#[doc(inline)]
pub use crate::de::{
    StreamDeserializer, from_slice, from_slice_stream, from_slice_stream_with, from_slice_value,
    from_slice_value_with, from_slice_with, from_slice_with_constants, from_str, from_str_stream,
    from_str_stream_with, from_str_stream_with_constants, from_str_value, from_str_value_with,
    from_str_value_with_constants, from_str_with, from_str_with_constants, iter_stream,
    iter_stream_with, iter_stream_with_constants,
};

/// AXON scientific notation format parser and writer components.
pub mod scientific;
/// Stream processing elements for reading AXON sequentially.
pub mod stream;
