//! A hand-written recursive-descent parser for AXON 2026 Core text, producing
//! [`Value`]. Grammar-generator crates fight AXON's context (maximal-munch
//! numbers, the numeric no-adjacency rule, caret temporals), so the parser is
//! written directly to strictly standardize the ingestion pipeline. Error
//! categories match the reference implementation precisely, providing a
//! rigorous and fail-safe parsing architecture.

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use chrono::{NaiveDate, Offset as _, TimeZone as _};
use chrono_tz::Tz;

use crate::error::{Category, Error, Result};
use crate::value::{Date, DateTime, Decimal, Link, Node, NodeStyle, Temporal, Time, Value, Zone};

/// Resource limits applied during parsing (AXON §16). Every field and default
/// matches the reference implementation's `Options` one-for-one.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Maximum container / node-body nesting depth (default 128).
    pub max_depth: u32,
    /// Maximum number of values in a single document (default 1048576).
    pub max_values: usize,
    /// Maximum decoded string length, in bytes (default 16 MiB).
    pub max_string: usize,
    /// Maximum decoded binary length, in bytes (default 16 MiB).
    pub max_binary: usize,
    /// Maximum number of characters in a numeric literal (default 1024).
    pub max_number: usize,
    /// Maximum number of anchors in a document (default 65536).
    pub max_anchors: usize,
    /// Maximum number of references in a document (default 65536).
    pub max_references: usize,
    /// Maximum anchor/reference label length, in bytes (default 1024).
    pub max_label: usize,
    /// Maximum bare-name segment length, in bytes (default 1024).
    pub max_bare_name: usize,
    /// Maximum quoted-name length, in bytes (default 4096).
    pub max_quoted_name: usize,
    /// Maximum temporal literal length, in characters (default 128).
    pub max_temporal: usize,
    /// Maximum number of entries in a single container (default 1048576).
    pub max_container: usize,
    /// Maximum length of a header `require` list (default 64).
    pub max_header_require: usize,
    /// Maximum input length, in bytes (default 64 MiB).
    pub max_input: usize,
    /// Maximum number of CST tokens in a document (default 1048576).
    pub max_cst_tokens: usize,
}

impl Default for Limits {
    /// The §16 defaults, identical to the reference implementation's.
    fn default() -> Self {
        Limits {
            max_depth: 128,
            max_values: 1 << 20,
            max_string: 16 << 20,
            max_binary: 16 << 20,
            max_number: 1024,
            max_anchors: 65536,
            max_references: 65536,
            max_label: 1024,
            max_bare_name: 1024,
            max_quoted_name: 4096,
            max_temporal: 128,
            max_container: 1 << 20,
            max_header_require: 64,
            max_input: 64 << 20,
            max_cst_tokens: 1 << 20,
        }
    }
}

/// Parser options, mirroring the reference implementation's `Options` field for
/// field: the profile switches (§15.1), the `compat.*` legacy-reader bundle
/// (§15.3), and the §16 resource limits.
///
/// `Options::default()` is Core: every profile off, every compat flag off.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    // ---- profiles (§15.1) ---- //
    /// Graph profile: `&label` / `*label` produce [`Value::Anchor`] / [`Value::Ref`].
    pub graph: bool,
    /// Document Link profile: `cid(..)` / `link(..)` produce [`Value::Link`].
    pub doc_link: bool,
    /// Preserve a temporal fraction's significant scale.
    pub scale_significant: bool,
    /// Require parsed input to already be in canonical form.
    pub canonical_verify: bool,
    /// Named IANA time zones (`^..[Area/Zone]`).
    pub tz_names: bool,

    // ---- compat bundle (§15.3) ---- //
    /// Enable the whole legacy-reader bundle (implies every `compat.*` flag).
    pub compatibility: bool,
    /// Accept temporals without the `^` prefix.
    pub bare_temporals: bool,
    /// Read a temporal fraction as legacy microseconds rather than nanoseconds.
    pub raw_fraction: bool,
    /// Legacy value-binding rules.
    pub legacy_binding: bool,
    /// Accept legacy indented node bodies.
    pub indented_bodies: bool,
    /// Accept legacy number spellings (leading zeros, `+`, trailing point).
    pub lax_numbers: bool,
    /// Accept legacy temporal spellings (unpadded fields, loose offsets).
    pub lax_temporals: bool,
    /// Accept the legacy escape table.
    pub legacy_escapes: bool,
    /// Accept the legacy binary spelling.
    pub legacy_binary: bool,
    /// Treat `#_` as a comment rather than the discard token.
    pub hash_underscore_comment: bool,
    /// Last duplicate key wins instead of raising `duplicate-key`.
    pub last_wins: bool,
    /// Silently deduplicate set elements instead of raising.
    pub duplicate_set_dedup: bool,
    /// Resolve an undefined `*label` to a sentinel rather than raising.
    pub undefined_ref_sentinel: bool,
    /// Use Python's name classes rather than UAX #31 for identifiers.
    pub python_name_classes: bool,
    /// Accept raw control characters in strings.
    pub lax_strings: bool,

    // ---- policy ---- //
    /// Allow an `axon{require:[..]}` header to enable profiles (default true).
    pub allow_header_profiles: bool,
    /// Accept `_` digit separators in numbers (default true).
    pub numeric_separators: bool,
    /// Parser resource limits (§16).
    pub limits: Limits,
}

impl Default for Options {
    /// Core: every profile and compat flag off, header profiles and numeric
    /// separators permitted -- identical to the reference implementation's
    /// `Options()` defaults.
    fn default() -> Self {
        Options {
            graph: false,
            doc_link: false,
            scale_significant: false,
            canonical_verify: false,
            tz_names: false,
            compatibility: false,
            bare_temporals: false,
            raw_fraction: false,
            legacy_binding: false,
            indented_bodies: false,
            lax_numbers: false,
            lax_temporals: false,
            legacy_escapes: false,
            legacy_binary: false,
            hash_underscore_comment: false,
            last_wins: false,
            duplicate_set_dedup: false,
            undefined_ref_sentinel: false,
            python_name_classes: false,
            lax_strings: false,
            allow_header_profiles: true,
            numeric_separators: true,
            limits: Limits::default(),
        }
    }
}

impl Options {
    /// The `compat` bundle (§15.3): the normative legacy pyaxon reader. Sets
    /// `compatibility` and every flag it implies, exactly as the reference
    /// implementation's header handling does.
    pub fn compat() -> Self {
        Options {
            compatibility: true,
            bare_temporals: true,
            raw_fraction: true,
            legacy_binding: true,
            indented_bodies: true,
            lax_numbers: true,
            lax_temporals: true,
            legacy_escapes: true,
            legacy_binary: true,
            hash_underscore_comment: true,
            last_wins: true,
            duplicate_set_dedup: true,
            undefined_ref_sentinel: true,
            python_name_classes: true,
            lax_strings: true,
            ..Options::default()
        }
    }

    /// True when the whole legacy bundle is on, or this individual flag is.
    /// Mirrors the reference's `options.<flag> or options.compatibility` tests.
    fn lax_temporals(&self) -> bool {
        self.lax_temporals || self.compatibility
    }

    /// True when legacy number spellings are accepted.
    fn lax_numbers(&self) -> bool {
        self.lax_numbers || self.compatibility
    }

    /// True when raw control characters are accepted inside strings.
    fn lax_strings(&self) -> bool {
        self.lax_strings || self.compatibility
    }

    /// True when a duplicate key should overwrite rather than raise.
    ///
    /// Note: unlike most compat flags, `last_wins` is *not* implied by
    /// `compatibility` alone -- the reference's `_put` checks `options.last_wins`
    /// with no `or compatibility` fallback. Only the header-expanded `compat`
    /// bundle (which sets every field, see [`Options::compat`]) turns it on.
    fn last_wins(&self) -> bool {
        self.last_wins
    }

    /// True when duplicate set elements should be dropped rather than raise.
    /// Like `last_wins`, not implied by `compatibility` alone.
    fn set_dedup(&self) -> bool {
        self.duplicate_set_dedup
    }

    /// True when `#_` is a comment rather than the discard token.
    fn hash_comment(&self) -> bool {
        self.hash_underscore_comment || self.compatibility
    }

    /// True when a temporal fraction is legacy microseconds.
    fn raw_fraction(&self) -> bool {
        self.raw_fraction || self.compatibility
    }

    /// True when bare (un-`^`-prefixed) temporals are accepted. Not implied by
    /// `compatibility` alone (bare `options.bare_temporals` in the reference).
    fn bare_temporals(&self) -> bool {
        self.bare_temporals
    }

    /// True when an undefined `*label` resolves to a sentinel. Not implied by
    /// `compatibility` alone (bare `options.undefined_ref_sentinel` in the
    /// reference).
    fn undefined_ref_sentinel(&self) -> bool {
        self.undefined_ref_sentinel
    }

    /// True when top-level commas are tolerated (legacy binding).
    fn legacy_binding(&self) -> bool {
        self.legacy_binding || self.compatibility
    }

    /// True when the legacy escape table applies (backslashes kept literally).
    fn legacy_escapes(&self) -> bool {
        self.legacy_escapes || self.compatibility
    }

    /// True when the legacy binary spelling is accepted.
    fn legacy_binary(&self) -> bool {
        self.legacy_binary || self.compatibility
    }

    /// True when identifiers use Python's name classes rather than UAX #31.
    fn python_name_classes(&self) -> bool {
        self.python_name_classes || self.compatibility
    }

    /// True when legacy indented node bodies are accepted. Public to the crate
    /// so the document entry points can apply the source rewrite before parsing.
    /// Not implied by `compatibility` alone (bare `options.indented_bodies` in
    /// the reference's `loads2026`).
    pub(crate) fn indented_bodies_enabled(&self) -> bool {
        self.indented_bodies
    }
}

pub(crate) struct Parser<'a> {
    chars: &'a str,
    pos: usize,
    line: u32,
    col: u32,
    opts: Options,
    /// Anchor labels bound so far, for `duplicate-anchor` and `max_anchors`.
    anchors: Vec<String>,
    /// Reference count, for `max_references`.
    references: usize,
    /// Total values produced, for `max_values`.
    values: usize,
    /// Profiles switched on by an `axon{require:[..]}` header (§11.4).
    header_done: bool,
    /// Byte offset of the payload (past any consumed header), for
    /// `canonical_verify`.
    payload_start: usize,
    /// Whether completed semantic values should be recorded with their exact
    /// byte ranges for the lossless CST layer.
    trace_values: bool,
    /// Completed semantic values in parser-completion order. Nested values are
    /// recorded before their containing value, matching the Python reference.
    value_traces: Vec<ValueTrace>,
    /// A non-empty application registry replaces the normative defaults.
    constants: Vec<(String, Value)>,
}

/// A semantic value produced from an exact byte range in the source.
#[derive(Clone, Debug)]
pub(crate) struct ValueTrace {
    /// Start byte offset, inclusive.
    pub(crate) start: usize,
    /// End byte offset, exclusive.
    pub(crate) end: usize,
    /// The value produced by the parser for that range.
    pub(crate) value: Value,
}

/// Determines if the given character is valid as the starting character of an AXON identifier.
/// This verifies conformance with the specification's identifier rules (§9.1, UAX #31).
fn is_name_start(c: char) -> bool {
    c == '_' || unicode_ident_start(c)
}
/// Determines if the given character is valid as a continuing character within an AXON identifier.
/// This ensures proper parsing of multi-character names and aliases.
fn is_name_continue(c: char) -> bool {
    c == '_' || unicode_ident_continue(c)
}
/// The legacy pyaxon name-start class: `_` or anything `str.isalpha()` accepts.
/// Wider than UAX #31 XID_Start, which is why it is a compat flag rather than
/// the default.
fn is_py_name_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}
/// The legacy pyaxon name-continue class: `_` or `str.isalnum()`.
fn is_py_name_continue(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}
// Full UAX #31 identifier classification via `unicode-ident` (the exact
// XID_Start / XID_Continue tables the AXON name grammar, §9.1, specifies).
fn unicode_ident_start(c: char) -> bool {
    unicode_ident::is_xid_start(c)
}
fn unicode_ident_continue(c: char) -> bool {
    unicode_ident::is_xid_continue(c)
}

impl<'a> Parser<'a> {
    pub(crate) fn new(input: &'a str, opts: Options) -> Self {
        Self::with_tracing(input, opts, false)
    }

