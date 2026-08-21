//! Lossless concrete syntax tree for AXON 2026.
//!
//! The CST owns every source byte: [`CstDocument::render`] reproduces the input
//! exactly, so editors can retain malformed input, comments, and formatting.
//! It is independent of the semantic parser. This is a port of the reference
//! `cst2026`; the tokenizer is verified to produce the same token stream
//! (kinds + spans) as the reference, and the round-trip `render() == source`
//! invariant is enforced.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::de::{from_str_stream_with, parse_document_with_trace};
use crate::error::{Category, Error, Result};
use crate::parser::Options;
use crate::value::Value;

/// A source span: byte range plus 1-based line/column of the start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// Start byte offset.
    pub start: usize,
    /// End byte offset (exclusive).
    pub end: usize,
    /// 1-based line of the start.
    pub line: u32,
    /// 1-based column of the start.
    pub column: u32,
}

/// A lexical token. `trivia` marks whitespace and comments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CstToken {
    /// Token kind (e.g., `"name"`, `"string"`, `"whitespace"`, `"punctuation"`).
    pub kind: &'static str,
    /// The exact source text of the token.
    pub text: String,
    /// The token's source span.
    pub span: Span,
    /// True for whitespace and comment tokens.
    pub trivia: bool,
    /// Trivia token indexes attached before this structural token.
    pub leading_trivia: Vec<CstTokenId>,
    /// Trivia token indexes attached after this structural token.
    pub trailing_trivia: Vec<CstTokenId>,
}

/// Index of a token in [`CstDocument::tokens`].
pub type CstTokenId = usize;

/// Index of a node in [`CstDocument::nodes`].
pub type CstNodeId = usize;

/// One child of a CST production node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CstElement {
    /// A token index in the owning document.
    Token(CstTokenId),
    /// A node index in the owning document.
    Node(CstNodeId),
}

/// A byte-addressed editor repair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CstFix {
    /// Short editor-facing title.
    pub title: String,
    /// Start byte offset, inclusive.
    pub start: usize,
    /// End byte offset, exclusive.
    pub end: usize,
    /// Replacement Unicode text.
    pub replacement: String,
}

/// A diagnostic (parse issue) anchored to a span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CstDiagnostic {
    /// Stable error category (§17.2).
    pub category: String,
    /// Human-readable message.
    pub message: String,
    /// The span the diagnostic anchors to.
    pub span: Span,
    /// Expected token or production labels.
    pub expected: Vec<String>,
    /// Directly applicable byte edits.
    pub fixes: Vec<CstFix>,
}

/// Semantic payload attached to a CST production.
#[derive(Clone, Debug, PartialEq)]
pub enum CstSemanticValue {
    /// One AXON value.
    Value(Value),
    /// The ordered top-level document stream.
    Stream(Vec<Value>),
}

/// One production in the lossless syntax tree.
///
/// Nodes live in an arena owned by [`CstDocument`]. This avoids recursive drop
/// and traversal when malformed input contains thousands of delimiters.
#[derive(Clone, Debug, PartialEq)]
pub struct CstNode {
    /// Stable arena index.
    pub id: CstNodeId,
    /// Grammar production kind.
    pub kind: &'static str,
    /// Byte and source-position span.
    pub span: Span,
    /// Token and child-node references in source order.
    pub children: Vec<CstElement>,
    /// Semantic payload when this production owns one.
    pub semantic_value: Option<CstSemanticValue>,
}

/// A semantic value linked to its exact source range and nearest CST node.
#[derive(Clone, Debug, PartialEq)]
pub struct CstSemanticTrace {
    /// Exact source span of the semantic value.
    pub span: Span,
    /// Parsed value.
    pub value: Value,
    /// Nearest containing node in [`CstDocument::nodes`].
    pub owner: CstNodeId,
}

/// A lossless CST document.
#[derive(Clone, Debug)]
pub struct CstDocument {
    /// The original source.
    pub source: String,
    /// The full token stream (including trivia).
    pub tokens: Vec<CstToken>,
    /// Arena of grammar-production nodes.
    pub nodes: Vec<CstNode>,
    /// Root `document` node index.
    pub root: CstNodeId,
    /// Successfully parsed top-level semantic values.
    pub values: Vec<Value>,
    /// Diagnostics collected during parsing.
    pub diagnostics: Vec<CstDiagnostic>,
    /// Top-level stream nodes corresponding to [`Self::values`].
    pub semantic_nodes: Vec<CstNodeId>,
    /// Nested, discarded, and containing semantic value traces.
    pub semantic_traces: Vec<CstSemanticTrace>,
}

impl CstDocument {
    /// Reproduce the source exactly by concatenating token texts.
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(self.source.len());
        for t in &self.tokens {
            out.push_str(&t.text);
        }
        out
    }

    /// Render as UTF-8 bytes.
    pub fn bytes(&self) -> Vec<u8> {
        self.render().into_bytes()
    }

    /// The token covering `byte_offset`, if any.
    pub fn token_at(&self, byte_offset: usize) -> Option<&CstToken> {
        self.tokens
            .iter()
            .find(|t| t.span.start <= byte_offset && byte_offset < t.span.end)
    }

    /// Non-trivia tokens (structural content only).
    pub fn non_trivia(&self) -> Vec<&CstToken> {
        self.tokens.iter().filter(|t| !t.trivia).collect()
    }

    /// Return a production node by arena index.
    pub fn node(&self, id: CstNodeId) -> Option<&CstNode> {
        self.nodes.get(id)
    }

    /// Return every token owned below `node`, in source order, without
    /// recursion.
    pub fn node_tokens(&self, node: CstNodeId) -> Vec<&CstToken> {
        let Some(root) = self.nodes.get(node) else {
            return Vec::new();
        };
        let mut stack: Vec<CstElement> = root.children.iter().rev().copied().collect();
        let mut out = Vec::new();
        while let Some(element) = stack.pop() {
            match element {
                CstElement::Token(id) => {
                    if let Some(token) = self.tokens.get(id) {
                        out.push(token);
                    }
                }
                CstElement::Node(id) => {
                    if let Some(child) = self.nodes.get(id) {
                        stack.extend(child.children.iter().rev().copied());
                    }
                }
            }
        }
        out
    }

    /// Apply a UTF-8 byte edit and reparse the resulting document.
    pub fn replace(
        &self,
        start: usize,
        end: usize,
        replacement: &str,
        opts: &Options,
    ) -> Result<CstDocument> {
        if start > end || end > self.source.len() {
            return Err(cst_error(
                Category::UnexpectedToken,
                "CST edit has invalid byte bounds",
                0,
                0,
            ));
        }
        if !self.source.is_char_boundary(start) || !self.source.is_char_boundary(end) {
            return Err(cst_error(
                Category::InvalidUnicode,
                "CST edit bound is not a UTF-8 scalar boundary",
                0,
                0,
            ));
        }
        let mut updated =
            String::with_capacity(self.source.len() - (end - start) + replacement.len());
        updated.push_str(&self.source[..start]);
        updated.push_str(replacement);
        updated.push_str(&self.source[end..]);
        parse_cst(&updated, opts)
    }
}

