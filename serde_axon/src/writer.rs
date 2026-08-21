//! Writing [`Value`] back to AXON 2026 text. `to_string` emits a compact,
//! round-trippable form; `to_string_canonical` follows the §14 canonical rules
//! (sorted map keys, decimal normal form, caret temporals, closed-pipe binary).

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

use crate::error::{Category, Error, Result};
use crate::graph::{GraphDocument, GraphId, GraphNode, GraphObject, GraphValue};
use crate::value::{Anchor, Decimal, Link, NodeStyle, Temporal, Time, Value, Zone};

const DEFAULT_MAX_DEPTH: u32 = 128;

/// Options for semantic compact AXON output.
///
/// The existing [`to_string`] and [`to_string_stream`] APIs preserve explicit
/// `Value::Anchor` / `Value::Ref` markers exactly. The optioned APIs resolve
/// those markers first, matching `dumps2026(..., graph=...)`: with `graph`
/// disabled, acyclic sharing is expanded and cycles are rejected; with it
/// enabled, shared and cyclic composites receive deterministic numeric labels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WriteOptions {
    /// Preserve shared and cyclic composite identity with `&N` / `*N` markers.
    pub graph: bool,
}

/// Emit `value` as compact AXON text.
pub fn to_string(value: &Value) -> Result<String> {
    validate_write(value, false, 1)?;
    let mut out = String::new();
    write_value(&mut out, value, false);
    Ok(out)
}

/// Emit an ordered AXON document stream in compact form.
///
/// Top-level values are separated by a single line feed. An empty slice emits
/// an empty document.
pub fn to_string_stream(values: &[Value]) -> Result<String> {
    let mut out = String::new();
    for (index, value) in values.iter().enumerate() {
        validate_write(value, false, 1)?;
        if index != 0 {
            out.push('\n');
        }
        write_value(&mut out, value, false);
    }
    Ok(out)
}

/// Emit a semantic value as compact AXON with explicit Graph-profile policy.
///
/// Unlike [`to_string`], this resolves structural graph markers before
/// writing. It is the Rust equivalent of Python's
/// `dumps2026([value], canonical=False, graph=options.graph)` for one value.
pub fn to_string_with(value: &Value, options: WriteOptions) -> Result<String> {
    to_string_stream_with(core::slice::from_ref(value), options)
}

/// Emit a semantic document stream as compact AXON with explicit Graph-profile
/// policy.
///
/// Resolution spans the complete stream, so a later root can share an object
/// anchored by an earlier root. With `graph: false`, cycles return
/// `unsupported-profile-feature` rather than recursing indefinitely.
pub fn to_string_stream_with(values: &[Value], options: WriteOptions) -> Result<String> {
    for value in values {
        validate_write(value, false, 1)?;
    }
    let graph = GraphDocument::from_values(values)?;
    let normalized = if options.graph {
        CanonicalGraph::new(&graph, false).emit_stream()
    } else {
        materialize_graph_stream(&graph)?
    };
    let mut out = String::new();
    for (index, value) in normalized.iter().enumerate() {
        if index != 0 {
            out.push('\n');
        }
        write_value(&mut out, value, false);
    }
    Ok(out)
}

/// Emit `value` as Canonical AXON text (§14): deterministic, key-sorted.
///
/// Under the Graph profile, canonical form also normalizes anchors and
/// references (§14): an anchor whose label is never referenced, or that binds a
/// scalar, is inlined (the wrapper and label vanish); an anchor that binds a
/// composite and is referenced keeps its identity but is relabeled to a
/// 1-based ordinal assigned in traversal order, and its references become
/// `*N`. This is what makes "same value -> same bytes -> same CID" hold across
/// implementations for graph documents.
pub fn to_string_canonical(value: &Value) -> Result<String> {
    validate_write(value, true, 1)?;
    let normalized = canonicalize_graph_checked(value)?;
    let mut out = String::new();
    write_value(&mut out, &normalized, true);
    Ok(out)
}

/// Emit an ordered AXON document stream in Canonical AXON form.
///
/// Graph labels are analyzed and normalized across the complete stream. This
/// distinction matters when an anchor is one top-level value and a later
/// top-level value references it: independently canonicalizing each value
/// would lose that shared identity. Top-level values are separated by a single
/// line feed; an empty slice emits an empty document.
pub fn to_string_stream_canonical(values: &[Value]) -> Result<String> {
    for value in values {
        validate_write(value, true, 1)?;
    }
    let normalized = canonicalize_graph_stream_checked(values)?;
    let mut out = String::new();
    for (index, value) in normalized.iter().enumerate() {
        if index != 0 {
            out.push('\n');
        }
        write_value(&mut out, value, true);
    }
    Ok(out)
}

fn write_error(category: Category, message: &str) -> Error {
    Error::new(category, message, 0, 0)
}

