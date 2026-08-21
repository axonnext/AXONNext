//! Transport-neutral editor helpers built on the lossless AXON CST.
//!
//! The types in this module deliberately contain no Language Server Protocol
//! transport types.  An editor integration can translate them directly to its
//! protocol representation while retaining AXON's UTF-8 byte spans.  Lines and
//! characters are zero-based Unicode-scalar positions; byte ranges are
//! half-open UTF-8 offsets into the original source.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::cst::{format_cst, parse_cst};
use crate::error::Result;
use crate::parser::Options;
use crate::value::Value;

/// A directly applicable repair attached to an editor diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspFix {
    /// Short, editor-facing title for the repair.
    pub title: String,
    /// Start byte offset, inclusive.
    pub start_byte: usize,
    /// End byte offset, exclusive.
    pub end_byte: usize,
    /// Replacement Unicode text.
    pub new_text: String,
}

/// One parser or CST-recovery diagnostic in editor-facing coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspDiagnostic {
    /// Stable AXON error category.
    pub code: String,
    /// Human-readable diagnostic message.
    pub message: String,
    /// Start byte offset, inclusive.
    pub start_byte: usize,
    /// End byte offset, exclusive.
    pub end_byte: usize,
    /// Zero-based source line of the diagnostic start.
    pub line: u32,
    /// Zero-based Unicode-scalar column of the diagnostic start.
    pub character: u32,
    /// Expected token or production labels, when recovery can identify them.
    pub expected: Vec<String>,
    /// Directly applicable repairs proposed by CST recovery.
    pub fixes: Vec<LspFix>,
}

/// Information about the token covering a hovered UTF-8 byte offset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspHover {
    /// CST token kind, such as `"name"`, `"string"`, or `"comment"`.
    pub kind: String,
    /// Exact token text from the source.
    pub text: String,
    /// Start byte offset, inclusive.
    pub start_byte: usize,
    /// End byte offset, exclusive.
    pub end_byte: usize,
    /// Zero-based source line of the token start.
    pub line: u32,
    /// Zero-based Unicode-scalar column of the token start.
    pub character: u32,
    /// Whether the token is whitespace or a comment.
    pub trivia: bool,
}

/// A whole-document formatting edit expressed in UTF-8 byte offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspTextEdit {
    /// Start byte offset, inclusive.
    pub start_byte: usize,
    /// End byte offset, exclusive.
    pub end_byte: usize,
    /// Replacement Unicode text.
    pub new_text: String,
}

/// One component of a document-symbol path.
///
/// Collection and top-level positions use [`Self::Index`].  Mapping keys and
/// node attribute components use [`Self::Key`].  To mirror the reference
/// helper exactly, a node attribute appends the two keys `"@"` and the
/// attribute name to its containing path.
#[derive(Clone, Debug, PartialEq)]
pub enum SymbolPathSegment {
    /// A zero-based top-level or collection index.
    Index(usize),
    /// A mapping key or node-attribute path component.
    Key(Value),
}

/// A named AXON node discovered in the semantic document.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentSymbol {
    /// The AXON node name.
    pub name: String,
    /// Stable symbol kind; currently always `"node"`.
    pub kind: String,
    /// Semantic path from the top-level value stream to this node.
    pub path: Vec<SymbolPathSegment>,
}

/// Parse `source` and return every CST diagnostic in editor-facing form.
///
/// Ordinary syntax problems are represented in the returned vector.  A hard
/// resource or input failure from CST construction is returned as an error.
pub fn diagnostics(source: &str, options: &Options) -> Result<Vec<LspDiagnostic>> {
    let document = parse_cst(source, options)?;
    Ok(document
        .diagnostics
        .into_iter()
        .map(|item| LspDiagnostic {
            code: item.category,
            message: item.message,
            start_byte: item.span.start,
            end_byte: item.span.end,
            line: item.span.line.saturating_sub(1),
            character: item.span.column.saturating_sub(1),
            expected: item.expected,
            fixes: item
                .fixes
                .into_iter()
                .map(|fix| LspFix {
                    title: fix.title,
                    start_byte: fix.start,
                    end_byte: fix.end,
                    new_text: fix.replacement,
                })
                .collect(),
        })
        .collect())
}