impl CstFix {
    /// Apply this repair to `document` and reparse it.
    pub fn apply(&self, document: &CstDocument, opts: &Options) -> Result<CstDocument> {
        document.replace(self.start, self.end, &self.replacement, opts)
    }
}

/// Parse `source` into a lossless CST document.
///
/// Ordinary syntax failures are returned inside [`CstDocument::diagnostics`]
/// so malformed editor buffers remain lossless. Input and token resource-limit
/// failures return `Err` before an unbounded allocation is performed.
pub fn parse_cst(source: &str, opts: &Options) -> Result<CstDocument> {
    if source.len() > opts.limits.max_input {
        return Err(cst_error(
            Category::ResourceLimitExceeded,
            "CST input is too large",
            1,
            1,
        ));
    }
    let mut tokens = tokenize(source, opts)?;
    attach_trivia(&mut tokens);
    let (mut nodes, root, mut diagnostics, depth_exceeded) =
        build_production_tree(&tokens, source.len(), opts);
    diagnostics.extend(recover_token_errors(&tokens, source.len(), opts));
    if !depth_exceeded {
        refine_production_tree(&mut nodes, root, &tokens, opts);
    }
    let (values, semantic_nodes, semantic_traces) =
        populate_semantics(source, opts, &tokens, &mut nodes, root, &mut diagnostics);
    dedupe_diagnostics(&mut diagnostics);
    diagnostics.sort_by(|left, right| {
        (left.span.start, left.span.end, left.category.as_str()).cmp(&(
            right.span.start,
            right.span.end,
            right.category.as_str(),
        ))
    });
    let document = CstDocument {
        source: source.to_string(),
        tokens,
        nodes,
        root,
        values,
        diagnostics,
        semantic_nodes,
        semantic_traces,
    };
    if document.render() != source {
        return Err(cst_error(
            Category::UnexpectedToken,
            "CST tokenizer failed to own every source byte",
            0,
            0,
        ));
    }
    Ok(document)
}

fn cst_error(category: Category, message: &str, line: u32, column: u32) -> Error {
    Error::new(category, message, line, column)
}

fn fix(title: impl Into<String>, start: usize, end: usize, replacement: &str) -> CstFix {
    CstFix {
        title: title.into(),
        start,
        end,
        replacement: replacement.to_string(),
    }
}

fn diagnostic(
    category: &str,
    message: impl Into<String>,
    span: Span,
    expected: &[&str],
    fixes: Vec<CstFix>,
) -> CstDiagnostic {
    CstDiagnostic {
        category: category.to_string(),
        message: message.into(),
        span,
        expected: expected.iter().map(|item| (*item).to_string()).collect(),
        fixes,
    }
}

/// Attach trivia indexes according to the normative CST rule.
fn attach_trivia(tokens: &mut [CstToken]) {
    let structural: Vec<usize> = tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| (!token.trivia).then_some(index))
        .collect();
    if structural.is_empty() {
        return;
    }
    let mut leading = alloc::vec![Vec::<usize>::new(); tokens.len()];
    let mut trailing = alloc::vec![Vec::<usize>::new(); tokens.len()];
    for position in 0..=structural.len() {
        let previous = position.checked_sub(1).map(|index| structural[index]);
        let following = structural.get(position).copied();
        let run_start = previous.map_or(0, |index| index + 1);
        let run_end = following.unwrap_or(tokens.len());
        if run_start >= run_end {
            continue;
        }
        match (previous, following) {
            (None, Some(next)) => leading[next].extend(run_start..run_end),
            (Some(prev), None) => trailing[prev].extend(run_start..run_end),
            (Some(prev), Some(next)) => {
                let previous_end_line = tokens[prev].span.line
                    + tokens[prev].text.chars().filter(|&ch| ch == '\n').count() as u32;
                let same_line_comment = (run_start..run_end).find(|&index| {
                    tokens[index].kind == "comment" && tokens[index].span.line == previous_end_line
                });
                if let Some(comment) = same_line_comment {
                    trailing[prev].extend(run_start..=comment);
                    leading[next].extend(comment + 1..run_end);
                } else {
                    leading[next].extend(run_start..run_end);
                }
            }
            (None, None) => {}
        }
    }
    for index in structural {
        tokens[index].leading_trivia = core::mem::take(&mut leading[index]);
        tokens[index].trailing_trivia = core::mem::take(&mut trailing[index]);
    }
}

fn push_node(
    nodes: &mut Vec<CstNode>,
    kind: &'static str,
    span: Span,
    children: Vec<CstElement>,
) -> CstNodeId {
    let id = nodes.len();
    nodes.push(CstNode {
        id,
        kind,
        span,
        children,
        semantic_value: None,
    });
    id
}

fn element_span(element: CstElement, nodes: &[CstNode], tokens: &[CstToken]) -> Span {
    match element {
        CstElement::Token(id) => tokens[id].span,
        CstElement::Node(id) => nodes[id].span,
    }
}

fn elements_span(elements: &[CstElement], nodes: &[CstNode], tokens: &[CstToken]) -> Span {
    let first = element_span(elements[0], nodes, tokens);
    let last = element_span(elements[elements.len() - 1], nodes, tokens);
    Span {
        start: first.start,
        end: last.end,
        line: first.line,
        column: first.column,
    }
}

fn token_production(kind: &str) -> &'static str {
    match kind {
        "name" | "quoted-name" => "name",
        "string" | "raw-string" => "string-literal",
        "binary" => "binary-literal",
        "temporal" => "temporal-literal",
        "atom" => "scalar-literal",
        "constant" => "constant-reference",
        "anchor" => "anchor-prefix",
        "reference" => "reference-prefix",
        "discard" => "discard-prefix",
        _ => "lexical-value",
    }
}