fn validate_write(value: &Value, canonical: bool, depth: u32) -> Result<()> {
    if depth > DEFAULT_MAX_DEPTH {
        return Err(write_error(
            Category::ResourceLimitExceeded,
            "maximum writer nesting depth exceeded",
        ));
    }
    let nested = depth.saturating_add(1);
    match value {
        Value::Int(text) => {
            let digits = text
                .strip_prefix('-')
                .or_else(|| text.strip_prefix('+'))
                .unwrap_or(text.as_str());
            if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(write_error(
                    Category::InvalidNumber,
                    "invalid integer value",
                ));
            }
        }
        Value::Decimal(Decimal::ScaledFinite { .. }) if canonical => {
            return Err(write_error(
                Category::UnsupportedProfileFeature,
                "scale-significant values are incompatible with canonical output",
            ));
        }
        Value::Decimal(
            Decimal::Finite { mantissa, exponent } | Decimal::ScaledFinite { mantissa, exponent },
        ) => {
            // Positional notation writes every place between the mantissa and
            // the exponent, so an extreme exponent turns a few bytes of input
            // into gigabytes of output. Refused here, before anything is
            // allocated, in the same place the other writer limits are enforced.
            let too_long = decimal_render_len(mantissa, *exponent)
                .is_none_or(|length| length > MAX_DECIMAL_RENDER_DIGITS);
            if too_long {
                return Err(write_error(
                    Category::ResourceLimitExceeded,
                    "decimal exponent expands beyond the maximum writable length",
                ));
            }
        }
        Value::Temporal(Temporal::Time(time)) => validate_time(time)?,
        Value::Temporal(Temporal::DateTime(datetime)) => validate_time(&datetime.time)?,
        Value::List(items) | Value::Tuple(items) => {
            for item in items {
                validate_write(item, canonical, nested)?;
            }
        }
        Value::Set(items) => {
            // The parser raises `duplicate-set-element` in compact and
            // canonical mode alike, so never emit text it would refuse.
            for (index, item) in items.iter().enumerate() {
                if items[..index].iter().any(|previous| previous == item) {
                    return Err(write_error(
                        Category::DuplicateSetElement,
                        "cannot encode a set containing duplicate values",
                    ));
                }
            }
            for item in items {
                validate_write(item, canonical, nested)?;
            }
        }
        Value::Map(pairs) | Value::OrderedMap(pairs) => {
            for (index, (key, item)) in pairs.iter().enumerate() {
                let Value::String(text) = key else {
                    return Err(write_error(
                        Category::UnexpectedToken,
                        "AXON map keys must be strings",
                    ));
                };
                let repeated = pairs[..index]
                    .iter()
                    .any(|(previous, _)| matches!(previous, Value::String(p) if p == text));
                if repeated {
                    return Err(write_error(
                        Category::DuplicateKey,
                        "cannot encode a map containing duplicate keys",
                    ));
                }
                validate_write(item, canonical, nested)?;
            }
        }
        Value::Node(node) => {
            match node.style {
                NodeStyle::Unit => {
                    if !node.attributes.is_empty() || !node.children.is_empty() {
                        return Err(write_error(
                            Category::UnexpectedToken,
                            "a unit-style node cannot carry attributes or children",
                        ));
                    }
                }
                NodeStyle::Tuple | NodeStyle::List => {
                    if !node.attributes.is_empty() {
                        return Err(write_error(
                            Category::UnexpectedToken,
                            "a sequence-style node cannot carry attributes",
                        ));
                    }
                }
                NodeStyle::Brace => {
                    for (index, (name, _)) in node.attributes.iter().enumerate() {
                        if node.attributes[..index]
                            .iter()
                            .any(|(prev, _)| prev == name)
                        {
                            return Err(write_error(
                                Category::DuplicateKey,
                                "cannot encode a node with duplicate attribute names",
                            ));
                        }
                    }
                }
            }
            for (_, item) in &node.attributes {
                validate_write(item, canonical, nested)?;
            }
            for item in &node.children {
                validate_write(item, canonical, nested)?;
            }
        }
        Value::Anchor(anchor) => validate_write(&anchor.value, canonical, nested)?,
        Value::Undefined => {
            return Err(write_error(
                Category::UnknownReference,
                "undefined sentinel has no Core spelling",
            ));
        }
        Value::Null
        | Value::Bool(_)
        | Value::Float(_)
        | Value::Decimal(_)
        | Value::String(_)
        | Value::Bytes(_)
        | Value::Temporal(Temporal::Date(_))
        | Value::Link(_)
        | Value::Ref(_) => {}
    }
    Ok(())
}

fn validate_time(time: &Time) -> Result<()> {
    if time.hour > 23
        || time.minute > 59
        || time.second > 59
        || time.nanosecond > 999_999_999
        || time
            .fraction_digits
            .is_some_and(|digits| !(1..=9).contains(&digits))
    {
        return Err(write_error(
            Category::InvalidTemporal,
            "time field out of range",
        ));
    }
    Ok(())
}

/// Resolve graph markers and re-emit identity, optionally following canonical
/// container traversal order. Binary AXON uses the non-canonical mode to retain
/// insertion order while still turning labels into object identity.
pub(crate) fn normalize_graph(value: &Value, canonical: bool) -> Result<Value> {
    // Propagate graph-construction failures (e.g., a duplicate/invalid anchor)
    // rather than silently falling back to the unnormalized tree, which would
    // let a broken graph reach share-index encoding (audit M8).
    let graph = GraphDocument::from_value(value)?;
    Ok(CanonicalGraph::new(&graph, canonical)
        .emit_stream()
        .pop()
        .expect("a one-value graph stream has one result"))
}

fn canonicalize_graph_checked(value: &Value) -> Result<Value> {
    canonicalize_graph_stream_checked(core::slice::from_ref(value)).map(|mut values| {
        values
            .pop()
            .expect("a one-value graph stream has one result")
    })
}

fn canonicalize_graph_stream_checked(values: &[Value]) -> Result<Vec<Value>> {
    let graph = GraphDocument::from_values(values)?;
    Ok(CanonicalGraph::new(&graph, true).emit_stream())
}

/// Expand an identity graph into a plain tree, duplicating acyclic shared
/// objects. This is compact output with Python's `graph=False` policy.
fn materialize_graph_stream(graph: &GraphDocument) -> Result<Vec<Value>> {
    let mut out = Vec::with_capacity(graph.roots().len());
    let mut active = Vec::new();
    for root in graph.roots() {
        out.push(materialize_graph_value(graph, root, &mut active, 1)?);
    }
    Ok(out)
}

