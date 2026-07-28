//! Identity-preserving graph view of AXON values.
//!
//! [`Value`](crate::Value) intentionally remains a Serde-friendly tree. This
//! module resolves its `&label` / `*label` markers into a small arena so graph
//! identity is observable without `Rc` cycles, leaks, or recursive `Debug` and
//! equality implementations. Object references are stable [`GraphId`] values;
//! consequently a self-cycle is just an arena edge back to the same id.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::error::{Category, Error, Result};
use crate::value::{NodeStyle, Value};

/// Recursion bound for lowering a `Value` into the arena, matching the
/// section-16 default the parser and writer enforce. Parsed values are already
/// within it; the guard covers a programmatically built value handed straight
/// to [`GraphDocument::from_value`], which would otherwise recurse until the
/// call stack was exhausted.
const DEFAULT_MAX_DEPTH: u32 = 128;

/// Stable index of a composite value in a [`GraphDocument`] arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GraphId(usize);

impl GraphId {
    /// Return this id's zero-based arena index.
    pub const fn index(self) -> usize {
        self.0
    }
}

/// A value in an identity-preserving graph.
///
/// Scalars are owned tree values. Composite values are represented by an
/// arena id, so cloning a `GraphValue::Object` retains identity rather than
/// duplicating the object.
#[derive(Clone, Debug, PartialEq)]
pub enum GraphValue {
    /// A scalar AXON value.
    Scalar(Value),
    /// A list, tuple, mapping, set, or node stored in the document arena.
    Object(GraphId),
}

impl GraphValue {
    /// Return the arena id when this value has observable object identity.
    pub const fn object_id(&self) -> Option<GraphId> {
        match self {
            Self::Object(id) => Some(*id),
            Self::Scalar(_) => None,
        }
    }

    /// Return the scalar value, or `None` for an arena object.
    pub const fn scalar(&self) -> Option<&Value> {
        match self {
            Self::Scalar(value) => Some(value),
            Self::Object(_) => None,
        }
    }
}

/// Identity-bearing composite value stored in a [`GraphDocument`].
#[derive(Clone, Debug, PartialEq)]
pub enum GraphObject {
    /// An ordered list.
    List(Vec<GraphValue>),
    /// An AXON tuple.
    Tuple(Vec<GraphValue>),
    /// An unordered mapping.
    Map(Vec<(GraphValue, GraphValue)>),
    /// An insertion-ordered mapping.
    OrderedMap(Vec<(GraphValue, GraphValue)>),
    /// An unordered set.
    Set(Vec<GraphValue>),
    /// A tagged AXON node.
    Node(GraphNode),
}

impl GraphObject {
    /// Return the stable kind name used by diagnostics and tooling.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::List(_) => "list",
            Self::Tuple(_) => "tuple",
            Self::Map(_) => "map",
            Self::OrderedMap(_) => "ordered-map",
            Self::Set(_) => "set",
            Self::Node(_) => "node",
        }
    }
}

/// Identity-preserving counterpart of [`crate::Node`].
#[derive(Clone, Debug, PartialEq)]
pub struct GraphNode {
    /// Node name, including any namespace segments.
    pub name: String,
    /// Body form.
    pub style: NodeStyle,
    /// Attribute values in source order.
    pub attributes: Vec<(String, GraphValue)>,
    /// Child values in source order.
    pub children: Vec<GraphValue>,
}

/// A resolved AXON graph document.
///
/// Roots and edges use [`GraphId`] for composites. Two occurrences have shared
/// identity exactly when their ids compare equal. Cycles require no special
/// ownership mechanism: an object's child may contain its own id.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphDocument {
    roots: Vec<GraphValue>,
    objects: Vec<GraphObject>,
}

impl GraphDocument {
    /// Resolve one structural [`Value`] into an identity-preserving graph.
    pub fn from_value(value: &Value) -> Result<Self> {
        Self::from_values(core::slice::from_ref(value))
    }

    /// Resolve a complete AXON stream into one graph arena.
    ///
    /// Resolving the complete stream is important: a later root can refer to
    /// an object anchored by an earlier root.
    pub fn from_values(values: &[Value]) -> Result<Self> {
        let mut builder = Builder::new();
        let mut roots = Vec::with_capacity(values.len());
        for value in values {
            roots.push(builder.lower(value, 0)?);
        }
        let mut objects = Vec::with_capacity(builder.objects.len());
        for object in builder.objects {
            objects.push(object.ok_or_else(|| {
                graph_error(
                    Category::UnexpectedToken,
                    "anchor body did not materialize as a graph object",
                )
            })?);
        }
        Ok(Self { roots, objects })
    }

    /// Ordered top-level values in the document stream.
    pub fn roots(&self) -> &[GraphValue] {
        &self.roots
    }

    /// Resolve an arena id to its object.
    pub fn object(&self, id: GraphId) -> Option<&GraphObject> {
        self.objects.get(id.0)
    }

