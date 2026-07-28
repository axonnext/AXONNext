//! AXON Schema 2026 normalisation, registry, and validation.
//!
//! Schemas are ordinary AXON maps or `schema{...}` / `field{...}` nodes. The
//! validator operates only on semantic [`Value`]s, performs no I/O, and keeps
//! every diagnostic path structured.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt;

use crate::regex::safe_regex_search;
use crate::value::{Decimal, Node, NodeStyle, Temporal, Value};

/// The only schema dialect identifier supported by this implementation.
pub const SCHEMA_DIALECT_2026: &str = "urn:axonnext:schema:2026";

const DEFAULT_MAX_MODULES: usize = 1024;
const DEFAULT_MAX_DEPTH: usize = 128;
const DEFAULT_MAX_NODES: usize = 1_048_576;
const DEFAULT_MAX_ISSUES: usize = 256;

/// Classification for an error while loading, normalising, or governing a
/// schema registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchemaErrorKind {
    /// The schema or registry operation is structurally invalid.
    InvalidSchema,
    /// A configured schema resource limit was exceeded.
    ResourceLimit,
    /// A mutation was attempted after the registry was frozen.
    RegistryFrozen,
}

/// Error returned by schema normalisation and registry operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaError {
    /// Stable error classification.
    pub kind: SchemaErrorKind,
    /// Human-readable detail matching the reference implementation.
    pub message: String,
}

impl SchemaError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            kind: SchemaErrorKind::InvalidSchema,
            message: message.into(),
        }
    }

    fn resource(message: impl Into<String>) -> Self {
        Self {
            kind: SchemaErrorKind::ResourceLimit,
            message: message.into(),
        }
    }

    fn frozen() -> Self {
        Self {
            kind: SchemaErrorKind::RegistryFrozen,
            message: "schema registry is frozen".to_string(),
        }
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl core::error::Error for SchemaError {}

/// One component in a value or schema traversal path.
#[derive(Clone, Debug, PartialEq)]
pub enum PathSegment {
    /// A collection offset. Python represents this as an `int` path member.
    Index(usize),
    /// A mapping key. The complete value is retained because AXON map keys are
    /// not restricted to strings.
    Key(Value),
}

impl From<usize> for PathSegment {
    fn from(value: usize) -> Self {
        Self::Index(value)
    }
}

impl From<&str> for PathSegment {
    fn from(value: &str) -> Self {
        Self::Key(Value::String(value.to_string()))
    }
}

impl From<String> for PathSegment {
    fn from(value: String) -> Self {
        Self::Key(Value::String(value))
    }
}

impl From<Value> for PathSegment {
    fn from(value: Value) -> Self {
        Self::Key(value)
    }
}

/// A distinct schema validation failure.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidationIssue {
    /// Stable kebab-case issue code.
    pub code: String,
    /// Structured path through the validated value.
    pub path: Vec<PathSegment>,
    /// Human-readable explanation.
    pub message: String,
    /// Structured path through the normalised schema.
    pub schema_path: Vec<PathSegment>,
}

impl ValidationIssue {
    /// Construct an issue with an empty schema path.
    pub fn new(
        code: impl Into<String>,
        path: Vec<PathSegment>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            path,
            message: message.into(),
            schema_path: Vec::new(),
        }
    }

    /// Attach the structured schema path that produced this issue.
    pub fn with_schema_path(mut self, schema_path: Vec<PathSegment>) -> Self {
        self.schema_path = schema_path;
        self
    }
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at $", self.code)?;
        for item in &self.path {
            write!(f, "[{}]", path_repr(item))?;
        }
        write!(f, ": {}", self.message)
    }
}

/// Error returned by [`assert_valid2026`] when validation fails.
#[derive(Clone, Debug, PartialEq)]
pub struct SchemaValidationError {
    /// Ordered, deduplicated validation issues.
    pub issues: Vec<ValidationIssue>,
}

impl fmt::Display for SchemaValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, issue) in self.issues.iter().enumerate() {
            if index != 0 {
                f.write_str("; ")?;
            }
            write!(f, "{issue}")?;
        }
        Ok(())
    }
}

impl core::error::Error for SchemaValidationError {}

/// Aggregate result of a validation run.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidationReport {
    /// Ordered, deduplicated validation issues.
    pub issues: Vec<ValidationIssue>,
    /// Counts by issue code, sorted by code.
    pub counts: Vec<(String, usize)>,
    /// Whether the issue limit replaced further diagnostics with the limit
    /// sentinel.
    pub truncated: bool,
}

impl ValidationReport {
    /// Whether validation produced no issues.
    pub fn valid(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Resource controls for a validation run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidationOptions {
    /// Maximum schema/validation nesting depth.
    pub max_depth: usize,
    /// Maximum number of returned issues, including the truncation sentinel.
    pub max_issues: usize,
    /// Treat decimals in the value as scale-significant for `const`/`enum`.
    ///
    /// Schema documents use Core semantics, so a scale-significant value-side
    /// decimal is intentionally distinct from an otherwise equal schema-side
    /// Core decimal. Numeric constraints remain value-based comparisons.
    pub scale_significant: bool,
}

impl Default for ValidationOptions {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
            max_issues: DEFAULT_MAX_ISSUES,
            scale_significant: false,
        }
    }
}

/// Governed local-only registry for AXON Schema modules.
#[derive(Clone, Debug)]
pub struct SchemaRegistry2026 {
    modules: Vec<(String, Value)>,
    max_modules: usize,
    max_depth: usize,
    max_nodes: usize,
    frozen: bool,
}

impl Default for SchemaRegistry2026 {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemaRegistry2026 {
    /// Construct an empty registry with the reference limits.
    pub fn new() -> Self {
        Self {
            modules: Vec::new(),
            max_modules: DEFAULT_MAX_MODULES,
            max_depth: DEFAULT_MAX_DEPTH,
            max_nodes: DEFAULT_MAX_NODES,
            frozen: false,
        }
    }

    /// Construct an empty registry with explicit positive limits.
    pub fn with_limits(
        max_modules: usize,
        max_depth: usize,
        max_nodes: usize,
    ) -> Result<Self, SchemaError> {
        if max_modules < 1 {
            return Err(SchemaError::invalid(
                "max_modules must be a positive integer",
            ));
        }
        if max_depth < 1 {
            return Err(SchemaError::invalid("max_depth must be a positive integer"));
        }
        if max_nodes < 1 {
            return Err(SchemaError::invalid("max_nodes must be a positive integer"));
        }
        Ok(Self {
            modules: Vec::new(),
            max_modules,
            max_depth,
            max_nodes,
            frozen: false,
        })
    }

    /// Validate an absolute ASCII module URI without a fragment.
    pub fn validate_identifier(identifier: &str) -> Result<&str, SchemaError> {
        if identifier.is_empty() {
            return Err(SchemaError::invalid(
                "schema module identifier must be a non-empty string",
            ));
        }
        if !identifier.is_ascii() {
            return Err(SchemaError::invalid(
                "schema module identifier must be ASCII",
            ));
        }
        if identifier.bytes().any(|byte| byte <= b' ' || byte == 0x7f) {
            return Err(SchemaError::invalid(
                "schema module identifier contains whitespace or controls",
            ));
        }
        if identifier
            .split_once('#')
            .is_some_and(|(_, fragment)| !fragment.is_empty())
        {
            return Err(SchemaError::invalid(
                "schema module identifier must be an absolute URI without a fragment",
            ));
        }
        let Some(colon) = identifier.find(':') else {
            return Err(SchemaError::invalid(
                "schema module identifier must be an absolute URI without a fragment",
            ));
        };
        let scheme = &identifier[..colon];
        let mut chars = scheme.chars();
        if !chars.next().is_some_and(|ch| ch.is_ascii_alphabetic())
            || !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
        {
            return Err(SchemaError::invalid(
                "schema module identifier must be an absolute URI without a fragment",
            ));
        }
        if identifier.contains('[') || identifier.contains(']') {
            let authority = identifier
                .get(colon + 1..)
                .unwrap_or_default()
                .strip_prefix("//")
                .and_then(|rest| rest.split(['/', '?']).next());
            if authority.is_some_and(|part| {
                (part.contains('[') || part.contains(']')) && !valid_bracketed_authority(part)
            }) {
                return Err(SchemaError::invalid("invalid schema module URI"));
            }
        }
        Ok(identifier)
    }

    /// Register and normalise a module. Existing identifiers are rejected.
    pub fn register(&mut self, identifier: &str, schema: &Value) -> Result<Value, SchemaError> {
        self.register_with(identifier, schema, false)
    }

    /// Register and normalise a module, optionally replacing an existing one.
    pub fn register_with(
        &mut self,
        identifier: &str,
        schema: &Value,
        replace: bool,
    ) -> Result<Value, SchemaError> {
        if self.frozen {
            return Err(SchemaError::frozen());
        }
        Self::validate_identifier(identifier)?;
        let existing = self.modules.iter().position(|(name, _)| name == identifier);
        if existing.is_some() && !replace {
            return Err(SchemaError::invalid(format!(
                "duplicate schema module identifier {}",
                python_string_repr(identifier)
            )));
        }
        if existing.is_none() && self.modules.len() >= self.max_modules {
            return Err(SchemaError::resource(
                "maximum schema registry size exceeded",
            ));
        }
        let normalized = normalize_schema_with(schema, self.max_depth, self.max_nodes)?;
        if let Some(Value::String(declared)) = schema_get(&normalized, "$id")
            && declared != identifier
        {
            return Err(SchemaError::invalid(
                "schema $id does not match its registry identifier",
            ));
        }
        if let Some(index) = existing {
            self.modules[index].1 = normalized.clone();
        } else {
            self.modules
                .push((identifier.to_string(), normalized.clone()));
        }
        Ok(normalized)
    }

    /// Freeze the registry against future mutations.
    pub fn freeze(&mut self) -> &mut Self {
        self.frozen = true;
        self
    }

    /// Whether this registry is frozen.
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// Registry resolution is always local and never performs network I/O.
    pub fn network_resolution(&self) -> bool {
        false
    }

    /// Number of registered modules.
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    /// Whether no modules are registered.
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// Whether a module identifier is present.
    pub fn contains(&self, identifier: &str) -> bool {
        self.modules.iter().any(|(name, _)| name == identifier)
    }