fn build_production_tree(
    tokens: &[CstToken],
    source_end: usize,
    opts: &Options,
) -> (Vec<CstNode>, CstNodeId, Vec<CstDiagnostic>, bool) {
    let mut nodes = Vec::new();
    let root = push_node(
        &mut nodes,
        "document",
        Span {
            start: 0,
            end: source_end,
            line: 1,
            column: 1,
        },
        Vec::new(),
    );
    let mut stack: Vec<(CstNodeId, char)> = alloc::vec![(root, '\0')];
    let mut diagnostics = Vec::new();
    let mut depth_exceeded = false;
    for (token_id, token) in tokens.iter().enumerate() {
        let current = stack.last().expect("CST root is always present").0;
        let opener = if token.kind == "punctuation" {
            match token.text.as_str() {
                "{" => Some(('}', "brace")),
                "[" => Some((']', "bracket")),
                "(" => Some((')', "tuple")),
                _ => None,
            }
        } else {
            None
        };
        if let Some((expected, kind)) = opener {
            let node = push_node(
                &mut nodes,
                kind,
                token.span,
                alloc::vec![CstElement::Token(token_id)],
            );
            nodes[current].children.push(CstElement::Node(node));
            stack.push((node, expected));
            if stack.len() - 1 > opts.limits.max_depth as usize && !depth_exceeded {
                depth_exceeded = true;
                diagnostics.push(diagnostic(
                    "resource-limit-exceeded",
                    "maximum CST nesting depth exceeded",
                    token.span,
                    &[],
                    Vec::new(),
                ));
            }
            continue;
        }
        let is_closer =
            token.kind == "punctuation" && matches!(token.text.as_str(), "}" | "]" | ")");
        if is_closer {
            if stack.len() == 1 {
                nodes[current].children.push(CstElement::Token(token_id));
                diagnostics.push(diagnostic(
                    "unexpected-token",
                    "unmatched closing delimiter",
                    token.span,
                    &["value"],
                    alloc::vec![fix(
                        "Remove unmatched delimiter",
                        token.span.start,
                        token.span.end,
                        "",
                    )],
                ));
                continue;
            }
            let (node, expected) = *stack.last().expect("nested CST node");
            nodes[node].children.push(CstElement::Token(token_id));
            nodes[node].span.end = token.span.end;
            if !token.text.starts_with(expected) {
                let expected_text = expected.to_string();
                diagnostics.push(diagnostic(
                    "unexpected-token",
                    format!("mismatched delimiter: expected {expected}"),
                    token.span,
                    &[expected_text.as_str()],
                    alloc::vec![fix(
                        format!("Replace with {expected}"),
                        token.span.start,
                        token.span.end,
                        expected_text.as_str(),
                    )],
                ));
            }
            stack.pop();
            continue;
        }
        if token.trivia || token.kind == "punctuation" {
            nodes[current].children.push(CstElement::Token(token_id));
        } else {
            let node = push_node(
                &mut nodes,
                token_production(token.kind),
                token.span,
                alloc::vec![CstElement::Token(token_id)],
            );
            nodes[current].children.push(CstElement::Node(node));
        }
    }
    while stack.len() > 1 {
        let (node, expected) = stack.pop().expect("nested CST node");
        let expected_text = expected.to_string();
        diagnostics.push(diagnostic(
            "unexpected-end",
            format!("unterminated {}; expected {expected}", nodes[node].kind),
            Span {
                start: source_end,
                end: source_end,
                line: nodes[node].span.line,
                column: nodes[node].span.column,
            },
            &[expected_text.as_str()],
            alloc::vec![fix(
                format!("Insert missing {expected}"),
                source_end,
                source_end,
                expected_text.as_str(),
            )],
        ));
        nodes[node].span.end = source_end;
    }
    (nodes, root, diagnostics, depth_exceeded)
}

fn quoted_token_closed(text: &str, delimiter: char) -> bool {
    if text.chars().count() < 2 || !text.ends_with(delimiter) {
        return false;
    }
    let mut backslashes = 0usize;
    for ch in text[..text.len() - delimiter.len_utf8()].chars().rev() {
        if ch != '\\' {
            break;
        }
        backslashes += 1;
    }
    backslashes.is_multiple_of(2)
}

fn recover_token_errors(
    tokens: &[CstToken],
    source_end: usize,
    opts: &Options,
) -> Vec<CstDiagnostic> {
    let structural: Vec<usize> = tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| (!token.trivia).then_some(index))
        .collect();
    let mut diagnostics = Vec::new();
    for (position, &token_id) in structural.iter().enumerate() {
        let token = &tokens[token_id];
        let previous = position
            .checked_sub(1)
            .and_then(|index| structural.get(index))
            .map(|&index| &tokens[index]);
        let following = structural.get(position + 1).map(|&index| &tokens[index]);
        if token.text == ","
            && previous.is_none_or(|item| matches!(item.text.as_str(), "{" | "[" | "(" | ","))
        {
            diagnostics.push(diagnostic(
                "unexpected-token",
                "comma does not have a preceding item",
                token.span,
                &["value"],
                alloc::vec![fix("Remove comma", token.span.start, token.span.end, "",)],
            ));
        }
        let prefix_expected = match token.kind {
            "anchor" => Some("anchor label"),
            "reference" => Some("reference label"),
            "constant" => Some("constant name"),
            "discard" => Some("value"),
            _ => None,
        };
        if let Some(expected) = prefix_expected
            && following.is_none_or(|item| matches!(item.text.as_str(), "}" | "]" | ")" | ","))
        {
            let category = if following.is_none() {
                "unexpected-end"
            } else {
                "unexpected-token"
            };
            diagnostics.push(diagnostic(
                category,
                format!("{} requires {expected}", token.text),
                Span {
                    start: token.span.end,
                    end: token.span.end,
                    line: token.span.line,
                    column: token.span.column + token.text.chars().count() as u32,
                },
                &[expected],
                Vec::new(),
            ));
        }
        if token.text == ":"
            && following.is_none_or(|item| matches!(item.text.as_str(), "}" | "]" | ")" | ","))
        {
            let category = if following.is_none() {
                "unexpected-end"
            } else {
                "unexpected-token"
            };
            diagnostics.push(diagnostic(
                category,
                "pair requires a value after ':'",
                Span {
                    start: token.span.end,
                    end: token.span.end,
                    line: token.span.line,
                    column: token.span.column + 1,
                },
                &["value"],
                alloc::vec![fix(
                    "Insert null value",
                    token.span.end,
                    token.span.end,
                    "null",
                )],
            ));
        }
        if matches!(token.kind, "string" | "quoted-name") {
            let delimiter = token.text.chars().next().unwrap_or('"');
            if !quoted_token_closed(&token.text, delimiter) {
                let delimiter_text = delimiter.to_string();
                diagnostics.push(diagnostic(
                    "unexpected-end",
                    "unterminated quoted token",
                    token.span,
                    &[delimiter_text.as_str()],
                    alloc::vec![fix(
                        format!("Insert closing {delimiter}"),
                        token.span.end,
                        token.span.end,
                        delimiter_text.as_str(),
                    )],
                ));
            }
        }
        if token.kind == "raw-string" {
            let opener = token.text.split('"').next().unwrap_or("");
            let closer = format!("\"{}", "#".repeat(opener.matches('#').count()));
            if !token.text.ends_with(&closer) {
                diagnostics.push(diagnostic(
                    "unexpected-end",
                    "unterminated raw string",
                    token.span,
                    &[closer.as_str()],
                    alloc::vec![fix(
                        "Insert raw-string terminator",
                        token.span.end,
                        token.span.end,
                        closer.as_str(),
                    )],
                ));
            }
        }
        if token.kind == "binary"
            && !(opts.legacy_binary || opts.compatibility)
            && !(token.text.len() >= 2 && token.text.ends_with('|'))
        {
            diagnostics.push(diagnostic(
                "unexpected-end",
                "binary literal requires a closing pipe",
                Span {
                    start: source_end,
                    end: source_end,
                    line: token.span.line,
                    column: token.span.column,
                },
                &["|"],
                alloc::vec![fix("Insert closing pipe", source_end, source_end, "|",)],
            ));
        }
        if token.kind == "temporal" && token.text.contains('[') && !token.text.ends_with(']') {
            diagnostics.push(diagnostic(
                "unexpected-end",
                "named time-zone suffix requires ']'",
                token.span,
                &["]"],
                alloc::vec![fix("Insert closing ]", token.span.end, token.span.end, "]",)],
            ));
        }
    }
    diagnostics
}

