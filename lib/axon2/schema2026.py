"""AXON Schema 2026 companion validator.

Schemas are ordinary Python mappings or AXON ``schema{...}`` nodes.  The
validator operates on semantic values and never executes or imports code.
"""
from dataclasses import dataclass
from collections import Counter
from copy import deepcopy
from decimal import Decimal, InvalidOperation
import re
try:
    # The regex primitives moved to the private ``re._parser`` in CPython 3.11;
    # the top-level ``sre_parse`` alias is deprecated and slated for removal.
    import re._parser as sre_parse
except ImportError:  # pragma: no cover - older interpreters
    import sre_parse
from urllib.parse import urlsplit
from .v2026 import AxonSet, AxonTuple, Link, NanoDateTime, NanoTime, Node, ScaledDecimal, loads2026
SCHEMA_DIALECT_2026 = 'urn:axonnext:schema:2026'

@dataclass(frozen=True)
class ValidationIssue:
    """
Represents a distinct validation failure, detailing the error code, traversal path, and specific message.
"""
    code: str
    path: tuple
    message: str
    schema_path: tuple = ()

    def __str__(self):
        """
Formats the validation issue into a human-readable diagnostic string, including the structural path.
"""
        location = '$' + ''.join(('[%r]' % part for part in self.path))
        return '%s at %s: %s' % (self.code, location, self.message)

class SchemaValidationError(ValueError):
    """
An exception raised when a value fails to satisfy the constraints of an AXON schema.
"""

    def __init__(self, issues):
        self.issues = tuple(issues)
        super().__init__('; '.join(map(str, self.issues)))

@dataclass(frozen=True)
class ValidationReport:
    """
Aggregates the total results of a validation run, capturing all issues and statistical counts.
"""
    issues: tuple[ValidationIssue, ...]
    counts: tuple[tuple[str, int], ...]
    truncated: bool = False

    @property
    def valid(self):
        """
Returns a boolean indicating whether the validation report is entirely free of issues.
"""
        return not self.issues

class SchemaRegistry2026:
    """Governed, local-only registry for AXON Schema modules.

    Module identifiers are absolute ASCII URIs without fragments.  Registering
    an identifier never performs I/O, and references are resolved exclusively
    against this explicit registry.
    """

    def __init__(self, modules=None, *, max_modules=1024, max_depth=128, max_nodes=1048576):
        if not isinstance(max_modules, int) or max_modules < 1:
            raise ValueError('max_modules must be a positive integer')
        if not isinstance(max_depth, int) or max_depth < 1:
            raise ValueError('max_depth must be a positive integer')
        if not isinstance(max_nodes, int) or max_nodes < 1:
            raise ValueError('max_nodes must be a positive integer')
        self.max_modules = max_modules
        self.max_depth = max_depth
        self.max_nodes = max_nodes
        self._modules = {}
        self._frozen = False
        for identifier, schema in (modules or {}).items():
            self.register(identifier, schema)

    @staticmethod
    def validate_identifier(identifier):
        """Return *identifier* if it is an absolute ASCII URI without a fragment.

Raises :class:`ValueError` otherwise.
        """
        if not isinstance(identifier, str) or not identifier:
            raise ValueError('schema module identifier must be a non-empty string')
        try:
            identifier.encode('ascii')
        except UnicodeEncodeError as exc:
            raise ValueError('schema module identifier must be ASCII') from exc
        if any((ord(ch) <= 32 or ord(ch) == 127 for ch in identifier)):
            raise ValueError('schema module identifier contains whitespace or controls')
        try:
            parsed = urlsplit(identifier)
        except ValueError as exc:
            raise ValueError('invalid schema module URI') from exc
        if not parsed.scheme or parsed.fragment:
            raise ValueError('schema module identifier must be an absolute URI without a fragment')
        return identifier

    def register(self, identifier, schema, *, replace=False):
        """Normalise *schema* and register it under *identifier*.

Returns a copy, so mutating the result cannot alter the registry.
Registration performs no I/O. Pass ``replace=True`` to overwrite.
        """
        if self._frozen:
            raise RuntimeError('schema registry is frozen')
        identifier = self.validate_identifier(identifier)
        if identifier in self._modules and (not replace):
            raise ValueError('duplicate schema module identifier %r' % identifier)
        if identifier not in self._modules and len(self._modules) >= self.max_modules:
            raise _SchemaResourceError('maximum schema registry size exceeded')
        normalized = normalize_schema(schema, max_depth=self.max_depth, max_nodes=self.max_nodes)
        declared = normalized.get('$id')
        if declared is not None and declared != identifier:
            raise ValueError('schema $id does not match its registry identifier')
        self._modules[identifier] = normalized
        return deepcopy(normalized)

    def freeze(self):
        """Reject any further registrations and return the registry."""
        self._frozen = True
        return self

    @property
    def frozen(self):
        """Whether the registry has been frozen."""
        return self._frozen

    @property
    def network_resolution(self):
        """Always ``False``: references resolve against this registry only, never over the network."""
        return False

    def snapshot(self):
        """Return a deep copy of the registered modules."""
        return deepcopy(self._modules)

    def __len__(self):
        return len(self._modules)

    def __contains__(self, identifier):
        return identifier in self._modules

    def __getitem__(self, identifier):
        return deepcopy(self._modules[identifier])

    def items(self):
        """Return the registered ``(identifier, schema)`` pairs as deep copies."""
        return tuple(((identifier, deepcopy(schema)) for identifier, schema in self._modules.items()))