    /// Return an isolated clone of a registered module.
    pub fn get(&self, identifier: &str) -> Option<Value> {
        self.get_ref(identifier).cloned()
    }

    /// Return isolated clones of every registered module in registration order.
    pub fn snapshot(&self) -> Vec<(String, Value)> {
        self.modules.clone()
    }

    /// Alias for [`SchemaRegistry2026::snapshot`].
    pub fn items(&self) -> Vec<(String, Value)> {
        self.snapshot()
    }

    fn get_ref(&self, identifier: &str) -> Option<&Value> {
        self.modules
            .iter()
            .find(|(name, _)| name == identifier)
            .map(|(_, schema)| schema)
    }
}

fn valid_bracketed_authority(authority: &str) -> bool {
    let Some(open) = authority.find('[') else {
        return false;
    };
    let Some(close) = authority.find(']') else {
        return false;
    };
    if open >= close || authority[open + 1..].contains('[') || authority[close + 1..].contains(']')
    {
        return false;
    }

    let prefix = &authority[..open];
    let suffix = &authority[close + 1..];
    if (!prefix.is_empty() && !prefix.ends_with('@'))
        || (!suffix.is_empty() && !suffix.starts_with(':'))
    {
        return false;
    }

    let host = &authority[open + 1..close];
    if let Some(future) = host.strip_prefix('v') {
        let Some((version, address)) = future.split_once('.') else {
            return false;
        };
        return !version.is_empty()
            && version.bytes().all(|byte| byte.is_ascii_hexdigit())
            && !address.is_empty();
    }

    let address = host.split_once('%').map_or(
        host,
        |(address, zone)| {
            if zone.is_empty() { host } else { address }
        },
    );
    address.parse::<core::net::Ipv6Addr>().is_ok()
}

/// Parse exactly one Core AXON value and normalise it as a schema.
pub fn load_schema2026(text: &str) -> Result<Value, SchemaError> {
    let values =
        crate::from_str_stream(text).map_err(|error| SchemaError::invalid(error.to_string()))?;
    if values.len() != 1 {
        return Err(SchemaError::invalid(
            "schema text must contain exactly one value",
        ));
    }
    normalize_schema(&values[0])
}

/// Normalise a mapping or `schema{...}` node with the reference limits.
pub fn normalize_schema(schema: &Value) -> Result<Value, SchemaError> {
    normalize_schema_with(schema, DEFAULT_MAX_DEPTH, DEFAULT_MAX_NODES)
}

/// Normalise a schema with explicit positive depth and node limits.
pub fn normalize_schema_with(
    schema: &Value,
    max_depth: usize,
    max_nodes: usize,
) -> Result<Value, SchemaError> {
    if max_depth < 1 {
        return Err(SchemaError::invalid("max_depth must be a positive integer"));
    }
    if max_nodes < 1 {
        return Err(SchemaError::invalid("max_nodes must be a positive integer"));
    }
    let mut state = NormalizeState {
        root: schema,
        max_depth,
        max_nodes,
        nodes: 0,
        active_labels: Vec::new(),
        anchors: None,
    };
    state.normalize(schema, 0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GraphScanLimit {
    Depth,
    Nodes,
}

#[derive(Clone, Copy)]
enum ScanRole {
    Schema(usize),
    SchemaMap(usize),
    SchemaSequence(usize),
    Opaque,
}

fn collect_schema_anchors(
    root: &Value,
    max_depth: usize,
    max_nodes: usize,
) -> Result<Vec<(&str, &Value)>, GraphScanLimit> {
    let mut anchors = Vec::new();
    let mut stack = alloc::vec![(root, ScanRole::Schema(0))];
    let mut nodes = 0usize;

    while let Some((value, role)) = stack.pop() {
        if matches!(role, ScanRole::Schema(depth) if depth > max_depth) {
            return Err(GraphScanLimit::Depth);
        }
        if nodes >= max_nodes {
            return Err(GraphScanLimit::Nodes);
        }
        nodes += 1;

        if let Value::Anchor(anchor) = value {
            anchors.push((anchor.label.as_str(), &anchor.value));
            // Anchors are transparent to both schema shape and schema depth.
            stack.push((&anchor.value, role));
            continue;
        }

        match role {
            ScanRole::Schema(depth) => match value {
                Value::Map(pairs) | Value::OrderedMap(pairs) => {
                    for (key, item) in pairs.iter().rev() {
                        let item_role = key.as_str().map_or(ScanRole::Opaque, |keyword| {
                            schema_keyword_scan_role(keyword, item, depth)
                        });
                        stack.push((item, item_role));
                        stack.push((key, ScanRole::Opaque));
                    }
                }
                Value::Node(node) => {
                    stack.extend(
                        node.children
                            .iter()
                            .rev()
                            .map(|child| (child, ScanRole::Schema(depth.saturating_add(1)))),
                    );
                    for (keyword, item) in node.attributes.iter().rev() {
                        stack.push((item, schema_keyword_scan_role(keyword, item, depth)));
                    }
                }
                _ => push_opaque_children(value, &mut stack),
            },
            ScanRole::SchemaMap(depth) => match value {
                Value::Map(pairs) | Value::OrderedMap(pairs) => {
                    for (key, item) in pairs.iter().rev() {
                        stack.push((item, ScanRole::Schema(depth)));
                        stack.push((key, ScanRole::Opaque));
                    }
                }
                _ => push_opaque_children(value, &mut stack),
            },
            ScanRole::SchemaSequence(depth) => match value {
                Value::List(items) | Value::Tuple(items) => {
                    stack.extend(
                        items
                            .iter()
                            .rev()
                            .map(|item| (item, ScanRole::Schema(depth))),
                    );
                }
                _ => push_opaque_children(value, &mut stack),
            },
            ScanRole::Opaque => push_opaque_children(value, &mut stack),
        }
    }
    Ok(anchors)
}

fn collect_value_anchors(
    root: &Value,
    max_nodes: usize,
) -> Result<Vec<(&str, &Value)>, GraphScanLimit> {
    let mut anchors = Vec::new();
    let mut stack = alloc::vec![root];
    let mut nodes = 0usize;
    while let Some(value) = stack.pop() {
        if nodes >= max_nodes {
            return Err(GraphScanLimit::Nodes);
        }
        nodes += 1;
        if let Value::Anchor(anchor) = value {
            anchors.push((anchor.label.as_str(), &anchor.value));
            stack.push(&anchor.value);
        } else {
            push_value_children(value, &mut stack);
        }
    }
    Ok(anchors)
}

fn schema_keyword_scan_role(keyword: &str, value: &Value, depth: usize) -> ScanRole {
    let nested = depth.saturating_add(1);
    match keyword {
        "properties" | "$defs" => ScanRole::SchemaMap(nested),
        "prefix_items" => ScanRole::SchemaSequence(nested),
        "items" | "children" | "attributes" => ScanRole::Schema(nested),
        "additional" if !matches!(value, Value::Bool(_)) => ScanRole::Schema(nested),
        _ => ScanRole::Opaque,
    }
}

fn push_opaque_children<'a>(value: &'a Value, stack: &mut Vec<(&'a Value, ScanRole)>) {
    match value {
        Value::List(items) | Value::Tuple(items) | Value::Set(items) => {
            stack.extend(items.iter().rev().map(|item| (item, ScanRole::Opaque)));
        }
        Value::Map(pairs) | Value::OrderedMap(pairs) => {
            for (key, item) in pairs.iter().rev() {
                stack.push((item, ScanRole::Opaque));
                stack.push((key, ScanRole::Opaque));
            }
        }
        Value::Node(node) => {
            stack.extend(
                node.children
                    .iter()
                    .rev()
                    .map(|child| (child, ScanRole::Opaque)),
            );
            stack.extend(
                node.attributes
                    .iter()
                    .rev()
                    .map(|(_, item)| (item, ScanRole::Opaque)),
            );
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
        | Value::Anchor(_)
        | Value::Ref(_)
        | Value::Undefined => {}
    }
}

fn push_value_children<'a>(value: &'a Value, stack: &mut Vec<&'a Value>) {
    match value {
        Value::List(items) | Value::Tuple(items) | Value::Set(items) => {
            stack.extend(items.iter().rev());
        }
        Value::Map(pairs) | Value::OrderedMap(pairs) => {
            for (key, item) in pairs.iter().rev() {
                stack.push(item);
                stack.push(key);
            }
        }
        Value::Node(node) => {
            stack.extend(node.children.iter().rev());
            stack.extend(node.attributes.iter().rev().map(|(_, item)| item));
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
        | Value::Anchor(_)
        | Value::Ref(_)
        | Value::Undefined => {}
    }
}

struct NormalizeState<'a> {
    root: &'a Value,
    max_depth: usize,
    max_nodes: usize,
    nodes: usize,
    active_labels: Vec<String>,
    anchors: Option<Vec<(&'a str, &'a Value)>>,
}

impl<'a> NormalizeState<'a> {
    fn resolve_anchor(&mut self, label: &str) -> Result<&'a Value, SchemaError> {
        if self.anchors.is_none() {
            let anchors = collect_schema_anchors(self.root, self.max_depth, self.max_nodes)
                .map_err(|limit| {
                    SchemaError::resource(match limit {
                        GraphScanLimit::Depth => "maximum schema nesting depth exceeded",
                        GraphScanLimit::Nodes => "maximum schema node count exceeded",
                    })
                })?;
            self.anchors = Some(anchors);
        }
        self.anchors
            .as_ref()
            .and_then(|anchors| {
                anchors
                    .iter()
                    .find_map(|(name, value)| (*name == label).then_some(*value))
            })
            .ok_or_else(|| SchemaError::invalid(format!("unknown schema reference *{label}")))
    }

    fn normalize(&mut self, schema: &Value, depth: usize) -> Result<Value, SchemaError> {
        if depth > self.max_depth {
            return Err(SchemaError::resource(
                "maximum schema nesting depth exceeded",
            ));
        }
        if self.nodes >= self.max_nodes {
            return Err(SchemaError::resource("maximum schema node count exceeded"));
        }
        self.nodes += 1;
        match schema {
            Value::Anchor(anchor) => {
                if self.active_labels.contains(&anchor.label) {
                    return Err(SchemaError::resource("direct cyclic schemas must use $ref"));
                }
                self.active_labels.push(anchor.label.clone());
                let result = self.normalize_active(&anchor.value, depth);
                self.active_labels.pop();
                result
            }
            Value::Ref(label) => {
                if self.active_labels.contains(label) {
                    return Err(SchemaError::resource("direct cyclic schemas must use $ref"));
                }
                let target = self.resolve_anchor(label)?;
                self.active_labels.push(label.clone());
                let result = self.normalize_active(target, depth);
                self.active_labels.pop();
                result
            }
            _ => self.normalize_active(schema, depth),
        }
    }

    fn normalize_active(&mut self, schema: &Value, depth: usize) -> Result<Value, SchemaError> {
        let mut result = match schema {
            Value::Node(node) => self.schema_node_to_map(node)?,
            Value::Map(pairs) | Value::OrderedMap(pairs) => {
                if pairs
                    .iter()
                    .any(|(key, _)| !matches!(key, Value::String(_)))
                {
                    return Err(SchemaError::invalid("schema keyword names must be strings"));
                }
                Value::Map(pairs.clone())
            }
            _ => {
                return Err(SchemaError::invalid(
                    "schema must be a mapping or schema node",
                ));
            }
        };
        self.normalize_keywords(&mut result, depth)?;
        Ok(result)
    }

    fn schema_node_to_map(&self, node: &Node) -> Result<Value, SchemaError> {
        if node.name != "schema" && node.name != "field" {
            return Err(SchemaError::invalid(
                "schema node must be named schema or field",
            ));
        }
        let mut result: Vec<(Value, Value)> = node
            .attributes
            .iter()
            .map(|(key, value)| (Value::String(key.clone()), value.clone()))
            .collect();
        if !node.children.is_empty() {
            let mut properties = match string_map_get(&result, "properties") {
                None => Vec::new(),
                Some(Value::Map(pairs)) | Some(Value::OrderedMap(pairs)) => pairs.clone(),
                Some(_) => return Err(SchemaError::invalid("properties must be a mapping")),
            };
            for child in &node.children {
                let Value::Node(field) = child else {
                    return Err(SchemaError::invalid("schema children must be field nodes"));
                };
                if field.name != "field" {
                    return Err(SchemaError::invalid("schema children must be field nodes"));
                }
                let Some((_, Value::String(name))) =
                    field.attributes.iter().find(|(key, _)| key == "name")
                else {
                    return Err(SchemaError::invalid("field requires a string name"));
                };
                string_map_insert(&mut properties, name, child.clone());
            }
            string_map_insert(&mut result, "properties", Value::Map(properties));
        }
        string_map_remove(&mut result, "name");
        Ok(Value::Map(result))
    }

    fn normalize_keywords(&mut self, schema: &mut Value, depth: usize) -> Result<(), SchemaError> {
        if let Some(dialect) = schema_get(schema, "$schema")
            && !matches!(dialect, Value::Null)
            && !matches!(dialect, Value::String(value) if value == SCHEMA_DIALECT_2026)
        {
            return Err(SchemaError::invalid(format!(
                "unsupported AXON Schema dialect {}",
                python_repr(dialect)
            )));
        }
        for key in ["$id", "$ref"] {
            if schema_get(schema, key).is_some_and(|value| !matches!(value, Value::String(_))) {
                return Err(SchemaError::invalid(format!("{key} must be a string")));
            }
        }

        if let Some(value) = schema_take(schema, "properties") {
            let pairs = mapping_pairs(&value)
                .ok_or_else(|| SchemaError::invalid("properties must be a mapping"))?;
            if pairs
                .iter()
                .any(|(key, _)| !matches!(key, Value::String(_)))
            {
                return Err(SchemaError::invalid("property names must be strings"));
            }
            let mut normalized = Vec::with_capacity(pairs.len());
            for (key, value) in pairs {
                normalized.push((key.clone(), self.normalize(value, depth + 1)?));
            }
            schema_put(schema, "properties", Value::Map(normalized));
        }

        for key in ["items", "children", "attributes"] {
            if let Some(value) = schema_take(schema, key) {
                if !is_schema_shape(&value) {
                    return Err(SchemaError::invalid(format!("{key} must be a schema")));
                }
                let normalized = self.normalize(&value, depth + 1)?;
                schema_put(schema, key, normalized);
            }
        }

        if let Some(value) = schema_take(schema, "additional") {
            match value {
                Value::Bool(flag) => schema_put(schema, "additional", Value::Bool(flag)),
                value if is_schema_shape(&value) => {
                    let normalized = self.normalize(&value, depth + 1)?;
                    schema_put(schema, "additional", normalized);
                }
                _ => {
                    return Err(SchemaError::invalid(
                        "additional must be a boolean or schema",
                    ));
                }
            }
        }

        if let Some(value) = schema_take(schema, "prefix_items") {
            let items = match value {
                Value::List(items) | Value::Tuple(items) => items,
                _ => {
                    return Err(SchemaError::invalid(
                        "prefix_items must be a sequence of schemas",
                    ));
                }
            };
            let mut normalized = Vec::with_capacity(items.len());
            for item in &items {
                normalized.push(self.normalize(item, depth + 1)?);
            }
            schema_put(schema, "prefix_items", Value::List(normalized));
        }

        if let Some(value) = schema_take(schema, "$defs") {
            let pairs = mapping_pairs(&value)
                .ok_or_else(|| SchemaError::invalid("$defs must be a mapping"))?;
            if pairs
                .iter()
                .any(|(key, _)| !matches!(key, Value::String(_)))
            {
                return Err(SchemaError::invalid("$defs names must be strings"));
            }
            let mut normalized = Vec::with_capacity(pairs.len());
            for (key, value) in pairs {
                normalized.push((key.clone(), self.normalize(value, depth + 1)?));
            }
            schema_put(schema, "$defs", Value::Map(normalized));
        }

        check_schema_keywords(schema)
    }
}

const KINDS: &[&str] = &[
    "any",
    "null",
    "bool",
    "boolean",
    "int",
    "integer",
    "float",
    "decimal",
    "number",
    "string",
    "bytes",
    "binary",
    "date",
    "time",
    "datetime",
    "node",
    "set",
    "map",
    "ordered-map",
    "mapping",
    "list",
    "tuple",
    "sequence",
    "collection",
    "link",
];

const KEYWORDS: &[&str] = &[
    "$schema",
    "$id",
    "$ref",
    "$defs",
    "properties",
    "items",
    "children",
    "attributes",
    "additional",
    "prefix_items",
    "kind",
    "const",
    "enum",
    "minimum",
    "maximum",
    "exclusive_minimum",
    "exclusive_maximum",
    "min_length",
    "max_length",
    "pattern",
    "key_pattern",
    "min_items",
    "max_items",
    "name",
    "style",
    "min_children",
    "max_children",
    "required",
    "title",
    "description",
    "$comment",
    "examples",
    "default",
    "deprecated",
];

fn check_schema_keywords(schema: &Value) -> Result<(), SchemaError> {
    let pairs = mapping_pairs(schema).expect("normalized schema is a map");
    let mut unknown: Vec<&str> = pairs
        .iter()
        .filter_map(|(key, _)| key.as_str())
        .filter(|key| !KEYWORDS.contains(key))
        .collect();
    unknown.sort_unstable();
    if let Some(key) = unknown.first() {
        return Err(SchemaError::invalid(format!(
            "unknown schema keyword {}",
            python_string_repr(key)
        )));
    }

    let kinds: Vec<&str> = match schema_get(schema, "kind") {
        None => alloc::vec!["any"],
        Some(Value::String(kind)) => alloc::vec![kind.as_str()],
        Some(Value::List(items)) | Some(Value::Tuple(items))
            if !items.is_empty() && items.iter().all(|item| matches!(item, Value::String(_))) =>
        {
            items.iter().filter_map(Value::as_str).collect()
        }
        _ => {
            return Err(SchemaError::invalid(
                "kind must be a string or non-empty string sequence",
            ));
        }
    };
    let mut unknown_kinds: Vec<&str> = kinds
        .iter()
        .copied()
        .filter(|kind| !KINDS.contains(kind))
        .collect();
    unknown_kinds.sort_unstable();
    unknown_kinds.dedup();
    if let Some(kind) = unknown_kinds.first() {
        return Err(SchemaError::invalid(format!(
            "unknown schema kind {}",
            python_string_repr(kind)
        )));
    }

    if schema_get(schema, "enum")
        .is_some_and(|value| !matches!(value, Value::List(_) | Value::Tuple(_) | Value::Set(_)))
    {
        return Err(SchemaError::invalid("enum must be a sequence or set"));
    }

    if let Some(required) = schema_get(schema, "required") {
        let items = match required {
            Value::List(items) | Value::Tuple(items) => items,
            _ => {
                return Err(SchemaError::invalid(
                    "required must be a sequence of strings",
                ));
            }
        };
        if items.iter().any(|item| !matches!(item, Value::String(_))) {
            return Err(SchemaError::invalid(
                "required must be a sequence of strings",
            ));
        }
        for (index, item) in items.iter().enumerate() {
            if items[..index].iter().any(|prior| prior == item) {
                return Err(SchemaError::invalid("required must not contain duplicates"));
            }
        }
    }

    if schema_get(schema, "pattern").is_some_and(|value| !matches!(value, Value::String(_))) {
        return Err(SchemaError::invalid("pattern must be a string"));
    }

    if schema_get(schema, "key_pattern").is_some_and(|value| !matches!(value, Value::String(_))) {
        return Err(SchemaError::invalid("key_pattern must be a string"));
    }

    for key in [
        "min_length",
        "max_length",
        "min_items",
        "max_items",
        "min_children",
        "max_children",
    ] {
        if schema_get(schema, key).is_some_and(|value| nonnegative_int(value).is_none()) {
            return Err(SchemaError::invalid(format!(
                "{key} must be a non-negative integer"
            )));
        }
    }
    for (minimum, maximum) in [
        ("min_length", "max_length"),
        ("min_items", "max_items"),
        ("min_children", "max_children"),
    ] {
        if let (Some(left), Some(right)) =
            (schema_get(schema, minimum), schema_get(schema, maximum))
            && compare_nonnegative_int(left, right) == Some(Ordering::Greater)
        {
            return Err(SchemaError::invalid(format!(
                "{minimum} cannot exceed {maximum}"
            )));
        }
    }
    Ok(())
}

fn is_schema_shape(value: &Value) -> bool {
    matches!(
        value,
        Value::Map(_) | Value::OrderedMap(_) | Value::Node(_) | Value::Anchor(_) | Value::Ref(_)
    )
}

fn mapping_pairs(value: &Value) -> Option<&[(Value, Value)]> {
    match value {
        Value::Map(pairs) | Value::OrderedMap(pairs) => Some(pairs),
        _ => None,
    }
}

fn mapping_pairs_mut(value: &mut Value) -> Option<&mut Vec<(Value, Value)>> {
    match value {
        Value::Map(pairs) | Value::OrderedMap(pairs) => Some(pairs),
        _ => None,
    }
}

fn string_map_get<'a>(pairs: &'a [(Value, Value)], key: &str) -> Option<&'a Value> {
    pairs.iter().find_map(|(candidate, value)| {
        matches!(candidate, Value::String(text) if text == key).then_some(value)
    })
}