    /// Construct a parser with an application constant registry. An empty
    /// registry selects the four normative defaults.
    pub(crate) fn new_with_constants(
        input: &'a str,
        opts: Options,
        constants: &[(String, Value)],
    ) -> Self {
        let mut parser = Self::with_tracing(input, opts, false);
        parser.constants = constants.to_vec();
        parser
    }

    /// Construct a parser that records semantic value spans for the CST.
    pub(crate) fn new_tracing(input: &'a str, opts: Options) -> Self {
        Self::with_tracing(input, opts, true)
    }

    fn with_tracing(input: &'a str, opts: Options, trace_values: bool) -> Self {
        Parser {
            chars: input,
            pos: 0,
            line: 1,
            col: 1,
            opts,
            anchors: Vec::new(),
            references: 0,
            values: 0,
            header_done: false,
            payload_start: 0,
            trace_values,
            value_traces: Vec::new(),
            constants: Vec::new(),
        }
    }

    /// Consume the parser and return its completed semantic value traces.
    pub(crate) fn into_value_traces(self) -> Vec<ValueTrace> {
        self.value_traces
    }

    /// The byte offset where the payload begins, past any consumed header.
    pub(crate) fn payload_start(&self) -> usize {
        self.payload_start
    }

    /// Whether the parser was configured to require canonical input.
    pub(crate) fn canonical_verify(&self) -> bool {
        self.opts.canonical_verify
    }

    /// Whether scale-significant semantics are active, including header
    /// profile activation.
    pub(crate) fn scale_significant(&self) -> bool {
        self.opts.scale_significant
    }

    /// Current 1-based parser line and column.
    pub(crate) fn line_column(&self) -> (u32, u32) {
        (self.line, self.col)
    }

    /// Enforce §16's `max_input` before any scanning work is done.
    pub(crate) fn check_input(&self) -> Result<()> {
        if self.chars.len() > self.opts.limits.max_input {
            return Err(self.err(Category::ResourceLimitExceeded, "input is too large"));
        }
        Ok(())
    }

    /// Constructs a new error enriched with the parser's current line and column information.
    /// This provides precise diagnostic context for unexpected tokens or malformed input.
    fn err(&self, cat: Category, msg: impl Into<String>) -> Error {
        Error::new_with_span(cat, msg, self.pos, self.pos, self.line, self.col)
    }

    /// Identifier name-start test, honoring `python_name_classes` (§9.1).
    fn name_start(&self, c: char) -> bool {
        if self.opts.python_name_classes() {
            is_py_name_start(c)
        } else {
            is_name_start(c)
        }
    }

    /// Identifier name-continue test, honoring `python_name_classes` (§9.1).
    fn name_continue(&self, c: char) -> bool {
        if self.opts.python_name_classes() {
            is_py_name_continue(c)
        } else {
            is_name_continue(c)
        }
    }

    /// Peeks at the next character in the input stream without advancing the parser's position.
    /// Returns `None` if the end of the input has been reached.
    fn peek(&self) -> Option<char> {
        self.chars[self.pos..].chars().next()
    }