def load_schema2026(text):
    """Parse AXON *text* containing exactly one schema and return it normalised."""
    values = loads2026(text)
    if len(values) != 1:
        raise ValueError('schema text must contain exactly one value')
    return normalize_schema(values[0])

class _SchemaResourceError(ValueError):
    pass

def normalize_schema(schema, *, max_depth=128, max_nodes=1048576):
    """Validate a schema's shape and return it in normalised mapping form.

Nested sub-schemas are normalised recursively, bounded by *max_depth* and
*max_nodes*; a schema that is cyclic other than through ``$ref`` is
rejected.
    """
    if not isinstance(max_depth, int) or max_depth < 1:
        raise ValueError('max_depth must be a positive integer')
    if not isinstance(max_nodes, int) or max_nodes < 1:
        raise ValueError('max_nodes must be a positive integer')
    return _normalize_schema(schema, set(), 0, [0], max_depth, max_nodes)

def _normalize_schema(schema, active, depth, count, max_depth, max_nodes):
    if depth > max_depth:
        raise _SchemaResourceError('maximum schema nesting depth exceeded')
    count[0] += 1
    if count[0] > max_nodes:
        raise _SchemaResourceError('maximum schema node count exceeded')
    track = isinstance(schema, (Node, dict))
    ident = id(schema)
    if track:
        if ident in active:
            raise _SchemaResourceError('direct cyclic schemas must use $ref')
        active.add(ident)
    try:
        return _normalize_schema_active(schema, active, depth, count, max_depth, max_nodes)
    finally:
        if track:
            active.remove(ident)

def _normalize_schema_active(schema, active, depth, count, max_depth, max_nodes):
    if isinstance(schema, Node):
        if schema.name not in ('schema', 'field'):
            raise ValueError('schema node must be named schema or field')
        result = dict(schema.attributes)
        if schema.children:
            properties = dict(result.get('properties', {}))
            for child in schema.children:
                if not isinstance(child, Node) or child.name != 'field':
                    raise ValueError('schema children must be field nodes')
                name = child.attributes.get('name')
                if not isinstance(name, str):
                    raise ValueError('field requires a string name')
                properties[name] = child
            result['properties'] = properties
        result.pop('name', None)
        # Normalise the node form through the same keyword logic as the mapping
        # form so nested sub-schemas held in attributes (items, additional,
        # attributes, children, prefix_items, $defs, properties) are recursively
        # normalised rather than left as raw nodes the validator cannot traverse.
        return _normalize_schema_keywords(result, active, depth, count, max_depth, max_nodes)
    if isinstance(schema, dict):
        if any((not isinstance(key, str) for key in schema)):
            raise ValueError('schema keyword names must be strings')
        return _normalize_schema_keywords(dict(schema), active, depth, count, max_depth, max_nodes)
    raise TypeError('schema must be a mapping or schema node')