    /// Number of identity-bearing objects in the arena.
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Test whether two graph values denote the same composite object.
    ///
    /// Scalars return `false`, matching the Graph profile's rule that only
    /// composite kinds carry observable shared identity.
    pub const fn same_identity(left: &GraphValue, right: &GraphValue) -> bool {
        match (left, right) {
            (GraphValue::Object(left), GraphValue::Object(right)) => left.0 == right.0,
            _ => false,
        }
    }
}

/// Resolve one structural AXON value into an identity-preserving graph.
pub fn resolve_graph(value: &Value) -> Result<GraphDocument> {
    GraphDocument::from_value(value)
}

/// Resolve a complete structural AXON stream into one graph arena.
pub fn resolve_graph_stream(values: &[Value]) -> Result<GraphDocument> {
    GraphDocument::from_values(values)
}

/// Parse an AXON document with the Graph profile and return resolved identity.
pub fn from_str_graph(source: &str) -> Result<GraphDocument> {
    from_str_graph_with(source, crate::Options::default())
}

/// Parse an AXON document with caller options and return resolved identity.
///
/// The Graph profile is enabled by this entry point. Other options, including
/// resource limits and document-header policy, are retained.
pub fn from_str_graph_with(source: &str, mut options: crate::Options) -> Result<GraphDocument> {
    options.graph = true;
    let values = crate::from_str_stream_with(source, options)?;
    GraphDocument::from_values(&values)
}

#[derive(Clone, Debug)]
enum Binding {
    Pending,
    Value(GraphValue),
}

struct Builder {
    objects: Vec<Option<GraphObject>>,
    bindings: Vec<(String, Binding)>,
}

impl Builder {
    fn new() -> Self {
        Self {
            objects: Vec::new(),
            bindings: Vec::new(),
        }
    }

    fn lower(&mut self, value: &Value, depth: u32) -> Result<GraphValue> {
        if depth > DEFAULT_MAX_DEPTH {
            return Err(graph_error(
                Category::ResourceLimitExceeded,
                "maximum graph nesting depth exceeded",
            ));
        }
        match value {
            Value::Anchor(anchor) => self.lower_anchor(&anchor.label, &anchor.value, depth),
            Value::Ref(label) => self.reference(label),
            Value::List(_)
            | Value::Tuple(_)
            | Value::Map(_)
            | Value::OrderedMap(_)
            | Value::Set(_)
            | Value::Node(_) => self.lower_fresh_object(value, depth),
            other => Ok(GraphValue::Scalar(other.clone())),
        }
    }

    fn lower_anchor(&mut self, label: &str, body: &Value, depth: u32) -> Result<GraphValue> {
        if self.bindings.iter().any(|(known, _)| known == label) {
            return Err(graph_error(
                Category::DuplicateAnchor,
                &alloc::format!("duplicate anchor {label:?}"),
            ));
        }

        if underlying_object(body).is_some() {
            let id = self.reserve_object();
            let target = GraphValue::Object(id);
            self.bindings
                .push((label.to_string(), Binding::Value(target.clone())));
            self.lower_into_reserved(body, id, depth)?;
            Ok(target)
        } else {
            // Register pending before walking so `&a *a` is distinguished from
            // an unknown label and receives Python's reference-only-cycle error.
            let slot = self.bindings.len();
            self.bindings.push((label.to_string(), Binding::Pending));
            let target = self.lower(body, depth.saturating_add(1))?;
            self.bindings[slot].1 = Binding::Value(target.clone());
            Ok(target)
        }
    }

    fn lower_into_reserved(&mut self, value: &Value, id: GraphId, depth: u32) -> Result<()> {
        if depth > DEFAULT_MAX_DEPTH {
            return Err(graph_error(
                Category::ResourceLimitExceeded,
                "maximum graph nesting depth exceeded",
            ));
        }
        match value {
            Value::Anchor(anchor) => {
                if self
                    .bindings
                    .iter()
                    .any(|(known, _)| known == &anchor.label)
                {
                    return Err(graph_error(
                        Category::DuplicateAnchor,
                        &alloc::format!("duplicate anchor {:?}", anchor.label),
                    ));
                }
                self.bindings
                    .push((anchor.label.clone(), Binding::Value(GraphValue::Object(id))));
                self.lower_into_reserved(&anchor.value, id, depth.saturating_add(1))
            }
            Value::List(_)
            | Value::Tuple(_)
            | Value::Map(_)
            | Value::OrderedMap(_)
            | Value::Set(_)
            | Value::Node(_) => {
                let object = self.lower_object_body(value, depth)?;
                self.objects[id.0] = Some(object);
                Ok(())
            }
            _ => Err(graph_error(
                Category::UnexpectedToken,
                "anchor body did not materialize as a graph object",
            )),
        }
    }

    fn lower_fresh_object(&mut self, value: &Value, depth: u32) -> Result<GraphValue> {
        let id = self.reserve_object();
        let object = self.lower_object_body(value, depth)?;
        self.objects[id.0] = Some(object);
        Ok(GraphValue::Object(id))
    }