    fn peek2(&self) -> Option<char> {
        let mut it = self.chars[self.pos..].chars();
        it.next();
        it.next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Core whitespace is SPACE and TAB only; newlines are structural but also
    /// skipped between values. Comments run `#` to end of line, except `#_`
    /// which is the discard token (handled by callers) -- unless the
    /// `hash_underscore_comment` compat flag restores the legacy reading of
    /// `#_` as an ordinary comment.
    fn skip_ws(&mut self) {
        let discard_is_comment = self.opts.hash_comment();
        loop {
            match self.peek() {
                Some(' ') | Some('\t') | Some('\r') | Some('\n') => {
                    self.bump();
                }
                Some('#') if discard_is_comment || self.peek2() != Some('_') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                _ => break,
            }
        }
    }

    /// True when a discard token starts here (and discards are enabled).
    fn at_discard(&self) -> bool {
        !self.opts.hash_comment() && self.peek() == Some('#') && self.peek2() == Some('_')
    }

    /// Parse the next top-level value in a stream, returning `None` if EOF.
    pub(crate) fn parse_next_value(&mut self) -> Result<Option<Value>> {
        loop {
            self.skip_ws();
            if self.peek().is_none() {
                return Ok(None);
            }
            self.discard_values(0)?;
            self.skip_ws();
            if self.peek().is_none() {
                return Ok(None);
            }
            let v = self.parse_value(0)?;
            // The first value of a document may be an `axon{..}` header, which
            // configures the parser and is not part of the data (§11.4).
            if !self.header_done {
                self.header_done = true;
                if self.consume_header(&v)? {
                    // Record where the real payload starts, so canonical
                    // verification compares against the post-header text.
                    self.skip_ws();
                    self.payload_start = self.pos;
                    continue;
                }
            }
            self.skip_ws();
            if self.peek() == Some(',') {
                if !self.opts.legacy_binding() {
                    return Err(self.err(
                        Category::UnexpectedToken,
                        "top-level commas are not permitted in Core",
                    ));
                }
                self.bump();
            }
            return Ok(Some(v));
        }
    }

    /// Parse a whole document (a stream of top-level values).
    ///
    /// A document whose first top-level item is a `key: value` pair is an
    /// *attribute document* (§11.3): the entire document is one ordered map,
    /// and every subsequent item must also be a pair (mixing pairs and bare
    /// values is `unexpected-token`). Otherwise it is an ordinary value stream.
    /// This mirrors the reference `parse_document`.
    pub(crate) fn parse_document(&mut self) -> Result<Vec<Value>> {
        self.check_input()?;
        self.skip_ws();
        if self.peek().is_none() {
            return Ok(Vec::new());
        }
        let save = (self.pos, self.line, self.col);
        if let Some(key) = self.try_key()? {
            let val = self.parse_value(0)?;
            let mut pairs = Vec::new();
            self.put_pair(&mut pairs, key, val)?;
            loop {
                self.separator_top()?;
                if self.peek().is_none() {
                    break;
                }
                if self.at_discard() {
                    self.discard_pairs(0, "attribute document")?;
                    continue;
                }
                let Some(key) = self.try_key()? else {
                    return Err(self.err(
                        Category::UnexpectedToken,
                        "attribute document cannot mix pairs and values",
                    ));
                };
                let val = self.parse_value(0)?;
                self.put_pair(&mut pairs, key, val)?;
            }
            return Ok(alloc::vec![Value::OrderedMap(pairs)]);
        }
        self.restore(save);
        let mut out = Vec::new();
        while let Some(v) = self.parse_next_value()? {
            out.push(v);
        }
        Ok(out)
    }

    /// Probe whether the document begins in attribute-document form without
    /// consuming input. Incremental readers use this to preserve the Python
    /// authority's rule that an attribute document is yielded atomically.
    pub(crate) fn starts_attribute_document(&mut self) -> Result<bool> {
        self.check_input()?;
        let save = (self.pos, self.line, self.col);
        self.skip_ws();
        let result = if self.peek().is_none() {
            false
        } else {
            self.try_key()?.is_some()
        };
        self.restore(save);
        Ok(result)
    }

    /// A top-level separator between attribute-document pairs: optional
    /// whitespace, then an optional single comma (consecutive commas are
    /// rejected). Mirrors the reference `separator(top=True)`; note that,
    /// unlike a stream of bare values, an attribute document *does* tolerate a
    /// comma separator even in Core.
    fn separator_top(&mut self) -> Result<()> {
        self.skip_ws();
        if self.peek() == Some(',') {
            self.bump();
            self.skip_ws();
            if self.peek() == Some(',') {
                return Err(self.err(
                    Category::UnexpectedToken,
                    "consecutive commas are not allowed",
                ));
            }
        }
        Ok(())
    }

    /// Consume a run of `#_` discard tokens and the values they discard. AXON
    /// allows them to stack (`#_ #_ a b` discards both), matching the
    /// reference's discard-count loop.
    fn discard_values(&mut self, depth: u32) -> Result<()> {
        for _ in 0..self.discard_count() {
            let _ = self.parse_value(depth)?;
            self.skip_ws();
        }
        Ok(())
    }

    /// Count and consume a run of stacked `#_` tokens.
    fn discard_count(&mut self) -> usize {
        let mut count = 0;
        while self.at_discard() {
            self.bump();
            self.bump();
            self.skip_ws();
            count += 1;
        }
        count
    }

    /// Consume a run of `#_` tokens in a pair position, discarding a `key: value`
    /// for each. A discard in a keyed body must cover a pair, not a bare value.
    fn discard_pairs(&mut self, depth: u32, context: &str) -> Result<()> {
        for _ in 0..self.discard_count() {
            if self.try_key()?.is_none() {
                return Err(self.err(
                    Category::UnexpectedToken,
                    alloc::format!("discard in {} requires a pair", context),
                ));
            }
            let _ = self.parse_value(depth)?;
            self.skip_ws();
        }
        Ok(())
    }

    /// If `v` is an `axon{..}` document header, apply it to the parser options
    /// and report true so the caller drops it from the value stream (§11.4).
    fn consume_header(&mut self, v: &Value) -> Result<bool> {
        let Value::Node(node) = v else {
            return Ok(false);
        };
        if node.name != "axon" {
            return Ok(false);
        }
        let attr = |k: &str| node.attributes.iter().find(|(n, _)| n == k).map(|(_, v)| v);
        match attr("edition") {
            None => {}
            Some(Value::String(e)) if e == "2026" => {}
            Some(_) => {
                return Err(self.err(
                    Category::UnsupportedProfileFeature,
                    "unsupported AXON edition",
                ));
            }
        }
        let required: Vec<&str> = match attr("require") {
            None => Vec::new(),
            Some(Value::List(items)) => {
                let mut names = Vec::with_capacity(items.len());
                for item in items {
                    match item {
                        Value::String(s) => names.push(s.as_str()),
                        _ => {
                            return Err(self.err(
                                Category::UnsupportedProfileFeature,
                                "header require must be a list of strings",
                            ));
                        }
                    }
                }
                names
            }
            Some(_) => {
                return Err(self.err(
                    Category::UnsupportedProfileFeature,
                    "header require must be a list of strings",
                ));
            }
        };
        if required.len() > self.opts.limits.max_header_require {
            return Err(self.err(
                Category::ResourceLimitExceeded,
                "header require list is too long",
            ));
        }
        if !required.is_empty() && !self.opts.allow_header_profiles {
            let mut names = required.clone();
            names.sort_unstable();
            return Err(self.err(
                Category::UnsupportedProfileFeature,
                alloc::format!(
                    "caller policy forbids header-enabled profiles: {}",
                    names.join(", ")
                ),
            ));
        }
        for name in &required {
            match *name {
                "graph" => self.opts.graph = true,
                "doc-link" => self.opts.doc_link = true,
                "canonical-verify" => self.opts.canonical_verify = true,
                "scale-significant" => self.opts.scale_significant = true,
                "tz-names" => self.opts.tz_names = true,
                "cst" => {}
                "compat" => {
                    let limits = self.opts.limits;
                    let allow = self.opts.allow_header_profiles;
                    self.opts = Options {
                        limits,
                        allow_header_profiles: allow,
                        graph: self.opts.graph,
                        doc_link: self.opts.doc_link,
                        canonical_verify: self.opts.canonical_verify,
                        scale_significant: self.opts.scale_significant,
                        tz_names: self.opts.tz_names,
                        ..Options::compat()
                    };
                }
                other => {
                    return Err(self.err(
                        Category::UnsupportedProfileFeature,
                        alloc::format!("unsupported profile '{}'", other),
                    ));
                }
            }
        }
        if self.opts.canonical_verify && self.opts.scale_significant {
            return Err(self.err(
                Category::UnsupportedProfileFeature,
                "scale-significant and canonical-verify are incompatible",
            ));
        }
        Ok(true)
    }

    fn enter(&self, depth: u32) -> Result<u32> {
        if depth + 1 > self.opts.limits.max_depth {
            return Err(self.err(
                Category::ResourceLimitExceeded,
                "maximum nesting depth exceeded",
            ));
        }
        Ok(depth + 1)
    }

    fn parse_value(&mut self, depth: u32) -> Result<Value> {
        self.values += 1;
        if self.values > self.opts.limits.max_values {
            return Err(self.err(
                Category::ResourceLimitExceeded,
                "too many values in document",
            ));
        }
        self.skip_ws();
        let start = self.pos;
        let c = self
            .peek()
            .ok_or_else(|| self.err(Category::UnexpectedEnd, "expected a value"))?;
        let value = match c {
            '^' => self.parse_temporal(),
            '"' => self.parse_string('"'),
            '`' => self.parse_backtick(),
            'r' if self.at_raw_string() => self.parse_raw_string(),
            '|' => self.parse_binary(),
            '[' => self.parse_bracket(depth),
            '{' => self.parse_brace(depth),
            '(' => self.parse_paren(depth),
            '∅' => {
                self.bump();
                Ok(Value::Set(Vec::new()))
            }
            '$' => self.parse_constant_ref(),
            '&' => self.parse_anchor(depth),
            '*' => self.parse_reference(),
            '?' | '∞' => self.parse_special_number(),
            '-' => {
                // could be a negative number or -∞ / -∞d/D/$ (decimal -inf)
                if self.peek2() == Some('∞') {
                    self.bump(); // -
                    self.bump(); // ∞
                    if matches!(self.peek(), Some('d') | Some('D') | Some('$')) {
                        self.bump();
                        self.special_no_adjacent()?;
                        Ok(Value::Decimal(Decimal::NegInfinity))
                    } else {
                        // §5.9 applies to `-∞` exactly as it does to `∞`.
                        self.special_no_adjacent()?;
                        Ok(Value::Float(f64::NEG_INFINITY))
                    }
                } else {
                    self.parse_number()
                }
            }
            c if c.is_ascii_digit() => self.parse_number(),
            // A quoted name is a node name, not a value on its own: `'weird
            // name'{x:1}` is a Node (§9.2).
            '\'' => self.parse_name_or_node(depth),
            c if self.name_start(c) => self.parse_name_or_node(depth),
            other => Err(self.err(
                Category::UnexpectedToken,
                alloc::format!("unexpected character {:?}", other),
            )),
        }?;
        if self.trace_values {
            self.value_traces.push(ValueTrace {
                start,
                end: self.pos,
                value: value.clone(),
            });
        }
        Ok(value)
    }

    /// True when an `r` here opens a raw string (`r"..."` / `r#"..."#`) rather
    /// than beginning an ordinary name.
    fn at_raw_string(&self) -> bool {
        let mut it = self.chars[self.pos..].chars();
        if it.next() != Some('r') {
            return false;
        }
        for c in it {
            match c {
                '#' => continue,
                '"' => return true,
                _ => return false,
            }
        }
        false
    }

    /// Parse a raw string `r"..."` or `r#"..."#` (§7.5): no escape processing,
    /// terminated by a quote followed by the same number of hashes it opened
    /// with.
    fn parse_raw_string(&mut self) -> Result<Value> {
        self.bump(); // r
        let mut hashes = 0usize;
        while self.peek() == Some('#') {
            hashes += 1;
            self.bump();
        }
        if !self.eat('"') {
            return Err(self.err(Category::UnexpectedToken, "invalid raw string opener"));
        }
        let mut terminator = String::from("\"");
        for _ in 0..hashes {
            terminator.push('#');
        }
        let start = self.pos;
        let rel = self.chars[start..]
            .find(&terminator)
            .ok_or_else(|| self.err(Category::UnexpectedEnd, "unterminated raw string"))?;
        let body = &self.chars[start..start + rel];
        if body.chars().count() > self.opts.limits.max_string {
            return Err(self.err(Category::ResourceLimitExceeded, "string is too long"));
        }
        let body = body.to_owned();
        while self.pos < start + rel + terminator.len() {
            self.bump();
        }
        Ok(Value::String(body))
    }

    /// Parse `&label value` (Graph profile, §10.1).
    fn parse_anchor(&mut self, depth: u32) -> Result<Value> {
        if !self.opts.graph {
            return Err(self.err(
                Category::UnsupportedProfileFeature,
                "anchors require the graph profile",
            ));
        }
        self.bump(); // &
        let label = self.read_label()?;
        if self.anchors.contains(&label) {
            return Err(self.err(
                Category::DuplicateAnchor,
                alloc::format!("duplicate anchor {}", python_string_repr(&label)),
            ));
        }
        if self.anchors.len() + 1 > self.opts.limits.max_anchors {
            return Err(self.err(Category::ResourceLimitExceeded, "too many anchors"));
        }
        self.anchors.push(label.clone());
        let value = self.parse_value(depth)?;
        Ok(Value::Anchor(Box::new(crate::value::Anchor {
            label,
            value,
        })))
    }

    /// Parse `*label` (Graph profile, §10.2).
    fn parse_reference(&mut self) -> Result<Value> {
        if !self.opts.graph {
            return Err(self.err(
                Category::UnsupportedProfileFeature,
                "references require the graph profile",
            ));
        }
        self.bump(); // *
        let label = self.read_label()?;
        if !self.anchors.contains(&label) && !self.opts.undefined_ref_sentinel() {
            return Err(self.err(
                Category::UnknownReference,
                alloc::format!(
                    "reference must follow its anchor {}",
                    python_string_repr(&label)
                ),
            ));
        }
        self.references += 1;
        if self.references > self.opts.limits.max_references {
            return Err(self.err(Category::ResourceLimitExceeded, "too many references"));
        }
        if self.anchors.contains(&label) {
            Ok(Value::Ref(label))
        } else {
            Ok(Value::Undefined)
        }
    }

    /// Read an anchor / reference label, enforcing §16's `max_label`.
    fn read_label(&mut self) -> Result<String> {
        let start = self.pos;
        if !matches!(self.peek(), Some(c) if c == '_' || c.is_alphanumeric()) {
            return Err(self.err(Category::UnexpectedToken, "expected anchor label"));
        }
        while let Some(c) = self.peek() {
            if c == '_' || c.is_alphanumeric() {
                self.bump();
            } else {
                break;
            }
        }
        let label = self.chars[start..self.pos].to_owned();
        if label.chars().count() > self.opts.limits.max_label {
            return Err(self.err(Category::ResourceLimitExceeded, "label is too long"));
        }
        Ok(label)
    }

    // ---- numbers ---------------------------------------------------------- //

    /// Parses special numeric values such as Not-a-Number (`?`) or infinity (`∞`).
    /// Also supports decimal-domain specials like `?d` and `∞d`.
    fn parse_special_number(&mut self) -> Result<Value> {
        let c = self.bump().unwrap();
        let base = match c {
            '?' => Value::Float(f64::NAN),
            '∞' => Value::Float(f64::INFINITY),
            _ => unreachable!(),
        };
        // `?d`/`?D`/`?$` and `∞d`/`∞D`/`∞$` are the decimal-domain specials (§1.6.4).
        if matches!(self.peek(), Some('d') | Some('D') | Some('$')) {
            self.bump();
            let d = match c {
                '?' => Decimal::NaN,
                '∞' => Decimal::Infinity,
                _ => unreachable!(),
            };
            self.special_no_adjacent()?;
            return Ok(Value::Decimal(d));
        }
        self.special_no_adjacent()?;
        Ok(base)
    }

    /// Parses a constant reference beginning with `$`.
    /// Resolves standard numeric constants; unrecognized constants yield an error.
    fn parse_constant_ref(&mut self) -> Result<Value> {
        // `$name` -- registered constant reference. The defaults resolve; unknown
        // names are an error. We only support the numeric-special defaults here.
        self.bump(); // $
        let start = self.pos;
        while let Some(c) = self.peek() {
            if self.name_continue(c) {
                self.bump();
            } else {
                break;
            }
        }
        let name = &self.chars[start..self.pos];
        if name.is_empty() {
            return Err(self.err(Category::UnexpectedToken, "`$` must be followed by a name"));
        }
        let v = if self.constants.is_empty() {
            match name {
                "NaN" => Value::Float(f64::NAN),
                "Inf" => Value::Float(f64::INFINITY),
                "NegInf" => Value::Float(f64::NEG_INFINITY),
                "NaND" => Value::Decimal(Decimal::NaN),
                other => {
                    return Err(self.err(
                        Category::UnknownConstant,
                        alloc::format!("unknown constant ${}", other),
                    ));
                }
            }
        } else {
            self.constants
                .iter()
                .find_map(|(key, value)| (key == name).then(|| value.clone()))
                .ok_or_else(|| {
                    self.err(
                        Category::UnknownConstant,
                        alloc::format!("unknown constant ${}", name),
                    )
                })?
        };
        Ok(v)
    }

    /// After a numeric literal, a name-start or digit must not immediately
    /// follow (AXON §5.9).
    fn no_adjacent(&self) -> Result<()> {
        if self.opts.legacy_binding() {
            return Ok(());
        }
        if let Some(c) = self.peek()
            && (c.is_ascii_digit() || self.name_start(c))
        {
            return Err(self.err(
                Category::UnexpectedToken,
                "a numeric literal must be separated from a following name or digit",
            ));
        }
        Ok(())
    }

    fn special_no_adjacent(&self) -> Result<()> {
        if self.opts.legacy_binding() {
            return Ok(());
        }
        if let Some(c) = self.peek()
            && (c.is_ascii_digit() || self.name_start(c))
        {
            return Err(self.err(
                Category::UnexpectedToken,
                "a numeric special must be separated from a following value",
            ));
        }
        Ok(())
    }

    /// Parses a standard numeric literal, handling integers, floats, and decimals.
    /// Incorporates validation for adjacency rules, leading zeros, and limits.
    fn parse_number(&mut self) -> Result<Value> {
        // Legacy documents wrote temporals without the `^` prefix; under
        // `bare_temporals` a digit run that looks temporal is committed as one
        // before the number path claims it.
        if self.opts.bare_temporals()
            && let Some(v) = self.try_bare_temporal()?
        {
            return Ok(v);
        }
        let start = self.pos;
        // optional leading minus, and a leading `+` only for legacy readers
        if self.opts.lax_numbers() {
            self.eat('+');
        }
        self.eat('-');
        // integer part -- a `_` separator is consumed only between two digits
        // (never doubled, never trailing), per AXON §5.6; a stray `_` ends the
        // run and is then diagnosed by the no-adjacency rule below.
        let int_start = self.pos;
        let mut digits = 0usize;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.bump();
                digits += 1;
            } else if c == '_' && matches!(self.peek2(), Some(d) if d.is_ascii_digit()) {
                self.bump();
            } else {
                break;
            }
        }
        // digits then `-`/`:` is the bare-temporal tripwire
        if matches!(self.peek(), Some('-') | Some(':')) {
            return Err(self.err(
                Category::InvalidTemporal,
                "bare temporals are not permitted in Core; prefix with `^`",
            ));
        }
        if digits == 0 {
            return Err(self.err(Category::InvalidNumber, "expected a digit"));
        }
        // leading zeros are invalid except the single digit `0` (AXON §5.2)
        let int_run: String = self.chars[int_start..self.pos]
            .chars()
            .filter(|&c| c != '_')
            .collect();
        if int_run.len() > 1 && int_run.starts_with('0') && !self.opts.lax_numbers() {
            return Err(self.err(Category::InvalidNumber, "leading zeros are not permitted"));
        }
        let mut is_float = false;
        let mut is_decimal = false;
        // fraction
        if self.peek() == Some('.') {
            // trailing point (5.) is invalid outside legacy readers
            if !matches!(self.peek2(), Some(d) if d.is_ascii_digit()) && !self.opts.lax_numbers() {
                return Err(self.err(
                    Category::InvalidNumber,
                    "a trailing decimal point is not permitted",
                ));
            }
            is_float = true;
            self.bump();
            while let Some(c) = self.peek() {
                if c.is_ascii_digit()
                    || (c == '_' && matches!(self.peek2(), Some(d) if d.is_ascii_digit()))
                {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        // exponent
        if matches!(self.peek(), Some('e') | Some('E')) {
            is_float = true;
            self.bump();
            if matches!(self.peek(), Some('+') | Some('-')) {
                self.bump();
            }
            let mut e = 0;
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    self.bump();
                    e += 1;
                } else {
                    break;
                }
            }
            if e == 0 {
                return Err(self.err(Category::InvalidNumber, "exponent has no digits"));
            }
        }
        // decimal suffix d/D
        if matches!(self.peek(), Some('d') | Some('D')) {
            is_decimal = true;
            self.bump();
        } else if self.peek() == Some('$') {
            // general `$` suffix does not exist
            return Err(self.err(Category::UnexpectedToken, "`$` is not a decimal suffix"));
        }

        // The bare-temporal tripwire again, now that the *whole* literal has
        // been consumed. The earlier check fires only after the integer run, so
        // `3.5:` and `1d:` slipped past it and were reported as a stray token
        // rather than as the bare temporal they look like. The specification's
        // constraint index is explicit: in Core, digits immediately followed by
        // `-` or `:` are the bare-shape tripwire and report `invalid-temporal`
        // with a migration hint.
        if matches!(self.peek(), Some('-') | Some(':')) {
            return Err(self.err(
                Category::InvalidTemporal,
                "bare temporals are not permitted in Core; prefix with `^`",
            ));
        }

        let literal = &self.chars[start..self.pos];
        if !self.opts.numeric_separators && literal.contains('_') {
            return Err(self.err(Category::InvalidNumber, "numeric separators are disabled"));
        }
        let raw: String = literal.chars().filter(|&c| c != '_').collect();
        if raw.len() > self.opts.limits.max_number {
            return Err(self.err(Category::ResourceLimitExceeded, "numeric literal too long"));
        }
        self.no_adjacent()?;

        if is_decimal {
            let body = &raw[..raw.len() - 1]; // strip d/D
            return match parse_decimal(body, self.opts.scale_significant) {
                Some(d) => Ok(Value::Decimal(d)),
                None => Err(self.err(
                    Category::InvalidNumber,
                    "decimal exponent is out of the representable range",
                )),
            };
        }
        if is_float {
            let f: f64 = raw
                .parse()
                .map_err(|_| self.err(Category::InvalidNumber, "malformed float"))?;
            return Ok(Value::Float(f));
        }
        // integer -- keep as canonical text (strip a redundant leading + none here)
        Ok(Value::Int(normalize_int(&raw)))
    }