fn materialize_graph_value(
    graph: &GraphDocument,
    value: &GraphValue,
    active: &mut Vec<GraphId>,
    depth: u32,
) -> Result<Value> {
    if depth > DEFAULT_MAX_DEPTH {
        return Err(write_error(
            Category::ResourceLimitExceeded,
            "maximum writer nesting depth exceeded",
        ));
    }
    let GraphValue::Object(id) = value else {
        return match value {
            GraphValue::Scalar(value) => Ok(value.clone()),
            GraphValue::Object(_) => unreachable!(),
        };
    };
    if active.contains(id) {
        return Err(write_error(
            Category::UnsupportedProfileFeature,
            "cyclic values require graph output",
        ));
    }
    active.push(*id);
    let nested = depth.saturating_add(1);
    let result = match graph.object(*id).expect("GraphId belongs to its document") {
        GraphObject::List(items) => {
            Value::List(materialize_graph_each(graph, items, active, nested)?)
        }
        GraphObject::Tuple(items) => {
            Value::Tuple(materialize_graph_each(graph, items, active, nested)?)
        }
        GraphObject::Set(items) => {
            Value::Set(materialize_graph_each(graph, items, active, nested)?)
        }
        GraphObject::Map(pairs) => {
            Value::Map(materialize_graph_pairs(graph, pairs, active, nested)?)
        }
        GraphObject::OrderedMap(pairs) => {
            Value::OrderedMap(materialize_graph_pairs(graph, pairs, active, nested)?)
        }
        GraphObject::Node(node) => {
            let mut attributes = Vec::with_capacity(node.attributes.len());
            for (key, value) in &node.attributes {
                attributes.push((
                    key.clone(),
                    materialize_graph_value(graph, value, active, nested)?,
                ));
            }
            Value::Node(Box::new(crate::value::Node {
                name: node.name.clone(),
                style: node.style,
                attributes,
                children: materialize_graph_each(graph, &node.children, active, nested)?,
            }))
        }
    };
    active.pop();
    Ok(result)
}

fn materialize_graph_each(
    graph: &GraphDocument,
    values: &[GraphValue],
    active: &mut Vec<GraphId>,
    depth: u32,
) -> Result<Vec<Value>> {
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        out.push(materialize_graph_value(graph, value, active, depth)?);
    }
    Ok(out)
}

fn materialize_graph_pairs(
    graph: &GraphDocument,
    pairs: &[(GraphValue, GraphValue)],
    active: &mut Vec<GraphId>,
    depth: u32,
) -> Result<Vec<(Value, Value)>> {
    let mut out = Vec::with_capacity(pairs.len());
    for (key, value) in pairs {
        out.push((
            materialize_graph_value(graph, key, active, depth)?,
            materialize_graph_value(graph, value, active, depth)?,
        ));
    }
    Ok(out)
}

/// Canonical traversal over a resolved graph arena.
///
/// Labels belong to object identity, not to the source location of an `&label`
/// marker.  Resolving first is what allows an unordered set to move the first
/// occurrence of an object and then place its canonical anchor there.
struct CanonicalGraph<'a> {
    graph: &'a GraphDocument,
    counts: Vec<usize>,
    cyclic: Vec<bool>,
    emitted: Vec<bool>,
    labels: Vec<Option<usize>>,
    next_label: usize,
    canonical: bool,
}

impl<'a> CanonicalGraph<'a> {
    fn new(graph: &'a GraphDocument, canonical: bool) -> Self {
        let count = graph.object_count();
        let mut this = Self {
            graph,
            counts: alloc::vec![0; count],
            cyclic: alloc::vec![false; count],
            emitted: alloc::vec![false; count],
            labels: alloc::vec![None; count],
            next_label: 1,
            canonical,
        };
        let mut expanded = alloc::vec![false; count];
        let mut active = Vec::new();
        for root in graph.roots() {
            this.scan(root, &mut expanded, &mut active);
        }
        this
    }

    fn scan(&mut self, value: &GraphValue, expanded: &mut [bool], active: &mut Vec<GraphId>) {
        let GraphValue::Object(id) = value else {
            return;
        };
        self.counts[id.index()] = self.counts[id.index()].saturating_add(1);
        if active.contains(id) {
            self.cyclic[id.index()] = true;
            return;
        }
        if expanded[id.index()] {
            return;
        }
        expanded[id.index()] = true;
        active.push(*id);
        self.for_each_child(*id, |this, child| this.scan(child, expanded, active));
        active.pop();
    }

    fn for_each_child(&mut self, id: GraphId, mut visit: impl FnMut(&mut Self, &GraphValue)) {
        match self
            .graph
            .object(id)
            .expect("GraphId belongs to its document")
        {
            GraphObject::List(items) | GraphObject::Tuple(items) | GraphObject::Set(items) => {
                for item in items {
                    visit(self, item);
                }
            }
            GraphObject::Map(pairs) => {
                let mut order: Vec<usize> = (0..pairs.len()).collect();
                if self.canonical {
                    order.sort_by(|left, right| {
                        graph_key_bytes(&pairs[*left].0).cmp(&graph_key_bytes(&pairs[*right].0))
                    });
                }
                for index in order {
                    visit(self, &pairs[index].1);
                }
            }
            GraphObject::OrderedMap(pairs) => {
                for (_, value) in pairs {
                    visit(self, value);
                }
            }
            GraphObject::Node(node) => {
                let mut order: Vec<usize> = (0..node.attributes.len()).collect();
                if self.canonical {
                    order.sort_by(|left, right| {
                        node.attributes[*left]
                            .0
                            .as_bytes()
                            .cmp(node.attributes[*right].0.as_bytes())
                    });
                }
                for index in order {
                    visit(self, &node.attributes[index].1);
                }
                for child in &node.children {
                    visit(self, child);
                }
            }
        }
    }

    fn emit_stream(mut self) -> Vec<Value> {
        let mut roots = Vec::with_capacity(self.graph.roots().len());
        for root in self.graph.roots() {
            roots.push(self.emit(root));
        }
        roots
    }

    fn emit(&mut self, value: &GraphValue) -> Value {
        match value {
            GraphValue::Scalar(value) => value.clone(),
            GraphValue::Object(id) => {
                let shared = self.counts[id.index()] > 1 || self.cyclic[id.index()];
                if !shared {
                    return self.emit_object(*id);
                }
                let label = match self.labels[id.index()] {
                    Some(label) => label,
                    None => {
                        let label = self.next_label;
                        self.next_label += 1;
                        self.labels[id.index()] = Some(label);
                        label
                    }
                };
                if self.emitted[id.index()] {
                    return Value::Ref(label.to_string());
                }
                self.emitted[id.index()] = true;
                Value::Anchor(Box::new(Anchor {
                    label: label.to_string(),
                    value: self.emit_object(*id),
                }))
            }
        }
    }