fn next_structural(children: &[CstElement], start: usize, tokens: &[CstToken]) -> Option<usize> {
    (start..children.len()).find(|&index| match children[index] {
        CstElement::Token(token) => !tokens[token].trivia,
        CstElement::Node(_) => true,
    })
}

fn refine_node(nodes: &mut Vec<CstNode>, node: CstNodeId, tokens: &[CstToken], opts: &Options) {
    let nested: Vec<CstNodeId> = nodes[node]
        .children
        .iter()
        .filter_map(|element| match *element {
            CstElement::Node(id) if matches!(nodes[id].kind, "brace" | "bracket" | "tuple") => {
                Some(id)
            }
            _ => None,
        })
        .collect();
    for child in nested {
        refine_node(nodes, child, tokens, opts);
    }

    let source_children = core::mem::take(&mut nodes[node].children);
    let mut children = Vec::with_capacity(source_children.len());
    let mut cursor = 0usize;
    while cursor < source_children.len() {
        let child = source_children[cursor];
        let next_index = next_structural(&source_children, cursor + 1, tokens);
        let bind = match (child, next_index.map(|index| source_children[index])) {
            (CstElement::Node(name), Some(CstElement::Node(body))) => {
                nodes[name].kind == "name"
                    && matches!(nodes[body].kind, "brace" | "bracket" | "tuple")
                    && ((opts.compatibility || opts.legacy_binding)
                        || !source_children[cursor + 1..next_index.expect("body index")]
                            .iter()
                            .any(|element| match *element {
                                CstElement::Token(id) => {
                                    tokens[id].text.contains('\n') || tokens[id].text.contains('\r')
                                }
                                CstElement::Node(_) => false,
                            }))
            }
            _ => false,
        };
        if bind {
            let next = next_index.expect("bound body index");
            let CstElement::Node(name) = child else {
                unreachable!()
            };
            let CstElement::Node(container) = source_children[next] else {
                unreachable!()
            };
            nodes[name].kind = "node-name";
            let body = push_node(
                nodes,
                "body",
                nodes[container].span,
                alloc::vec![CstElement::Node(container)],
            );
            let mut segment = source_children[cursor..=next].to_vec();
            *segment.last_mut().expect("node segment") = CstElement::Node(body);
            let span = elements_span(&segment, nodes, tokens);
            let production = push_node(nodes, "node", span, segment);
            children.push(CstElement::Node(production));
            cursor = next + 1;
        } else {
            children.push(child);
            cursor += 1;
        }
    }

    let mut output = Vec::with_capacity(children.len());
    let mut index = 0usize;
    while index < children.len() {
        let child = children[index];
        let CstElement::Node(child_id) = child else {
            output.push(child);
            index += 1;
            continue;
        };
        let next_index = next_structural(&children, index + 1, tokens);
        let child_kind = nodes[child_id].kind;
        if matches!(child_kind, "name" | "string-literal")
            && let Some(colon_index) = next_index
            && let CstElement::Token(colon_id) = children[colon_index]
            && tokens[colon_id].text == ":"
            && let Some(value_index) = next_structural(&children, colon_index + 1, tokens)
        {
            let segment = children[index..=value_index].to_vec();
            let span = elements_span(&segment, nodes, tokens);
            let pair = push_node(nodes, "pair", span, segment);
            output.push(CstElement::Node(pair));
            index = value_index + 1;
            continue;
        }
        if child_kind == "discard-prefix"
            && let Some(value_index) = next_index
        {
            let segment = children[index..=value_index].to_vec();
            let span = elements_span(&segment, nodes, tokens);
            let discard = push_node(nodes, "discard-operand", span, segment);
            output.push(CstElement::Node(discard));
            index = value_index + 1;
            continue;
        }
        if matches!(
            child_kind,
            "name"
                | "string-literal"
                | "binary-literal"
                | "temporal-literal"
                | "scalar-literal"
                | "constant-reference"
                | "anchor-prefix"
                | "reference-prefix"
                | "lexical-value"
        ) {
            let atom = push_node(
                nodes,
                "atom",
                nodes[child_id].span,
                alloc::vec![CstElement::Node(child_id)],
            );
            output.push(CstElement::Node(atom));
        } else {
            output.push(child);
        }
        index += 1;
    }
    nodes[node].children = output;
}

fn refine_production_tree(
    nodes: &mut Vec<CstNode>,
    root: CstNodeId,
    tokens: &[CstToken],
    opts: &Options,
) {
    refine_node(nodes, root, tokens, opts);
    let structural: Vec<CstNodeId> = nodes[root]
        .children
        .iter()
        .filter_map(|element| match *element {
            CstElement::Node(id) => Some(id),
            CstElement::Token(_) => None,
        })
        .collect();
    let attribute_document = !structural.is_empty()
        && structural.iter().any(|&id| nodes[id].kind == "pair")
        && structural
            .iter()
            .all(|&id| matches!(nodes[id].kind, "pair" | "discard-operand"));
    if attribute_document {
        let children = core::mem::take(&mut nodes[root].children);
        let span = elements_span(&children, nodes, tokens);
        let attributes = push_node(nodes, "attribute-document", span, children);
        let stream = push_node(
            nodes,
            "stream-value",
            span,
            alloc::vec![CstElement::Node(attributes)],
        );
        nodes[root].children = alloc::vec![CstElement::Node(stream)];
        return;
    }
    let children = core::mem::take(&mut nodes[root].children);
    let mut wrapped = Vec::with_capacity(children.len());
    for child in children {
        match child {
            CstElement::Node(id) => {
                let stream = push_node(
                    nodes,
                    "stream-value",
                    nodes[id].span,
                    alloc::vec![CstElement::Node(id)],
                );
                wrapped.push(CstElement::Node(stream));
            }
            CstElement::Token(_) => wrapped.push(child),
        }
    }
    nodes[root].children = wrapped;
}

fn byte_offset_for_line_column(source: &str, line: u32, column: u32) -> usize {
    if line == 0 || column == 0 {
        return 0;
    }
    let mut current_line = 1u32;
    let mut current_column = 1u32;
    for (offset, ch) in source.char_indices() {
        if current_line == line && current_column == column {
            return offset;
        }
        if ch == '\n' {
            current_line += 1;
            current_column = 1;
        } else {
            current_column += 1;
        }
    }
    source.len()
}

/// Byte offsets of every `\n` in a source text, so that a byte offset can be
/// turned into a line and column without rescanning the prefix each time.
///
/// Building the table costs one pass over the source; each lookup is then a
/// binary search plus a walk of the current line. Without it, resolving a span
/// per semantic trace event is quadratic in the size of the document.
struct LineIndex {
    newlines: Vec<usize>,
}

