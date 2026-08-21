//! Migration of documented legacy AXON spellings to AXON 2026 Core.
//!
//! Canonical migration reads with the complete compatibility bundle and emits
//! one deterministic Canonical AXON document stream. Selective migration first
//! repairs presentation-local legacy spellings through the lossless CST, then
//! proves that the Core result has the same canonical semantics. If that proof
//! fails, it reports an explicit canonical fallback instead of risking drift.

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use core::ops::Deref;

use crate::cst::{migrate_indented, parse_cst};
use crate::de::from_str_stream_with;
use crate::error::{Category, Error, Result};
use crate::parser::{Options, rewrite_indented_bodies};
use crate::value::Value;
use crate::writer;

/// One deterministic, editor-facing migration notice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationEvent {
    /// Stable machine-readable event code.
    pub code: String,
    /// Human-readable explanation of the rewrite or warning.
    pub message: String,
    /// UTF-8 byte offset in the original source.
    pub offset: usize,
}

/// A non-overlapping UTF-8 byte edit produced by selective migration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationEdit {
    /// Stable machine-readable edit code.
    pub code: String,
    /// Inclusive UTF-8 byte offset in the source.
    pub start: usize,
    /// Exclusive UTF-8 byte offset in the source.
    pub end: usize,
    /// Unicode replacement text.
    pub replacement: String,
}

/// How a migration result was produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationMode {
    /// The requested deterministic whole-document canonical rewrite.
    Canonical,
    /// A semantics-proven selective rewrite that retained unaffected text.
    Selective,
    /// Selective proof failed, so the deterministic canonical rewrite was used.
    CanonicalFallback,
}

impl MigrationMode {
    /// Stable spelling used by fixtures and transport adapters.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::Selective => "selective",
            Self::CanonicalFallback => "canonical-fallback",
        }
    }
}

impl fmt::Display for MigrationMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Migrated text together with its immutable event and edit report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationResult {
    /// Migrated AXON 2026 Core text.
    pub text: String,
    /// Rewrites and compatibility warnings observed in the original source.
    pub events: Vec<MigrationEvent>,
    /// UTF-8 edits that reproduce [`Self::text`] from the original source.
    pub edits: Vec<MigrationEdit>,
    /// Whether the output is canonical, selective, or a canonical fallback.
    pub mode: MigrationMode,
}

impl AsRef<str> for MigrationResult {
    fn as_ref(&self) -> &str {
        &self.text
    }
}

impl Deref for MigrationResult {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

impl fmt::Display for MigrationResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.text)
    }
}

/// Canonically migrate a legacy AXON document without enabling Graph syntax.
pub fn migrate(source: &str) -> Result<MigrationResult> {
    migrate_with(source, false, false)
}

/// Selectively migrate a legacy AXON document without enabling Graph syntax.
pub fn migrate_selective(source: &str) -> Result<MigrationResult> {
    migrate_with(source, false, true)
}

/// Canonically migrate a legacy AXON document with optional Graph syntax.
pub fn migrate_with_graph(source: &str, graph: bool) -> Result<MigrationResult> {
    migrate_with(source, graph, false)
}

/// Selectively migrate a legacy AXON document with optional Graph syntax.
pub fn migrate_selective_with_graph(source: &str, graph: bool) -> Result<MigrationResult> {
    migrate_with(source, graph, true)
}

/// Above this input size, selective migration skips its superlinear diff and
/// returns the linear canonical rewrite instead (audit H5). Matches the
/// reference's `_SELECTIVE_MIGRATION_LIMIT`.
const SELECTIVE_MIGRATION_LIMIT: usize = 65536;