    fn emit_object(&mut self, id: GraphId) -> Value {
        match self
            .graph
            .object(id)
            .expect("GraphId belongs to its document")
        {
            GraphObject::List(items) => Value::List(self.emit_each(items)),
            GraphObject::Tuple(items) => Value::Tuple(self.emit_each(items)),
            GraphObject::Set(items) => {
                if !self.canonical {
                    return Value::Set(self.emit_each(items));
                }
                let mut decorated: Vec<(Vec<u8>, &GraphValue)> = items
                    .iter()
                    .map(|item| (self.sort_preview(item, Vec::new(), Vec::new(), 1), item))
                    .collect();
                decorated.sort_by(|left, right| left.0.cmp(&right.0));
                Value::Set(
                    decorated
                        .into_iter()
                        .map(|(_, item)| self.emit(item))
                        .collect(),
                )
            }
            GraphObject::Map(pairs) => {
                let mut order: Vec<usize> = (0..pairs.len()).collect();
                if self.canonical {
                    order.sort_by(|left, right| {
                        graph_key_bytes(&pairs[*left].0).cmp(&graph_key_bytes(&pairs[*right].0))
                    });
                }
                Value::Map(
                    order
                        .into_iter()
                        .map(|index| (self.emit(&pairs[index].0), self.emit(&pairs[index].1)))
                        .collect(),
                )
            }
            GraphObject::OrderedMap(pairs) => Value::OrderedMap(self.emit_pairs(pairs)),
            GraphObject::Node(node) => Value::Node(Box::new(self.emit_node(node))),
        }
    }

    fn emit_each(&mut self, items: &[GraphValue]) -> Vec<Value> {
        items.iter().map(|item| self.emit(item)).collect()
    }

    fn emit_pairs(&mut self, pairs: &[(GraphValue, GraphValue)]) -> Vec<(Value, Value)> {
        pairs
            .iter()
            .map(|(key, value)| (self.emit(key), self.emit(value)))
            .collect()
    }

    fn emit_node(&mut self, node: &GraphNode) -> crate::value::Node {
        let mut order: Vec<usize> = (0..node.attributes.len()).collect();
        if self.canonical {
            order.sort_by(|left, right| {
                node.attributes[*left]
                    .0
                    .as_bytes()
                    .cmp(node.attributes[*right].0.as_bytes())
            });
        }
        crate::value::Node {
            name: node.name.clone(),
            style: node.style,
            attributes: order
                .into_iter()
                .map(|index| {
                    (
                        node.attributes[index].0.clone(),
                        self.emit(&node.attributes[index].1),
                    )
                })
                .collect(),
            children: self.emit_each(&node.children),
        }
    }

    /// Deterministic label-free set-order preview, mirroring pyaxon's graph
    /// canonicalizer. Object ids never enter the bytes: only local traversal
    /// ordinals, graph kind, occurrence count, and cycle state do.
    fn sort_preview(
        &self,
        value: &GraphValue,
        mut active: Vec<GraphId>,
        mut seen: Vec<GraphId>,
        depth: u32,
    ) -> Vec<u8> {
        self.sort_preview_in(value, &mut active, &mut seen, depth)
    }

    fn sort_preview_in(
        &self,
        value: &GraphValue,
        active: &mut Vec<GraphId>,
        seen: &mut Vec<GraphId>,
        depth: u32,
    ) -> Vec<u8> {
        if depth > DEFAULT_MAX_DEPTH {
            return b"\xffdepth".to_vec();
        }
        let GraphValue::Object(id) = value else {
            return graph_scalar_bytes(value);
        };
        if active.contains(id) {
            let ordinal = seen.iter().position(|known| known == id).unwrap_or(0) + 1;
            return graph_backedge(
                b"cycle",
                graph_marker(self.graph.object(*id).unwrap()),
                ordinal,
            );
        }
        if let Some(index) = seen.iter().position(|known| known == id) {
            return graph_backedge(
                b"ref",
                graph_marker(self.graph.object(*id).unwrap()),
                index + 1,
            );
        }
        seen.push(*id);
        active.push(*id);
        let object = self
            .graph
            .object(*id)
            .expect("GraphId belongs to its document");
        let mut prefix = self.preview_object(object, active, seen, depth);
        active.pop();
        prefix.extend_from_slice(b"\0graph:");
        prefix.extend_from_slice(graph_marker(object));
        prefix.push(b':');
        prefix.extend_from_slice(self.counts[id.index()].to_string().as_bytes());
        prefix.push(b':');
        prefix.extend_from_slice(if self.cyclic[id.index()] {
            b"cycle"
        } else {
            b"tree"
        });
        prefix
    }

    fn preview_object(
        &self,
        object: &GraphObject,
        active: &mut Vec<GraphId>,
        seen: &mut Vec<GraphId>,
        depth: u32,
    ) -> Vec<u8> {
        match object {
            GraphObject::List(items) => {
                self.preview_sequence(b'[', b']', items, active, seen, depth)
            }
            GraphObject::Tuple(items) => {
                self.preview_sequence(b'(', b')', items, active, seen, depth)
            }
            GraphObject::Set(items) => {
                if items.is_empty() {
                    return "∅".as_bytes().to_vec();
                }
                let mut children: Vec<Vec<u8>> = items
                    .iter()
                    .map(|item| {
                        let mut member_active = active.clone();
                        let mut member_seen = seen.clone();
                        self.sort_preview_in(item, &mut member_active, &mut member_seen, depth + 1)
                    })
                    .collect();
                children.sort();
                join_preview(b'{', b'}', &children)
            }
            GraphObject::Map(pairs) | GraphObject::OrderedMap(pairs) => {
                let ordered = matches!(object, GraphObject::OrderedMap(_));
                if ordered && pairs.is_empty() {
                    return b"[:]".to_vec();
                }
                let mut order: Vec<usize> = (0..pairs.len()).collect();
                if !ordered {
                    order.sort_by(|left, right| {
                        graph_key_bytes(&pairs[*left].0).cmp(&graph_key_bytes(&pairs[*right].0))
                    });
                }
                let mut children = Vec::with_capacity(pairs.len());
                for index in order {
                    let mut part = graph_key_text(&pairs[index].0).into_bytes();
                    part.push(b':');
                    part.extend(self.sort_preview_in(&pairs[index].1, active, seen, depth + 1));
                    children.push(part);
                }
                join_preview(
                    if ordered { b'[' } else { b'{' },
                    if ordered { b']' } else { b'}' },
                    &children,
                )
            }
            GraphObject::Node(node) => self.preview_node(node, active, seen, depth),
        }
    }