impl LineIndex {
    fn new(source: &str) -> Self {
        Self {
            newlines: source
                .bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(offset, _)| offset)
                .collect(),
        }
    }

    fn span_at_bytes(&self, source: &str, start: usize, end: usize) -> Span {
        let mut safe_start = start.min(source.len());
        while !source.is_char_boundary(safe_start) {
            safe_start -= 1;
        }
        let mut safe_end = end.min(source.len()).max(safe_start);
        while !source.is_char_boundary(safe_end) {
            safe_end -= 1;
        }
        let preceding = self.newlines.partition_point(|&offset| offset < safe_start);
        let line = preceding as u32 + 1;
        let line_start = preceding
            .checked_sub(1)
            .map_or(0, |index| self.newlines[index] + 1);
        let column = source[line_start..safe_start].chars().count() as u32 + 1;
        Span {
            start: safe_start,
            end: safe_end,
            line,
            column,
        }
    }
}

fn semantic_diagnostic(error: &Error, source: &str, tokens: &[CstToken]) -> CstDiagnostic {
    let start = byte_offset_for_line_column(source, error.line, error.column);
    let message = error.to_string();
    let lower = message.to_ascii_lowercase();
    let mut expected: Vec<&str> = Vec::new();
    let mut fixes = Vec::new();
    if lower.contains("expected a value") || lower.contains("requires a pair") {
        expected.push("value");
    } else if lower.contains("expected anchor label") || lower.contains("expected a label") {
        expected.push("anchor label");
    } else if lower.contains("expected a name") {
        expected.push("name");
    } else if lower.contains("closing pipe") || lower.contains("binary literal") {
        expected.push("|");
        fixes.push(fix("Insert closing pipe", start, start, "|"));
    } else if lower.contains("unterminated quoted name") {
        expected.push("'");
        fixes.push(fix("Insert closing quote", start, start, "'"));
    } else if lower.contains("unterminated string") {
        expected.push("\"");
        fixes.push(fix("Insert closing quote", start, start, "\""));
    } else if lower.contains("unterminated named time-zone") {
        expected.push("]");
        fixes.push(fix("Insert closing ]", start, start, "]"));
    } else if lower.contains("consecutive commas")
        && let Some(token) = tokens
            .iter()
            .find(|token| token.span.start <= start && start < token.span.end)
    {
        expected.push("value");
        fixes.push(fix("Remove comma", token.span.start, token.span.end, ""));
    }
    diagnostic(
        error.category.as_str(),
        message,
        Span {
            start,
            end: start,
            line: error.line,
            column: error.column,
        },
        &expected,
        fixes,
    )
}

fn child_node_index(nodes: &[CstNode]) -> Vec<Vec<CstNodeId>> {
    let mut index = Vec::with_capacity(nodes.len());
    for node in nodes {
        let mut children: Vec<CstNodeId> = node
            .children
            .iter()
            .filter_map(|element| match *element {
                CstElement::Node(id) => Some(id),
                CstElement::Token(_) => None,
            })
            .collect();
        children.sort_by_key(|&id| (nodes[id].span.start, nodes[id].span.end));
        index.push(children);
    }
    index
}

fn nearest_owner(
    nodes: &[CstNode],
    children: &[Vec<CstNodeId>],
    root: CstNodeId,
    start: usize,
    end: usize,
) -> CstNodeId {
    let mut current = root;
    loop {
        let candidates = &children[current];
        let position = candidates.partition_point(|&id| nodes[id].span.start <= start);
        let Some(&candidate) = position
            .checked_sub(1)
            .and_then(|index| candidates.get(index))
        else {
            return current;
        };
        let span = nodes[candidate].span;
        if span.start <= start && span.end >= end {
            current = candidate;
        } else {
            return current;
        }
    }
}

fn stream_semantic_candidates(nodes: &[CstNode], root: CstNodeId) -> Vec<CstNodeId> {
    nodes[root]
        .children
        .iter()
        .filter_map(|element| match *element {
            CstElement::Node(id) if nodes[id].kind == "stream-value" => Some(id),
            _ => None,
        })
        .filter(|&id| {
            !matches!(
                nodes[id].children.first(),
                Some(CstElement::Node(child)) if nodes[*child].kind == "discard-operand"
            )
        })
        .collect()
}

fn populate_semantics(
    source: &str,
    opts: &Options,
    tokens: &[CstToken],
    nodes: &mut [CstNode],
    root: CstNodeId,
    diagnostics: &mut Vec<CstDiagnostic>,
) -> (Vec<Value>, Vec<CstNodeId>, Vec<CstSemanticTrace>) {
    let parsed = parse_document_with_trace(source, *opts);
    let has_error = parsed.error.is_some();
    if let Some(error) = parsed.error.as_ref() {
        let start = byte_offset_for_line_column(source, error.line, error.column);
        let terminal_recovery = error.category == Category::UnexpectedEnd
            && diagnostics.iter().any(|item| {
                item.category == error.category.as_str() && item.span.end == source.len()
            });
        if !terminal_recovery
            && !diagnostics
                .iter()
                .any(|item| item.category == error.category.as_str() && item.span.start == start)
        {
            diagnostics.push(semantic_diagnostic(error, source, tokens));
        }
    }
    let values = if has_error { Vec::new() } else { parsed.values };
    let value_traces = parsed.traces;
    nodes[root].semantic_value = Some(CstSemanticValue::Stream(values.clone()));
    let candidates = stream_semantic_candidates(nodes, root);
    let semantic_nodes = candidates[candidates.len().saturating_sub(values.len())..].to_vec();
    for (&node, value) in semantic_nodes.iter().zip(values.iter()) {
        nodes[node].semantic_value = Some(CstSemanticValue::Value(value.clone()));
    }

    let child_index = child_node_index(nodes);
    let lines = LineIndex::new(source);
    let mut traces = Vec::with_capacity(value_traces.len());
    for event in value_traces {
        let span = lines.span_at_bytes(source, event.start, event.end);
        let owner = nearest_owner(nodes, &child_index, root, span.start, span.end);
        if nodes[owner].semantic_value.is_none() {
            nodes[owner].semantic_value = Some(CstSemanticValue::Value(event.value.clone()));
        }
        traces.push(CstSemanticTrace {
            span,
            value: event.value,
            owner,
        });
    }
    (values, semantic_nodes, traces)
}

fn dedupe_diagnostics(diagnostics: &mut Vec<CstDiagnostic>) {
    let mut output = Vec::with_capacity(diagnostics.len());
    for item in diagnostics.drain(..) {
        let duplicate = output.iter().any(|existing: &CstDiagnostic| {
            existing.category == item.category
                && existing.span.start == item.span.start
                && existing.span.end == item.span.end
                && existing.expected == item.expected
        });
        if !duplicate {
            output.push(item);
        }
    }
    *diagnostics = output;
}