fn string_map_insert(pairs: &mut Vec<(Value, Value)>, key: &str, value: Value) {
    if let Some((_, current)) = pairs
        .iter_mut()
        .find(|(candidate, _)| matches!(candidate, Value::String(text) if text == key))
    {
        *current = value;
    } else {
        pairs.push((Value::String(key.to_string()), value));
    }
}

fn string_map_remove(pairs: &mut Vec<(Value, Value)>, key: &str) -> Option<Value> {
    pairs
        .iter()
        .position(|(candidate, _)| matches!(candidate, Value::String(text) if text == key))
        .map(|index| pairs.remove(index).1)
}

fn schema_get<'a>(schema: &'a Value, key: &str) -> Option<&'a Value> {
    mapping_pairs(schema).and_then(|pairs| string_map_get(pairs, key))
}

fn schema_take(schema: &mut Value, key: &str) -> Option<Value> {
    mapping_pairs_mut(schema).and_then(|pairs| string_map_remove(pairs, key))
}

fn schema_put(schema: &mut Value, key: &str, value: Value) {
    string_map_insert(
        mapping_pairs_mut(schema).expect("normalized schema is a map"),
        key,
        value,
    );
}

fn nonnegative_int(value: &Value) -> Option<&str> {
    match value {
        Value::Int(text)
            if !text.starts_with('-')
                && !text.is_empty()
                && text.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            Some(text)
        }
        _ => None,
    }
}