def _normalize_schema_keywords(result, active, depth, count, max_depth, max_nodes):
    """Validate a schema's reserved keywords and recursively normalize every
    nested sub-schema.  Shared by the mapping and node forms so both normalize
    sub-schema keywords identically."""
    dialect = result.get('$schema')
    if dialect not in (None, SCHEMA_DIALECT_2026):
        raise ValueError('unsupported AXON Schema dialect %r' % dialect)
    for key in ('$id', '$ref'):
        if key in result and (not isinstance(result[key], str)):
            raise ValueError('%s must be a string' % key)
    if 'properties' in result:
        if not isinstance(result['properties'], dict):
            raise ValueError('properties must be a mapping')
        if any((not isinstance(key, str) for key in result['properties'])):
            raise ValueError('property names must be strings')
        result['properties'] = {key: _normalize_schema(value, active, depth + 1, count, max_depth, max_nodes) for key, value in result['properties'].items()}
    for key in ('items', 'children', 'attributes'):
        if key in result:
            if not isinstance(result[key], (dict, Node)):
                raise ValueError('%s must be a schema' % key)
            result[key] = _normalize_schema(result[key], active, depth + 1, count, max_depth, max_nodes)
    if 'additional' in result:
        if not isinstance(result['additional'], (bool, dict, Node)):
            raise ValueError('additional must be a boolean or schema')
        if isinstance(result['additional'], (dict, Node)):
            result['additional'] = _normalize_schema(result['additional'], active, depth + 1, count, max_depth, max_nodes)
    if 'prefix_items' in result:
        if not isinstance(result['prefix_items'], (list, tuple, AxonTuple)):
            raise ValueError('prefix_items must be a sequence of schemas')
        result['prefix_items'] = [_normalize_schema(item, active, depth + 1, count, max_depth, max_nodes) for item in result['prefix_items']]
    if '$defs' in result:
        if not isinstance(result['$defs'], dict):
            raise ValueError('$defs must be a mapping')
        if any((not isinstance(key, str) for key in result['$defs'])):
            raise ValueError('$defs names must be strings')
        result['$defs'] = {key: _normalize_schema(value, active, depth + 1, count, max_depth, max_nodes) for key, value in result['$defs'].items()}
    _check_schema_keywords(result)
    return result
_KINDS = {'any', 'null', 'bool', 'boolean', 'int', 'integer', 'float', 'decimal', 'number', 'string', 'bytes', 'binary', 'date', 'time', 'datetime', 'node', 'set', 'map', 'ordered-map', 'mapping', 'list', 'tuple', 'sequence', 'collection', 'link'}
_KEYWORDS = {'$schema', '$id', '$ref', '$defs', 'properties', 'items', 'children', 'attributes', 'additional', 'prefix_items', 'kind', 'const', 'enum', 'minimum', 'maximum', 'exclusive_minimum', 'exclusive_maximum', 'min_length', 'max_length', 'pattern', 'key_pattern', 'min_items', 'max_items', 'name', 'style', 'min_children', 'max_children', 'required', 'title', 'description', '$comment', 'examples', 'default', 'deprecated'}

def _check_schema_keywords(schema):
    unknown_keywords = sorted((key for key in schema if key not in _KEYWORDS))
    if unknown_keywords:
        raise ValueError('unknown schema keyword %r' % unknown_keywords[0])
    expected = schema.get('kind', 'any')
    if isinstance(expected, str):
        kinds = (expected,)
    elif isinstance(expected, (list, tuple, AxonTuple)) and expected and all((isinstance(item, str) for item in expected)):
        kinds = tuple(expected)
    else:
        raise ValueError('kind must be a string or non-empty string sequence')
    unknown = sorted(set(kinds) - _KINDS)
    if unknown:
        raise ValueError('unknown schema kind %r' % unknown[0])
    if 'enum' in schema and (not isinstance(schema['enum'], (list, tuple, AxonTuple, AxonSet))):
        raise ValueError('enum must be a sequence or set')
    if 'required' in schema:
        required = schema['required']
        if not isinstance(required, (list, tuple, AxonTuple)) or any((not isinstance(item, str) for item in required)):
            raise ValueError('required must be a sequence of strings')
        if len(required) != len(set(required)):
            raise ValueError('required must not contain duplicates')
    if 'pattern' in schema and (not isinstance(schema['pattern'], str)):
        raise ValueError('pattern must be a string')
    if 'key_pattern' in schema and (not isinstance(schema['key_pattern'], str)):
        raise ValueError('key_pattern must be a string')
    for key in ('min_length', 'max_length', 'min_items', 'max_items', 'min_children', 'max_children'):
        if key in schema and (isinstance(schema[key], bool) or not isinstance(schema[key], int) or schema[key] < 0):
            raise ValueError('%s must be a non-negative integer' % key)
    for minimum, maximum in (('min_length', 'max_length'), ('min_items', 'max_items'), ('min_children', 'max_children')):
        if minimum in schema and maximum in schema and (schema[minimum] > schema[maximum]):
            raise ValueError('%s cannot exceed %s' % (minimum, maximum))