/// Tokenize into a lossless stream (every source byte is owned by exactly one
/// token). Mirrors the reference `_tokenize`.
fn tokenize(source: &str, opts: &Options) -> Result<Vec<CstToken>> {
    let chars: Vec<char> = source.chars().collect();
    let n = chars.len();
    let mut tokens: Vec<CstToken> = Vec::new();
    let mut byte_offset = 0usize;
    let mut line = 1u32;
    let mut column = 1u32;
    let hash_comment = opts.hash_underscore_comment || opts.compatibility;
    let legacy_binary = opts.legacy_binary || opts.compatibility;

    let emit = |kind: &'static str,
                start: usize,
                end: usize,
                trivia: bool,
                tokens: &mut Vec<CstToken>,
                byte_offset: &mut usize,
                line: &mut u32,
                column: &mut u32| {
        let text: String = chars[start..end].iter().collect();
        let start_byte = *byte_offset;
        let text_len = text.len();
        tokens.push(CstToken {
            kind,
            span: Span {
                start: start_byte,
                end: start_byte + text_len,
                line: *line,
                column: *column,
            },
            trivia,
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
            text,
        });
        // advance position over the emitted characters
        for &c in &chars[start..end] {
            *byte_offset += c.len_utf8();
            if c == '\n' {
                *line += 1;
                *column = 1;
            } else {
                *column += 1;
            }
        }
    };

    let starts_with = |i: usize, pat: &[char]| -> bool {
        pat.iter()
            .enumerate()
            .all(|(k, &pc)| chars.get(i + k) == Some(&pc))
    };
    // find `needle` (as chars) in chars[from..]; return the char index or None.
    let find_from = |needle: &[char], from: usize| -> Option<usize> {
        if needle.is_empty() || from > n {
            return None;
        }
        let last = n.checked_sub(needle.len())?;
        (from..=last).find(|&s| chars[s..s + needle.len()] == *needle)
    };

    let mut i = 0usize;
    while i < n {
        if tokens.len() >= opts.limits.max_cst_tokens {
            return Err(cst_error(
                Category::ResourceLimitExceeded,
                "maximum CST token count exceeded",
                line,
                column,
            ));
        }
        let start = i;
        let ch = chars[i];
        // whitespace run
        if matches!(ch, ' ' | '\t' | '\r' | '\n') {
            i += 1;
            while i < n && matches!(chars[i], ' ' | '\t' | '\r' | '\n') {
                i += 1;
            }
            emit(
                "whitespace",
                start,
                i,
                true,
                &mut tokens,
                &mut byte_offset,
                &mut line,
                &mut column,
            );
            continue;
        }
        // `#_` as legacy comment
        if starts_with(i, &['#', '_']) && hash_comment {
            i += 2;
            while i < n && !matches!(chars[i], '\r' | '\n') {
                i += 1;
            }
            emit(
                "comment",
                start,
                i,
                true,
                &mut tokens,
                &mut byte_offset,
                &mut line,
                &mut column,
            );
            continue;
        }
        // `#_` discard token
        if starts_with(i, &['#', '_']) {
            i += 2;
            emit(
                "discard",
                start,
                i,
                false,
                &mut tokens,
                &mut byte_offset,
                &mut line,
                &mut column,
            );
            continue;
        }
        // `#` comment to end of line
        if ch == '#' {
            i += 1;
            while i < n && !matches!(chars[i], '\r' | '\n') {
                i += 1;
            }
            emit(
                "comment",
                start,
                i,
                true,
                &mut tokens,
                &mut byte_offset,
                &mut line,
                &mut column,
            );
            continue;
        }
        // raw string r"..." / r#"..."#
        if ch == 'r' && i + 1 < n && matches!(chars[i + 1], '#' | '"') {
            let mut j = i + 1;
            while j < n && chars[j] == '#' {
                j += 1;
            }
            if j < n && chars[j] == '"' {
                let hashes = j - (i + 1);
                let mut endmark: Vec<char> = alloc::vec!['#'; hashes];
                endmark.insert(0, '"');
                i = match find_from(&endmark, j + 1) {
                    Some(stop) => stop + endmark.len(),
                    None => n,
                };
                emit(
                    "raw-string",
                    start,
                    i,
                    false,
                    &mut tokens,
                    &mut byte_offset,
                    &mut line,
                    &mut column,
                );
                continue;
            }
        }
        // quoted forms: " ` '
        if matches!(ch, '"' | '`' | '\'') {
            let end_delim = ch;
            i += 1;
            let mut escaped = false;
            while i < n {
                let current = chars[i];
                i += 1;
                if escaped {
                    escaped = false;
                } else if current == '\\' {
                    escaped = true;
                } else if current == end_delim {
                    break;
                }
            }
            let kind = if ch == '\'' { "quoted-name" } else { "string" };
            emit(
                kind,
                start,
                i,
                false,
                &mut tokens,
                &mut byte_offset,
                &mut line,
                &mut column,
            );
            continue;
        }
        // binary |...|
        if ch == '|' {
            i += 1;
            if legacy_binary {
                while i < n {
                    let current = chars[i];
                    if current == '=' {
                        while i < n && chars[i] == '=' {
                            i += 1;
                        }
                        break;
                    }
                    if current.is_ascii_alphanumeric()
                        || current == '+'
                        || current == '/'
                        || (current as u32) <= 32
                    {
                        i += 1;
                        continue;
                    }
                    break;
                }
            } else {
                while i < n && chars[i] != '|' {
                    i += 1;
                }
                if i < n {
                    i += 1;
                }
            }
            emit(
                "binary",
                start,
                i,
                false,
                &mut tokens,
                &mut byte_offset,
                &mut line,
                &mut column,
            );
            continue;
        }
        // temporal ^...
        if ch == '^' {
            i += 1;
            let mut in_zone = false;
            while i < n {
                let current = chars[i];
                if current == '[' {
                    in_zone = true;
                    i += 1;
                    continue;
                }
                if current == ']' && in_zone {
                    i += 1;
                    break;
                }
                if !in_zone
                    && (matches!(current, ' ' | '\t' | '\r' | '\n')
                        || matches!(current, ',' | ']' | '}' | ')' | '#'))
                {
                    break;
                }
                i += 1;
            }
            emit(
                "temporal",
                start,
                i,
                false,
                &mut tokens,
                &mut byte_offset,
                &mut line,
                &mut column,
            );
            continue;
        }
        // single-character punctuation and markers
        if matches!(
            ch,
            '{' | '}' | '[' | ']' | '(' | ')' | ':' | ',' | '&' | '*' | '$' | '^' | '∅'
        ) {
            i += 1;
            let kind = match ch {
                '&' => "anchor",
                '*' => "reference",
                '$' => "constant",
                '^' => "temporal-marker",
                _ => "punctuation",
            };
            emit(
                kind,
                start,
                i,
                false,
                &mut tokens,
                &mut byte_offset,
                &mut line,
                &mut column,
            );
            continue;
        }
        // numeric atom
        if is_python_digit(ch) || matches!(ch, '-' | '?' | '∞') {
            i += 1;
            while i < n
                && !matches!(chars[i], ' ' | '\t' | '\r' | '\n')
                && !matches!(chars[i], '{' | '}' | '[' | ']' | '(' | ')' | ',' | '#')
            {
                i += 1;
            }
            emit(
                "atom",
                start,
                i,
                false,
                &mut tokens,
                &mut byte_offset,
                &mut line,
                &mut column,
            );
            continue;
        }
        // bare name (default)
        i += 1;
        while i < n
            && !matches!(chars[i], ' ' | '\t' | '\r' | '\n')
            && !matches!(
                chars[i],
                '{' | '}' | '[' | ']' | '(' | ')' | ':' | ',' | '&' | '*' | '$' | '^' | '#'
            )
        {
            i += 1;
        }
        emit(
            "name",
            start,
            i,
            false,
            &mut tokens,
            &mut byte_offset,
            &mut line,
            &mut column,
        );
    }
    Ok(tokens)
}