    /// Under `bare_temporals`, try to read a temporal that was written without
    /// its `^` prefix (the legacy pyaxon spelling). Returns `None` -- with the
    /// position restored -- when the run ahead is an ordinary number.
    fn try_bare_temporal(&mut self) -> Result<Option<Value>> {
        let save = (self.pos, self.line, self.col);
        let start = self.pos;
        // True while inside an RFC 9557 zone bracket.
        let mut zone_open = false;
        while let Some(c) = self.peek() {
            if c == '[' {
                if !self.opts.tz_names {
                    // This is a *speculative* parse -- `save` exists so the
                    // number path can claim the run instead. Erroring here made
                    // any number followed by a list fail: `1d[]` reported
                    // `unsupported-profile-feature`. End the token and let the
                    // caller decide; a genuine zoned temporal is still rejected
                    // below, because the run before `[` will not validate.
                    break;
                }
                zone_open = true;
                self.bump();
                continue;
            }
            if c == ']' {
                // A `]` belongs to the temporal only when it closes an RFC 9557
                // zone bracket this temporal opened. Consuming it regardless
                // made `[^2026-07-01]` unparseable -- and the writer emits that
                // exact shape, canonical form included, so the crate could not
                // re-read its own output when a list ended with a temporal.
                if !zone_open {
                    break;
                }
                zone_open = false;
                self.bump();
                continue;
            }
            // Same structural terminator as the `^` form. An allow-list
            // stopped at `{` and `*`, so `2026-01-01{1 1}` and
            // `12:00*x` split into two values where the reference
            // reads one malformed temporal.
            if matches!(c, ' ' | '\t' | '\r' | '\n' | ',' | '}' | ')' | '#' | ']') {
                break;
            }
            self.bump();
        }
        let raw = &self.chars[start..self.pos];
        // Only commit when it is temporal-shaped; a plain number must stay a
        // number, and `1.5` must not become a date.
        let shaped = raw.contains(':') || (raw.matches('-').count() >= 2 && raw.contains('-'));
        if !shaped {
            self.restore(save);
            return Ok(None);
        }
        match parse_temporal_text(raw, &self.opts) {
            Ok(v) => Ok(Some(v)),
            Err(_) => {
                self.restore(save);
                Ok(None)
            }
        }
    }

    // ---- temporals -------------------------------------------------------- //

    /// Parses a temporal value prefixed by a caret (`^`).
    /// Delegates the actual string analysis to the temporal text parser.
    fn parse_temporal(&mut self) -> Result<Value> {
        self.bump(); // ^
        // read a run of temporal characters
        let start = self.pos;
        // True while inside an RFC 9557 zone bracket.
        let mut zone_open = false;
        while let Some(c) = self.peek() {
            if c == '[' {
                if !self.opts.tz_names {
                    return Err(self.err(
                        Category::UnsupportedProfileFeature,
                        "named time zones require tz-names profile",
                    ));
                }
                zone_open = true;
                self.bump();
                continue;
            }
            if c == ']' {
                // A `]` belongs to the temporal only when it closes an RFC 9557
                // zone bracket this temporal opened. Consuming it regardless
                // made `[^2026-07-01]` unparseable -- and the writer emits that
                // exact shape, canonical form included, so the crate could not
                // re-read its own output when a list ended with a temporal.
                if !zone_open {
                    break;
                }
                zone_open = false;
                self.bump();
                continue;
            }
            // Terminate on the structural set the Python reference uses, and
            // absorb everything else into the run for `parse_temporal_text`
            // to reject. An allow-list ended the token at the first character
            // it did not recognize, so `^2026-01-01@` reported
            // `unexpected-token` -- "there is a stray @" -- where the
            // reference reports `invalid-temporal`: the temporal itself is
            // malformed, which is the more useful thing to say. This diverged
            // on 240 of 3,327 differential documents.
            if matches!(c, ' ' | '\t' | '\r' | '\n' | ',' | '}' | ')' | '#') {
                break;
            }
            self.bump();
        }
        let raw = &self.chars[start..self.pos];
        if raw.chars().count() > self.opts.limits.max_temporal {
            return Err(self.err(Category::ResourceLimitExceeded, "temporal is too long"));
        }
        parse_temporal_text(raw, &self.opts)
            .map_err(|message| self.err(Category::InvalidTemporal, message))
    }

    // ---- strings ---------------------------------------------------------- //

    /// Parses a delimited string literal (e.g., standard double-quoted strings).
    /// Handles escape sequences and enforces length limits to prevent resource exhaustion.
    fn parse_string(&mut self, quote: char) -> Result<Value> {
        self.bump(); // opening quote
        let s = self.read_escaped(quote, true)?;
        Ok(Value::String(s))
    }

    /// Parses a backtick-delimited string literal, typical for certain AXON profiles.
    /// This follows the same escape mechanics as double-quoted strings.
    fn parse_backtick(&mut self) -> Result<Value> {
        self.bump();
        let s = self.read_escaped('`', true)?;
        Ok(Value::String(s))
    }

    /// Read the body of a `"..."` or backtick string, applying the AXON 2026
    /// escape table. `allow_escapes` is always true for Core strings.
    /// Reads the internal contents of an escaped string sequence until the matching quote.
    /// Validates and expands supported escape sequences according to the AXON 2026 specification.
    fn read_escaped(&mut self, quote: char, _allow_escapes: bool) -> Result<String> {
        let mut out = String::new();
        loop {
            let c = self
                .peek()
                .ok_or_else(|| self.err(Category::UnexpectedEnd, "unterminated string"))?;
            if c == quote {
                self.bump();
                if out.chars().count() > self.opts.limits.max_string {
                    return Err(self.err(Category::ResourceLimitExceeded, "string too long"));
                }
                return Ok(out);
            }
            if c == '\\' {
                self.bump();
                let e = self
                    .peek()
                    .ok_or_else(|| self.err(Category::UnexpectedEnd, "escape at end of input"))?;
                // A backslash before a line break is a continuation: the break
                // is dropped. CR, LF, and CRLF all count.
                if e == '\n' || e == '\r' {
                    self.bump();
                    if e == '\r' && self.peek() == Some('\n') {
                        self.bump();
                    }
                    if self.opts.legacy_escapes() {
                        // Bug-for-bug with the original pyaxon reader, which
                        // duplicates the accumulated buffer here and then
                        // appends a lone backslash. Verified against the legacy
                        // loader in this tree; §19.1 requires compat flags to
                        // reproduce ground-truthed legacy behavior, warts and
                        // all, so this is deliberate rather than an oversight.
                        // The duplication is exponential in the number of
                        // continuations, so enforce `max_string` here -- the
                        // close-quote guard alone would let a tiny input build
                        // a multi-gigabyte buffer first (audit H1).
                        if out.chars().count() > self.opts.limits.max_string {
                            return Err(
                                self.err(Category::ResourceLimitExceeded, "string too long")
                            );
                        }
                        let dup = out.clone();
                        out.push_str(&dup);
                        out.push('\\');
                    }
                    continue;
                }
                if self.opts.legacy_escapes() {
                    // Legacy pyaxon kept the backslash and the escaped
                    // character literally, recognizing no escape table at all.
                    out.push('\\');
                    out.push(e);
                    self.bump();
                    continue;
                }
                match e {
                    '\\' => {
                        out.push('\\');
                        self.bump();
                    }
                    '"' => {
                        out.push('"');
                        self.bump();
                    }
                    '`' => {
                        out.push('`');
                        self.bump();
                    }
                    '\'' => {
                        out.push('\'');
                        self.bump();
                    }
                    'n' => {
                        out.push('\n');
                        self.bump();
                    }
                    'r' => {
                        out.push('\r');
                        self.bump();
                    }
                    't' => {
                        out.push('\t');
                        self.bump();
                    }
                    'u' => {
                        self.bump();
                        out.push(self.read_braced_unicode()?);
                    }
                    other => {
                        return Err(self.err(
                            Category::InvalidEscape,
                            alloc::format!("invalid escape \\{}", other),
                        ));
                    }
                }
            } else if c == '\r' {
                // Literal CR and CRLF normalize to one LF in semantic strings.
                out.push('\n');
                self.bump();
                if self.peek() == Some('\n') {
                    self.bump();
                }
            } else if (c as u32) < 0x20
                && c != '\t'
                && c != '\n'
                && c != '\r'
                && !self.opts.lax_strings()
            {
                // The reference (and spec §17.2, which has no `invalid-string`)
                // categorizes a raw C0 control in a string as `invalid-escape`.
                return Err(self.err(
                    Category::InvalidEscape,
                    "unescaped control character in string",
                ));
            } else {
                out.push(c);
                self.bump();
            }
        }
    }

    /// Parses a braced Unicode escape sequence, e.g., `\u{1F600}`.
    /// Ensures the resulting value constitutes a valid Unicode scalar value.
    fn read_braced_unicode(&mut self) -> Result<char> {
        if !self.eat('{') {
            return Err(self.err(Category::InvalidEscape, "\\u must be followed by {"));
        }
        let mut v: u32 = 0;
        let mut n = 0;
        while let Some(c) = self.peek() {
            let Some(d) = c.to_digit(16) else {
                break;
            };
            self.bump();
            n += 1;
            // At most six hex digits can name a scalar; stop folding into `v`
            // past that so the accumulation cannot overflow `u32` (audit H6).
            // The `n > 6` check below rejects the over-long escape.
            if n <= 6 {
                v = v * 16 + d;
            }
        }
        if !self.eat('}') || n == 0 || n > 6 {
            return Err(self.err(Category::InvalidEscape, "malformed \\u{...} escape"));
        }
        char::from_u32(v).ok_or_else(|| {
            self.err(
                Category::InvalidEscape,
                "escape is not a Unicode scalar value",
            )
        })
    }

    // ---- binary ----------------------------------------------------------- //