fn compare_nonnegative_int(left: &Value, right: &Value) -> Option<Ordering> {
    Some(compare_unsigned_decimal(
        nonnegative_int(left)?,
        nonnegative_int(right)?,
    ))
}

fn compare_unsigned_decimal(left: &str, right: &str) -> Ordering {
    let left = left.trim_start_matches('0');
    let right = right.trim_start_matches('0');
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

/// Validate with the reference registry and resource defaults.
pub fn validate2026(value: &Value, schema: &Value) -> Vec<ValidationIssue> {
    validate2026_with(value, schema, None, ValidationOptions::default())
}

/// Validate with an optional local registry and explicit resource controls.
pub fn validate2026_with(
    value: &Value,
    schema: &Value,
    registry: Option<&SchemaRegistry2026>,
    options: ValidationOptions,
) -> Vec<ValidationIssue> {
    let max_issues = options.max_issues.max(1);
    let mut issues = IssueCollector::new(max_issues);
    if options.max_issues == 0 {
        issues.push(issue(
            "invalid-schema",
            &[],
            "max_issues must be a positive integer",
            &[],
        ));
        return issues.finish();
    }
    let normalized = match normalize_schema_with(schema, options.max_depth, DEFAULT_MAX_NODES) {
        Ok(schema) => schema,
        Err(error) => {
            let code = if error.kind == SchemaErrorKind::ResourceLimit {
                "resource-limit-exceeded"
            } else {
                "invalid-schema"
            };
            issues.push(issue(code, &[], error.message, &[]));
            return issues.finish();
        }
    };
    let anchors = match collect_value_anchors(value, DEFAULT_MAX_NODES) {
        Ok(anchors) => anchors,
        Err(_) => {
            issues.push(issue(
                "resource-limit-exceeded",
                &[],
                "maximum validation node count exceeded",
                &[],
            ));
            return issues.finish();
        }
    };
    let mut context = ValidationContext {
        root: &normalized,
        registry,
        graph: ValueGraph { anchors },
        max_depth: options.max_depth,
        depth: 0,
        scale_significant: options.scale_significant,
    };
    let mut seen = Vec::new();
    validate_value(
        value,
        &normalized,
        &[],
        &[],
        &mut issues,
        &mut seen,
        &mut context,
    );
    issues.finish()
}

/// Produce issues, counts by code, validity, and truncation state.
pub fn validation_report2026(value: &Value, schema: &Value) -> ValidationReport {
    validation_report2026_with(value, schema, None, ValidationOptions::default())
}

/// Produce a validation report with an optional registry and explicit limits.
pub fn validation_report2026_with(
    value: &Value,
    schema: &Value,
    registry: Option<&SchemaRegistry2026>,
    options: ValidationOptions,
) -> ValidationReport {
    let issues = validate2026_with(value, schema, registry, options);
    let mut counts: Vec<(String, usize)> = Vec::new();
    for issue in &issues {
        if let Some((_, count)) = counts.iter_mut().find(|(code, _)| code == &issue.code) {
            *count += 1;
        } else {
            counts.push((issue.code.clone(), 1));
        }
    }
    counts.sort_by(|left, right| left.0.cmp(&right.0));
    let truncated = issues
        .iter()
        .any(|issue| issue.code == "issue-limit-exceeded");
    ValidationReport {
        issues,
        counts,
        truncated,
    }
}

/// Return `value` when it validates, otherwise return all ordered issues.
pub fn assert_valid2026<'a>(
    value: &'a Value,
    schema: &Value,
) -> Result<&'a Value, SchemaValidationError> {
    let issues = validate2026(value, schema);
    if issues.is_empty() {
        Ok(value)
    } else {
        Err(SchemaValidationError { issues })
    }
}

struct IssueCollector {
    issues: Vec<ValidationIssue>,
    max_issues: usize,
    truncated: bool,
}

impl IssueCollector {
    fn new(max_issues: usize) -> Self {
        Self {
            issues: Vec::new(),
            max_issues,
            truncated: false,
        }
    }

    fn push(&mut self, issue: ValidationIssue) {
        let reserve = usize::from(self.max_issues > 1);
        if self.issues.len() < self.max_issues - reserve {
            self.issues.push(issue);
        } else {
            self.truncated = true;
        }
    }

    fn finish(mut self) -> Vec<ValidationIssue> {
        if self.truncated
            && !self
                .issues
                .iter()
                .any(|issue| issue.code == "issue-limit-exceeded")
        {
            if self.issues.len() >= self.max_issues {
                self.issues.truncate(self.max_issues - 1);
            }
            self.issues.push(ValidationIssue::new(
                "issue-limit-exceeded",
                Vec::new(),
                format!("validation produced more than {} issues", self.max_issues),
            ));
        }
        order_issues(&mut self.issues);
        self.issues
    }
}

struct ValidationContext<'schema, 'value> {
    root: &'schema Value,
    registry: Option<&'schema SchemaRegistry2026>,
    graph: ValueGraph<'value>,
    max_depth: usize,
    depth: usize,
    scale_significant: bool,
}

struct ValueGraph<'a> {
    anchors: Vec<(&'a str, &'a Value)>,
}

impl<'a> ValueGraph<'a> {
    fn resolve(&self, label: &str) -> Option<&'a Value> {
        self.anchors
            .iter()
            .find_map(|(candidate, value)| (*candidate == label).then_some(*value))
    }
}

impl<'schema, 'value> ValidationContext<'schema, 'value> {
    fn resolve(&self, reference: &str) -> Option<&'schema Value> {
        let (document, has_marker, fragment) = match reference.split_once('#') {
            Some((document, fragment)) => (document, true, fragment),
            None => (reference, false, ""),
        };
        let mut target = if document.is_empty() {
            self.root
        } else {
            self.registry?.get_ref(document)?
        };
        if has_marker {
            if !fragment.is_empty() && !fragment.starts_with('/') {
                return None;
            }
            for raw in fragment.split('/').filter(|part| !part.is_empty()) {
                if invalid_pointer_escape(raw) {
                    return None;
                }
                let part = raw.replace("~1", "/").replace("~0", "~");
                target = match target {
                    Value::List(items) | Value::Tuple(items) => {
                        if part.chars().count() > 18 {
                            return None;
                        }
                        let mut index = 0usize;
                        for ch in part.chars() {
                            index = index
                                .checked_mul(10)?
                                .checked_add(usize::from(python_decimal_digit(ch)?))?;
                        }
                        items.get(index)?
                    }
                    Value::Map(pairs) | Value::OrderedMap(pairs) => string_map_get(pairs, &part)?,
                    _ => return None,
                };
            }
        }
        Some(target)
    }
}

fn invalid_pointer_escape(part: &str) -> bool {
    let bytes = part.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'~'
            && (index + 1 == bytes.len() || !matches!(bytes[index + 1], b'0' | b'1'))
        {
            return true;
        }
        index += 1;
    }
    false
}