/// Exact Unicode `str.isdigit()` ranges from the Python 3.13 reference used to
/// generate the CST corpus. This is intentionally narrower than
/// `char::is_numeric()` (`U+00BD` and CJK numerals are numeric but not digits).
fn is_python_digit(c: char) -> bool {
    matches!(
        c,
        '\u{30}'..='\u{39}'
            | '\u{B2}'..='\u{B3}'
            | '\u{B9}'
            | '\u{660}'..='\u{669}'
            | '\u{6F0}'..='\u{6F9}'
            | '\u{7C0}'..='\u{7C9}'
            | '\u{966}'..='\u{96F}'
            | '\u{9E6}'..='\u{9EF}'
            | '\u{A66}'..='\u{A6F}'
            | '\u{AE6}'..='\u{AEF}'
            | '\u{B66}'..='\u{B6F}'
            | '\u{BE6}'..='\u{BEF}'
            | '\u{C66}'..='\u{C6F}'
            | '\u{CE6}'..='\u{CEF}'
            | '\u{D66}'..='\u{D6F}'
            | '\u{DE6}'..='\u{DEF}'
            | '\u{E50}'..='\u{E59}'
            | '\u{ED0}'..='\u{ED9}'
            | '\u{F20}'..='\u{F29}'
            | '\u{1040}'..='\u{1049}'
            | '\u{1090}'..='\u{1099}'
            | '\u{1369}'..='\u{1371}'
            | '\u{17E0}'..='\u{17E9}'
            | '\u{1810}'..='\u{1819}'
            | '\u{1946}'..='\u{194F}'
            | '\u{19D0}'..='\u{19DA}'
            | '\u{1A80}'..='\u{1A89}'
            | '\u{1A90}'..='\u{1A99}'
            | '\u{1B50}'..='\u{1B59}'
            | '\u{1BB0}'..='\u{1BB9}'
            | '\u{1C40}'..='\u{1C49}'
            | '\u{1C50}'..='\u{1C59}'
            | '\u{2070}'
            | '\u{2074}'..='\u{2079}'
            | '\u{2080}'..='\u{2089}'
            | '\u{2460}'..='\u{2468}'
            | '\u{2474}'..='\u{247C}'
            | '\u{2488}'..='\u{2490}'
            | '\u{24EA}'
            | '\u{24F5}'..='\u{24FD}'
            | '\u{24FF}'
            | '\u{2776}'..='\u{277E}'
            | '\u{2780}'..='\u{2788}'
            | '\u{278A}'..='\u{2792}'
            | '\u{A620}'..='\u{A629}'
            | '\u{A8D0}'..='\u{A8D9}'
            | '\u{A900}'..='\u{A909}'
            | '\u{A9D0}'..='\u{A9D9}'
            | '\u{A9F0}'..='\u{A9F9}'
            | '\u{AA50}'..='\u{AA59}'
            | '\u{ABF0}'..='\u{ABF9}'
            | '\u{FF10}'..='\u{FF19}'
            | '\u{104A0}'..='\u{104A9}'
            | '\u{10A40}'..='\u{10A43}'
            | '\u{10D30}'..='\u{10D39}'
            | '\u{10E60}'..='\u{10E68}'
            | '\u{11052}'..='\u{1105A}'
            | '\u{11066}'..='\u{1106F}'
            | '\u{110F0}'..='\u{110F9}'
            | '\u{11136}'..='\u{1113F}'
            | '\u{111D0}'..='\u{111D9}'
            | '\u{112F0}'..='\u{112F9}'
            | '\u{11450}'..='\u{11459}'
            | '\u{114D0}'..='\u{114D9}'
            | '\u{11650}'..='\u{11659}'
            | '\u{116C0}'..='\u{116C9}'
            | '\u{11730}'..='\u{11739}'
            | '\u{118E0}'..='\u{118E9}'
            | '\u{11950}'..='\u{11959}'
            | '\u{11C50}'..='\u{11C59}'
            | '\u{11D50}'..='\u{11D59}'
            | '\u{11DA0}'..='\u{11DA9}'
            | '\u{11F50}'..='\u{11F59}'
            | '\u{16A60}'..='\u{16A69}'
            | '\u{16AC0}'..='\u{16AC9}'
            | '\u{16B50}'..='\u{16B59}'
            | '\u{1D7CE}'..='\u{1D7FF}'
            | '\u{1E140}'..='\u{1E149}'
            | '\u{1E2F0}'..='\u{1E2F9}'
            | '\u{1E4F0}'..='\u{1E4F9}'
            | '\u{1E950}'..='\u{1E959}'
            | '\u{1F100}'..='\u{1F10A}'
            | '\u{1FBF0}'..='\u{1FBF9}'
    )
}

/// Split `s` into lines, keeping the line terminators, using Python's universal
/// newline set (`\n`, `\r`, `\r\n`, `\v`, `\f`, `\x1c`, `\x1d`, `\x1e`, `\x85`,
/// ` `, ` `). Mirrors `str.splitlines(keepends=True)`.
fn splitlines_keepends(s: &str) -> Vec<String> {
    let is_boundary = |c: char| {
        matches!(
            c,
            '\n' | '\r'
                | '\u{0b}'
                | '\u{0c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        )
    };
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        current.push(c);
        if is_boundary(c) {
            if c == '\r' && chars.peek() == Some(&'\n') {
                current.push(chars.next().unwrap());
            }
            lines.push(core::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// The visual indentation width of `line`'s leading whitespace: spaces count 1,
/// tabs advance to the next multiple of 8. Mirrors `_indent_width`.
fn indent_width(line: &str) -> usize {
    let mut width = 0usize;
    for c in line.chars() {
        match c {
            ' ' => width += 1,
            '\t' => width = (width / 8 + 1) * 8,
            _ => break,
        }
    }
    width
}

/// True when `text` is a bare or single-namespaced node name (`a` or `ns/a`)
/// whose parts are all valid identifiers. Mirrors `_indented_node_name`.
fn indented_node_name(text: &str) -> bool {
    let parts: Vec<&str> = text.split('/').collect();
    if parts.len() > 2 || parts.iter().any(|p| p.is_empty()) {
        return false;
    }
    parts.iter().all(|p| is_py_identifier(p))
}

/// Matches Python's `str.isidentifier()`: a non-empty string whose first
/// character is `_` or `XID_Start` and whose remaining characters are all
/// `XID_Continue` (which already includes `_`).
fn is_py_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        None => false,
        Some(first) => {
            if first != '_' && !unicode_ident::is_xid_start(first) {
                return false;
            }
            chars.all(unicode_ident::is_xid_continue)
        }
    }
}

/// Rewrite historical indentation-bound node bodies as Core brace bodies.
///
/// Comments, blank lines, spelling, and the indentation inside each migrated
/// body are retained; only the braces needed to remove indentation-sensitive
/// semantics are inserted. Mirrors `migrate_indented2026`: the result is
/// validated by re-parsing, so an input that would migrate to invalid AXON is
/// reported as an error.
pub fn migrate_indented(source: &str, opts: &Options) -> Result<String> {
    // Preflight the lossless parse so both byte and token limits apply.
    let _ = parse_cst(source, opts)?;
    let lines = splitlines_keepends(source);

    // Significant lines: non-blank, non-comment. Record (index, indent, content).
    let mut significant: Vec<(usize, usize, String)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let content = line.trim_start_matches([' ', '\t']);
        if !content.trim().is_empty() && !content.starts_with('#') {
            significant.push((
                index,
                indent_width(line),
                trim_end_crlf(content).to_string(),
            ));
        }
    }
    // For each significant line index, cache the following line and width.
    let mut next_significant = alloc::vec![None; lines.len()];
    for pair in significant.windows(2) {
        next_significant[pair[0].0] = Some((pair[1].0, pair[1].1));
    }

    let mut output: Vec<String> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let stripped = line.trim_start_matches([' ', '\t']);
        if !stripped.trim().is_empty() && !stripped.starts_with('#') {
            let current_indent = indent_width(line);
            while stack.last().is_some_and(|&top| current_indent <= top) {
                let popped = stack.pop().unwrap();
                output.push(format!("{}}}\n", " ".repeat(popped)));
            }
            let following = next_significant[index];
            let code = trim_end_crlf(stripped);
            if indented_node_name(code) && following.is_some_and(|(_, w)| w > current_indent) {
                let newline = if line.ends_with("\r\n") { "\r\n" } else { "\n" };
                let prefix = &line[..line.len() - stripped.len()];
                output.push(format!("{prefix}{code}{{{newline}"));
                stack.push(current_indent);
                continue;
            }
        }
        output.push(line.clone());
    }
    while let Some(popped) = stack.pop() {
        output.push(format!("{}}}\n", " ".repeat(popped)));
    }
    let result: String = output.concat();
    // Validate the migration parses, matching the reference's `loads2026` gate.
    from_str_stream_with(&result, *opts)?;
    Ok(result)
}