    fn preview_sequence(
        &self,
        open: u8,
        close: u8,
        items: &[GraphValue],
        active: &mut Vec<GraphId>,
        seen: &mut Vec<GraphId>,
        depth: u32,
    ) -> Vec<u8> {
        let mut children = Vec::with_capacity(items.len());
        for item in items {
            children.push(self.sort_preview_in(item, active, seen, depth + 1));
        }
        join_preview(open, close, &children)
    }

    fn preview_node(
        &self,
        node: &GraphNode,
        active: &mut Vec<GraphId>,
        seen: &mut Vec<GraphId>,
        depth: u32,
    ) -> Vec<u8> {
        let mut name = String::new();
        write_name(&mut name, &node.name);
        if node.style == NodeStyle::Unit {
            return name.into_bytes();
        }
        let (open, close) = match node.style {
            NodeStyle::Unit => unreachable!(),
            NodeStyle::Brace => (b'{', b'}'),
            NodeStyle::Tuple => (b'(', b')'),
            NodeStyle::List => (b'[', b']'),
        };
        let mut children = Vec::new();
        if node.style == NodeStyle::Brace {
            let mut order: Vec<usize> = (0..node.attributes.len()).collect();
            order.sort_by(|left, right| {
                node.attributes[*left]
                    .0
                    .as_bytes()
                    .cmp(node.attributes[*right].0.as_bytes())
            });
            for index in order {
                let mut key = String::new();
                write_name(&mut key, &node.attributes[index].0);
                let mut part = key.into_bytes();
                part.push(b':');
                part.extend(self.sort_preview_in(
                    &node.attributes[index].1,
                    active,
                    seen,
                    depth + 1,
                ));
                children.push(part);
            }
        }
        for child in &node.children {
            children.push(self.sort_preview_in(child, active, seen, depth + 1));
        }
        let mut out = name.into_bytes();
        out.extend(join_preview(open, close, &children));
        out
    }
}

fn graph_marker(object: &GraphObject) -> &'static [u8] {
    match object {
        GraphObject::Node(_) => b"node",
        GraphObject::OrderedMap(_) => b"ordered-map",
        GraphObject::Map(_) => b"map",
        GraphObject::Set(_) => b"set",
        GraphObject::List(_) => b"list",
        GraphObject::Tuple(_) => b"tuple",
    }
}

fn graph_backedge(kind: &[u8], marker: &[u8], ordinal: usize) -> Vec<u8> {
    let mut out = alloc::vec![0xff];
    out.extend_from_slice(kind);
    out.push(b':');
    out.extend_from_slice(marker);
    out.push(b':');
    out.extend_from_slice(ordinal.to_string().as_bytes());
    out
}

fn graph_scalar_bytes(value: &GraphValue) -> Vec<u8> {
    let GraphValue::Scalar(value) = value else {
        return Vec::new();
    };
    let mut text = String::new();
    write_value(&mut text, value, true);
    text.into_bytes()
}

fn graph_key_bytes(value: &GraphValue) -> Vec<u8> {
    match value {
        GraphValue::Scalar(Value::String(text)) => text.as_bytes().to_vec(),
        _ => graph_scalar_bytes(value),
    }
}

fn graph_key_text(value: &GraphValue) -> String {
    let mut text = String::new();
    if let GraphValue::Scalar(value) = value {
        write_map_key(&mut text, value, true);
    }
    text
}

fn join_preview(open: u8, close: u8, children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = alloc::vec![open];
    for (index, child) in children.iter().enumerate() {
        if index != 0 {
            out.push(b' ');
        }
        out.extend_from_slice(child);
    }
    out.push(close);
    out
}

fn write_value(out: &mut String, v: &Value, canonical: bool) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Int(s) => write_integer(out, s),
        Value::Float(f) => write_float(out, *f),
        Value::Decimal(d) => write_decimal(out, d, canonical),
        Value::String(s) => write_string(out, s),
        Value::Bytes(b) => write_bytes(out, b),
        Value::Temporal(t) => write_temporal(out, t),
        Value::List(items) => write_seq(out, '[', ']', items, canonical),
        Value::Tuple(items) => write_seq(out, '(', ')', items, canonical),
        Value::Set(items) => {
            if items.is_empty() {
                out.push('∅');
            } else if canonical {
                // `canonicalize_graph_stream` has already ordered the set by a
                // label-free graph preview. Preserve that order here: sorting
                // the rendered `&N` / `*N` spellings would make source anchor
                // placement observable and can move the first occurrence.
                out.push('{');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    write_value(out, item, true);
                }
                out.push('}');
            } else {
                write_seq(out, '{', '}', items, canonical)
            }
        }
        Value::Map(pairs) => write_map(out, '{', '}', pairs, canonical),
        Value::OrderedMap(pairs) => write_map(out, '[', ']', pairs, canonical),
        Value::Node(node) => write_node(out, node, canonical),
        Value::Link(link) => match link {
            Link::Cid(s) => {
                out.push_str("cid(");
                write_string(out, s);
                out.push(')');
            }
            Link::Uri(s) => {
                out.push_str("link(");
                write_string(out, s);
                out.push(')');
            }
        },
        Value::Anchor(a) => {
            out.push('&');
            out.push_str(&a.label);
            out.push(' ');
            write_value(out, &a.value, canonical);
        }
        Value::Ref(label) => {
            out.push('*');
            out.push_str(label);
        }
        // Checked entry points reject this before rendering.
        Value::Undefined => {}
    }
}