fn python_decimal_digit(ch: char) -> Option<u8> {
    let code = ch as u32;
    let base = match code {
        0x0030..=0x0039 => 0x0030,
        0x0660..=0x0669 => 0x0660,
        0x06F0..=0x06F9 => 0x06F0,
        0x07C0..=0x07C9 => 0x07C0,
        0x0966..=0x096F => 0x0966,
        0x09E6..=0x09EF => 0x09E6,
        0x0A66..=0x0A6F => 0x0A66,
        0x0AE6..=0x0AEF => 0x0AE6,
        0x0B66..=0x0B6F => 0x0B66,
        0x0BE6..=0x0BEF => 0x0BE6,
        0x0C66..=0x0C6F => 0x0C66,
        0x0CE6..=0x0CEF => 0x0CE6,
        0x0D66..=0x0D6F => 0x0D66,
        0x0DE6..=0x0DEF => 0x0DE6,
        0x0E50..=0x0E59 => 0x0E50,
        0x0ED0..=0x0ED9 => 0x0ED0,
        0x0F20..=0x0F29 => 0x0F20,
        0x1040..=0x1049 => 0x1040,
        0x1090..=0x1099 => 0x1090,
        0x17E0..=0x17E9 => 0x17E0,
        0x1810..=0x1819 => 0x1810,
        0x1946..=0x194F => 0x1946,
        0x19D0..=0x19D9 => 0x19D0,
        0x1A80..=0x1A89 => 0x1A80,
        0x1A90..=0x1A99 => 0x1A90,
        0x1B50..=0x1B59 => 0x1B50,
        0x1BB0..=0x1BB9 => 0x1BB0,
        0x1C40..=0x1C49 => 0x1C40,
        0x1C50..=0x1C59 => 0x1C50,
        0xA620..=0xA629 => 0xA620,
        0xA8D0..=0xA8D9 => 0xA8D0,
        0xA900..=0xA909 => 0xA900,
        0xA9D0..=0xA9D9 => 0xA9D0,
        0xA9F0..=0xA9F9 => 0xA9F0,
        0xAA50..=0xAA59 => 0xAA50,
        0xABF0..=0xABF9 => 0xABF0,
        0xFF10..=0xFF19 => 0xFF10,
        0x104A0..=0x104A9 => 0x104A0,
        0x10D30..=0x10D39 => 0x10D30,
        0x11066..=0x1106F => 0x11066,
        0x110F0..=0x110F9 => 0x110F0,
        0x11136..=0x1113F => 0x11136,
        0x111D0..=0x111D9 => 0x111D0,
        0x112F0..=0x112F9 => 0x112F0,
        0x11450..=0x11459 => 0x11450,
        0x114D0..=0x114D9 => 0x114D0,
        0x11650..=0x11659 => 0x11650,
        0x116C0..=0x116C9 => 0x116C0,
        0x11730..=0x11739 => 0x11730,
        0x118E0..=0x118E9 => 0x118E0,
        0x11950..=0x11959 => 0x11950,
        0x11C50..=0x11C59 => 0x11C50,
        0x11D50..=0x11D59 => 0x11D50,
        0x11DA0..=0x11DA9 => 0x11DA0,
        0x11F50..=0x11F59 => 0x11F50,
        0x16A60..=0x16A69 => 0x16A60,
        0x16AC0..=0x16AC9 => 0x16AC0,
        0x16B50..=0x16B59 => 0x16B50,
        0x1D7CE..=0x1D7FF => 0x1D7CE + ((code - 0x1D7CE) / 10) * 10,
        0x1E140..=0x1E149 => 0x1E140,
        0x1E2F0..=0x1E2F9 => 0x1E2F0,
        0x1E4F0..=0x1E4F9 => 0x1E4F0,
        0x1E950..=0x1E959 => 0x1E950,
        0x1FBF0..=0x1FBF9 => 0x1FBF0,
        _ => return None,
    };
    Some((code - base) as u8)
}

fn validate_value(
    value: &Value,
    schema: &Value,
    path: &[PathSegment],
    schema_path: &[PathSegment],
    issues: &mut IssueCollector,
    seen: &mut Vec<(*const Value, *const Value)>,
    context: &mut ValidationContext<'_, '_>,
) {
    if issues.truncated {
        return;
    }
    // Apply the depth and (value, schema) cycle guards BEFORE unwrapping a graph
    // anchor/reference, so a self-referential graph value (e.g., `&a *a`) is
    // bounded rather than recursing indefinitely (audit H13).
    if context.depth >= context.max_depth {
        issues.push(issue(
            "resource-limit-exceeded",
            path,
            "maximum validation nesting depth exceeded",
            schema_path,
        ));
        return;
    }
    let marker = (core::ptr::from_ref(value), core::ptr::from_ref(schema));
    if seen.contains(&marker) {
        return;
    }
    seen.push(marker);
    context.depth += 1;
    match value {
        // Graph transparency: `&a v` validates as `v`, `*a` as its target.
        Value::Anchor(anchor) => {
            validate_value(
                &anchor.value,
                schema,
                path,
                schema_path,
                issues,
                seen,
                context,
            );
        }
        Value::Ref(label) if context.graph.resolve(label).is_some() => {
            let target = context.graph.resolve(label).expect("resolved just above");
            validate_value(target, schema, path, schema_path, issues, seen, context);
        }
        _ => validate_active(value, schema, path, schema_path, issues, seen, context),
    }
    context.depth -= 1;
    seen.pop();
}

fn validate_active(
    value: &Value,
    schema: &Value,
    path: &[PathSegment],
    schema_path: &[PathSegment],
    issues: &mut IssueCollector,
    seen: &mut Vec<(*const Value, *const Value)>,
    context: &mut ValidationContext<'_, '_>,
) {
    if let Some(Value::String(reference)) = schema_get(schema, "$ref") {
        let mut reference_schema_path = append_key(schema_path, "$ref");
        if let Some(target) = context.resolve(reference) {
            reference_schema_path.push(reference.as_str().into());
            validate_value(
                value,
                target,
                path,
                &reference_schema_path,
                issues,
                seen,
                context,
            );
        } else {
            issues.push(issue(
                "unresolved-ref",
                path,
                format!(
                    "unresolved schema reference {}",
                    python_string_repr(reference)
                ),
                &reference_schema_path,
            ));
            return;
        }
    }

    let actual = value_kind(value);
    let expected: Vec<&str> = match schema_get(schema, "kind") {
        Some(Value::String(kind)) => alloc::vec![kind],
        Some(Value::List(kinds)) | Some(Value::Tuple(kinds)) => {
            kinds.iter().filter_map(Value::as_str).collect()
        }
        _ => alloc::vec!["any"],
    };
    if !expected.contains(&"any") && !expected.iter().any(|kind| kind_matches(kind, actual)) {
        issues.push(issue(
            "kind",
            path,
            format!("expected {}, got {actual}", expected.join("|")),
            &append_key(schema_path, "kind"),
        ));
        return;
    }

    if let Some(constant) = schema_get(schema, "const")
        && !schema_equal(value, constant, &context.graph, context.scale_significant)
    {
        issues.push(issue(
            "const",
            path,
            "value does not equal required constant",
            &append_key(schema_path, "const"),
        ));
    }
    if let Some(enumeration) = schema_get(schema, "enum") {
        let values = collection_items(enumeration).unwrap_or_default();
        if !values.iter().any(|candidate| {
            schema_equal(value, candidate, &context.graph, context.scale_significant)
        }) {
            issues.push(issue(
                "enum",
                path,
                "value is not in the permitted enumeration",
                &append_key(schema_path, "enum"),
            ));
        }
    }

    if matches!(actual, "int" | "float" | "decimal") {
        validate_numeric(value, schema, path, schema_path, issues);
    }
    if actual == "string" {
        validate_string(value, schema, path, issues);
    }
    if matches!(actual, "list" | "tuple" | "set") {
        validate_collection(value, schema, path, schema_path, issues, seen, context);
    }
    if matches!(actual, "map" | "ordered-map") {
        let pairs = mapping_pairs(value).expect("mapping kind has pairs");
        validate_mapping_pairs(pairs, schema, path, schema_path, issues, seen, context);
    }
    if let Value::Node(node) = value {
        validate_node(node, schema, path, schema_path, issues, seen, context);
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Decimal(_) => "decimal",
        Value::String(_) => "string",
        Value::Bytes(_) => "bytes",
        Value::Temporal(Temporal::Date(_)) => "date",
        Value::Temporal(Temporal::Time(_)) => "time",
        Value::Temporal(Temporal::DateTime(_)) => "datetime",
        Value::Node(_) => "node",
        Value::Set(_) => "set",
        Value::OrderedMap(_) => "ordered-map",
        Value::Map(_) => "map",
        Value::List(_) => "list",
        Value::Tuple(_) => "tuple",
        Value::Link(_) => "link",
        Value::Anchor(_) => "anchor",
        Value::Ref(_) => "reference",
        Value::Undefined => "undefined",
    }
}

fn kind_matches(expected: &str, actual: &str) -> bool {
    expected == actual
        || matches!(
            (expected, actual),
            ("integer", "int")
                | ("boolean", "bool")
                | ("binary", "bytes")
                | ("number", "int" | "float" | "decimal")
                | ("mapping", "map" | "ordered-map")
                | ("sequence", "list" | "tuple")
                | (
                    "collection",
                    "list" | "tuple" | "set" | "map" | "ordered-map"
                )
        )
}

fn validate_numeric(
    value: &Value,
    schema: &Value,
    path: &[PathSegment],
    schema_path: &[PathSegment],
    issues: &mut IssueCollector,
) {
    for (keyword, code, relation, message, include_schema_path) in [
        (
            "minimum",
            "minimum",
            NumericRelation::Less,
            "value is below minimum",
            true,
        ),
        (
            "maximum",
            "maximum",
            NumericRelation::Greater,
            "value is above maximum",
            true,
        ),
        (
            "exclusive_minimum",
            "exclusive-minimum",
            NumericRelation::LessOrEqual,
            "value must be greater than exclusive minimum",
            false,
        ),
        (
            "exclusive_maximum",
            "exclusive-maximum",
            NumericRelation::GreaterOrEqual,
            "value must be less than exclusive maximum",
            false,
        ),
    ] {
        let Some(bound) = schema_get(schema, keyword) else {
            continue;
        };
        match compare_numeric(value, bound) {
            NumericComparison::Ordered(ordering) if relation.matches(ordering) => {
                let generated_path = if include_schema_path {
                    append_key(schema_path, keyword)
                } else {
                    Vec::new()
                };
                issues.push(issue(code, path, message, &generated_path));
            }
            NumericComparison::Ordered(_) | NumericComparison::Unordered => {}
            NumericComparison::Error => {
                issues.push(issue(
                    "schema-comparison",
                    path,
                    "numeric constraint is not comparable with the value",
                    schema_path,
                ));
                break;
            }
        }
    }
}

#[derive(Clone, Copy)]
enum NumericRelation {
    Less,
    Greater,
    LessOrEqual,
    GreaterOrEqual,
}