    /// Parses a base64-encoded binary literal enclosed in pipe characters (`|...|`).
    /// Discards whitespace and validates characters against the standard base64 alphabet.
    fn parse_binary(&mut self) -> Result<Value> {
        self.bump(); // opening |
        if self.opts.legacy_binary() {
            return self.parse_legacy_binary();
        }
        let mut b64 = String::new();
        loop {
            let c = self.peek().ok_or_else(|| {
                self.err(
                    Category::UnexpectedEnd,
                    "binary literal requires a closing pipe",
                )
            })?;
            if c == '|' {
                self.bump();
                break;
            }
            if c.is_whitespace() {
                self.bump();
                continue;
            }
            if c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' {
                b64.push(c);
                self.bump();
            } else {
                return Err(self.err(Category::InvalidBinary, "invalid base64 character"));
            }
        }
        let bytes = base64_decode(&b64).map_err(|m| self.err(Category::InvalidBinary, m))?;
        if bytes.len() > self.opts.limits.max_binary {
            return Err(self.err(Category::ResourceLimitExceeded, "binary value is too long"));
        }
        Ok(Value::Bytes(bytes))
    }

    /// The legacy pyaxon binary spelling: no closing pipe, whitespace skipped,
    /// and a `=` padding run is mandatory as the terminator. Mirrors the
    /// reference's `legacy_binary` branch.
    fn parse_legacy_binary(&mut self) -> Result<Value> {
        let mut b64 = String::new();
        let mut saw_padding = false;
        while let Some(c) = self.peek() {
            if c == '=' {
                saw_padding = true;
                while self.peek() == Some('=') {
                    b64.push('=');
                    self.bump();
                }
                break;
            }
            if (c as u32) <= 32 {
                self.bump();
                continue;
            }
            if c.is_ascii_alphanumeric() || c == '+' || c == '/' {
                b64.push(c);
                self.bump();
            } else {
                return Err(self.err(Category::InvalidBinary, "invalid legacy Base64 character"));
            }
        }
        if !saw_padding {
            return Err(self.err(
                Category::UnexpectedEnd,
                "legacy Base64 requires a padding terminator",
            ));
        }
        let bytes = base64_decode(&b64).map_err(|m| self.err(Category::InvalidBinary, m))?;
        if bytes.len() > self.opts.limits.max_binary {
            return Err(self.err(Category::ResourceLimitExceeded, "binary value is too long"));
        }
        Ok(Value::Bytes(bytes))
    }

    // ---- containers ------------------------------------------------------- //

    /// Parses a bracketed collection, either a standard List (`[...]`) or an OrderedMap (`[...]` with keys).
    /// Dynamically infers the collection type based on the presence of a leading key-value pair.
    fn parse_bracket(&mut self, depth: u32) -> Result<Value> {
        let depth = self.enter(depth)?;
        self.bump(); // [
        self.skip_ws();
        // empty forms: [] list, [:] ordered map
        if self.peek() == Some(']') {
            self.bump();
            return Ok(Value::List(Vec::new()));
        }
        if self.peek() == Some(':') && self.peek2() == Some(']') {
            self.bump();
            self.bump();
            return Ok(Value::OrderedMap(Vec::new()));
        }
        if self.peek().is_none() {
            return Err(self.err(Category::UnexpectedEnd, "unterminated container"));
        }
        // A discard may lead the container. `finish_seq` handles discards
        // between later elements, but the first element was read directly,
        // so `[#_ 1 2]` and `(#_ c 1)` were rejected while `{a:1 #_ b:2}`
        // and a top-level discard both worked.
        while self.at_discard() {
            self.discard_values(depth)?;
            self.skip_ws();
        }
        // A discard run may have consumed everything up to the close.
        if self.peek() == Some(']') {
            self.bump();
            return Ok(Value::List(Vec::new()));
        }
        // Decide list vs ordered-map by whether the first element is a `key:`
        // pair. A key is a bare name or a `"string"` (never an arbitrary value),
        // and the semantic key is a String (AXON §3.7 / §9.6).
        if let Some(key) = self.try_key()? {
            let val = self.parse_value(depth)?;
            let mut pairs = Vec::new();
            self.put_pair(&mut pairs, key, val)?;
            self.finish_map(depth, ']', &mut pairs)?;
            Ok(Value::OrderedMap(pairs))
        } else {
            let first = self.parse_value(depth)?;
            let mut items = alloc::vec![first];
            self.finish_seq(depth, ']', &mut items)?;
            Ok(Value::List(items))
        }
    }

    /// Apply §12.1's duplicate-key policy to a freshly parsed map body: Core
    /// raises `duplicate-key`; the `last_wins` compat flag keeps the final
    /// binding instead, matching the reference's `_put`.
    fn put_pair(&self, pairs: &mut Vec<(Value, Value)>, key: String, value: Value) -> Result<()> {
        if let Some((_, previous)) = pairs
            .iter_mut()
            .find(|(known, _)| matches!(known, Value::String(text) if text == &key))
        {
            if !self.opts.last_wins() {
                return Err(self.err(
                    Category::DuplicateKey,
                    alloc::format!("duplicate key {}", python_string_repr(&key)),
                ));
            }
            *previous = value;
        } else {
            pairs.push((Value::String(key), value));
            self.check_container(pairs.len())?;
        }
        Ok(())
    }

    /// Apply §12.2's duplicate-set policy: Core raises `duplicate-set-element`;
    /// the `set_dedup` compat flag silently drops repeats (baseline pyaxon).
    fn dedup_set(&self, items: &mut Vec<Value>) -> Result<()> {
        if !self.opts.set_dedup() {
            for i in 0..items.len() {
                for j in (i + 1)..items.len() {
                    if items[i] == items[j] {
                        return Err(
                            self.err(Category::DuplicateSetElement, "duplicate set element")
                        );
                    }
                }
            }
            return Ok(());
        }
        let mut out: Vec<Value> = Vec::with_capacity(items.len());
        for v in items.drain(..) {
            if !out.contains(&v) {
                out.push(v);
            }
        }
        *items = out;
        Ok(())
    }

    /// Parses a parenthesized Tuple collection (`(...)`).
    /// Verifies adherence to the tuple schema limits.
    fn parse_paren(&mut self, depth: u32) -> Result<Value> {
        let depth = self.enter(depth)?;
        self.bump(); // (
        self.skip_ws();
        if self.peek() == Some(')') {
            self.bump();
            return Ok(Value::Tuple(Vec::new()));
        }
        if self.peek().is_none() {
            return Err(self.err(Category::UnexpectedEnd, "unterminated container"));
        }
        let mut items = Vec::new();
        // A discard may lead the container. `finish_seq` handles discards
        // between later elements, but the first element was read directly,
        // so `[#_ 1 2]` and `(#_ c 1)` were rejected while `{a:1 #_ b:2}`
        // and a top-level discard both worked.
        while self.at_discard() {
            self.discard_values(depth)?;
            self.skip_ws();
        }
        if self.peek() == Some(')') {
            self.bump();
            return Ok(Value::Tuple(Vec::new()));
        }
        let first = self.parse_value(depth)?;
        items.push(first);
        self.finish_seq(depth, ')', &mut items)?;
        Ok(Value::Tuple(items))
    }

    /// Parses a braced collection, dynamically differentiating between a Map and a Set.
    /// Distinguishes between the two by checking for a colon following the first element.
    fn parse_brace(&mut self, depth: u32) -> Result<Value> {
        let depth = self.enter(depth)?;
        self.bump(); // {
        self.skip_ws();
        if self.peek() == Some('}') {
            self.bump();
            return Ok(Value::Map(Vec::new()));
        }
        if self.peek().is_none() {
            return Err(self.err(Category::UnexpectedEnd, "unterminated container"));
        }
        // map (first element a `key:` pair) vs set (first element a bare value)
        if let Some(key) = self.try_key()? {
            let val = self.parse_value(depth)?;
            let mut pairs = Vec::new();
            self.put_pair(&mut pairs, key, val)?;
            self.finish_map(depth, '}', &mut pairs)?;
            Ok(Value::Map(pairs))
        } else {
            let first = self.parse_value(depth)?;
            let mut items = alloc::vec![first];
            self.finish_seq(depth, '}', &mut items)?;
            self.dedup_set(&mut items)?;
            Ok(Value::Set(items))
        }
    }

    /// Consumes the remaining elements of a sequential collection (List, Tuple, or Set) until the closing character.
    /// Manages comma separators and permits a single trailing comma.
    fn finish_seq(&mut self, depth: u32, close: char, items: &mut Vec<Value>) -> Result<()> {
        loop {
            self.skip_ws();
            if self.at_discard() {
                self.discard_values(depth)?;
                continue;
            }
            match self.peek() {
                Some(c) if c == close => {
                    self.bump();
                    self.check_container(items.len())?;
                    return Ok(());
                }
                None => return Err(self.err(Category::UnexpectedEnd, "unterminated container")),
                Some(',') => {
                    self.bump();
                    self.skip_ws();
                    if self.peek() == Some(close) {
                        // trailing comma allowed
                        self.bump();
                        self.check_container(items.len())?;
                        return Ok(());
                    }
                    if self.at_discard() {
                        continue;
                    }
                    items.push(self.parse_value(depth)?);
                }
                Some(_) => {
                    items.push(self.parse_value(depth)?);
                }
            }
        }
    }

    /// Enforce §16's `max_container` entry count.
    fn check_container(&self, size: usize) -> Result<()> {
        if size > self.opts.limits.max_container {
            return Err(self.err(
                Category::ResourceLimitExceeded,
                "too many entries in container",
            ));
        }
        Ok(())
    }

    /// Consumes the remaining key-value pairs of a mapping collection (Map or OrderedMap) until the closing character.
    /// Handles structural formatting and allows a single trailing comma.
    fn finish_map(
        &mut self,
        depth: u32,
        close: char,
        pairs: &mut Vec<(Value, Value)>,
    ) -> Result<()> {
        loop {
            self.skip_ws();
            if self.at_discard() {
                self.discard_pairs(depth, "map")?;
                continue;
            }
            match self.peek() {
                Some(c) if c == close => {
                    self.bump();
                    self.check_container(pairs.len())?;
                    return Ok(());
                }
                None => return Err(self.err(Category::UnexpectedEnd, "unterminated map")),
                Some(',') => {
                    self.bump();
                    self.skip_ws();
                    if self.peek() == Some(close) {
                        self.bump();
                        self.check_container(pairs.len())?;
                        return Ok(());
                    }
                    if self.at_discard() {
                        continue;
                    }
                    self.parse_pair(depth, pairs)?;
                }
                Some(_) => {
                    self.parse_pair(depth, pairs)?;
                }
            }
        }
    }

    /// Parses a single key-value pair within a map context.
    /// Verifies that the key adheres to the strict bare name or string format.
    fn parse_pair(&mut self, depth: u32, pairs: &mut Vec<(Value, Value)>) -> Result<()> {
        match self.try_key()? {
            Some(key) => {
                let val = self.parse_value(depth)?;
                self.put_pair(pairs, key, val)
            }
            None => Err(self.err(
                Category::UnexpectedToken,
                "a map key must be a bare name or a \"string\"",
            )),
        }
    }

    /// Try to read a map / ordered-map key at the current position: a bare name
    /// or a `"string"`, immediately followed by `:`. On success the colon is
    /// consumed and the key text is returned -- the semantic key is always a
    /// `String`, so a bare-name key and a string key with identical text denote
    /// the same key (AXON §3.7). Otherwise the position is restored and `None`
    /// is returned, letting the caller treat the container as a list/set.
    /// Reserved barewords (`true`/`false`/`null`) are not keys (§2.3).
    fn try_key(&mut self) -> Result<Option<String>> {
        let save = (self.pos, self.line, self.col);
        self.skip_ws();
        let key = match self.peek() {
            Some('"') => match self.parse_string('"')? {
                Value::String(s) => s,
                _ => unreachable!("parse_string always yields a String"),
            },
            Some(c) if self.name_start(c) => {
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if self.name_continue(c) {
                        self.bump();
                    } else {
                        break;
                    }
                }
                let name = self.chars[start..self.pos].to_owned();
                if name == "true" || name == "false" || name == "null" {
                    self.restore(save);
                    return Ok(None);
                }
                name
            }
            _ => {
                self.restore(save);
                return Ok(None);
            }
        };
        self.skip_ws();
        if self.eat(':') {
            Ok(Some(key))
        } else {
            self.restore(save);
            Ok(None)
        }
    }

    /// Restores the parser's internal position, line, and column state to a previously saved checkpoint.
    /// Essential for backtracking when attempting to read optional structures like map keys.
    fn restore(&mut self, save: (usize, u32, u32)) {
        self.pos = save.0;
        self.line = save.1;
        self.col = save.2;
    }

    // ---- names, nodes, constants ------------------------------------------ //

    /// Parses an identifier to determine if it is a reserved constant (`true`, `false`, `null`) or an element node.
    /// Resolves node bodies dynamically based on the proceeding tokens.
    fn parse_name_or_node(&mut self, depth: u32) -> Result<Value> {
        // A name is one or more `/`-joined segments, each a bare name or a
        // `'quoted'` name (§9.2, §9.3).
        let quoted_start = self.peek() == Some('\'');
        let name = self.parse_name()?;
        let save = (self.pos, self.line, self.col);

        // Baseline compatibility used a pending-name builder. It skipped all
        // trivia, bound only a following brace body (even across lines),
        // produced null before any other value, and retained a unit node only
        // at end of input.
        if self.opts.legacy_binding() {
            self.skip_ws();
            if !quoted_start {
                let constant = match name.as_str() {
                    "true" => Some(Value::Bool(true)),
                    "false" => Some(Value::Bool(false)),
                    "null" => Some(Value::Null),
                    _ => None,
                };
                if let Some(value) = constant {
                    if self.pos == save.0 && matches!(self.peek(), Some('{' | '(' | '[')) {
                        return Err(self.err(
                            Category::UnexpectedToken,
                            "reserved constant cannot have a body",
                        ));
                    }
                    self.restore(save);
                    return Ok(value);
                }
            }
            return match self.peek() {
                Some('{') => self.parse_node_brace(depth, name),
                Some(',') => Err(self.err(
                    Category::UnexpectedToken,
                    "comma after a pending legacy node name",
                )),
                Some(_) => Ok(Value::Null),
                None => {
                    self.restore(save);
                    Ok(Value::Node(Box::new(Node {
                        name,
                        style: NodeStyle::Unit,
                        attributes: Vec::new(),
                        children: Vec::new(),
                    })))
                }
            };
        }
        if quoted_start {
            // A quoted name is only ever a node name, so it must take a body or
            // stand alone as a unit node -- never fall through to a constant.
            return self.parse_node_body(depth, name);
        }
        // reserved constants
        match name.as_str() {
            "true" => {
                if self.peek() == Some('{') || self.peek() == Some('(') || self.peek() == Some('[')
                {
                    return Err(
                        self.err(Category::UnexpectedToken, "a constant cannot take a body")
                    );
                }
                return Ok(Value::Bool(true));
            }
            "false" => {
                if self.peek() == Some('{') || self.peek() == Some('(') || self.peek() == Some('[')
                {
                    return Err(
                        self.err(Category::UnexpectedToken, "a constant cannot take a body")
                    );
                }
                return Ok(Value::Bool(false));
            }
            "null" => {
                if self.peek() == Some('{') || self.peek() == Some('(') || self.peek() == Some('[')
                {
                    return Err(
                        self.err(Category::UnexpectedToken, "a constant cannot take a body")
                    );
                }
                return Ok(Value::Null);
            }
            _ => {}
        }
        self.parse_node_body(depth, name)
    }

    /// Bind a body to a node name, if one opens here. A body binds only when
    /// its opener is on the same line -- whitespace other than a newline may
    /// precede it -- otherwise the name stands alone as a unit node.
    fn parse_node_body(&mut self, depth: u32, name: String) -> Result<Value> {
        let save = (self.pos, self.line, self.col);
        self.skip_inline_ws();
        match self.peek() {
            Some('{') => self.parse_node_brace(depth, name),
            Some('(') => self.parse_node_seq(depth, name, NodeStyle::Tuple, '(', ')'),
            Some('[') => self.parse_node_seq(depth, name, NodeStyle::List, '[', ']'),
            _ => {
                // restore: it's a unit node
                self.restore(save);
                Ok(Value::Node(Box::new(Node {
                    name,
                    style: NodeStyle::Unit,
                    attributes: Vec::new(),
                    children: Vec::new(),
                })))
            }
        }
    }

    /// Read a name: one or more `/`-joined segments, each a bare name or a
    /// `'quoted'` name (§9.2 quoted names, §9.3 namespacing).
    fn parse_name(&mut self) -> Result<String> {
        let mut name = String::new();
        let mut slashes = 0;
        loop {
            if self.peek() == Some('\'') {
                let part = self.parse_quoted_name()?;
                name.push_str(&part);
            } else {
                let start = self.pos;
                if !matches!(self.peek(), Some(c) if self.name_start(c)) {
                    return Err(self.err(Category::InvalidName, "expected a name"));
                }
                while let Some(c) = self.peek() {
                    if self.name_continue(c) {
                        self.bump();
                    } else {
                        break;
                    }
                }
                let part = &self.chars[start..self.pos];
                if part.len() > self.opts.limits.max_bare_name {
                    return Err(self.err(
                        Category::ResourceLimitExceeded,
                        "bare name segment is too long",
                    ));
                }
                name.push_str(part);
            }
            if self.peek() != Some('/') {
                return Ok(name);
            }
            if slashes >= 1 {
                // §9.3: a namespaced name has exactly one `/` (audit M5).
                return Err(self.err(
                    Category::InvalidName,
                    "a name may contain at most one namespace slash",
                ));
            }
            slashes += 1;
            self.bump();
            name.push('/');
            if !(self.peek() == Some('\'') || matches!(self.peek(), Some(c) if self.name_start(c)))
            {
                return Err(self.err(
                    Category::InvalidName,
                    "namespace slash requires another name segment",
                ));
            }
        }
    }

    /// Read a `'quoted name'` (§9.2). A literal newline inside one is
    /// `invalid-name` in Core.
    fn parse_quoted_name(&mut self) -> Result<String> {
        self.bump(); // '
        let mut s = String::new();
        loop {
            let c = self
                .peek()
                .ok_or_else(|| self.err(Category::UnexpectedEnd, "unterminated quoted name"))?;
            if c == '\'' {
                self.bump();
                break;
            }
            // AXON 9.2 forbids a literal newline in a Core quoted name,
            // but the registry entry for `compat.lax-strings` lists
            // "newlines in quoted names" as precisely what it permits.
            // Rejecting unconditionally meant migration could not read
            // its own input: it parses with `Options::compat()`, and
            // Section 19 requires such a name to be *rewritten* to the
            // escaped Core spelling, not refused.
            if c == '\n' || c == '\r' {
                if !(self.opts.lax_strings || self.opts.compatibility) {
                    return Err(self.err(Category::InvalidName, "newline in quoted name"));
                }
                s.push(c);
                self.bump();
                continue;
            }
            if c == '\\' {
                self.bump();
                match self.peek() {
                    Some('\'') => {
                        s.push('\'');
                        self.bump();
                    }
                    Some('\\') => {
                        s.push('\\');
                        self.bump();
                    }
                    // AXON 9.2: quoted names take the Section 7.3 escape
                    // table, and Section 19 makes \n / \r the Core spelling
                    // for a name that held a literal newline. Accepting only
                    // \\ and \' meant this crate could not read a name its
                    // own documented migration path produces, and rejected
                    // documents the reference accepts.
                    Some('n') => {
                        s.push('\n');
                        self.bump();
                    }
                    Some('t') => {
                        s.push('\t');
                        self.bump();
                    }
                    Some('r') => {
                        s.push('\r');
                        self.bump();
                    }
                    Some('\n') => {
                        return Err(self.err(
                            Category::InvalidName,
                            "escaped a literal newline in a quoted name",
                        ));
                    }
                    _ => {
                        return Err(
                            self.err(Category::InvalidEscape, "invalid escape in quoted name")
                        );
                    }
                }
            } else {
                s.push(c);
                self.bump();
            }
        }
        if s.len() > self.opts.limits.max_quoted_name {
            return Err(self.err(Category::ResourceLimitExceeded, "quoted name is too long"));
        }
        Ok(s)
    }

    /// Advances the parser position past any contiguous inline whitespace (spaces or tabs).
    /// Does not consume structural formatting like newlines or comments.
    fn skip_inline_ws(&mut self) {
        while matches!(self.peek(), Some(' ') | Some('\t')) {
            self.bump();
        }
    }

    /// Parses a node body delimited by braces (`{...}`), consisting of attributes and child values.
    /// Associates the parsed components with the node's identifier name.
    fn parse_node_brace(&mut self, depth: u32, name: String) -> Result<Value> {
        // doc-link: cid("..") / link("..") are Links only under the profile, and
        // only for the tuple-body form; brace form is a plain node.
        let depth = self.enter(depth)?;
        self.bump(); // {
        let mut attributes: Vec<(String, Value)> = Vec::new();
        let mut children: Vec<Value> = Vec::new();
        loop {
            self.skip_ws();
            if self.at_discard() {
                // A discard in a node body covers whatever the body holds: a
                // pair while we are still reading attributes, a value once
                // children have started.
                if children.is_empty() {
                    let save = (self.pos, self.line, self.col);
                    let count = self.discard_count();
                    self.restore(save);
                    let mut pair_shaped = true;
                    if count > 0 {
                        let probe = (self.pos, self.line, self.col);
                        self.discard_count();
                        pair_shaped = self.try_attr_key()?.is_some() && {
                            self.skip_ws();
                            self.peek() == Some(':')
                        };
                        self.restore(probe);
                    }
                    if pair_shaped {
                        self.discard_attr_pairs(depth)?;
                    } else {
                        self.discard_values(depth)?;
                    }
                } else {
                    self.discard_values(depth)?;
                }
                continue;
            }
            match self.peek() {
                Some('}') => {
                    self.bump();
                    break;
                }
                None => return Err(self.err(Category::UnexpectedEnd, "unterminated node body")),
                Some(',') => {
                    self.bump();
                    continue;
                }
                _ => {}
            }
            // attribute (bare name or 'quoted' name followed by :) vs child value
            let save = (self.pos, self.line, self.col);
            let quoted_key = self.peek() == Some('\'');
            if let Some(key) = self.try_attr_key()? {
                self.skip_ws();
                if self.eat(':') {
                    // AXON 2.3: a reserved bareword is never a name. The check
                    // existed for map keys but not for node attributes, so
                    // `n{true:1}` was accepted here and produced a Node whose
                    // attribute name is `true` -- a document the Python
                    // reference rejects, and therefore one this crate could
                    // write and the reference could not read.
                    //
                    // Only the *bareword* is reserved: `n{'true':1}` is a
                    // quoted name and stays valid, and `n{true}` is still a
                    // boolean child because that path never reaches a `:`.
                    if !quoted_key && matches!(key.as_str(), "true" | "false" | "null") {
                        return Err(self.err(
                            Category::UnexpectedToken,
                            "a reserved bareword cannot be a name",
                        ));
                    }
                    let val = self.parse_value(depth)?;
                    if let Some(slot) = attributes.iter_mut().find(|(k, _)| *k == key) {
                        // §12.1 governs node attributes exactly as it governs
                        // map keys -- the reference routes both through `_put`.
                        if !self.opts.last_wins() {
                            return Err(self.err(
                                Category::DuplicateKey,
                                alloc::format!("duplicate key {}", python_string_repr(&key)),
                            ));
                        }
                        slot.1 = val;
                    } else {
                        attributes.push((key, val));
                        self.check_container(attributes.len())?;
                    }
                    continue;
                }
                // not an attribute; restore and parse as a value
                self.restore(save);
            }
            children.push(self.parse_value(depth)?);
            self.check_container(children.len())?;
        }
        Ok(Value::Node(Box::new(Node {
            name,
            style: NodeStyle::Brace,
            attributes,
            children,
        })))
    }

    /// Consume a run of `#_` tokens in a node's attribute position, discarding
    /// an `attr: value` for each.
    fn discard_attr_pairs(&mut self, depth: u32) -> Result<()> {
        for _ in 0..self.discard_count() {
            if self.try_attr_key()?.is_none() {
                return Err(self.err(
                    Category::UnexpectedToken,
                    "discard in attribute document requires a pair",
                ));
            }
            self.skip_ws();
            if !self.eat(':') {
                return Err(self.err(
                    Category::UnexpectedToken,
                    "discard in attribute document requires a pair",
                ));
            }
            let _ = self.parse_value(depth)?;
            self.skip_ws();
        }
        Ok(())
    }

    /// Try to read an attribute key: a bare name or a `'single-quoted'` name.
    /// Returns None if the next token is not a possible key.
    fn try_attr_key(&mut self) -> Result<Option<String>> {
        match self.peek() {
            Some('\'') => Ok(Some(self.parse_name()?)),
            Some(c) if self.name_start(c) => Ok(Some(self.parse_name()?)),
            _ => Ok(None),
        }
    }

    /// Parses a sequence-style node body (tuple or list style) and processes its child elements.
    /// Handles the specialized document-link resolution if the `doc_link` profile is enabled.
    fn parse_node_seq(
        &mut self,
        depth: u32,
        name: String,
        style: NodeStyle,
        open: char,
        close: char,
    ) -> Result<Value> {
        let depth = self.enter(depth)?;
        debug_assert_eq!(self.peek(), Some(open));
        self.bump(); // opener
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek().is_none() {
            return Err(self.err(Category::UnexpectedEnd, "unterminated container"));
        }
        if self.peek() != Some(close) {
            items.push(self.parse_value(depth)?);
            self.finish_seq(depth, close, &mut items)?;
        } else {
            self.bump();
        }
        // doc-link recognition (tuple body, single string arg). The target is
        // validated here: an unvalidated content address is not a content
        // address, and the reference raises `invalid-link` for both forms.
        if self.opts.doc_link
            && style == NodeStyle::Tuple
            && items.len() == 1
            && let Value::String(s) = &items[0]
        {
            if name == "cid" {
                validate_cid(s).map_err(|m| self.err(Category::InvalidLink, m))?;
                return Ok(Value::Link(Link::Cid(s.clone())));
            }
            if name == "link" {
                if !valid_absolute_uri(s) {
                    return Err(self.err(Category::InvalidLink, "link requires an absolute URI"));
                }
                return Ok(Value::Link(Link::Uri(s.clone())));
            }
        }
        Ok(Value::Node(Box::new(Node {
            name,
            style,
            attributes: Vec::new(),
            children: items,
        })))
    }
}