/// Return information about the token covering `byte_offset`.
///
/// Offsets are UTF-8 byte offsets, so every byte within a multi-byte scalar
/// selects its containing token.  An offset outside all tokens returns `None`.
pub fn hover(source: &str, byte_offset: usize, options: &Options) -> Result<Option<LspHover>> {
    let document = parse_cst(source, options)?;
    Ok(document.token_at(byte_offset).map(|token| LspHover {
        kind: token.kind.to_string(),
        text: token.text.clone(),
        start_byte: token.span.start,
        end_byte: token.span.end,
        line: token.span.line.saturating_sub(1),
        character: token.span.column.saturating_sub(1),
        trivia: token.trivia,
    }))
}

/// Format `source` and return the reference helper's single replacement edit.
///
/// An already formatted document produces an empty vector.  Otherwise the
/// returned edit replaces the complete UTF-8 source range.
pub fn formatting_edits(
    source: &str,
    indent: usize,
    options: &Options,
) -> Result<Vec<LspTextEdit>> {
    let formatted = format_cst(source, indent, options)?;
    if formatted == source {
        return Ok(Vec::new());
    }
    Ok(alloc::vec![LspTextEdit {
        start_byte: 0,
        end_byte: source.len(),
        new_text: formatted,
    }])
}

/// Return every named AXON node in semantic depth-first document order.
///
/// As in the reference helper, any CST diagnostic suppresses the symbol list.
/// Graph anchors are transparent.  References resolve against anchors across
/// the complete document, and each anchored identity is traversed at most once,
/// which preserves shared-value behavior and makes cyclic graphs safe despite
/// [`Value`] representing graph syntax structurally.
pub fn document_symbols(source: &str, options: &Options) -> Result<Vec<DocumentSymbol>> {
    let document = parse_cst(source, options)?;
    if !document.diagnostics.is_empty() {
        return Ok(Vec::new());
    }

    let anchors = collect_anchors(&document.values);
    let mut visited_anchors: Vec<&str> = Vec::new();
    let mut symbols = Vec::new();
    let mut path = Vec::new();
    for (index, value) in document.values.iter().enumerate() {
        path.push(SymbolPathSegment::Index(index));
        visit_symbols(
            value,
            &mut path,
            &anchors,
            &mut visited_anchors,
            &mut symbols,
        );
        path.pop();
    }
    Ok(symbols)
}

/// Build the document-wide graph binding table.  The parser diagnoses duplicate
/// labels, but retaining the first binding here keeps the traversal fail-closed
/// if a caller later supplies a CST implementation with partial semantics.
fn collect_anchors(values: &[Value]) -> Vec<(&str, &Value)> {
    let mut anchors = Vec::new();
    for value in values {
        for (label, target) in value.anchors() {
            if !anchors.iter().any(|(known, _)| *known == label) {
                anchors.push((label, target));
            }
        }
    }
    anchors
}

fn visit_symbols<'a>(
    value: &'a Value,
    path: &mut Vec<SymbolPathSegment>,
    anchors: &[(&'a str, &'a Value)],
    visited_anchors: &mut Vec<&'a str>,
    symbols: &mut Vec<DocumentSymbol>,
) {
    match value {
        Value::Anchor(anchor) => visit_anchor(
            anchor.label.as_str(),
            &anchor.value,
            path,
            anchors,
            visited_anchors,
            symbols,
        ),
        Value::Ref(label) => {
            if let Some(&(bound_label, target)) = anchors
                .iter()
                .find(|(bound_label, _)| *bound_label == label.as_str())
            {
                visit_anchor(bound_label, target, path, anchors, visited_anchors, symbols);
            }
        }
        Value::Node(node) => {
            symbols.push(DocumentSymbol {
                name: node.name.clone(),
                kind: "node".to_string(),
                path: path.clone(),
            });

            for (key, child) in &node.attributes {
                path.push(SymbolPathSegment::Key(Value::String("@".to_string())));
                path.push(SymbolPathSegment::Key(Value::String(key.clone())));
                visit_symbols(child, path, anchors, visited_anchors, symbols);
                path.pop();
                path.pop();
            }
            for (index, child) in node.children.iter().enumerate() {
                path.push(SymbolPathSegment::Index(index));
                visit_symbols(child, path, anchors, visited_anchors, symbols);
                path.pop();
            }
        }
        Value::List(items) | Value::Tuple(items) | Value::Set(items) => {
            for (index, child) in items.iter().enumerate() {
                path.push(SymbolPathSegment::Index(index));
                visit_symbols(child, path, anchors, visited_anchors, symbols);
                path.pop();
            }
        }
        Value::Map(pairs) | Value::OrderedMap(pairs) => {
            for (key, child) in pairs {
                path.push(SymbolPathSegment::Key(key.clone()));
                visit_symbols(child, path, anchors, visited_anchors, symbols);
                path.pop();
            }
        }
        Value::Null
        | Value::Bool(_)
        | Value::Int(_)
        | Value::Float(_)
        | Value::Decimal(_)
        | Value::String(_)
        | Value::Bytes(_)
        | Value::Temporal(_)
        | Value::Link(_)
        | Value::Undefined => {}
    }
}