impl NumericRelation {
    fn matches(self, ordering: Ordering) -> bool {
        match self {
            Self::Less => ordering == Ordering::Less,
            Self::Greater => ordering == Ordering::Greater,
            Self::LessOrEqual => ordering != Ordering::Greater,
            Self::GreaterOrEqual => ordering != Ordering::Less,
        }
    }
}

fn validate_string(
    value: &Value,
    schema: &Value,
    path: &[PathSegment],
    issues: &mut IssueCollector,
) {
    let Value::String(text) = value else {
        return;
    };
    let length = text.chars().count();
    if schema_get(schema, "min_length").is_some_and(|limit| count_below(length, limit)) {
        issues.push(issue("min-length", path, "string is too short", &[]));
    }
    if schema_get(schema, "max_length").is_some_and(|limit| count_above(length, limit)) {
        issues.push(issue("max-length", path, "string is too long", &[]));
    }
    if let Some(Value::String(pattern)) = schema_get(schema, "pattern") {
        match safe_regex_search(pattern, text) {
            Ok(true) => {}
            Ok(false) => issues.push(issue("pattern", path, "string does not match pattern", &[])),
            Err(error) => issues.push(issue(
                "pattern",
                path,
                format!("unsafe or invalid pattern: {}", error.message),
                &[],
            )),
        }
    }
}

fn validate_collection(
    value: &Value,
    schema: &Value,
    path: &[PathSegment],
    schema_path: &[PathSegment],
    issues: &mut IssueCollector,
    seen: &mut Vec<(*const Value, *const Value)>,
    context: &mut ValidationContext<'_, '_>,
) {
    let items = collection_items(value).expect("collection kind has items");
    if schema_get(schema, "min_items").is_some_and(|limit| count_below(items.len(), limit)) {
        issues.push(issue("min-items", path, "container has too few items", &[]));
    }
    if schema_get(schema, "max_items").is_some_and(|limit| count_above(items.len(), limit)) {
        issues.push(issue(
            "max-items",
            path,
            "container has too many items",
            &[],
        ));
    }
    let prefix = schema_get(schema, "prefix_items")
        .and_then(collection_items)
        .unwrap_or_default();
    for (index, subschema) in prefix.iter().enumerate() {
        if let Some(item) = items.get(index) {
            validate_value(
                item,
                subschema,
                &append_index(path, index),
                &append_index(&append_key(schema_path, "prefix_items"), index),
                issues,
                seen,
                context,
            );
        }
    }
    if let Some(item_schema) = schema_get(schema, "items") {
        for (index, item) in items.iter().enumerate().skip(prefix.len()) {
            validate_value(
                item,
                item_schema,
                &append_index(path, index),
                &append_key(schema_path, "items"),
                issues,
                seen,
                context,
            );
        }
    }
}

fn validate_mapping_pairs(
    value: &[(Value, Value)],
    schema: &Value,
    path: &[PathSegment],
    schema_path: &[PathSegment],
    issues: &mut IssueCollector,
    seen: &mut Vec<(*const Value, *const Value)>,
    context: &mut ValidationContext<'_, '_>,
) {
    let properties = schema_get(schema, "properties")
        .and_then(mapping_pairs)
        .unwrap_or_default();
    let mut required: Vec<&str> = schema_get(schema, "required")
        .and_then(collection_items)
        .unwrap_or_default()
        .into_iter()
        .filter_map(Value::as_str)
        .collect();
    required.sort_by_key(|left| python_string_repr(left));
    for key in required {
        if !value
            .iter()
            .any(|(candidate, _)| matches!(candidate, Value::String(text) if text == key))
        {
            issues.push(issue(
                "required",
                &append_key(path, key),
                "required key is missing",
                &append_key(schema_path, "required"),
            ));
        }
    }
    if let Some(Value::String(key_pattern)) = schema_get(schema, "key_pattern") {
        let mut keys: Vec<&Value> = value.iter().map(|(key, _)| key).collect();
        keys.sort_by_key(|key| value_sort_key(key));
        for key in keys {
            let Some(text) = key.as_str() else {
                issues.push(issue(
                    "key-pattern",
                    &append_value_key(path, key.clone()),
                    "mapping key must be a string to match key_pattern",
                    &append_key(schema_path, "key_pattern"),
                ));
                continue;
            };
            match safe_regex_search(key_pattern, text) {
                Ok(true) => {}
                Ok(false) => issues.push(issue(
                    "key-pattern",
                    &append_value_key(path, key.clone()),
                    "mapping key does not match key_pattern",
                    &append_key(schema_path, "key_pattern"),
                )),
                Err(error) => {
                    issues.push(issue(
                        "key-pattern",
                        &append_value_key(path, key.clone()),
                        alloc::format!("unsafe or invalid key_pattern: {}", error.message),
                        &append_key(schema_path, "key_pattern"),
                    ));
                    break;
                }
            }
        }
    }
    let mut ordered: Vec<&(Value, Value)> = value.iter().collect();
    ordered.sort_by_key(|item| value_sort_key(&item.0));
    for (key, item) in ordered {
        let property = properties
            .iter()
            .find_map(|(candidate, schema)| schema_key_equal(key, candidate).then_some(schema));
        if let Some(property_schema) = property {
            validate_value(
                item,
                property_schema,
                &append_value_key(path, key.clone()),
                &append_value_key(&append_key(schema_path, "properties"), key.clone()),
                issues,
                seen,
                context,
            );
        } else {
            match schema_get(schema, "additional") {
                Some(Value::Bool(false)) => issues.push(issue(
                    "additional",
                    &append_value_key(path, key.clone()),
                    "additional key is not permitted",
                    &[],
                )),
                Some(additional) if mapping_pairs(additional).is_some() => validate_value(
                    item,
                    additional,
                    &append_value_key(path, key.clone()),
                    &append_key(schema_path, "additional"),
                    issues,
                    seen,
                    context,
                ),
                _ => {}
            }
        }
    }
}

fn validate_node(
    node: &Node,
    schema: &Value,
    path: &[PathSegment],
    schema_path: &[PathSegment],
    issues: &mut IssueCollector,
    seen: &mut Vec<(*const Value, *const Value)>,
    context: &mut ValidationContext<'_, '_>,
) {
    if let Some(expected) = schema_get(schema, "name")
        && !matches!(expected, Value::String(name) if name == &node.name)
    {
        issues.push(issue(
            "node-name",
            path,
            format!("expected node {}", python_repr(expected)),
            &[],
        ));
    }
    if let Some(expected) = schema_get(schema, "style") {
        let actual = match node.style {
            NodeStyle::Unit => "unit",
            NodeStyle::Brace => "brace",
            NodeStyle::Tuple => "tuple",
            NodeStyle::List => "list",
        };
        if !matches!(expected, Value::String(style) if style == actual) {
            issues.push(issue(
                "node-style",
                path,
                format!("expected node body style {}", python_repr(expected)),
                &[],
            ));
        }
    }

    let mut attribute_schema = schema_get(schema, "attributes")
        .cloned()
        .unwrap_or_else(|| map_value(&[("kind", Value::String("map".to_string()))]));
    if let Some(properties) = schema_get(schema, "properties")
        && schema_get(&attribute_schema, "properties").is_none()
    {
        schema_put(&mut attribute_schema, "properties", properties.clone());
    }
    let attributes: Vec<(Value, Value)> = node
        .attributes
        .iter()
        .map(|(key, value)| (Value::String(key.clone()), value.clone()))
        .collect();
    validate_mapping_pairs(
        &attributes,
        &attribute_schema,
        &append_key(path, "@"),
        &append_key(schema_path, "attributes"),
        issues,
        seen,
        context,
    );

    if let Some(child_schema) = schema_get(schema, "children")
        && value_truthy(child_schema)
    {
        for (index, child) in node.children.iter().enumerate() {
            validate_value(
                child,
                child_schema,
                &append_index(path, index),
                &append_key(schema_path, "children"),
                issues,
                seen,
                context,
            );
        }
    }
    if schema_get(schema, "min_children")
        .is_some_and(|limit| count_below(node.children.len(), limit))
    {
        issues.push(issue(
            "min-children",
            path,
            "node has too few children",
            &[],
        ));
    }
    if schema_get(schema, "max_children")
        .is_some_and(|limit| count_above(node.children.len(), limit))
    {
        issues.push(issue(
            "max-children",
            path,
            "node has too many children",
            &[],
        ));
    }
}

fn value_truthy(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::Int(text) => !text.trim_start_matches(['-', '0']).is_empty(),
        Value::Float(value) => *value != 0.0,
        Value::Decimal(Decimal::Finite { mantissa, .. }) => {
            !mantissa.trim_start_matches(['-', '0']).is_empty()
        }
        Value::String(value) => !value.is_empty(),
        Value::Bytes(value) => !value.is_empty(),
        Value::List(value) | Value::Tuple(value) | Value::Set(value) => !value.is_empty(),
        Value::Map(value) | Value::OrderedMap(value) => !value.is_empty(),
        _ => true,
    }
}

fn count_below(count: usize, limit: &Value) -> bool {
    nonnegative_int(limit).is_some_and(|digits| {
        compare_unsigned_decimal(&count.to_string(), digits) == Ordering::Less
    })
}

fn count_above(count: usize, limit: &Value) -> bool {
    nonnegative_int(limit).is_some_and(|digits| {
        compare_unsigned_decimal(&count.to_string(), digits) == Ordering::Greater
    })
}

fn collection_items(value: &Value) -> Option<Vec<&Value>> {
    match value {
        Value::List(items) | Value::Tuple(items) | Value::Set(items) => {
            Some(items.iter().collect())
        }
        _ => None,
    }
}

fn map_value(entries: &[(&str, Value)]) -> Value {
    Value::Map(
        entries
            .iter()
            .map(|(key, value)| (Value::String((*key).to_string()), value.clone()))
            .collect(),
    )
}

fn issue(
    code: impl Into<String>,
    path: &[PathSegment],
    message: impl Into<String>,
    schema_path: &[PathSegment],
) -> ValidationIssue {
    ValidationIssue {
        code: code.into(),
        path: path.to_vec(),
        message: message.into(),
        schema_path: schema_path.to_vec(),
    }
}

fn append_key(path: &[PathSegment], key: &str) -> Vec<PathSegment> {
    let mut result = path.to_vec();
    result.push(key.into());
    result
}