/// Write the semantic integer value in Core spelling. Compatibility parsing
/// can retain a leading plus or redundant zeroes in the text-backed integer
/// representation, but neither spelling is legal in Core output.
fn write_integer(out: &mut String, integer: &str) {
    let (negative, digits) = if let Some(rest) = integer.strip_prefix('-') {
        (true, rest)
    } else {
        (false, integer.strip_prefix('+').unwrap_or(integer))
    };
    if digits.is_empty() || !digits.as_bytes().iter().all(u8::is_ascii_digit) {
        out.push_str(integer);
        return;
    }
    let significant = digits.trim_start_matches('0');
    if significant.is_empty() {
        out.push('0');
    } else {
        if negative {
            out.push('-');
        }
        out.push_str(significant);
    }
}

/// Write a float in the AXON spelling: specials as `?` / `∞` / `-∞`, and finite
/// values in ECMAScript/JCS layout (§14.3) with a `.0` suffix when the layout
/// would otherwise read back as an integer.
fn write_float(out: &mut String, f: f64) {
    if f.is_nan() {
        out.push('?');
        return;
    }
    if f == f64::INFINITY {
        out.push('∞');
        return;
    }
    if f == f64::NEG_INFINITY {
        out.push_str("-∞");
        return;
    }
    if f == 0.0 && f.is_sign_negative() {
        out.push_str("-0.0");
        return;
    }
    let text = ecmascript_float(f);
    out.push_str(&text);
    if !text.contains('.') && !text.contains('e') {
        out.push_str(".0");
    }
}

/// Render a finite `f64` in ECMAScript `Number::toString` layout, which JCS
/// (RFC 8785) and AXON §14.3 both adopt: positional for `1e-6 <= |x| < 1e21`,
/// exponent form outside that window.
///
/// Rust's `Display` never emits exponent form, so `1e21` would canonicalize to
/// `1000000000000000000000.0` and disagree with the reference -- and therefore
/// produce a different CID for the same value. `{:e}` gives the same shortest
/// round-trip digits Python's `repr` does, which is the input this layout needs.
fn ecmascript_float(value: f64) -> String {
    let mut buffer = ryu_js::Buffer::new();
    buffer.format(value).to_string()
}

fn write_decimal(out: &mut String, d: &Decimal, canonical: bool) {
    match d {
        Decimal::Infinity => out.push_str("∞D"),
        Decimal::NegInfinity => out.push_str("-∞D"),
        Decimal::NaN => out.push_str("?D"),
        Decimal::Finite { mantissa, exponent } | Decimal::ScaledFinite { mantissa, exponent } => {
            let (mant, exp) = if canonical {
                normalize_decimal(mantissa, *exponent)
            } else {
                (mantissa.clone(), *exponent)
            };
            write_decimal_digits(out, &mant, exp);
            out.push('D');
        }
    }
}

fn normalize_decimal(mantissa: &str, exponent: i32) -> (String, i32) {
    let neg = mantissa.starts_with('-');
    let mut digits: String = mantissa.trim_start_matches('-').to_string();
    let mut exp = exponent;
    if digits.bytes().all(|b| b == b'0') {
        // Zero carries no scale in canonical form: `0D`, `0.0D` and `0.000D`
        // are one value and must have one spelling, or they take different
        // CIDs. The loop below cannot do this -- it stops while one digit
        // remains, so a mantissa of "0" with a negative exponent kept its
        // scale and rendered as `0.0D`.
        digits = alloc::string::String::from("0");
        exp = 0;
    } else {
        while digits.len() > 1 && digits.ends_with('0') && exp < 0 {
            digits.pop();
            exp += 1;
        }
    }
    // Zero carries no sign. AXON 5.2 states there is no signed integer zero, a
    // Decimal's mantissa is an integer, and Binary AXON encodes it as a CBOR
    // integer -- which has no -0 either, so `-0D` and `0D` share the bytes
    // `c4820000`. Preserving the sign in the text form would make the normative
    // text<->binary round-trip (Binary AXON 7) lossy for this one value.
    if neg && digits != "0" {
        (alloc::format!("-{}", digits), exp)
    } else {
        (digits, exp)
    }
}

/// Largest decimal this writer will expand into positional text.
///
/// A decimal is `mantissa * 10^exponent`, and positional notation writes every
/// place. The exponent is an `i32`, so `1E2147483647D` — fourteen bytes of
/// input — asks for two gibibytes of output, and `1E-2147483648D` asks for the
/// same after the point. Neither is a document anyone wrote on purpose, and
/// neither should be attempted.
///
/// A million digits is far beyond any real decimal — the parser already refuses
/// a numeric literal longer than two thousand characters — while being small
/// enough that reaching the bound costs a megabyte rather than the machine.
const MAX_DECIMAL_RENDER_DIGITS: usize = 1_000_000;

/// Length of the positional text a decimal expands to, or `None` when that
/// length is not representable.
///
/// Separate from the writer so the check runs in `validate_write`, before any
/// allocation, which is where every other writer limit is enforced.
fn decimal_render_len(mantissa: &str, exponent: i32) -> Option<usize> {
    let digits = mantissa.trim_start_matches('-').len();
    if exponent >= 0 {
        // `mantissa` followed by `exponent` zeroes.
        digits.checked_add(exponent.unsigned_abs() as usize)
    } else {
        // Either `ddd.ddd` or `0.000ddd`; the scale bounds both, and two more
        // characters cover the leading `0` and the point.
        let scale = exponent.unsigned_abs() as usize;
        digits.max(scale).checked_add(2)
    }
}