fn visit_anchor<'a>(
    label: &'a str,
    target: &'a Value,
    path: &mut Vec<SymbolPathSegment>,
    anchors: &[(&'a str, &'a Value)],
    visited_anchors: &mut Vec<&'a str>,
    symbols: &mut Vec<DocumentSymbol>,
) {
    if visited_anchors.contains(&label) {
        return;
    }
    // Mark before following the target: a self-reference therefore terminates
    // immediately, while siblings following it are still visited normally.
    visited_anchors.push(label);
    visit_symbols(target, path, anchors, visited_anchors, symbols);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_expose_reference_recovery_coordinates_and_fix() {
        let items = diagnostics("[1,,2]", &Options::default()).unwrap();
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.code, "unexpected-token");
        assert_eq!(item.message, "comma does not have a preceding item");
        assert_eq!((item.start_byte, item.end_byte), (3, 4));
        assert_eq!((item.line, item.character), (0, 3));
        assert_eq!(item.expected, alloc::vec!["value".to_string()]);
        assert_eq!(
            item.fixes,
            alloc::vec![LspFix {
                title: "Remove comma".to_string(),
                start_byte: 3,
                end_byte: 4,
                new_text: String::new(),
            }]
        );
    }

    #[test]
    fn hover_uses_utf8_bytes_and_zero_based_scalar_positions() {
        let source = "# café\nétage{name:\"A\"}";
        let start = source.find("étage").unwrap();
        let hovered = hover(source, start + 1, &Options::default())
            .unwrap()
            .unwrap();
        assert_eq!(hovered.kind, "name");
        assert_eq!(hovered.text, "étage");
        assert_eq!(hovered.start_byte, start);
        assert_eq!(hovered.end_byte, start + "étage".len());
        assert_eq!((hovered.line, hovered.character), (1, 0));
        assert!(!hovered.trivia);
        assert!(
            hover(source, source.len(), &Options::default())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn formatting_returns_one_full_byte_edit_and_is_idempotent() {
        let source = "étage{name:\"A\"}";
        let edits = formatting_edits(source, 2, &Options::default()).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!((edits[0].start_byte, edits[0].end_byte), (0, source.len()));
        let formatted = edits[0].new_text.clone();
        assert!(
            formatting_edits(&formatted, 2, &Options::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn symbols_match_semantic_paths_and_graph_identity() {
        let symbols = document_symbols("Foo{a:Bar child{z:Baz}}", &Options::default()).unwrap();
        assert_eq!(
            symbols
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            alloc::vec!["Foo", "Bar", "child", "Baz"]
        );
        assert_eq!(
            symbols[1].path,
            alloc::vec![
                SymbolPathSegment::Index(0),
                SymbolPathSegment::Key(Value::String("@".to_string())),
                SymbolPathSegment::Key(Value::String("a".to_string())),
            ]
        );
        assert_eq!(
            symbols[3].path,
            alloc::vec![
                SymbolPathSegment::Index(0),
                SymbolPathSegment::Index(0),
                SymbolPathSegment::Key(Value::String("@".to_string())),
                SymbolPathSegment::Key(Value::String("z".to_string())),
            ]
        );

        let graph = Options {
            graph: true,
            ..Options::default()
        };
        let symbols = document_symbols("&a Foo{self:*a child:Bar}\n*a", &graph).unwrap();
        assert_eq!(
            symbols
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            alloc::vec!["Foo", "Bar"]
        );
        assert_eq!(symbols[0].path, alloc::vec![SymbolPathSegment::Index(0)]);
        assert_eq!(
            symbols[1].path,
            alloc::vec![
                SymbolPathSegment::Index(0),
                SymbolPathSegment::Key(Value::String("@".to_string())),
                SymbolPathSegment::Key(Value::String("child".to_string())),
            ]
        );
    }
}