/// Migrate a legacy AXON document with explicit Graph and selective controls.
pub fn migrate_with(source: &str, graph: bool, selective: bool) -> Result<MigrationResult> {
    let compatibility = compatibility_options(graph);
    let values = compatibility_values(source, compatibility)?;
    validate_defined_references(&values)?;
    let rendered = writer::to_string_stream_canonical(&values)?;
    let events = migration_events(source, &rendered, graph)?;

    // The selective diff is superlinear; for large inputs fall back to the
    // linear canonical rewrite so a few kilobytes cannot monopolize CPU (H5).
    if !selective || source.len() > SELECTIVE_MIGRATION_LIMIT {
        return Ok(MigrationResult {
            text: rendered,
            events,
            edits: Vec::new(),
            mode: if selective {
                MigrationMode::CanonicalFallback
            } else {
                MigrationMode::Canonical
            },
        });
    }

    let candidate = selective_candidate(source, compatibility)?;
    let core = Options {
        graph,
        ..Options::default()
    };
    let equivalent = match from_str_stream_with(&candidate, core) {
        Ok(candidate_values) => {
            validate_defined_references(&candidate_values).is_ok()
                && writer::to_string_stream_canonical(&candidate_values)
                    .is_ok_and(|candidate| candidate == rendered)
        }
        Err(_) => false,
    };
    let (text, mode) = if equivalent {
        (candidate, MigrationMode::Selective)
    } else {
        (rendered, MigrationMode::CanonicalFallback)
    };
    let edits = derive_migration_edits(source, &text);
    debug_assert_eq!(
        apply_migration_edits(source, &edits).ok().as_deref(),
        Some(text.as_str())
    );
    Ok(MigrationResult {
        text,
        events,
        edits,
        mode,
    })
}

/// Apply validated, non-overlapping migration edits to UTF-8 text.
pub fn apply_migration_edits(source: &str, edits: &[MigrationEdit]) -> Result<String> {
    let mut ordered: Vec<&MigrationEdit> = edits.iter().collect();
    ordered.sort_by_key(|edit| (edit.start, edit.end));
    let mut previous_end = 0usize;
    for edit in &ordered {
        if edit.start < previous_end || edit.start > edit.end {
            return Err(migration_error(
                Category::UnexpectedToken,
                "migration edits overlap or have invalid bounds",
            ));
        }
        if edit.end > source.len() {
            return Err(migration_error(
                Category::UnexpectedToken,
                "migration edit exceeds source length",
            ));
        }
        if !source.is_char_boundary(edit.start) {
            return Err(migration_error(
                Category::InvalidUnicode,
                "migration edit start is not a UTF-8 scalar boundary",
            ));
        }
        if !source.is_char_boundary(edit.end) {
            return Err(migration_error(
                Category::InvalidUnicode,
                "migration edit end is not a UTF-8 scalar boundary",
            ));
        }
        previous_end = edit.end;
    }

    let mut output = source.to_string();
    for edit in ordered.into_iter().rev() {
        output.replace_range(edit.start..edit.end, &edit.replacement);
    }
    Ok(output)
}

fn migration_error(category: Category, message: &str) -> Error {
    Error::new(category, message, 0, 0)
}

fn compatibility_options(graph: bool) -> Options {
    Options {
        graph,
        ..Options::compat()
    }
}

fn compatibility_values(source: &str, options: Options) -> Result<Vec<Value>> {
    // The Rust Core parser represents names directly as unit nodes. Baseline
    // compatibility used a pending-name builder instead: a name before a brace
    // binds that brace, while a name before any other value contributes null.
    // Reconstruct that source-level behavior locally for migration without
    // weakening the Core parser.
    let expanded = rewrite_indented_bodies(source);
    let repaired = repair_legacy_compatibility(&expanded, options)?;
    from_str_stream_with(&repaired, options)
}