// ---- helpers -------------------------------------------------------------- //

fn python_string_repr(value: &str) -> String {
    let quote = if value.contains('\'') && !value.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut output = String::new();
    output.push(quote);
    for ch in value.chars() {
        match ch {
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            ch if ch == quote => {
                output.push('\\');
                output.push(ch);
            }
            ch if ch.is_control() => {
                let code = ch as u32;
                if code <= 0xff {
                    output.push_str(&alloc::format!("\\x{code:02x}"));
                } else if code <= 0xffff {
                    output.push_str(&alloc::format!("\\u{code:04x}"));
                } else {
                    output.push_str(&alloc::format!("\\U{code:08x}"));
                }
            }
            ch => output.push(ch),
        }
    }
    output.push(quote);
    output
}

/// Standardizes the string representation of an integer, eliminating redundancies such as `-0`.
/// Ensures canonical formatting for the internal integer value.
fn normalize_int(raw: &str) -> String {
    // raw has no separators; canonicalize -0 to 0 and strip a leading + (none).
    if raw == "-0" {
        "0".to_string()
    } else {
        raw.to_string()
    }
}

/// Extracts the mantissa and base-10 exponent from a raw decimal literal.
/// Preserves the precise scale and formatting of the original input.
/// Extract the mantissa and base-10 exponent from a raw decimal literal, or
/// `None` when the exponent cannot be represented in this decimal model's
/// `i32` scale. Returning `None` (rather than silently coercing an overflowing
/// exponent to 0, or panicking on the fractional adjustment) lets the caller
/// reject the literal cleanly (audit H7).
fn parse_decimal(body: &str, scale_significant: bool) -> Option<Decimal> {
    // body is a numeric literal without the d/D suffix, separators already
    // removed. Split into mantissa digits and a base-10 exponent, preserving
    // scale (trailing zeros stay).
    let neg = body.starts_with('-');
    let b = body.trim_start_matches('-');
    let (mantissa_digits, exp) = if let Some(epos) = b.find(['e', 'E']) {
        let (m, e) = b.split_at(epos);
        let e: i32 = e[1..].parse().ok()?;
        decimal_scale(m, e)?
    } else {
        decimal_scale(b, 0)?
    };
    let mantissa = if neg {
        alloc::format!("-{}", mantissa_digits)
    } else {
        mantissa_digits
    };
    Some(if scale_significant {
        Decimal::ScaledFinite {
            mantissa,
            exponent: exp,
        }
    } else {
        Decimal::Finite {
            mantissa,
            exponent: exp,
        }
    })
}

