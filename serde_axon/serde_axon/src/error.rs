//! Errors and error categories.
//!
//! The [`Category`] set mirrors the AXON 2026 reference implementation
//! (pyaxon), so a document that parses (or fails) one way there fails the same
//! way here -- the conformance vectors in `tests/` assert on these category
//! names.

use alloc::fmt;
use alloc::string::{String, ToString};
use core::fmt::Display;

/// A stable category for a parse or serialization failure, matching the AXON
/// 2026 error taxonomy.
///
/// The variants are exactly the §17.2 category registry, plus `InvalidLink`
/// (which the reference implementation emits for a malformed `cid(..)` /
/// `link(..)` target under the Document Link profile, though §17.2 does not yet
/// register it) and `Serde` (this crate's binding layer, which §17.2 explicitly
/// places outside the registry as `semantic-construction`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    /// Non-UTF-8 input (§1.1).
    ///
    /// This crate parses `&str`, so the UTF-8 decode happens in the caller and
    /// this category is never raised here; it exists so the registry is
    /// complete and callers decoding bytes can classify their own failures.
    InvalidUnicode,
    /// End of input inside a value, container, string, or binary literal.
    UnexpectedEnd,
    /// A structural violation: mixing after recognition, a pair after a child,
    /// comma abuse, stray input, a constant with a body, or a character that
    /// cannot begin any value.
    UnexpectedToken,
    /// A malformed number (leading zero, `+`, bad point/separator placement).
    InvalidNumber,
    /// A malformed temporal, or a bare temporal outside the `^` form.
    InvalidTemporal,
    /// An unknown or ill-formed escape sequence, or a raw control character in
    /// a string.
    InvalidEscape,
    /// A newline in a quoted name, or a bad namespace shape (§9.2, §9.3).
    InvalidName,
    /// A binary literal with a bad alphabet or an impossible length.
    InvalidBinary,
    /// A `cid(..)` / `link(..)` whose target is not a valid CID / URI.
    InvalidLink,
    /// A repeated map or ordered-map key.
    DuplicateKey,
    /// A repeated set element.
    DuplicateSetElement,
    /// A repeated anchor label.
    DuplicateAnchor,
    /// A reference to an anchor that was never defined.
    UnknownReference,
    /// A `$name` constant that is not registered.
    UnknownConstant,
    /// A profile-gated construct used without its profile enabled.
    UnsupportedProfileFeature,
    /// A configured resource limit (depth, length, or count) was exceeded.
    ResourceLimitExceeded,
    /// A Serde (de)serialization mismatch, carrying a human message.
    Serde,
}

impl Category {
    /// The kebab-case name used in AXON tooling and the conformance vectors.
    pub fn as_str(self) -> &'static str {
        match self {
            Category::InvalidUnicode => "invalid-unicode",
            Category::UnexpectedEnd => "unexpected-end",
            Category::UnexpectedToken => "unexpected-token",
            Category::InvalidNumber => "invalid-number",
            Category::InvalidTemporal => "invalid-temporal",
            Category::InvalidEscape => "invalid-escape",
            Category::InvalidName => "invalid-name",
            Category::InvalidBinary => "invalid-binary",
            Category::InvalidLink => "invalid-link",
            Category::DuplicateKey => "duplicate-key",
            Category::DuplicateSetElement => "duplicate-set-element",
            Category::DuplicateAnchor => "duplicate-anchor",
            Category::UnknownReference => "unknown-reference",
            Category::UnknownConstant => "unknown-constant",
            Category::UnsupportedProfileFeature => "unsupported-profile-feature",
            Category::ResourceLimitExceeded => "resource-limit-exceeded",
            Category::Serde => "serde",
        }
    }
}

/// A parse or serialization error, with a category, message, and (for parse
/// errors) a 1-based source position.
#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    /// The stable, testable classification of this failure.
    pub category: Category,
    /// A human-readable description of the failure.
    pub message: String,
    /// 1-based line, or 0 if not applicable.
    pub line: u32,
    /// 1-based column, or 0 if not applicable.
    pub column: u32,
    /// UTF-8 byte offset where the failure begins, or 0 when not applicable.
    pub offset: usize,
    /// Alias for [`Self::offset`], matching the Python authority's diagnostic API.
    pub start_offset: usize,
    /// Exclusive UTF-8 byte offset where the primary failure span ends.
    pub end_offset: usize,
    /// Optional secondary UTF-8 byte span for diagnostics involving two sites.
    pub secondary_span: Option<(usize, usize)>,
}

impl Error {
    /// Build an error from its category, message, and 1-based line/column.
    ///
    /// Byte offsets default to 0; the parser fills them in through the
    /// span-aware constructors so a caller-built error is still well formed.
    /// The [`Category`] is the stable part of the contract -- match on it
    /// rather than on the message text.
    pub fn new(category: Category, message: impl Into<String>, line: u32, column: u32) -> Self {
        Error {
            category,
            message: message.into(),
            line,
            column,
            offset: 0,
            start_offset: 0,
            end_offset: 0,
            secondary_span: None,
        }
    }

    /// Construct an error with an exact UTF-8 byte span.
    pub fn new_with_span(
        category: Category,
        message: impl Into<String>,
        offset: usize,
        end_offset: usize,
        line: u32,
        column: u32,
    ) -> Self {
        Error {
            category,
            message: message.into(),
            line,
            column,
            offset,
            start_offset: offset,
            end_offset,
            secondary_span: None,
        }
    }

    /// Attach a secondary UTF-8 byte span and return the updated error.
    pub fn with_secondary_span(mut self, start: usize, end: usize) -> Self {
        self.secondary_span = Some((start, end));
        self
    }

    /// The error category (the stable, testable classification).
    pub fn category(&self) -> Category {
        self.category
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line != 0 {
            write!(
                f,
                "{} at {}:{}: {}",
                self.category.as_str(),
                self.line,
                self.column,
                self.message
            )
        } else {
            write!(f, "{}: {}", self.category.as_str(), self.message)
        }
    }
}

impl core::error::Error for Error {}

impl serde::de::Error for Error {
    fn custom<T: Display>(msg: T) -> Self {
        Error::new(Category::Serde, msg.to_string(), 0, 0)
    }
}

impl serde::ser::Error for Error {
    fn custom<T: Display>(msg: T) -> Self {
        Error::new(Category::Serde, msg.to_string(), 0, 0)
    }
}

/// Convenience alias.
pub type Result<T> = core::result::Result<T, Error>;