class _IssueCollector(list):

    def __init__(self, max_issues):
        super().__init__()
        self.max_issues = max_issues
        self.truncated = False

    def append(self, issue):
        reserve = 1 if self.max_issues > 1 else 0
        if len(self) < self.max_issues - reserve:
            super().append(issue)
        else:
            self.truncated = True

    def finish(self):
        if self.truncated and (not any((item.code == 'issue-limit-exceeded' for item in self))):
            if len(self) >= self.max_issues:
                del self[self.max_issues - 1:]
            super().append(ValidationIssue('issue-limit-exceeded', (), 'validation produced more than %d issues' % self.max_issues))

def validate2026(value, schema, *, registry=None, raise_on_error=False, max_depth=128, max_issues=256):
    """Validate *value* against *schema* and return the issues found.

Issues come back in a deterministic order (by data path, then code), capped
at *max_issues*. Pass ``raise_on_error=True`` to raise
:class:`SchemaValidationError` instead of returning them.
    """
    if not isinstance(max_issues, int) or max_issues < 1:
        raise ValueError('max_issues must be a positive integer')
    issues = _IssueCollector(max_issues)
    try:
        schema = normalize_schema(schema, max_depth=max_depth)
        if isinstance(registry, SchemaRegistry2026):
            registry = registry.snapshot()
        else:
            registry = SchemaRegistry2026(registry or {}, max_depth=max_depth).snapshot()
    except (_SchemaResourceError, TypeError, ValueError) as exc:
        code = 'resource-limit-exceeded' if isinstance(exc, _SchemaResourceError) else 'invalid-schema'
        issues.append(ValidationIssue(code, (), str(exc)))
        issues.finish()
        if raise_on_error:
            raise SchemaValidationError(issues)
        return list(issues)
    context = _SchemaContext(schema, registry, max_depth)
    _validate(value, schema, (), (), issues, set(), context)
    issues.finish()
    ordered = _ordered_issues(issues)
    if ordered and raise_on_error:
        raise SchemaValidationError(ordered)
    return ordered

def validation_report2026(value, schema, *, registry=None, max_depth=128, max_issues=256):
    """Validate *value* and return a :class:`ValidationReport`.

The report carries the issues, per-code counts, and whether the issue limit
truncated the result.
    """
    issues = validate2026(value, schema, registry=registry, max_depth=max_depth, max_issues=max_issues)
    counts = tuple(sorted(Counter((item.code for item in issues)).items()))
    return ValidationReport(tuple(issues), counts, any((item.code == 'issue-limit-exceeded' for item in issues)))

def assert_valid2026(value, schema, *, registry=None):
    """Return *value* if it satisfies *schema*.

Raises :class:`SchemaValidationError` otherwise.
    """
    validate2026(value, schema, registry=registry, raise_on_error=True)
    return value

def _issue(issues, code, path, message, schema_path=()):
    issues.append(ValidationIssue(code, path, message, schema_path))

def _ordered_issues(issues):

    def path_key(path):
        return tuple(((type(item).__name__, repr(item)) for item in path))
    unique = {}
    for issue in issues:
        key = (path_key(issue.path), issue.code, path_key(issue.schema_path), issue.message)
        unique.setdefault(key, issue)
    return [unique[key] for key in sorted(unique)]

def _kind(value):
    if value is None:
        return 'null'
    if isinstance(value, bool):
        return 'bool'
    if isinstance(value, int):
        return 'int'
    if isinstance(value, float):
        return 'float'
    if isinstance(value, Decimal):
        return 'decimal'
    if isinstance(value, ScaledDecimal):
        return 'decimal'
    if isinstance(value, str):
        return 'string'
    if isinstance(value, bytes):
        return 'bytes'
    if isinstance(value, NanoTime):
        return 'time'
    if isinstance(value, NanoDateTime):
        return 'datetime'
    from datetime import date, datetime
    # datetime is a subclass of date; classify it first so a real datetime is
    # not mislabelled 'date' (audit H12).
    if isinstance(value, datetime):
        return 'datetime'
    if isinstance(value, date):
        return 'date'
    if isinstance(value, Node):
        return 'node'
    if isinstance(value, AxonSet):
        return 'set'
    from collections import OrderedDict
    if isinstance(value, OrderedDict):
        return 'ordered-map'
    if isinstance(value, dict):
        return 'map'
    if isinstance(value, list):
        return 'list'
    if isinstance(value, (tuple, AxonTuple)):
        return 'tuple'
    if isinstance(value, Link):
        return 'link'
    return type(value).__name__