/// Computes the fractional scale for a decimal mantissa, dropping the decimal
/// point, or `None` when the adjusted exponent overflows `i32` (audit H7).
fn decimal_scale(m: &str, extra_exp: i32) -> Option<(String, i32)> {
    if let Some(dot) = m.find('.') {
        let frac_len = i32::try_from(m.len() - dot - 1).ok()?;
        let mut digits: String = m.chars().filter(|&c| c != '.').collect();
        // strip leading zeros but keep at least one digit
        while digits.len() > 1 && digits.starts_with('0') {
            digits.remove(0);
        }
        Some((digits, extra_exp.checked_sub(frac_len)?))
    } else {
        let mut digits = m.to_string();
        while digits.len() > 1 && digits.starts_with('0') {
            digits.remove(0);
        }
        Some((digits, extra_exp))
    }
}

/// Validate a `cid(..)` target: CIDv1, raw codec, sha2-256, in canonical
/// lowercase base32 multibase. Mirrors the reference's `parse_cid2026`.
fn validate_cid(text: &str) -> core::result::Result<(), String> {
    let payload = text
        .strip_prefix('b')
        .ok_or("CID must use lowercase base32 multibase")?;
    if payload.is_empty()
        || !payload
            .bytes()
            .all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
    {
        return Err("invalid lowercase base32 CID".to_string());
    }
    let binary = base32_decode(payload)?;
    if binary.len() != 36 || binary[..4] != [1, 85, 18, 32] {
        return Err("CID must be v1/raw/sha2-256-256".to_string());
    }
    // Reject a non-canonical spelling: re-encoding must reproduce the input.
    if base32_encode(&binary) != payload {
        return Err("non-canonical base32 CID".to_string());
    }
    Ok(())
}

/// Validate a `link(..)` target as an ASCII RFC-3986 absolute URI, without
/// resolving it. Mirrors the reference's `_valid_absolute_uri`.
fn valid_absolute_uri(target: &str) -> bool {
    if target.is_empty() || !target.is_ascii() {
        return false;
    }
    if target
        .bytes()
        .any(|b| b <= 32 || b == 127 || b"<>\"{}|\\^`".contains(&b))
    {
        return false;
    }
    // A `%` must introduce two hex digits.
    let bytes = target.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'%'
            && !(i + 2 < bytes.len()
                && bytes[i + 1].is_ascii_hexdigit()
                && bytes[i + 2].is_ascii_hexdigit())
        {
            return false;
        }
    }
    // scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":"
    let Some(colon) = target.find(':') else {
        return false;
    };
    let scheme = &target[..colon];
    if scheme.is_empty() || !scheme.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return false;
    }
    if !scheme
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.')
    {
        return false;
    }
    // Python's `urlsplit` plus `.port` access requires HTTP(S) links to carry
    // a non-empty authority and validates bracket/port structure.
    let lower_scheme = scheme.to_ascii_lowercase();
    if lower_scheme == "http" || lower_scheme == "https" {
        let remainder = &target[colon + 1..];
        let Some(after_slashes) = remainder.strip_prefix("//") else {
            return false;
        };
        let authority_end = after_slashes
            .find(['/', '?', '#'])
            .unwrap_or(after_slashes.len());
        let authority = &after_slashes[..authority_end];
        if authority.is_empty() || !valid_http_authority(authority) {
            return false;
        }
    }
    true
}

fn valid_http_authority(authority: &str) -> bool {
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, value)| value);
    if host_port.is_empty() {
        return false;
    }
    let port = if let Some(rest) = host_port.strip_prefix('[') {
        let Some(close) = rest.find(']') else {
            return false;
        };
        if close == 0 {
            return false;
        }
        let suffix = &rest[close + 1..];
        if suffix.is_empty() {
            None
        } else {
            let Some(port) = suffix.strip_prefix(':') else {
                return false;
            };
            Some(port)
        }
    } else if host_port.contains(['[', ']']) {
        return false;
    } else if let Some((host, port)) = host_port.rsplit_once(':') {
        if host.is_empty() || host.contains(':') {
            return false;
        }
        Some(port)
    } else {
        None
    };
    match port {
        None => true,
        Some(text) if !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()) => text
            .parse::<u32>()
            .is_ok_and(|value| value <= u16::MAX as u32),
        Some(_) => false,
    }
}

/// Decode canonical lowercase base32 (RFC 4648, no padding) for CID checking.
pub(crate) fn base32_decode(s: &str) -> core::result::Result<Vec<u8>, String> {
    let mut bits: u32 = 0;
    let mut nbits: u32 = 0;
    let mut out = Vec::new();
    for b in s.bytes() {
        let v = match b {
            b'a'..=b'z' => b - b'a',
            b'2'..=b'7' => b - b'2' + 26,
            _ => return Err("invalid base32 CID".to_string()),
        } as u32;
        bits = (bits << 5) | v;
        nbits += 5;
        if nbits >= 8 {
            nbits -= 8;
            out.push((bits >> nbits) as u8);
            bits &= (1 << nbits) - 1;
        }
    }
    Ok(out)
}

/// Encode canonical lowercase base32 (RFC 4648, padding stripped).
pub(crate) fn base32_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut out = String::new();
    let mut bits: u32 = 0;
    let mut nbits: u32 = 0;
    for &b in data {
        bits = (bits << 8) | b as u32;
        nbits += 8;
        while nbits >= 5 {
            nbits -= 5;
            out.push(ALPHABET[((bits >> nbits) & 31) as usize] as char);
            bits &= (1 << nbits) - 1;
        }
    }
    if nbits > 0 {
        out.push(ALPHABET[((bits << (5 - nbits)) & 31) as usize] as char);
    }
    out
}

/// Rewrite a legacy indentation-bound document into Core brace form, inserting
/// `{`/`}` around bodies that historical pyaxon bound by indentation (the
/// `indented_bodies` compat flag). Mirrors the reference's
/// `_rewrite_indented_bodies`, applied to the whole source before parsing.
pub(crate) fn rewrite_indented_bodies(source: &str) -> String {
    let lines: Vec<&str> = split_keep_ends(source);
    // The indent of the next significant (non-blank, non-comment) line after
    // each line index.
    let mut next_significant_indent: Vec<Option<usize>> = alloc::vec![None; lines.len()];
    let mut last_sig: Option<usize> = None;
    for (i, line) in lines.iter().enumerate().rev() {
        let content = line.trim_start_matches([' ', '\t']);
        let is_sig = !content.trim().is_empty() && !content.starts_with('#');
        next_significant_indent[i] = last_sig.map(|j| indent_width(lines[j]));
        if is_sig {
            last_sig = Some(i);
        }
    }
    let mut out = String::new();
    let mut stack: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let stripped = line.trim_start_matches([' ', '\t']);
        let is_sig = !stripped.trim().is_empty() && !stripped.starts_with('#');
        if is_sig {
            let indent = indent_width(line);
            while let Some(&top) = stack.last() {
                if indent <= top {
                    stack.pop();
                    for _ in 0..top {
                        out.push(' ');
                    }
                    out.push_str("}\n");
                } else {
                    break;
                }
            }
            let code = stripped.trim_end_matches(['\r', '\n']);
            if indented_name(code)
                && let Some(next_indent) = next_significant_indent[i]
                && next_indent > indent
            {
                let lead = &line[..line.len() - stripped.len()];
                let newline = if line.ends_with("\r\n") { "\r\n" } else { "\n" };
                out.push_str(lead);
                out.push_str(code);
                out.push('{');
                out.push_str(newline);
                stack.push(indent);
                continue;
            }
        }
        out.push_str(line);
    }
    while let Some(top) = stack.pop() {
        for _ in 0..top {
            out.push(' ');
        }
        out.push_str("}\n");
    }
    out
}

/// Split into lines, keeping the line terminators, matching Python's
/// `str.splitlines(keepends=True)` for the `\n` / `\r\n` cases AXON documents use.
fn split_keep_ends(source: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let bytes = source.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            lines.push(&source[start..=i]);
            start = i + 1;
        }
        i += 1;
    }
    if start < source.len() {
        lines.push(&source[start..]);
    }
    lines
}

/// The display width of a line's leading indentation, with tabs rounded up to
/// the next multiple of 8 (matching the reference's `_indent_width`).
fn indent_width(line: &str) -> usize {
    let mut width = 0;
    for ch in line.chars() {
        match ch {
            ' ' => width += 1,
            '\t' => width = (width / 8 + 1) * 8,
            _ => break,
        }
    }
    width
}

/// True when `text` (with an optional trailing `:`) is a bare, possibly
/// namespaced name -- the condition for an indented line to open a node body.
fn indented_name(text: &str) -> bool {
    let text = text.strip_suffix(':').map(|t| t.trim_end()).unwrap_or(text);
    !text.is_empty()
        && text.split('/').all(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) if is_name_start(c) => chars.all(is_name_continue),
                _ => false,
            }
        })
}

/// Days in a Gregorian month, honoring leap years -- the check that makes
/// `^2026-02-30` and `^2026-04-31` the errors they are.
fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Parse the text after `^` into a temporal, or return an error message.
fn parse_temporal_text(raw: &str, opts: &Options) -> core::result::Result<Value, String> {
    let lax = opts.lax_temporals();
    let raw_fraction = opts.raw_fraction();
    // Split off a named-zone suffix `[Area/Zone]` if present.
    let (core_part, zone_name) = if let Some(open) = raw.find('[') {
        if !raw.ends_with(']') {
            return Err("malformed zone suffix".to_string());
        }
        let name = &raw[open + 1..raw.len() - 1];
        if name.is_empty() || !name.contains('/') {
            return Err("invalid IANA time-zone name".to_string());
        }
        (&raw[..open], Some(name.to_string()))
    } else {
        (raw, None)
    };

    if let Some(tpos) = core_part.find('T') {
        let date = parse_date(&core_part[..tpos], lax)?;
        let time = parse_time(
            &core_part[tpos + 1..],
            zone_name,
            lax,
            raw_fraction,
            opts.scale_significant,
        )?;
        validate_named_zone(&date, &time)?;
        Ok(Value::Temporal(Temporal::DateTime(DateTime { date, time })))
    } else if core_part.contains(':') {
        if zone_name.is_some() {
            return Err("named time zones require a datetime".to_string());
        }
        let time = parse_time(
            core_part,
            zone_name,
            lax,
            raw_fraction,
            opts.scale_significant,
        )?;
        Ok(Value::Temporal(Temporal::Time(time)))
    } else {
        if zone_name.is_some() {
            return Err("named time zones require a datetime".to_string());
        }
        let date = parse_date(core_part, lax)?;
        Ok(Value::Temporal(Temporal::Date(date)))
    }
}