fn append_value_key(path: &[PathSegment], key: Value) -> Vec<PathSegment> {
    let mut result = path.to_vec();
    result.push(key.into());
    result
}

fn append_index(path: &[PathSegment], index: usize) -> Vec<PathSegment> {
    let mut result = path.to_vec();
    result.push(index.into());
    result
}

fn schema_key_equal(left: &Value, right: &Value) -> bool {
    matches!((left, right), (Value::String(a), Value::String(b)) if a == b)
}

fn order_issues(issues: &mut Vec<ValidationIssue>) {
    issues.sort_by(|left, right| {
        compare_path(&left.path, &right.path)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| compare_path(&left.schema_path, &right.schema_path))
            .then_with(|| left.message.cmp(&right.message))
    });
    issues.dedup_by(|right, left| {
        compare_path(&left.path, &right.path) == Ordering::Equal
            && left.code == right.code
            && compare_path(&left.schema_path, &right.schema_path) == Ordering::Equal
            && left.message == right.message
    });
}

fn compare_path(left: &[PathSegment], right: &[PathSegment]) -> Ordering {
    for (left, right) in left.iter().zip(right) {
        let ordering = path_sort_key(left).cmp(&path_sort_key(right));
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

fn path_sort_key(item: &PathSegment) -> (String, String) {
    match item {
        PathSegment::Index(index) => ("int".to_string(), index.to_string()),
        PathSegment::Key(value) => value_sort_key(value),
    }
}

fn path_repr(item: &PathSegment) -> String {
    path_sort_key(item).1
}

fn value_sort_key(value: &Value) -> (String, String) {
    (python_type_name(value).to_string(), python_repr(value))
}

fn python_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Decimal(_) => "Decimal",
        Value::String(_) => "str",
        Value::Bytes(_) => "bytes",
        Value::Temporal(Temporal::Date(_)) => "date",
        Value::Temporal(Temporal::Time(_)) => "NanoTime",
        Value::Temporal(Temporal::DateTime(_)) => "NanoDateTime",
        Value::Node(_) => "Node",
        Value::Set(_) => "AxonSet",
        Value::OrderedMap(_) => "OrderedDict",
        Value::Map(_) => "dict",
        Value::List(_) => "list",
        Value::Tuple(_) => "AxonTuple",
        Value::Link(_) => "Link",
        Value::Anchor(_) => "Anchor",
        Value::Ref(_) => "Reference",
        Value::Undefined => "_Undefined",
    }
}

fn python_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Int(value) => value.clone(),
        Value::Float(value) if value.is_nan() => "nan".to_string(),
        Value::Float(value) if *value == f64::INFINITY => "inf".to_string(),
        Value::Float(value) if *value == f64::NEG_INFINITY => "-inf".to_string(),
        Value::Float(value) => format!("{value:?}"),
        Value::Decimal(value) => format!("Decimal({})", python_string_repr(&decimal_text(value))),
        Value::String(value) => python_string_repr(value),
        Value::Bytes(value) => python_bytes_repr(value),
        _ => crate::writer::to_string(value).unwrap_or_else(|_| format!("{value:?}")),
    }
}

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
                    output.push_str(&format!("\\x{code:02x}"));
                } else if code <= 0xffff {
                    output.push_str(&format!("\\u{code:04x}"));
                } else {
                    output.push_str(&format!("\\U{code:08x}"));
                }
            }
            ch => output.push(ch),
        }
    }
    output.push(quote);
    output
}

fn python_bytes_repr(value: &[u8]) -> String {
    let mut output = "b'".to_string();
    for byte in value {
        match byte {
            b'\\' => output.push_str("\\\\"),
            b'\'' => output.push_str("\\'"),
            b'\n' => output.push_str("\\n"),
            b'\r' => output.push_str("\\r"),
            b'\t' => output.push_str("\\t"),
            0x20..=0x7e => output.push(char::from(*byte)),
            _ => output.push_str(&format!("\\x{byte:02x}")),
        }
    }
    output.push('\'');
    output
}

fn decimal_text(value: &Decimal) -> String {
    match value {
        Decimal::Infinity => "Infinity".to_string(),
        Decimal::NegInfinity => "-Infinity".to_string(),
        Decimal::NaN => "NaN".to_string(),
        Decimal::Finite { mantissa, exponent } | Decimal::ScaledFinite { mantissa, exponent } => {
            if *exponent == 0 {
                mantissa.clone()
            } else {
                format!("{mantissa}E{exponent:+}")
            }
        }
    }
}

#[derive(Clone, Debug)]
struct ExactDecimal {
    negative: bool,
    digits: String,
    exponent: i64,
}

impl ExactDecimal {
    fn new(negative: bool, digits: &str, mut exponent: i64) -> Option<Self> {
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let digits = digits.trim_start_matches('0');
        if digits.is_empty() {
            return Some(Self {
                negative: false,
                digits: "0".to_string(),
                exponent: 0,
            });
        }
        let mut digits = digits.to_string();
        while digits.ends_with('0') {
            digits.pop();
            exponent = exponent.saturating_add(1);
        }
        Some(Self {
            negative,
            digits,
            exponent,
        })
    }

    fn from_signed(text: &str, exponent: i64) -> Option<Self> {
        let (negative, digits) = text
            .strip_prefix('-')
            .map(|digits| (true, digits))
            .unwrap_or((false, text));
        Self::new(negative, digits, exponent)
    }

    fn compare(&self, other: &Self) -> Ordering {
        if self.negative != other.negative {
            return if self.negative {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        let magnitude = compare_decimal_magnitude(self, other);
        if self.negative {
            magnitude.reverse()
        } else {
            magnitude
        }
    }
}

fn compare_decimal_magnitude(left: &ExactDecimal, right: &ExactDecimal) -> Ordering {
    let left_order =
        i128::try_from(left.digits.len()).unwrap_or(i128::MAX) + i128::from(left.exponent);
    let right_order =
        i128::try_from(right.digits.len()).unwrap_or(i128::MAX) + i128::from(right.exponent);
    let order = left_order.cmp(&right_order);
    if order != Ordering::Equal {
        return order;
    }
    let length = left.digits.len().max(right.digits.len());
    let left_bytes = left.digits.as_bytes();
    let right_bytes = right.digits.as_bytes();
    for index in 0..length {
        let left = left_bytes.get(index).copied().unwrap_or(b'0');
        let right = right_bytes.get(index).copied().unwrap_or(b'0');
        let order = left.cmp(&right);
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

enum NumberClass {
    NegativeInfinity,
    Finite(ExactDecimal),
    PositiveInfinity,
    FloatNaN,
    DecimalNaN,
}

enum NumericComparison {
    Ordered(Ordering),
    Unordered,
    Error,
}

fn compare_numeric(left: &Value, right: &Value) -> NumericComparison {
    let Some(left) = number_class(left) else {
        return NumericComparison::Error;
    };
    let Some(right) = number_class(right) else {
        return NumericComparison::Error;
    };
    use NumberClass::{DecimalNaN, Finite, FloatNaN, NegativeInfinity, PositiveInfinity};
    match (&left, &right) {
        (DecimalNaN, _) | (_, DecimalNaN) => NumericComparison::Error,
        (FloatNaN, _) | (_, FloatNaN) => NumericComparison::Unordered,
        (NegativeInfinity, NegativeInfinity) | (PositiveInfinity, PositiveInfinity) => {
            NumericComparison::Ordered(Ordering::Equal)
        }
        (NegativeInfinity, _) | (_, PositiveInfinity) => NumericComparison::Ordered(Ordering::Less),
        (PositiveInfinity, _) | (_, NegativeInfinity) => {
            NumericComparison::Ordered(Ordering::Greater)
        }
        (Finite(left), Finite(right)) => NumericComparison::Ordered(left.compare(right)),
    }
}

fn number_class(value: &Value) -> Option<NumberClass> {
    match value {
        Value::Bool(value) => Some(NumberClass::Finite(ExactDecimal::new(
            false,
            if *value { "1" } else { "0" },
            0,
        )?)),
        Value::Int(value) => Some(NumberClass::Finite(ExactDecimal::from_signed(value, 0)?)),
        Value::Float(value) if value.is_nan() => Some(NumberClass::FloatNaN),
        Value::Float(value) if *value == f64::INFINITY => Some(NumberClass::PositiveInfinity),
        Value::Float(value) if *value == f64::NEG_INFINITY => Some(NumberClass::NegativeInfinity),
        Value::Float(value) => Some(NumberClass::Finite(exact_float(*value))),
        Value::Decimal(Decimal::Infinity) => Some(NumberClass::PositiveInfinity),
        Value::Decimal(Decimal::NegInfinity) => Some(NumberClass::NegativeInfinity),
        Value::Decimal(Decimal::NaN) => Some(NumberClass::DecimalNaN),
        // A scale-significant decimal has the same numeric magnitude as an
        // ordinary finite decimal for range checks (audit H9); classify both.
        Value::Decimal(Decimal::Finite { mantissa, exponent })
        | Value::Decimal(Decimal::ScaledFinite { mantissa, exponent }) => Some(
            NumberClass::Finite(ExactDecimal::from_signed(mantissa, i64::from(*exponent))?),
        ),
        _ => None,
    }
}

fn exact_float(value: f64) -> ExactDecimal {
    if value == 0.0 {
        return ExactDecimal::new(false, "0", 0).expect("zero is valid");
    }
    let bits = value.to_bits();
    let negative = bits >> 63 != 0;
    let raw_exponent = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    let (mantissa, exponent_two) = if raw_exponent == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1u64 << 52), raw_exponent - 1023 - 52)
    };
    let mut integer = BigNat::from_u64(mantissa);
    let exponent_ten = if exponent_two >= 0 {
        for _ in 0..exponent_two {
            integer.multiply_small(2);
        }
        0
    } else {
        for _ in 0..-exponent_two {
            integer.multiply_small(5);
        }
        i64::from(exponent_two)
    };
    ExactDecimal::new(negative, &integer.to_decimal_string(), exponent_ten)
        .expect("float decomposition is decimal")
}

struct BigNat(Vec<u32>);

impl BigNat {
    const BASE: u64 = 1_000_000_000;

    fn from_u64(mut value: u64) -> Self {
        let mut limbs = Vec::new();
        while value != 0 {
            limbs.push((value % Self::BASE) as u32);
            value /= Self::BASE;
        }
        if limbs.is_empty() {
            limbs.push(0);
        }
        Self(limbs)
    }