def _kind_matches(expected, actual):
    aliases = {'integer': {'int'}, 'boolean': {'bool'}, 'binary': {'bytes'}, 'number': {'int', 'float', 'decimal'}, 'mapping': {'map', 'ordered-map'}, 'sequence': {'list', 'tuple'}, 'collection': {'list', 'tuple', 'set', 'map', 'ordered-map'}}
    return expected == actual or actual in aliases.get(expected, ())

class _SchemaContext:

    def __init__(self, root, registry, max_depth):
        self.root = root
        self.registry = registry
        self.max_depth = max_depth
        self.depth = 0

    def resolve(self, reference):
        if not isinstance(reference, str):
            raise KeyError(reference)
        document, marker, fragment = reference.partition('#')
        target = self.root if not document else self.registry.get(document)
        if target is None:
            raise KeyError(reference)
        if marker:
            if fragment and (not fragment.startswith('/')):
                raise KeyError(reference)
            for part in filter(None, fragment.split('/')):
                if re.search('~(?![01])', part):
                    raise KeyError(reference)
                part = part.replace('~1', '/').replace('~0', '~')
                if isinstance(target, (list, tuple)):
                    if not part.isdigit() or len(part) > 18:
                        raise KeyError(reference)
                    index = int(part)
                    if index >= len(target):
                        raise KeyError(reference)
                    target = target[index]
                else:
                    target = target[part]
        return target

def _validate(value, schema, path, schema_path, issues, seen, context):
    if issues.truncated:
        return
    if context.depth >= context.max_depth:
        _issue(issues, 'resource-limit-exceeded', path, 'maximum validation nesting depth exceeded', schema_path)
        return
    marker = (id(value), id(schema))
    if marker in seen:
        return
    seen.add(marker)
    context.depth += 1
    try:
        return _validate_active(value, schema, path, schema_path, issues, seen, context)
    finally:
        context.depth -= 1
        seen.remove(marker)