/// Parses the date component of a temporal value string (YYYY-MM-DD format).
/// Validates field lengths, numeric bounds, and padding correctness.
fn parse_date(s: &str, lax: bool) -> core::result::Result<Date, String> {
    let mut it = s.split('-');
    let y = it.next().ok_or("missing year")?;
    let m = it.next().ok_or("missing month")?;
    let d = it.next().ok_or("missing day")?;
    if it.next().is_some() {
        return Err("too many date fields".to_string());
    }
    let widths_ok = if lax {
        (1..=4).contains(&y.len()) && (1..=2).contains(&m.len()) && (1..=2).contains(&d.len())
    } else {
        y.len() == 4 && m.len() == 2 && d.len() == 2
    };
    if !widths_ok || !all_digits(y) || !all_digits(m) || !all_digits(d) {
        return Err("date must be YYYY-MM-DD".to_string());
    }
    let year: i32 = y.parse().map_err(|_| "bad year")?;
    let month: u8 = m.parse().map_err(|_| "bad month")?;
    let day: u8 = d.parse().map_err(|_| "bad day")?;
    // The reference builds a `datetime.date`, which rejects year 0 and any
    // day past the month's real length; match that exactly rather than the
    // looser 1..=31 bound.
    if !(1..=9999).contains(&year) {
        return Err(alloc::format!("year {year} is out of range"));
    }
    if !(1..=12).contains(&month) {
        return Err("month must be in 1..12".to_string());
    }
    if day < 1 || day > days_in_month(year, month) {
        return Err("day is out of range for month".to_string());
    }
    Ok(Date { year, month, day })
}

/// True when every byte is an ASCII digit (and there is at least one).
fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Parses the time component of a temporal value string, including fractional seconds.
/// Processes standard hours, minutes, and seconds while managing the associated time zone.
fn parse_time(
    s: &str,
    zone_name: Option<String>,
    lax: bool,
    raw_fraction: bool,
    scale_significant: bool,
) -> core::result::Result<Time, String> {
    // separate the zone/offset
    let (hms, zone) = split_zone(s, zone_name, lax)?;
    let mut parts = hms.splitn(3, ':');
    let h = parts.next().ok_or("missing hour")?;
    let mi = parts.next().ok_or("invalid time")?;
    let rest = parts.next();
    // Core field widths (§6.3): hour 1-2 digits, minute and second exactly 2.
    // `^12:5` is invalid -- the reference's _TIME regex is `[0-9]{1,2}:[0-9]{2}`.
    let minute_width_ok = if lax {
        (1..=2).contains(&mi.len())
    } else {
        mi.len() == 2
    };
    if !(1..=2).contains(&h.len()) || !all_digits(h) || !minute_width_ok || !all_digits(mi) {
        return Err("invalid time".to_string());
    }
    let hour: u8 = h.parse().map_err(|_| "bad hour")?;
    let minute: u8 = mi.parse().map_err(|_| "bad minute")?;
    let (second, nanosecond, fraction_digits) = if let Some(r) = rest {
        let (sec_text, frac) = match r.find('.') {
            Some(dot) => (&r[..dot], Some(&r[dot + 1..])),
            None => (r, None),
        };
        let sec_width_ok = if lax {
            (1..=2).contains(&sec_text.len())
        } else {
            sec_text.len() == 2
        };
        if !sec_width_ok || !all_digits(sec_text) {
            return Err("invalid time".to_string());
        }
        let sec: u8 = sec_text.parse().map_err(|_| "bad second")?;
        let nanos = match frac {
            None => 0,
            Some(f) => {
                if f.is_empty() || f.len() > 9 || !all_digits(f) {
                    return Err("bad fractional seconds".to_string());
                }
                if raw_fraction {
                    // Legacy pyaxon read a fraction as microseconds.
                    f[..f.len().min(6)]
                        .parse::<u32>()
                        .map_err(|_| "bad fraction")?
                        * 1000
                } else {
                    // positional nanoseconds: right-pad to 9 digits
                    let mut padded = f.to_string();
                    while padded.len() < 9 {
                        padded.push('0');
                    }
                    padded.parse::<u32>().map_err(|_| "bad fraction")?
                }
            }
        };
        (
            sec,
            nanos,
            frac.filter(|text| !text.is_empty())
                .filter(|_| scale_significant)
                .map(|text| text.len() as u8),
        )
    } else {
        (0, 0, None)
    };
    if hour > 23 || minute > 59 || second > 59 {
        return Err("time out of range".to_string());
    }
    if nanosecond > 999_999_999 {
        return Err("fraction exceeds nanosecond range".to_string());
    }
    Ok(Time {
        hour,
        minute,
        second,
        nanosecond,
        fraction_digits,
        zone,
    })
}

fn validate_named_zone(date: &Date, time: &Time) -> core::result::Result<(), String> {
    let Zone::Named { offset, name, .. } = &time.zone else {
        return Ok(());
    };
    let zone: Tz = name
        .parse()
        .map_err(|_| alloc::format!("unknown IANA time zone {:?}", name))?;
    let local = NaiveDate::from_ymd_opt(date.year, date.month as u32, date.day as u32)
        .and_then(|day| {
            day.and_hms_nano_opt(
                time.hour as u32,
                time.minute as u32,
                time.second as u32,
                time.nanosecond,
            )
        })
        .ok_or_else(|| "invalid named-zone datetime".to_string())?;
    let matches = |candidate: chrono::DateTime<Tz>| {
        candidate.offset().fix().local_minus_utc() / 60 == *offset
    };
    let valid = match zone.from_local_datetime(&local) {
        chrono::LocalResult::Single(value) => matches(value),
        chrono::LocalResult::Ambiguous(first, second) => matches(first) || matches(second),
        chrono::LocalResult::None => false,
    };
    if valid {
        Ok(())
    } else {
        Err("numeric offset does not match named time zone".to_string())
    }
}

/// Return the bundled IANA tzdb version used for RFC 9557 validation.
pub fn tzdb_version() -> String {
    alloc::format!("chrono-tz:{}", chrono_tz::IANA_TZDB_VERSION)
}

/// Separates the core time string from its trailing zone indicator.
/// Identifies local time, Zulu time, or offset-based zones.
fn split_zone(
    s: &str,
    zone_name: Option<String>,
    lax: bool,
) -> core::result::Result<(&str, Zone), String> {
    if let Some(name) = zone_name {
        // named zone: the offset precedes the [..]; find +/- after the seconds
        if let Some(p) = find_offset_sign(s) {
            let off = parse_offset(&s[p..], lax)?;
            return Ok((
                &s[..p],
                Zone::Named {
                    offset: off,
                    name,
                    zulu: false,
                },
            ));
        }
        // `Z` is an offset too. The reference requires only *an* offset here,
        // so `^..Z[Europe/London]` is valid and this rejected it -- RFC 9557
        // permits the spelling and 6.6 keeps it distinct from `+00:00`.
        if let Some(stripped) = s.strip_suffix('Z') {
            return Ok((
                stripped,
                Zone::Named {
                    offset: 0,
                    name,
                    zulu: true,
                },
            ));
        }
        return Err("named time zones require a numeric offset".to_string());
    }
    if let Some(stripped) = s.strip_suffix('Z') {
        return Ok((stripped, Zone::Zulu));
    }
    if let Some(p) = find_offset_sign(s) {
        let off = parse_offset(&s[p..], lax)?;
        return Ok((&s[..p], Zone::Offset(off)));
    }
    Ok((s, Zone::Local))
}

/// Locates the sign (`+` or `-`) denoting a time zone offset.
/// Ignores any leading signs that might belong to the broader temporal structure.
fn find_offset_sign(s: &str) -> Option<usize> {
    // an offset sign appears after the time digits, not as the first char
    s.char_indices()
        .skip(1)
        .find(|&(_, c)| c == '+' || c == '-')
        .map(|(i, _)| i)
}

/// Evaluates a numeric time zone offset in hours and minutes.
/// Normalizes the offset into a signed integer representing the total minutes.
fn parse_offset(s: &str, lax: bool) -> core::result::Result<i32, String> {
    let sign = match s.as_bytes().first() {
        Some(b'+') => 1,
        Some(b'-') => -1,
        _ => return Err("bad offset sign".to_string()),
    };
    let body = &s[1..];
    let (h, m) = match body.split_once(':') {
        Some((h, m)) => (h, m),
        // Core requires `+/-HH:MM`; legacy readers also accept `+/-HHMM` / `+/-HH`.
        None if lax && body.len() > 2 => (&body[..body.len() - 2], &body[body.len() - 2..]),
        None if lax => (body, "0"),
        None => return Err("offset requires :MM".to_string()),
    };
    if m.contains(':') {
        return Err("offset has too many fields".to_string());
    }
    let widths_ok = if lax {
        (1..=2).contains(&h.len()) && (1..=2).contains(&m.len())
    } else {
        h.len() == 2 && m.len() == 2
    };
    if !widths_ok || !all_digits(h) || !all_digits(m) {
        return Err("offset fields must be zero-padded".to_string());
    }
    let hours: i32 = h.parse().map_err(|_| "bad offset hour")?;
    let mins: i32 = m.parse().map_err(|_| "bad offset minute")?;
    // Each field is bounded independently: `+00:60` is not a spelling of
    // `+01:00`, it is an invalid offset. The reference checks
    // `hours > 23 or minutes > 59` -- a total-only bound let it through.
    let max_hours = if lax { 24 } else { 23 };
    if hours > max_hours || mins > 59 || (hours == 24 && mins != 0) {
        return Err("offset out of range".to_string());
    }
    Ok(sign * (hours * 60 + mins))
}

/// Minimal standard-alphabet base64 decoder (no external dep).
/// Constructs a byte vector by bitwise-combining parsed base64 characters.
fn base64_decode(s: &str) -> core::result::Result<Vec<u8>, String> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    // `=` may appear only as trailing padding (audit H8): the previous code
    // stripped every `=` regardless of position, so `=YQ==`, `Y=Q=`, `YQ===`
    // were all accepted. Padding must be 0/1/2 trailing chars, none may appear
    // inside the data, and -- when present -- it must complete a 4-char group.
    let bytes = s.as_bytes();
    let pad = bytes.iter().rev().take_while(|&&b| b == b'=').count();
    let data = &bytes[..bytes.len() - pad];
    if data.contains(&b'=') {
        return Err("base64 padding may only be trailing".to_string());
    }
    if pad > 2 {
        return Err("excess base64 padding".to_string());
    }
    if pad > 0 && !bytes.len().is_multiple_of(4) {
        return Err("padded base64 must be a multiple of four".to_string());
    }
    let rem = data.len() % 4;
    if rem == 1 {
        return Err("invalid base64 length".to_string());
    }
    if pad > 0 && pad != (4 - rem) % 4 {
        return Err("incorrect base64 padding count".to_string());
    }
    let mut out = Vec::with_capacity(data.len() / 4 * 3);
    let mut chunk = [0u8; 4];
    let mut n = 0;
    for &b in data {
        chunk[n] = val(b).ok_or("invalid base64 character")?;
        n += 1;
        if n == 4 {
            out.push((chunk[0] << 2) | (chunk[1] >> 4));
            out.push((chunk[1] << 4) | (chunk[2] >> 2));
            out.push((chunk[2] << 6) | chunk[3]);
            n = 0;
        }
    }
    match n {
        0 => {}
        2 => out.push((chunk[0] << 2) | (chunk[1] >> 4)),
        3 => {
            out.push((chunk[0] << 2) | (chunk[1] >> 4));
            out.push((chunk[1] << 4) | (chunk[2] >> 2));
        }
        _ => return Err("invalid base64 length".to_string()),
    }
    Ok(out)
}