    fn multiply_small(&mut self, multiplier: u32) {
        let mut carry = 0u64;
        for limb in &mut self.0 {
            let value = u64::from(*limb) * u64::from(multiplier) + carry;
            *limb = (value % Self::BASE) as u32;
            carry = value / Self::BASE;
        }
        if carry != 0 {
            self.0.push(carry as u32);
        }
    }

    fn to_decimal_string(&self) -> String {
        let mut output = self.0.last().copied().unwrap_or(0).to_string();
        for limb in self.0.iter().rev().skip(1) {
            output.push_str(&format!("{limb:09}"));
        }
        output
    }
}

fn schema_equal(
    left: &Value,
    right: &Value,
    graph: &ValueGraph<'_>,
    scale_significant: bool,
) -> bool {
    schema_equal_at(left, right, graph, scale_significant, 0)
}

fn schema_equal_at(
    left: &Value,
    right: &Value,
    graph: &ValueGraph<'_>,
    scale_significant: bool,
    depth: usize,
) -> bool {
    if depth > 512 {
        return false;
    }
    match left {
        Value::Anchor(anchor) => {
            return schema_equal_at(&anchor.value, right, graph, scale_significant, depth + 1);
        }
        Value::Ref(label) => {
            if let Some(target) = graph.resolve(label) {
                return schema_equal_at(target, right, graph, scale_significant, depth + 1);
            }
        }
        _ => {}
    }
    if scale_significant && matches!(left, Value::Decimal(_)) && number_class(right).is_some() {
        return false;
    }
    if !same_python_type(left, right) {
        return false;
    }
    loose_equal(left, right, depth, graph, scale_significant)
}

fn same_python_type(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Temporal(Temporal::Date(_)), Value::Temporal(Temporal::Date(_)))
        | (Value::Temporal(Temporal::Time(_)), Value::Temporal(Temporal::Time(_)))
        | (Value::Temporal(Temporal::DateTime(_)), Value::Temporal(Temporal::DateTime(_))) => true,
        _ => core::mem::discriminant(left) == core::mem::discriminant(right),
    }
}

fn loose_equal(
    left: &Value,
    right: &Value,
    depth: usize,
    graph: &ValueGraph<'_>,
    scale_significant: bool,
) -> bool {
    if depth > 512 {
        return false;
    }
    match left {
        Value::Anchor(anchor) => {
            return loose_equal(&anchor.value, right, depth + 1, graph, scale_significant);
        }
        Value::Ref(label) => {
            if let Some(target) = graph.resolve(label) {
                return loose_equal(target, right, depth + 1, graph, scale_significant);
            }
        }
        _ => {}
    }
    if scale_significant && matches!(left, Value::Decimal(_)) && number_class(right).is_some() {
        return false;
    }
    if number_class(left).is_some() && number_class(right).is_some() {
        return matches!(
            compare_numeric(left, right),
            NumericComparison::Ordered(Ordering::Equal)
        );
    }
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::String(left), Value::String(right)) => left == right,
        (Value::Bytes(left), Value::Bytes(right)) => left == right,
        (Value::Temporal(left), Value::Temporal(right)) => left == right,
        (Value::Link(left), Value::Link(right)) => left == right,
        (Value::List(left), Value::List(right)) | (Value::Tuple(left), Value::Tuple(right)) => {
            sequence_equal(left, right, depth + 1, graph, scale_significant)
        }
        (Value::Set(left), Value::Set(right)) => {
            sequence_equal(left, right, depth + 1, graph, scale_significant)
        }
        (Value::Map(left), Value::Map(right)) => {
            mapping_equal(left, right, false, depth + 1, graph, scale_significant)
        }
        (Value::OrderedMap(left), Value::OrderedMap(right)) => {
            mapping_equal(left, right, true, depth + 1, graph, scale_significant)
        }
        (Value::Map(left), Value::OrderedMap(right))
        | (Value::OrderedMap(left), Value::Map(right)) => {
            mapping_equal(left, right, false, depth + 1, graph, scale_significant)
        }
        (Value::Node(left), Value::Node(right)) => {
            left.name == right.name
                && left.style == right.style
                && left.attributes.len() == right.attributes.len()
                && left.attributes.iter().zip(&right.attributes).all(
                    |((left_key, left), (right_key, right))| {
                        left_key == right_key
                            && loose_equal(left, right, depth + 1, graph, scale_significant)
                    },
                )
                && sequence_equal(
                    &left.children,
                    &right.children,
                    depth + 1,
                    graph,
                    scale_significant,
                )
        }
        (Value::Ref(left), Value::Ref(right)) => left == right,
        _ => false,
    }
}

fn sequence_equal(
    left: &[Value],
    right: &[Value],
    depth: usize,
    graph: &ValueGraph<'_>,
    scale_significant: bool,
) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| loose_equal(left, right, depth, graph, scale_significant))
}

fn mapping_equal(
    left: &[(Value, Value)],
    right: &[(Value, Value)],
    ordered: bool,
    depth: usize,
    graph: &ValueGraph<'_>,
    scale_significant: bool,
) -> bool {
    if left.len() != right.len() {
        return false;
    }
    if ordered {
        return left
            .iter()
            .zip(right)
            .all(|((left_key, left_value), (right_key, right_value))| {
                loose_equal(left_key, right_key, depth, graph, scale_significant)
                    && loose_equal(left_value, right_value, depth, graph, scale_significant)
            });
    }
    let mut used = alloc::vec![false; right.len()];
    for (left_key, left_value) in left {
        let Some(index) = right.iter().enumerate().position(|(index, (key, value))| {
            !used[index]
                && loose_equal(left_key, key, depth, graph, scale_significant)
                && loose_equal(left_value, value, depth, graph, scale_significant)
        }) else {
            return false;
        };
        used[index] = true;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string(value: &str) -> Value {
        Value::String(value.to_string())
    }

    fn deeply_nested_list(depth: usize) -> Value {
        let mut value = Value::Null;
        for _ in 0..depth {
            value = Value::List(alloc::vec![value]);
        }
        value
    }

    fn deeply_nested_schema(depth: usize) -> Value {
        let mut value = Value::Map(Vec::new());
        for _ in 0..depth {
            value = Value::Map(alloc::vec![(string("items"), value)]);
        }
        value
    }

    #[test]
    fn registry_identifier_checks_bracketed_hosts_like_urlsplit() {
        for identifier in [
            "http://[notipv6]",
            "http://[]",
            "http://[127.0.0.1]",
            "http://[v1.]",
            "http://[::1]suffix",
            "http://host[::1]",
        ] {
            let error = SchemaRegistry2026::validate_identifier(identifier).unwrap_err();
            assert_eq!(error.message, "invalid schema module URI", "{identifier}");
        }

        for identifier in [
            "http://[::1]",
            "http://user@[::ffff:192.0.2.1]:80",
            "http://[v1.future]",
            "urn:[notipv6]",
        ] {
            assert_eq!(
                SchemaRegistry2026::validate_identifier(identifier),
                Ok(identifier),
                "{identifier}"
            );
        }
    }

    #[test]
    fn normalization_anchor_scan_obeys_depth_before_recursive_descent() {
        let schema = deeply_nested_schema(20_000);
        let error = collect_schema_anchors(&schema, 32, DEFAULT_MAX_NODES).unwrap_err();
        core::mem::forget(schema);
        assert_eq!(error, GraphScanLimit::Depth);
    }

    #[test]
    fn normalization_anchor_scan_obeys_node_limit() {
        let schema = Value::Map(alloc::vec![(
            string("const"),
            Value::List(alloc::vec![Value::Null; 8]),
        )]);
        let error = collect_schema_anchors(&schema, 32, 4).unwrap_err();
        assert_eq!(error, GraphScanLimit::Nodes);
    }

    #[test]
    fn validation_graph_scan_is_iterative_for_deep_manual_values() {
        let value = deeply_nested_list(20_000);
        let recursive_ref = map_value(&[("$ref", Value::String("#/$defs/list".to_string()))]);
        let schema = map_value(&[
            (
                "$defs",
                map_value(&[(
                    "list",
                    map_value(&[
                        ("kind", Value::String("list".to_string())),
                        ("items", recursive_ref.clone()),
                    ]),
                )]),
            ),
            ("$ref", Value::String("#/$defs/list".to_string())),
        ]);
        let issues = validate2026_with(
            &value,
            &schema,
            None,
            ValidationOptions {
                max_depth: 32,
                max_issues: 8,
                scale_significant: false,
            },
        );
        core::mem::forget(value);
        assert!(issues.iter().any(|issue| {
            issue.code == "resource-limit-exceeded"
                && issue.message == "maximum validation nesting depth exceeded"
        }));
    }

    #[test]
    fn scale_significant_option_changes_only_schema_equality() {
        let decimal = Value::Decimal(Decimal::Finite {
            mantissa: "150".to_string(),
            exponent: -2,
        });
        let nested = Value::List(alloc::vec![decimal.clone()]);
        let schema = Value::Map(alloc::vec![(string("const"), nested.clone())]);

        assert!(validate2026(&nested, &schema).is_empty());
        let issues = validate2026_with(
            &nested,
            &schema,
            None,
            ValidationOptions {
                scale_significant: true,
                ..ValidationOptions::default()
            },
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "const");

        let integer_schema = Value::Map(alloc::vec![(
            string("const"),
            Value::List(alloc::vec![Value::Int("1".to_string())]),
        )]);
        let integer_issues = validate2026_with(
            &Value::List(alloc::vec![Value::Decimal(Decimal::Finite {
                mantissa: "10".to_string(),
                exponent: -1,
            })]),
            &integer_schema,
            None,
            ValidationOptions {
                scale_significant: true,
                ..ValidationOptions::default()
            },
        );
        assert_eq!(integer_issues.len(), 1);
        assert_eq!(integer_issues[0].code, "const");

        let numeric_schema = Value::Map(alloc::vec![(string("minimum"), decimal.clone())]);
        assert!(
            validate2026_with(
                &decimal,
                &numeric_schema,
                None,
                ValidationOptions {
                    scale_significant: true,
                    ..ValidationOptions::default()
                },
            )
            .is_empty()
        );
    }
}