def _validate_active(value, schema, path, schema_path, issues, seen, context):
    if '$ref' in schema:
        reference = schema['$ref']
        try:
            target = context.resolve(reference)
        except (KeyError, TypeError, ValueError, IndexError):
            _issue(issues, 'unresolved-ref', path, 'unresolved schema reference %r' % reference, schema_path + ('$ref',))
            return
        _validate(value, target, path, schema_path + ('$ref', reference), issues, seen, context)
    expected = schema.get('kind', 'any')
    actual = _kind(value)
    if isinstance(expected, str):
        expected = [expected]
    if 'any' not in expected and (not any((_kind_matches(item, actual) for item in expected))):
        _issue(issues, 'kind', path, 'expected %s, got %s' % ('|'.join(expected), actual), schema_path + ('kind',))
        return
    if 'const' in schema and (not _equal(value, schema['const'])):
        _issue(issues, 'const', path, 'value does not equal required constant', schema_path + ('const',))
    if 'enum' in schema and (not any((_equal(value, item) for item in schema['enum']))):
        _issue(issues, 'enum', path, 'value is not in the permitted enumeration', schema_path + ('enum',))
    if actual in ('int', 'float', 'decimal'):
        numeric = value.value if isinstance(value, ScaledDecimal) else value
        try:
            if 'minimum' in schema and numeric < schema['minimum']:
                _issue(issues, 'minimum', path, 'value is below minimum', schema_path + ('minimum',))
            if 'maximum' in schema and numeric > schema['maximum']:
                _issue(issues, 'maximum', path, 'value is above maximum', schema_path + ('maximum',))
            if 'exclusive_minimum' in schema and numeric <= schema['exclusive_minimum']:
                _issue(issues, 'exclusive-minimum', path, 'value must be greater than exclusive minimum')
            if 'exclusive_maximum' in schema and numeric >= schema['exclusive_maximum']:
                _issue(issues, 'exclusive-maximum', path, 'value must be less than exclusive maximum')
        except (TypeError, ValueError, InvalidOperation):
            _issue(issues, 'schema-comparison', path, 'numeric constraint is not comparable with the value', schema_path)
    if actual == 'string':
        length = len(value)
        if length < schema.get('min_length', 0):
            _issue(issues, 'min-length', path, 'string is too short')
        if 'max_length' in schema and length > schema['max_length']:
            _issue(issues, 'max-length', path, 'string is too long')
        if 'pattern' in schema:
            try:
                matched = _safe_regex_search(schema['pattern'], value)
            except (TypeError, ValueError, re.error) as exc:
                _issue(issues, 'pattern', path, 'unsafe or invalid pattern: %s' % exc)
            else:
                if not matched:
                    _issue(issues, 'pattern', path, 'string does not match pattern')
    if actual in ('list', 'tuple', 'set'):
        items = value.items if isinstance(value, AxonSet) else list(value)
        if len(items) < schema.get('min_items', 0):
            _issue(issues, 'min-items', path, 'container has too few items')
        if 'max_items' in schema and len(items) > schema['max_items']:
            _issue(issues, 'max-items', path, 'container has too many items')
        prefix = schema.get('prefix_items', [])
        for index, subschema in enumerate(prefix):
            if index < len(items):
                _validate(items[index], subschema, path + (index,), schema_path + ('prefix_items', index), issues, seen, context)
        if 'items' in schema:
            for index, item in enumerate(items[len(prefix):], len(prefix)):
                _validate(item, schema['items'], path + (index,), schema_path + ('items',), issues, seen, context)
    if actual in ('map', 'ordered-map'):
        _validate_mapping(value, schema, path, schema_path, issues, seen, context)
    if actual == 'node':
        if 'name' in schema and value.name != schema['name']:
            _issue(issues, 'node-name', path, 'expected node %r' % schema['name'])
        if 'style' in schema and value.style != schema['style']:
            _issue(issues, 'node-style', path, 'expected node body style %r' % schema['style'])
        attr_schema = schema.get('attributes', {'kind': 'map'})
        if 'properties' in schema and 'properties' not in attr_schema:
            attr_schema = dict(attr_schema, properties=schema['properties'])
        _validate_mapping(value.attributes, attr_schema, path + ('@',), schema_path + ('attributes',), issues, seen, context)
        child_schema = schema.get('children')
        if child_schema:
            for index, child in enumerate(value.children):
                _validate(child, child_schema, path + (index,), schema_path + ('children',), issues, seen, context)
        if len(value.children) < schema.get('min_children', 0):
            _issue(issues, 'min-children', path, 'node has too few children')
        if 'max_children' in schema and len(value.children) > schema['max_children']:
            _issue(issues, 'max-children', path, 'node has too many children')

def _validate_mapping(value, schema, path, schema_path, issues, seen, context):
    properties = schema.get('properties', {})
    required = schema.get('required', [])
    for key in sorted(required, key=lambda item: (type(item).__name__, repr(item))):
        if key not in value:
            _issue(issues, 'required', path + (key,), 'required key is missing', schema_path + ('required',))
    key_pattern = schema.get('key_pattern')
    if isinstance(key_pattern, str):
        for key in sorted(value, key=lambda item: (type(item).__name__, repr(item))):
            if not isinstance(key, str):
                _issue(issues, 'key-pattern', path + (key,),
                       'mapping key must be a string to match key_pattern',
                       schema_path + ('key_pattern',))
                continue
            try:
                matched = _safe_regex_search(key_pattern, key)
            except (TypeError, ValueError, re.error) as exc:
                _issue(issues, 'key-pattern', path + (key,),
                       'unsafe or invalid key_pattern: %s' % exc,
                       schema_path + ('key_pattern',))
                break
            if not matched:
                _issue(issues, 'key-pattern', path + (key,),
                       'mapping key does not match key_pattern',
                       schema_path + ('key_pattern',))
    for key, item in sorted(value.items(), key=lambda pair: (type(pair[0]).__name__, repr(pair[0]))):
        if key in properties:
            _validate(item, properties[key], path + (key,), schema_path + ('properties', key), issues, seen, context)
        elif schema.get('additional', True) is False:
            _issue(issues, 'additional', path + (key,), 'additional key is not permitted')
        elif isinstance(schema.get('additional'), dict):
            _validate(item, schema['additional'], path + (key,), schema_path + ('additional',), issues, seen, context)