/// Strip a single trailing `\r`, `\n`, or `\r\n`-worth of those characters --
/// equivalent to Python's `str.rstrip('\r\n')` (which removes any run of them).
fn category_from_name(name: &str) -> Category {
    match name {
        "invalid-unicode" => Category::InvalidUnicode,
        "unexpected-end" => Category::UnexpectedEnd,
        "unexpected-token" => Category::UnexpectedToken,
        "invalid-number" => Category::InvalidNumber,
        "invalid-temporal" => Category::InvalidTemporal,
        "invalid-escape" => Category::InvalidEscape,
        "invalid-name" => Category::InvalidName,
        "invalid-binary" => Category::InvalidBinary,
        "invalid-link" => Category::InvalidLink,
        "duplicate-key" => Category::DuplicateKey,
        "duplicate-set-element" => Category::DuplicateSetElement,
        "duplicate-anchor" => Category::DuplicateAnchor,
        "unknown-reference" => Category::UnknownReference,
        "unknown-constant" => Category::UnknownConstant,
        "unsupported-profile-feature" => Category::UnsupportedProfileFeature,
        "resource-limit-exceeded" => Category::ResourceLimitExceeded,
        _ => Category::UnexpectedToken,
    }
}

/// Format Core AXON structurally while retaining every comment token.
pub fn format_cst(source: &str, indent: usize, opts: &Options) -> Result<String> {
    // Bound the caller-supplied indent so `depth * indent` cannot overflow or
    // request an unbounded allocation while building each line (audit L2).
    if indent > 256 {
        return Err(Error::new(
            Category::ResourceLimitExceeded,
            "format indent width is too large",
            0,
            0,
        ));
    }
    let migrated = migrate_indented(source, opts)?;
    let document = parse_cst(&migrated, opts)?;
    if let Some(item) = document.diagnostics.first() {
        return Err(Error::new(
            category_from_name(&item.category),
            item.message.clone(),
            item.span.line,
            item.span.column,
        ));
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut previous = String::new();
    let mut previous_text = String::new();
    let mut last_source_line = 1u32;

    fn ensure_indent(current: &mut String, depth: usize, indent: usize) {
        if current.is_empty() {
            current.push_str(&" ".repeat(depth.saturating_mul(indent)));
        }
    }

    fn flush(lines: &mut Vec<String>, current: &mut String) {
        if !current.trim().is_empty() {
            lines.push(current.trim_end().to_string());
        }
        current.clear();
    }

    for token in &document.tokens {
        if token.kind == "whitespace" {
            continue;
        }
        let text = token.text.as_str();
        if depth == 0
            && !current.trim().is_empty()
            && token.span.line > last_source_line
            && token.kind != "comment"
        {
            flush(&mut lines, &mut current);
        }
        if token.kind == "comment" {
            ensure_indent(&mut current, depth, indent);
            if !current.trim().is_empty() {
                current.push(' ');
            }
            current.push_str(text);
            flush(&mut lines, &mut current);
            previous.clear();
            previous.push_str("comment");
            last_source_line = token.span.line;
            continue;
        }
        if matches!(text, "}" | "]" | ")") {
            flush(&mut lines, &mut current);
            depth = depth.saturating_sub(1);
            ensure_indent(&mut current, depth, indent);
            current.push_str(text);
            previous.clear();
            previous.push_str(text);
            previous_text.clear();
            previous_text.push_str(text);
            last_source_line = token.span.line;
            continue;
        }

        ensure_indent(&mut current, depth, indent);
        if matches!(text, "{" | "[" | "(") {
            let binds_to_name =
                previous == "name" && !matches!(previous_text.as_str(), "null" | "true" | "false");
            if !current.trim().is_empty()
                && !binds_to_name
                && !matches!(previous.as_str(), "anchor" | "reference")
            {
                current.push(' ');
            }
            current.push_str(text);
            depth += 1;
            flush(&mut lines, &mut current);
        } else if text == ":" {
            while current.ends_with(char::is_whitespace) {
                current.pop();
            }
            current.push(':');
        } else if text == "," {
            while current.ends_with(char::is_whitespace) {
                current.pop();
            }
            flush(&mut lines, &mut current);
        } else {
            let ends_with_prefix = current
                .trim_end()
                .chars()
                .next_back()
                .is_some_and(|ch| matches!(ch, ':' | '&' | '*' | '$' | '^'));
            if !current.trim().is_empty()
                && !ends_with_prefix
                && !matches!(
                    previous.as_str(),
                    "anchor" | "reference" | "constant" | "temporal-marker"
                )
            {
                current.push(' ');
            }
            current.push_str(text);
        }
        previous.clear();
        if token.kind == "punctuation" {
            previous.push_str(text);
        } else {
            previous.push_str(token.kind);
        }
        previous_text.clear();
        previous_text.push_str(text);
        last_source_line = token.span.line;
    }
    flush(&mut lines, &mut current);
    let mut result = lines.join("\n");
    if !lines.is_empty() {
        result.push('\n');
    }
    from_str_stream_with(&result, *opts)?;
    Ok(result)
}

fn trim_end_crlf(s: &str) -> &str {
    s.trim_end_matches(['\r', '\n'])
}