fn write_decimal_digits(out: &mut String, mantissa: &str, exponent: i32) {
    if exponent >= 0 {
        out.push_str(mantissa);
        for _ in 0..exponent {
            out.push('0');
        }
        return;
    }
    let neg = mantissa.starts_with('-');
    let digits = mantissa.trim_start_matches('-');
    // `unsigned_abs` cannot overflow on `i32::MIN`, unlike negation. This is the
    // same fix `write_offset` carries as audit M7; this site was missed, and
    // negating `i32::MIN` here panicked in debug and wrapped in release, leaving
    // the padding loop below asking for about eighteen quintillion characters.
    //
    // `validate_write` now refuses such a value before the writer is reached,
    // but the writer must not be one arithmetic slip away from that again.
    let scale = exponent.unsigned_abs() as usize;
    if neg {
        out.push('-');
    }
    if digits.len() > scale {
        let point = digits.len() - scale;
        out.push_str(&digits[..point]);
        out.push('.');
        out.push_str(&digits[point..]);
    } else {
        out.push_str("0.");
        for _ in 0..(scale - digits.len()) {
            out.push('0');
        }
        out.push_str(digits);
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_bytes(out: &mut String, b: &[u8]) {
    out.push('|');
    base64_encode(out, b);
    out.push('|');
}

fn write_temporal(out: &mut String, t: &Temporal) {
    out.push('^');
    match t {
        Temporal::Date(d) => {
            let _ = write!(out, "{:04}-{:02}-{:02}", d.year, d.month, d.day);
        }
        Temporal::Time(tm) => write_time(out, tm),
        Temporal::DateTime(dt) => {
            let _ = write!(
                out,
                "{:04}-{:02}-{:02}T",
                dt.date.year, dt.date.month, dt.date.day
            );
            write_time(out, &dt.time);
        }
    }
}

fn write_time(out: &mut String, t: &Time) {
    let _ = write!(out, "{:02}:{:02}:{:02}", t.hour, t.minute, t.second);
    if let Some(width) = t.fraction_digits {
        let fraction = alloc::format!("{:09}", t.nanosecond);
        out.push('.');
        out.push_str(&fraction[..width as usize]);
    } else if t.nanosecond != 0 {
        let mut frac = alloc::format!("{:09}", t.nanosecond);
        while frac.ends_with('0') {
            frac.pop();
        }
        out.push('.');
        out.push_str(&frac);
    }
    match &t.zone {
        Zone::Local => {}
        Zone::Zulu => out.push('Z'),
        Zone::Offset(m) => write_offset(out, *m),
        Zone::Named { offset, name, zulu } => {
            if *zulu {
                out.push('Z');
            } else {
                write_offset(out, *offset);
            }
            out.push('[');
            out.push_str(name);
            out.push(']');
        }
    }
}

fn write_offset(out: &mut String, minutes: i32) {
    let sign = if minutes < 0 { '-' } else { '+' };
    // `unsigned_abs` cannot panic on i32::MIN, unlike `abs` (audit M7).
    let m = minutes.unsigned_abs();
    let _ = write!(out, "{}{:02}:{:02}", sign, m / 60, m % 60);
}

fn write_seq(out: &mut String, open: char, close: char, items: &[Value], canonical: bool) {
    out.push(open);
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        write_value(out, item, canonical);
    }
    out.push(close);
}

fn write_map(out: &mut String, open: char, close: char, pairs: &[(Value, Value)], canonical: bool) {
    if pairs.is_empty() && open == '[' {
        out.push_str("[:]");
        return;
    }
    // For canonical unordered maps, sort by the encoded key text.
    let mut order: Vec<usize> = (0..pairs.len()).collect();
    if canonical && open == '{' {
        order.sort_by(|&a, &b| {
            let ka = match &pairs[a].0 {
                Value::String(value) => value.as_bytes(),
                _ => &[],
            };
            let kb = match &pairs[b].0 {
                Value::String(value) => value.as_bytes(),
                _ => &[],
            };
            ka.cmp(kb)
        });
    }
    out.push(open);
    for (i, &idx) in order.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        write_map_key(out, &pairs[idx].0, canonical);
        out.push(':');
        write_value(out, &pairs[idx].1, canonical);
    }
    out.push(close);
}

fn write_map_key(out: &mut String, key: &Value, canonical: bool) {
    // Bare-name keys stay bare; everything else uses its value form.
    if let Value::String(s) = key
        && is_bare_name(s)
    {
        out.push_str(s);
        return;
    }
    write_value(out, key, canonical);
}

fn is_bare_name(s: &str) -> bool {
    // A reserved bareword (`null`/`true`/`false`) as a key or name must be
    // quoted, or it would read back as the constant (§2.3).
    if s == "null" || s == "true" || s == "false" {
        return false;
    }
    is_bare_name_part(s)
}

/// True when `s` is a single valid bare name segment (no `/`), ignoring the
/// reserved-bareword rule that only applies to whole names.
fn is_bare_name_part(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c == '_' || unicode_ident::is_xid_start(c) => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || unicode_ident::is_xid_continue(c))
}

/// Write an AXON name (a node name or a node attribute key): each `/`-separated
/// segment is emitted bare when it is a valid bare-name segment, otherwise
/// single-quoted (§9.2/§9.3). A lone reserved bareword is quoted.
fn write_name(out: &mut String, name: &str) {
    let parts: Vec<&str> = name.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            out.push('/');
        }
        let reserved_single =
            parts.len() == 1 && (*part == "null" || *part == "true" || *part == "false");
        if is_bare_name_part(part) && !reserved_single {
            out.push_str(part);
        } else {
            write_quoted_name(out, part);
        }
    }
}