    fn reserve_object(&mut self) -> GraphId {
        let id = GraphId(self.objects.len());
        self.objects.push(None);
        id
    }

    fn lower_object_body(&mut self, value: &Value, depth: u32) -> Result<GraphObject> {
        let nested = depth.saturating_add(1);
        Ok(match value {
            Value::List(items) => GraphObject::List(self.lower_each(items, nested)?),
            Value::Tuple(items) => GraphObject::Tuple(self.lower_each(items, nested)?),
            Value::Set(items) => GraphObject::Set(self.lower_each(items, nested)?),
            Value::Map(pairs) => GraphObject::Map(self.lower_pairs(pairs, nested)?),
            Value::OrderedMap(pairs) => GraphObject::OrderedMap(self.lower_pairs(pairs, nested)?),
            Value::Node(node) => {
                let mut attributes = Vec::with_capacity(node.attributes.len());
                for (key, value) in &node.attributes {
                    attributes.push((key.clone(), self.lower(value, nested)?));
                }
                GraphObject::Node(GraphNode {
                    name: node.name.clone(),
                    style: node.style,
                    attributes,
                    children: self.lower_each(&node.children, nested)?,
                })
            }
            _ => {
                return Err(graph_error(
                    Category::UnexpectedToken,
                    "expected a composite graph object",
                ));
            }
        })
    }

    fn lower_each(&mut self, values: &[Value], depth: u32) -> Result<Vec<GraphValue>> {
        let mut out = Vec::with_capacity(values.len());
        for value in values {
            out.push(self.lower(value, depth)?);
        }
        Ok(out)
    }

    fn lower_pairs(
        &mut self,
        pairs: &[(Value, Value)],
        depth: u32,
    ) -> Result<Vec<(GraphValue, GraphValue)>> {
        let mut out = Vec::with_capacity(pairs.len());
        for (key, value) in pairs {
            out.push((self.lower(key, depth)?, self.lower(value, depth)?));
        }
        Ok(out)
    }

    fn reference(&self, label: &str) -> Result<GraphValue> {
        match self
            .bindings
            .iter()
            .find(|(known, _)| known == label)
            .map(|(_, binding)| binding)
        {
            Some(Binding::Value(value)) => Ok(value.clone()),
            Some(Binding::Pending) => Err(graph_error(
                Category::UnexpectedToken,
                "reference-only cycle has no materialized value",
            )),
            None => Err(graph_error(
                Category::UnknownReference,
                &alloc::format!("reference must follow its anchor {label:?}"),
            )),
        }
    }
}

fn underlying_object(mut value: &Value) -> Option<&Value> {
    while let Value::Anchor(anchor) = value {
        value = &anchor.value;
    }
    match value {
        Value::List(_)
        | Value::Tuple(_)
        | Value::Map(_)
        | Value::OrderedMap(_)
        | Value::Set(_)
        | Value::Node(_) => Some(value),
        _ => None,
    }
}

fn graph_error(category: Category, message: &str) -> Error {
    Error::new(category, message, 0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Anchor;
    use alloc::boxed::Box;

    fn anchor(label: &str, value: Value) -> Value {
        Value::Anchor(Box::new(Anchor {
            label: label.to_string(),
            value,
        }))
    }

    #[test]
    fn cross_root_reference_retains_identity() {
        let values = [
            anchor("a", Value::List(alloc::vec![Value::Int("1".into())])),
            Value::Ref("a".into()),
        ];
        let graph = GraphDocument::from_values(&values).unwrap();
        assert!(GraphDocument::same_identity(
            &graph.roots()[0],
            &graph.roots()[1]
        ));
    }

    #[test]
    fn self_cycle_is_an_edge_to_the_same_arena_id() {
        let value = anchor("a", Value::List(alloc::vec![Value::Ref("a".into())]));
        let graph = GraphDocument::from_value(&value).unwrap();
        let root = graph.roots()[0].object_id().unwrap();
        let GraphObject::List(items) = graph.object(root).unwrap() else {
            panic!("expected list")
        };
        assert_eq!(items[0].object_id(), Some(root));
    }

    #[test]
    fn graph_parse_entry_point_resolves_stream_identity() {
        let graph = from_str_graph("&a [1]\n*a").unwrap();
        assert!(GraphDocument::same_identity(
            &graph.roots()[0],
            &graph.roots()[1]
        ));
    }

    #[test]
    fn distinct_empty_objects_do_not_gain_identity() {
        let value = Value::Tuple(alloc::vec![
            Value::Tuple(Vec::new()),
            Value::Tuple(Vec::new())
        ]);
        let graph = GraphDocument::from_value(&value).unwrap();
        let root = graph.roots()[0].object_id().unwrap();
        let GraphObject::Tuple(items) = graph.object(root).unwrap() else {
            panic!("expected tuple")
        };
        assert!(!GraphDocument::same_identity(&items[0], &items[1]));
    }
}