fn repair_legacy_compatibility(source: &str, options: Options) -> Result<String> {
    let document = parse_cst(source, &options)?;
    let structural: Vec<_> = document
        .tokens
        .iter()
        .filter(|token| !token.trivia)
        .collect();
    let mut depths = Vec::with_capacity(structural.len());
    let mut depth = 0usize;
    for token in &structural {
        if matches!(token.text.as_str(), ")" | "]" | "}") {
            depth = depth.saturating_sub(1);
        }
        depths.push(depth);
        if matches!(token.text.as_str(), "(" | "[" | "{") {
            depth += 1;
        }
    }

    let mut edits = Vec::new();
    for (index, token) in structural.iter().enumerate() {
        if (token.kind == "temporal" || (token.kind == "atom" && is_bare_temporal(&token.text)))
            && let Some(replacement) = normalize_legacy_temporal_fraction(&token.text)
        {
            edits.push(MigrationEdit {
                code: "temporal-fraction".to_string(),
                start: token.span.start,
                end: token.span.end,
                replacement,
            });
            continue;
        }
        // A leading plus is not a documented compatibility spelling. Do not
        // reinterpret its tokenizer `name` token as a pending legacy node.
        if token.text.starts_with('+') {
            continue;
        }
        if token.kind != "name" && token.kind != "quoted-name" {
            continue;
        }
        // AXON 2.3: a reserved bareword is never a name, so it can never be a
        // pending legacy node name either. The tokenizer classifies `true`,
        // `false` and `null` as `name` tokens, and without this they were
        // rewritten to `null` by the pending-name rule -- `true []` migrated to
        // `null []`, silently changing the value. The quoted spelling
        // `'true'` *is* a name and is deliberately unaffected.
        if token.kind == "name" && matches!(token.text.as_str(), "true" | "false" | "null") {
            continue;
        }
        if index != 0
            && matches!(
                structural[index - 1].kind,
                "anchor" | "reference" | "constant" | "temporal-marker"
            )
        {
            continue;
        }
        let next = structural.get(index + 1);
        if next.is_some_and(|following| following.text == ":" || following.text == "{") {
            continue;
        }
        let nested = depths[index] != 0;
        let followed_by_value = next.is_some();
        if !nested && !followed_by_value {
            continue;
        }
        if next.is_some_and(|following| following.text == ",") {
            return Err(Error::new(
                Category::UnexpectedToken,
                "comma after a pending legacy node name",
                token.span.line,
                token.span.column,
            ));
        }
        // Separate `null` from whatever follows whenever the source did not.
        // Only `(` and `[` were spaced before, so `'quoted name'1_0[1 2]`
        // became `null1_0[1 2]` -- one fused bareword -- and the `1_0` was
        // lost from the migrated document. Data loss, not a diagnostic
        // difference. A redundant space is harmless: canonical output
        // normalizes it away.
        let needs_space = next.is_some_and(|following| following.span.start <= token.span.end);
        edits.push(MigrationEdit {
            code: "legacy-binding".to_string(),
            start: token.span.start,
            end: token.span.end,
            replacement: if needs_space {
                "null ".to_string()
            } else {
                "null".to_string()
            },
        });
    }
    apply_migration_edits(source, &edits)
}