def _equal(left, right):
    if type(left) is not type(right):
        return False
    try:
        return bool(left == right)
    except RecursionError:
        # A pathologically self-referential value cannot equal the acyclic
        # constants a normalised schema can hold; treat it as unequal rather
        # than letting the recursion escape the validator (audit H12).
        return False

def _safe_regex_search(pattern, value):
    """Run a deliberately linear-time subset of Python regular expressions."""
    if not isinstance(pattern, str):
        raise TypeError('pattern must be a string')
    if len(pattern) > 1024:
        raise ValueError('pattern exceeds 1024 characters')
    parsed = sre_parse.parse(pattern)
    repeat_ops = {op for name in ('MAX_REPEAT', 'MIN_REPEAT', 'POSSESSIVE_REPEAT') if (op := getattr(sre_parse, name, None)) is not None}
    unsafe_ops = {op for name in ('GROUPREF', 'GROUPREF_EXISTS', 'GROUPREF_IGNORE', 'ASSERT', 'ASSERT_NOT') if (op := getattr(sre_parse, name, None)) is not None}
    branch_op = getattr(sre_parse, 'BRANCH')
    subpattern_op = getattr(sre_parse, 'SUBPATTERN')
    atomic_op = getattr(sre_parse, 'ATOMIC_GROUP', None)
    beginning_ops = {op for name in ('AT_BEGINNING', 'AT_BEGINNING_STRING') if (op := getattr(sre_parse, name, None)) is not None}
    at_op = getattr(sre_parse, 'AT')

    def safe(subpattern, inside_repeat=False):
        for operation, argument in subpattern:
            if operation in unsafe_ops:
                return False
            if operation in repeat_ops:
                if inside_repeat:
                    return False
                child = argument[-1]
                if any((op == branch_op for op, _ in child)):
                    return False
                if not safe(child, True):
                    return False
            elif operation == branch_op:
                if inside_repeat:
                    return False
                if not all((safe(branch, inside_repeat) for branch in argument[1])):
                    return False
            elif operation == subpattern_op:
                if not safe(argument[-1], inside_repeat):
                    return False
            elif atomic_op is not None and operation == atomic_op:
                if not safe(argument, inside_repeat):
                    return False
        return True
    if not safe(parsed):
        raise ValueError('pattern uses a potentially superlinear construct')
    variable_repeats = []

    def collect(subpattern):
        for operation, argument in subpattern:
            if operation in repeat_ops:
                minimum, maximum, child = argument
                if minimum != maximum:
                    variable_repeats.append(operation)
                collect(child)
            elif operation == branch_op:
                for branch in argument[1]:
                    collect(branch)
            elif operation == subpattern_op:
                collect(argument[-1])
            elif atomic_op is not None and operation == atomic_op:
                collect(argument)

    def fixed_work(subpattern):
        """Bound worst-case fixed-repeat work for one unanchored start."""
        work = 0
        for operation, argument in subpattern:
            if operation in repeat_ops:
                minimum, maximum, child = argument
                child_work = max(1, fixed_work(child))
                work += child_work if minimum != maximum else int(maximum) * child_work
            elif operation == branch_op:
                work += sum((fixed_work(branch) for branch in argument[1]))
            elif operation == subpattern_op:
                work += fixed_work(argument[-1])
            elif atomic_op is not None and operation == atomic_op:
                work += fixed_work(argument)
            else:
                work += 1
            if work > 4096:
                return 4097
        return work

    collect(parsed)
    anchored = bool(parsed and parsed[0][0] == at_op and (parsed[0][1] in beginning_ops))
    if len(variable_repeats) > 1 or (variable_repeats and (not anchored)):
        raise ValueError('variable quantifiers require a start anchor and one repeat path')
    if not anchored and fixed_work(parsed) > 4096:
        raise ValueError('pattern uses a potentially superlinear construct')
    return re.search(pattern, value) is not None
__all__ = ['SCHEMA_DIALECT_2026', 'ValidationIssue', 'ValidationReport', 'SchemaValidationError', 'SchemaRegistry2026', 'load_schema2026', 'normalize_schema', 'validate2026', 'validation_report2026', 'assert_valid2026']