/// Write a `'single-quoted name'` with the reference's escape table.
fn write_quoted_name(out: &mut String, value: &str) {
    out.push('\'');
    for ch in value.chars() {
        match ch {
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('\'');
}

fn write_node(out: &mut String, node: &crate::value::Node, canonical: bool) {
    write_name(out, &node.name);
    match node.style {
        NodeStyle::Unit => {}
        NodeStyle::Brace => {
            out.push('{');
            let mut first = true;
            let mut attr_order: Vec<usize> = (0..node.attributes.len()).collect();
            if canonical {
                attr_order.sort_by(|&a, &b| {
                    node.attributes[a]
                        .0
                        .as_bytes()
                        .cmp(node.attributes[b].0.as_bytes())
                });
            }
            for &i in &attr_order {
                if !first {
                    out.push(' ');
                }
                first = false;
                write_name(out, &node.attributes[i].0);
                out.push(':');
                write_value(out, &node.attributes[i].1, canonical);
            }
            for child in &node.children {
                if !first {
                    out.push(' ');
                }
                first = false;
                write_value(out, child, canonical);
            }
            out.push('}');
        }
        NodeStyle::Tuple => write_seq_children(out, '(', ')', &node.children, canonical),
        NodeStyle::List => write_seq_children(out, '[', ']', &node.children, canonical),
    }
}

fn write_seq_children(out: &mut String, open: char, close: char, items: &[Value], canonical: bool) {
    out.push(open);
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        write_value(out, item, canonical);
    }
    out.push(close);
}

fn base64_encode(out: &mut String, data: &[u8]) {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        out.push(T[(b[0] >> 2) as usize] as char);
        out.push(T[(((b[0] & 0x03) << 4) | (b[1] >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(T[(((b[1] & 0x0f) << 2) | (b[2] >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(T[(b[2] & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Anchor;

    #[test]
    fn canonical_stream_retains_cross_value_graph_identity() {
        let values = [
            Value::Anchor(Box::new(Anchor {
                label: "source".to_string(),
                value: Value::List(Vec::new()),
            })),
            Value::Ref("source".to_string()),
        ];
        assert_eq!(to_string_stream(&values).unwrap(), "&source []\n*source");
        assert_eq!(to_string_stream_canonical(&values).unwrap(), "&1 []\n*1");
    }

    #[test]
    fn compatibility_integer_text_is_emitted_in_core_spelling() {
        assert_eq!(to_string(&Value::Int("007".to_string())).unwrap(), "7");
        assert_eq!(to_string(&Value::Int("-0007".to_string())).unwrap(), "-7");
        assert_eq!(to_string(&Value::Int("+42".to_string())).unwrap(), "42");
        assert_eq!(to_string(&Value::Int("-0".to_string())).unwrap(), "0");
    }

    #[test]
    fn canonical_graph_set_order_is_topology_invariant() {
        let options = crate::Options {
            graph: true,
            ..crate::Options::default()
        };
        let left = crate::from_str_value_with("{[&x [1]] [*x 0]}", options).unwrap();
        let right = crate::from_str_value_with("{[&x [1] 0] [*x]}", options).unwrap();
        let expected = "{[&1 [1] 0] [*1]}";
        assert_eq!(to_string_canonical(&left).unwrap(), expected);
        assert_eq!(to_string_canonical(&right).unwrap(), expected);
    }

    #[test]
    fn canonical_labels_follow_sorted_mapping_and_node_attributes() {
        let options = crate::Options {
            graph: true,
            ..crate::Options::default()
        };
        let map = crate::from_str_value_with(
            "{z:&right [2] a:&left [1] again:*left zagain:*right}",
            options,
        )
        .unwrap();
        assert_eq!(
            to_string_canonical(&map).unwrap(),
            "{a:&1 [1] again:*1 z:&2 [2] zagain:*2}"
        );

        let node = crate::from_str_value_with(
            "N{z:&right [2] a:&left [1] again:*left zagain:*right}",
            options,
        )
        .unwrap();
        assert_eq!(
            to_string_canonical(&node).unwrap(),
            "N{a:&1 [1] again:*1 z:&2 [2] zagain:*2}"
        );
    }

    #[test]
    fn canonical_writer_rejects_unresolvable_graph_markers() {
        let unknown = to_string_canonical(&Value::Ref("missing".into())).unwrap_err();
        assert_eq!(unknown.category(), Category::UnknownReference);

        let reference_only_cycle = Value::Anchor(Box::new(Anchor {
            label: "a".into(),
            value: Value::Ref("a".into()),
        }));
        let error = to_string_canonical(&reference_only_cycle).unwrap_err();
        assert_eq!(error.category(), Category::UnexpectedToken);
    }

    #[test]
    fn compact_graph_option_matches_python_identity_policy() {
        let parse_options = crate::Options {
            graph: true,
            ..crate::Options::default()
        };
        let shared = crate::from_str_stream_with("&a []\n*a", parse_options).unwrap();
        assert_eq!(
            to_string_stream_with(&shared, WriteOptions { graph: false }).unwrap(),
            "[]\n[]"
        );
        assert_eq!(
            to_string_stream_with(&shared, WriteOptions { graph: true }).unwrap(),
            "&1 []\n*1"
        );

        let nested = crate::from_str_value_with("[&item point{x:1} *item]", parse_options).unwrap();
        assert_eq!(
            to_string_with(&nested, WriteOptions { graph: false }).unwrap(),
            "[point{x:1} point{x:1}]"
        );
        assert_eq!(
            to_string_with(&nested, WriteOptions { graph: true }).unwrap(),
            "[&1 point{x:1} *1]"
        );
    }

    #[test]
    fn compact_tree_mode_rejects_cycles_and_graph_mode_emits_them() {
        let cycle = crate::from_str_value_with(
            "&a [*a]",
            crate::Options {
                graph: true,
                ..crate::Options::default()
            },
        )
        .unwrap();
        let error = to_string_with(&cycle, WriteOptions { graph: false }).unwrap_err();
        assert_eq!(error.category(), Category::UnsupportedProfileFeature);
        assert_eq!(error.message, "cyclic values require graph output");
        assert_eq!(
            to_string_with(&cycle, WriteOptions { graph: true }).unwrap(),
            "&1 [*1]"
        );
    }
}