fn validate_defined_references(values: &[Value]) -> Result<()> {
    let mut anchors: Vec<&str> = Vec::new();
    let mut stack: Vec<&Value> = values.iter().rev().collect();
    while let Some(value) = stack.pop() {
        match value {
            Value::Undefined => {
                return Err(Error::new(
                    Category::UnknownReference,
                    "undefined sentinel has no Core spelling",
                    1,
                    1,
                ));
            }
            Value::Anchor(anchor) => {
                anchors.push(&anchor.label);
                stack.push(&anchor.value);
            }
            Value::Ref(label) => {
                if !anchors.iter().any(|known| *known == label) {
                    return Err(Error::new(
                        Category::UnknownReference,
                        "undefined sentinel has no Core spelling",
                        1,
                        1,
                    ));
                }
            }
            Value::List(items) | Value::Tuple(items) | Value::Set(items) => {
                stack.extend(items.iter().rev());
            }
            Value::Map(pairs) | Value::OrderedMap(pairs) => {
                for (key, child) in pairs.iter().rev() {
                    stack.push(child);
                    stack.push(key);
                }
            }
            Value::Node(node) => {
                stack.extend(node.children.iter().rev());
                for (_, child) in node.attributes.iter().rev() {
                    stack.push(child);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn selective_candidate(source: &str, options: Options) -> Result<String> {
    let mut candidate = migrate_indented(source, &options)?;
    let document = parse_cst(&candidate, &options)?;
    let mut edits = Vec::new();

    for token in &document.tokens {
        if token.kind == "comment" && token.text.starts_with("#_") {
            edits.push(MigrationEdit {
                code: "token-normalization".to_string(),
                start: token.span.start + 1,
                end: token.span.start + 1,
                replacement: " ".to_string(),
            });
            continue;
        }

        let rewrite = token.kind == "temporal"
            || token.kind == "binary"
            || (token.kind == "atom"
                && (is_bare_temporal(&token.text)
                    || is_leading_zero_number(&token.text)
                    || is_trailing_point_number(&token.text)))
            || ((token.kind == "string" || token.kind == "quoted-name")
                && token
                    .text
                    .chars()
                    .any(|character| character == '\\' || character == '\n' || character == '\r'));
        if !rewrite {
            continue;
        }

        let normalized_fraction = normalize_legacy_temporal_fraction(&token.text);
        let token_source = normalized_fraction.as_deref().unwrap_or(&token.text);
        let Ok(values) = compatibility_values(token_source, options) else {
            continue;
        };
        if values.len() != 1 || validate_defined_references(&values).is_err() {
            continue;
        }
        let replacement = writer::to_string(&values[0])?;
        if replacement != token.text {
            edits.push(MigrationEdit {
                code: "token-normalization".to_string(),
                start: token.span.start,
                end: token.span.end,
                replacement,
            });
        }
    }
    if !edits.is_empty() {
        candidate = apply_migration_edits(&candidate, &edits)?;
    }
    Ok(candidate)
}

fn migration_events(source: &str, rendered: &str, graph: bool) -> Result<Vec<MigrationEvent>> {
    let compatibility = compatibility_options(graph);
    let document = parse_cst(source, &compatibility)?;
    let mut events = Vec::new();

    if rewrite_indented_bodies(source) != source {
        push_event(
            &mut events,
            "indented-body",
            "rewrote an indentation-bound body with Core braces",
            0,
        );
    }

    let structural: Vec<_> = document
        .tokens
        .iter()
        .filter(|token| !token.trivia)
        .collect();
    for token in &document.tokens {
        if token.kind == "comment" && token.text.starts_with("#_") {
            push_event(
                &mut events,
                "hash-underscore-comment",
                "separated the legacy #_ comment opener from Core discard",
                token.span.start,
            );
        }
        if token.kind == "string" || token.kind == "quoted-name" {
            for relative in continuation_offsets(&token.text) {
                push_event(
                    &mut events,
                    "legacy-continuation",
                    "preserved the baseline continuation value with Core escapes",
                    token.span.start + relative,
                );
            }
        }
        if token.kind == "quoted-name"
            && token
                .text
                .chars()
                .any(|character| character == '\n' || character == '\r')
        {
            push_event(
                &mut events,
                "quoted-name-newline",
                "escaped a literal newline in a quoted name",
                token.span.start,
            );
        }
        if token.kind == "atom" && is_bare_temporal(&token.text) {
            push_event(
                &mut events,
                "bare-temporal",
                "added the Core temporal marker and field padding",
                token.span.start,
            );
        }
        if token.kind == "atom" && is_leading_zero_number(&token.text) {
            push_event(
                &mut events,
                "leading-zero-number",
                "removed legacy leading zeros",
                token.span.start,
            );
        }
        if token.kind == "atom" && is_trailing_point_number(&token.text) {
            push_event(
                &mut events,
                "trailing-point-number",
                "added a fractional digit to a trailing-point number",
                token.span.start,
            );
        }
        if token.kind == "binary" && !token.text.ends_with('|') {
            push_event(
                &mut events,
                "legacy-binary",
                "closed a legacy binary literal",
                token.span.start,
            );
        }
        if (token.kind == "temporal" || is_bare_temporal(&token.text))
            && temporal_fraction_digits(&token.text).is_some_and(|digits| digits != 6)
        {
            push_event(
                &mut events,
                "temporal-fraction",
                "normalized a legacy temporal fraction to six digits",
                token.span.start + temporal_fraction_offset(&token.text).unwrap_or(0),
            );
        }
    }

    let mut has_legacy_binding = false;
    for (index, window) in structural.windows(2).enumerate() {
        let marker_label = index != 0
            && matches!(
                structural[index - 1].kind,
                "anchor" | "reference" | "constant" | "temporal-marker"
            );
        // Must match the reference exactly: a quoted name counts, and a
        // reserved bareword does not. `'quoted name'[]` is a pending legacy
        // node; `true []` is a boolean beside a list and no name at all
        // (AXON 2.3). This condition is separate from the pending-name
        // *rewrite* above and had drifted from it.
        let is_name = window[0].kind == "quoted-name"
            || (window[0].kind == "name"
                && !matches!(window[0].text.as_str(), "true" | "false" | "null"));
        if !marker_label && is_name && (window[1].text == "(" || window[1].text == "[") {
            has_legacy_binding = true;
            break;
        }
    }
    if has_legacy_binding {
        push_event(
            &mut events,
            "legacy-binding",
            "preserved the historical pending-name value shape",
            0,
        );
    }

    let duplicate_check = Options {
        graph,
        last_wins: false,
        duplicate_set_dedup: false,
        ..Options::compat()
    };
    if let Ok(duplicate_document) = parse_cst(source, &duplicate_check) {
        for diagnostic in &duplicate_document.diagnostics {
            if diagnostic.category == "duplicate-key" {
                let offset = duplicate_event_offset(source, diagnostic.span.start);
                push_event(
                    &mut events,
                    "duplicate-key",
                    "kept the last duplicate key with a warning",
                    offset,
                );
            } else if diagnostic.category == "duplicate-set-element" {
                push_event(
                    &mut events,
                    "duplicate-set-element",
                    "dropped a duplicate set element with a warning",
                    diagnostic.span.start.min(source.len()),
                );
            }
        }
    }

    if rendered != source {
        push_event(
            &mut events,
            "core-canonicalization",
            "emitted one strict deterministic Core spelling",
            0,
        );
    }
    Ok(events)
}

fn push_event(events: &mut Vec<MigrationEvent>, code: &str, message: &str, offset: usize) {
    if events
        .iter()
        .any(|event| event.code == code && event.offset == offset)
    {
        return;
    }
    events.push(MigrationEvent {
        code: code.to_string(),
        message: message.to_string(),
        offset,
    });
}

fn duplicate_event_offset(source: &str, diagnostic_offset: usize) -> usize {
    let offset = diagnostic_offset.min(source.len());
    source[..offset]
        .char_indices()
        .next_back()
        .filter(|(_, character)| matches!(character, '}' | ']' | ')'))
        .map_or(offset, |(boundary, _)| boundary)
}

fn continuation_offsets(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut offsets = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            if bytes.get(index + 1) == Some(&b'\n') {
                offsets.push(index);
                index += 2;
                continue;
            }
            if bytes.get(index + 1) == Some(&b'\r') && bytes.get(index + 2) == Some(&b'\n') {
                offsets.push(index);
                index += 3;
                continue;
            }
        }
        index += 1;
    }
    offsets
}

fn is_ascii_digits(text: &str, minimum: usize, maximum: usize) -> bool {
    let length = text.len();
    length >= minimum && length <= maximum && text.as_bytes().iter().all(u8::is_ascii_digit)
}

fn is_bare_temporal(text: &str) -> bool {
    // Decide time-vs-date by which delimiter comes first. Testing for a colon
    // unconditionally misread `2026-01-01T00:00:00Z` as a bare *time*: its
    // first colon sits inside the time part, and `2026-01-01T00` is not a 1-2
    // digit hour, so the function returned false and the `bare-temporal`
    // migration event never fired for a bare datetime.
    let first_colon = text.find(':');
    let first_dash = text.find('-');
    let looks_like_time = match (first_colon, first_dash) {
        (Some(_), None) => true,
        (Some(c), Some(d)) => c < d,
        _ => false,
    };
    if looks_like_time && let Some(colon) = first_colon {
        return is_ascii_digits(&text[..colon], 1, 2)
            && text
                .as_bytes()
                .get(colon + 1..)
                .unwrap_or_default()
                .first()
                .is_some_and(u8::is_ascii_digit);
    }
    let mut parts = text.splitn(3, '-');
    let Some(year) = parts.next() else {
        return false;
    };
    let Some(month) = parts.next() else {
        return false;
    };
    let Some(day_and_time) = parts.next() else {
        return false;
    };
    if !is_ascii_digits(year, 1, 5) || !is_ascii_digits(month, 1, 2) {
        return false;
    }
    match day_and_time.split_once('T') {
        Some((day, time)) => {
            is_ascii_digits(day, 1, 2) && time.as_bytes().first().is_some_and(u8::is_ascii_digit)
        }
        None => is_ascii_digits(day_and_time, 1, 2),
    }
}

fn is_leading_zero_number(text: &str) -> bool {
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    unsigned.len() > 1
        && unsigned.starts_with('0')
        && unsigned.as_bytes().iter().all(u8::is_ascii_digit)
}

fn is_trailing_point_number(text: &str) -> bool {
    let Some(body) = text.strip_suffix('.') else {
        return false;
    };
    let unsigned = body.strip_prefix('-').unwrap_or(body);
    !unsigned.is_empty() && unsigned.as_bytes().iter().all(u8::is_ascii_digit)
}

fn temporal_fraction_digits(text: &str) -> Option<usize> {
    let (_, suffix) = text.split_once('.')?;
    let count = suffix
        .as_bytes()
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    (count != 0).then_some(count)
}

fn temporal_fraction_offset(text: &str) -> Option<usize> {
    text.find('.').map(|point| point + 1)
}

fn normalize_legacy_temporal_fraction(text: &str) -> Option<String> {
    let point = text.find('.')?;
    let fraction_start = point + 1;
    let fraction_len = text.as_bytes()[fraction_start..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if fraction_len == 0 || fraction_len == 6 {
        return None;
    }
    let fraction_end = fraction_start + fraction_len;
    let digits = &text[fraction_start..fraction_end];
    let mut output = String::with_capacity(text.len() + 6usize.saturating_sub(fraction_len));
    output.push_str(&text[..fraction_start]);
    if fraction_len < 6 {
        output.push_str(&"0".repeat(6 - fraction_len));
        output.push_str(digits);
    } else {
        output.push_str(&digits[..6]);
    }
    output.push_str(&text[fraction_end..]);
    Some(output)
}

/// Derive the same character-level edits as Python's
/// `difflib.SequenceMatcher(..., autojunk=False)`.
fn derive_migration_edits(source: &str, output: &str) -> Vec<MigrationEdit> {
    if source == output {
        return Vec::new();
    }
    let source_chars: Vec<char> = source.chars().collect();
    let output_chars: Vec<char> = output.chars().collect();
    let source_offsets = char_offsets(source);
    let output_offsets = char_offsets(output);
    let mut edits = Vec::new();
    let blocks = matching_blocks(&source_chars, &output_chars);
    let (mut source_at, mut output_at) = (0usize, 0usize);
    for (source_match, output_match, size) in blocks {
        if source_at < source_match || output_at < output_match {
            edits.push(MigrationEdit {
                code: "selective-rewrite".to_string(),
                start: source_offsets[source_at],
                end: source_offsets[source_match],
                replacement: output[output_offsets[output_at]..output_offsets[output_match]]
                    .to_string(),
            });
        }
        source_at = source_match + size;
        output_at = output_match + size;
    }
    edits
}

fn char_offsets(text: &str) -> Vec<usize> {
    let mut offsets: Vec<usize> = text.char_indices().map(|(offset, _)| offset).collect();
    offsets.push(text.len());
    offsets
}

fn longest_match(
    source: &[char],
    output: &[char],
    source_start: usize,
    source_end: usize,
    output_start: usize,
    output_end: usize,
) -> (usize, usize, usize) {
    let mut output_positions: alloc::collections::BTreeMap<char, Vec<usize>> =
        alloc::collections::BTreeMap::new();
    for (index, character) in output.iter().copied().enumerate() {
        output_positions.entry(character).or_default().push(index);
    }
    let (mut best_source, mut best_output, mut best_size) = (source_start, output_start, 0usize);
    let mut previous_lengths: alloc::collections::BTreeMap<usize, usize> =
        alloc::collections::BTreeMap::new();
    for (source_index, character) in source
        .iter()
        .copied()
        .enumerate()
        .take(source_end)
        .skip(source_start)
    {
        let mut current_lengths = alloc::collections::BTreeMap::new();
        if let Some(positions) = output_positions.get(&character) {
            for &output_index in positions {
                if output_index < output_start {
                    continue;
                }
                if output_index >= output_end {
                    break;
                }
                let size = previous_lengths
                    .get(&output_index.saturating_sub(1))
                    .copied()
                    .filter(|_| output_index != 0)
                    .unwrap_or(0)
                    + 1;
                current_lengths.insert(output_index, size);
                if size > best_size {
                    best_source = source_index + 1 - size;
                    best_output = output_index + 1 - size;
                    best_size = size;
                }
            }
        }
        previous_lengths = current_lengths;
    }

    while best_source > source_start
        && best_output > output_start
        && source[best_source - 1] == output[best_output - 1]
    {
        best_source -= 1;
        best_output -= 1;
        best_size += 1;
    }
    while best_source + best_size < source_end
        && best_output + best_size < output_end
        && source[best_source + best_size] == output[best_output + best_size]
    {
        best_size += 1;
    }
    (best_source, best_output, best_size)
}

fn matching_blocks(source: &[char], output: &[char]) -> Vec<(usize, usize, usize)> {
    let mut pending = vec![(0usize, source.len(), 0usize, output.len())];
    let mut matches = Vec::new();
    while let Some((source_start, source_end, output_start, output_end)) = pending.pop() {
        let (source_match, output_match, size) = longest_match(
            source,
            output,
            source_start,
            source_end,
            output_start,
            output_end,
        );
        if size == 0 {
            continue;
        }
        matches.push((source_match, output_match, size));
        if source_start < source_match && output_start < output_match {
            pending.push((source_start, source_match, output_start, output_match));
        }
        if source_match + size < source_end && output_match + size < output_end {
            pending.push((
                source_match + size,
                source_end,
                output_match + size,
                output_end,
            ));
        }
    }
    matches.sort_unstable();

    let mut collapsed: Vec<(usize, usize, usize)> = Vec::new();
    for (source_match, output_match, size) in matches {
        if let Some((previous_source, previous_output, previous_size)) = collapsed.last_mut()
            && *previous_source + *previous_size == source_match
            && *previous_output + *previous_size == output_match
        {
            *previous_size += size;
        } else {
            collapsed.push((source_match, output_match, size));
        }
    }
    collapsed.push((source.len(), output.len(), 0));
    collapsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_and_selective_basics() {
        let canonical = migrate("007\n12:30").unwrap();
        assert_eq!(canonical.text, "7\n^12:30:00");
        assert_eq!(canonical.mode, MigrationMode::Canonical);

        let selective = migrate_selective("# keep\n007\n").unwrap();
        assert_eq!(selective.text, "# keep\n7\n");
        assert_eq!(selective.mode, MigrationMode::Selective);
        assert_eq!(
            apply_migration_edits("# keep\n007\n", &selective.edits).unwrap(),
            selective.text
        );
    }

    #[test]
    fn selective_repairs_newly_governed_legacy_tokens() {
        let hash_comment = migrate_selective("#_foo\n1").unwrap();
        assert_eq!(hash_comment.text, "# _foo\n1");
        assert!(
            hash_comment
                .events
                .iter()
                .any(|event| event.code == "hash-underscore-comment")
        );

        assert_eq!(migrate_selective("5.").unwrap().text, "5.0");
        assert_eq!(migrate_selective("|SGVsbG8=").unwrap().text, "|SGVsbG8=|");

        let duplicate = migrate_selective("{a:1 a:2}").unwrap();
        assert_eq!(duplicate.text, "{a:2}");
        assert_eq!(duplicate.mode, MigrationMode::CanonicalFallback);
        assert_eq!(
            duplicate.edits,
            vec![MigrationEdit {
                code: "selective-rewrite".to_string(),
                start: 1,
                end: 5,
                replacement: String::new(),
            }]
        );
        assert!(
            duplicate
                .events
                .iter()
                .any(|event| event.code == "duplicate-key")
        );
    }

    #[test]
    fn selective_fallback_is_explicit() {
        let result = migrate_selective("Rgb(1 2 3)").unwrap();
        assert_eq!(result.text, "null\n(1 2 3)");
        assert_eq!(result.mode, MigrationMode::CanonicalFallback);
    }

    #[test]
    fn pending_legacy_names_are_reconstructed_without_breaking_brace_nodes() {
        assert_eq!(migrate("hello world").unwrap().text, "null\nworld");
        assert_eq!(migrate("[Foo Bar]").unwrap().text, "[null null]");
        assert_eq!(migrate("Foo {a:1}").unwrap().text, "Foo{a:1}");
        assert_eq!(
            migrate("person\n  name:\"Alex\"\n").unwrap().text,
            "person{name:\"Alex\"}"
        );
    }

    #[test]
    fn temporal_events_are_token_scoped() {
        let result = migrate("^12:00:00.1234567").unwrap();
        assert_eq!(result.text, "^12:00:00.123456");
        let fraction = result
            .events
            .iter()
            .find(|event| event.code == "temporal-fraction")
            .unwrap();
        assert_eq!(fraction.offset, 10);
        assert!(
            result
                .events
                .iter()
                .all(|event| event.code != "leading-zero-number")
        );

        assert_eq!(migrate("^12:00:00.5").unwrap().text, "^12:00:00.000005");
        let selective = migrate_selective("^12:00:00.5").unwrap();
        assert_eq!(selective.text, "^12:00:00.000005");
        assert_eq!(selective.mode, MigrationMode::Selective);
    }

    #[test]
    fn undocumented_leading_plus_is_rejected() {
        assert!(migrate("+42").is_err());
        assert!(migrate_selective("+42").is_err());
    }

    #[test]
    fn graph_streams_and_unknown_references_are_governed() {
        let result = migrate_with_graph("&a []\n*a", true).unwrap();
        assert_eq!(result.text, "&1 []\n*1");

        let error = migrate_with_graph("*missing", true).unwrap_err();
        assert_eq!(error.category(), Category::UnknownReference);
    }

    #[test]
    fn edit_validation_uses_utf8_byte_boundaries() {
        let invalid = [MigrationEdit {
            code: "test".to_string(),
            start: 1,
            end: 1,
            replacement: "x".to_string(),
        }];
        assert_eq!(
            apply_migration_edits("é", &invalid).unwrap_err().category(),
            Category::InvalidUnicode
        );
    }
}
