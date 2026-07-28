"""AXON 2026 edition engine for the original Python pyaxon project.

This module is intentionally pure Python.  It lives beside the historical
Cython reader rather than replacing it, so applications can migrate one
document at a time and compare the two editions empirically.
"""
from __future__ import annotations
from base64 import b32decode, b32encode, b64decode, b64encode
from collections import OrderedDict
from dataclasses import dataclass, field, replace
from datetime import date, datetime, time, timedelta, timezone
from decimal import Decimal, InvalidOperation
from difflib import SequenceMatcher
import math
import os
import re
import tempfile
from hashlib import sha256
from urllib.parse import urlsplit
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError
from zoneinfo import TZPATH

class Axon2026Error(ValueError):

    """A parse, write, or resource error.

Carries a stable ``category`` id plus the UTF-8 byte offset and 1-based
line/column of the failure. Match on ``category``; message text is not part
of the contract.
    """
    def __init__(self, category, message, offset=0, line=1, column=1, end_offset=None, secondary_span=None):
        super().__init__(message)
        self.category = category
        self.offset = offset
        self.start_offset = offset
        self.end_offset = offset if end_offset is None else end_offset
        self.line = line
        self.column = column
        self.secondary_span = secondary_span

    def __str__(self):
        return '%s at %d:%d: %s' % (self.category, self.line, self.column, super().__str__())

@dataclass(frozen=True)
class Options:
    """Profile switches, legacy-compatibility flags, and section-16 resource
limits governing one parse or write.

The defaults are strict Core AXON; :meth:`compat` builds the documented
legacy-reader bundle.
    """
    graph: bool = False
    doc_link: bool = False
    scale_significant: bool = False
    canonical_verify: bool = False
    compatibility: bool = False
    bare_temporals: bool = False
    raw_fraction: bool = False
    legacy_binding: bool = False
    indented_bodies: bool = False
    lax_numbers: bool = False
    lax_temporals: bool = False
    legacy_escapes: bool = False
    legacy_binary: bool = False
    hash_underscore_comment: bool = False
    last_wins: bool = False
    set_dedup: bool = False
    undefined_ref_sentinel: bool = False
    python_name_classes: bool = False
    lax_strings: bool = False
    allow_header_profiles: bool = True
    tz_names: bool = False
    numeric_separators: bool = True
    max_depth: int = 128
    max_values: int = 1048576
    max_string: int = 16 * 1024 * 1024
    max_binary: int = 16 * 1024 * 1024
    max_number: int = 1024
    max_anchors: int = 65536
    max_references: int = 65536
    max_label: int = 1024
    max_bare_name: int = 1024
    max_quoted_name: int = 4096
    max_temporal: int = 128
    max_container: int = 1048576
    max_header_require: int = 64
    max_cst_tokens: int = 1048576
    max_input: int = 64 * 1024 * 1024

    @classmethod
    def compat(cls, **overrides):
        """Build the documented legacy-reader bundle for pre-2026 documents."""
        values = dict(compatibility=True, bare_temporals=True, raw_fraction=True, legacy_binding=True, indented_bodies=True, lax_numbers=True, lax_temporals=True, legacy_escapes=True, legacy_binary=True, hash_underscore_comment=True, last_wins=True, set_dedup=True, undefined_ref_sentinel=True, python_name_classes=True, lax_strings=True)
        values.update(overrides)
        return cls(**values)

@dataclass
class Node:
    """A named node: a name, a body style (``unit``/``brace``/``tuple``/``list``),
ordered attributes, and children.
    """
    name: str
    style: str = 'unit'
    attributes: OrderedDict = field(default_factory=OrderedDict)
    children: list = field(default_factory=list)

    def __repr__(self):
        return 'Node(%r, %s, attrs=%r, children=%r)' % (self.name, self.style, self.attributes, self.children)

@dataclass
class AxonSet:
    """An AXON set.

Membership uses AXON's kind-aware equality, so ``1`` and ``1.0`` are
distinct elements rather than duplicates.
    """
    items: list = field(default_factory=list)

    def __iter__(self):
        return iter(self.items)

    def __len__(self):
        return len(self.items)

    def __contains__(self, value):
        return any((_value_equal(value, item) for item in self.items))

@dataclass
class AxonTuple:
    """Tuple-kind graph value whose storage can represent a self-cycle."""
    items: list = field(default_factory=list)

    def __iter__(self):
        return iter(self.items)

    def __len__(self):
        return len(self.items)

    def __getitem__(self, index):
        return self.items[index]

    def __eq__(self, other):
        if isinstance(other, AxonTuple):
            return self.items == other.items
        if isinstance(other, tuple):
            return self.items == list(other)
        return False

@dataclass(frozen=True)
class Link:
    """A document link: ``kind`` is ``'cid'`` for a content-addressed target or
``'uri'`` for a URI target.
    """
    kind: str
    target: str

@dataclass(frozen=True)
class MigrationEvent:
    """One change a migration made, with its code, message, and source byte offset."""
    code: str
    message: str
    offset: int = 0

@dataclass(frozen=True)
class MigrationEdit:
    """A non-overlapping UTF-8 byte edit produced by selective migration."""
    code: str
    start: int
    end: int
    replacement: str

class MigrationResult(str):
    """String-compatible migration output with an immutable event report."""

    def __new__(cls, text, events=(), edits=(), mode='canonical'):
        result = str.__new__(cls, text)
        result.events = tuple(events)
        result.edits = tuple(edits)
        result.mode = mode
        return result

@dataclass(frozen=True)
class ScaledDecimal:
    """A decimal that keeps the scale it was written with, so ``1.0D`` and
``1.00D`` stay distinct under the scale-significant profile.
    """
    value: Decimal
    exponent: int

    @classmethod
    def from_decimal(cls, value):
        """Wrap a :class:`decimal.Decimal`, keeping its written scale significant."""
        return cls(value, value.as_tuple().exponent)

@dataclass(frozen=True)
class NanoTime:
    """A time of day at nanosecond resolution.

The zone is retained as local, ``Z``, or an explicit offset -- ``Z`` and
``+00:00`` are not collapsed together.
    """
    hour: int
    minute: int
    second: int = 0
    nanosecond: int = 0
    offset_minutes: int | None = None
    zulu: bool = False
    fraction_digits: int | None = None

    def __post_init__(self):
        if not (0 <= self.hour <= 23 and 0 <= self.minute <= 59 and (0 <= self.second <= 59)):
            raise ValueError('time field out of range')
        if not 0 <= self.nanosecond <= 999999999:
            raise ValueError('nanosecond out of range')
        if self.fraction_digits is not None and (not 1 <= self.fraction_digits <= 9):
            raise ValueError('fraction digit count out of range')
        if self.offset_minutes is not None and abs(self.offset_minutes) > 23 * 60 + 59:
            raise ValueError('offset out of range')

    def to_time(self):
        """Convert to :class:`datetime.time`, truncating nanoseconds to microseconds."""
        from datetime import timedelta
        tz = None if self.offset_minutes is None else timezone(timedelta(minutes=self.offset_minutes))
        return time(self.hour, self.minute, self.second, self.nanosecond // 1000, tz)

@dataclass(frozen=True)
class NanoDateTime:
    """A date plus a nanosecond time, optionally carrying an RFC 9557 named zone."""
    calendar_date: date
    clock_time: NanoTime
    zone_name: str | None = None

    def to_datetime(self):
        """Convert to :class:`datetime.datetime`, truncating nanoseconds to microseconds."""
        return datetime.combine(self.calendar_date, self.clock_time.to_time())

    def to_zoned_datetime(self):
        """Convert to an aware :class:`datetime.datetime` in the named zone.

``fold`` is set so an ambiguous local time keeps the offset it was written
with.
        """
        if self.zone_name is None:
            return self.to_datetime()
        if self.clock_time.offset_minutes is None:
            raise ValueError('named time zone requires a numeric offset')
        local = datetime(self.calendar_date.year, self.calendar_date.month, self.calendar_date.day, self.clock_time.hour, self.clock_time.minute, self.clock_time.second, self.clock_time.nanosecond // 1000)
        instant = (local - timedelta(minutes=self.clock_time.offset_minutes)).replace(tzinfo=timezone.utc)
        return instant.astimezone(ZoneInfo(self.zone_name))

@dataclass(frozen=True)
class _Ref:
    label: str

class _PendingAnchor:
    pass
_PENDING_ANCHOR = _PendingAnchor()

class _Undefined:

    def __repr__(self):
        return '??'
UNDEFINED = _Undefined()
DEFAULT_CONSTANTS = {'Inf': math.inf, 'NegInf': -math.inf, 'NaN': math.nan, 'NaND': Decimal('NaN')}
_RESERVED_BAREWORDS = frozenset(('null', 'true', 'false'))
# Above this input size, selective migration skips its superlinear diff and
# returns the linear canonical rewrite instead (audit H5).
_SELECTIVE_MIGRATION_LIMIT = 65536

def _utf8_length(text):
    """Return encoded length or raise the edition's stable Unicode error."""
    try:
        return len(text.encode('utf-8'))
    except UnicodeEncodeError as exc:
        prefix = text[:exc.start]
        line = prefix.count('\n') + 1
        column = len(prefix.rsplit('\n', 1)[-1]) + 1
        try:
            offset = len(prefix.encode('utf-8'))
        except UnicodeEncodeError:
            offset = 0
        raise Axon2026Error('invalid-unicode', 'source contains a non-scalar Unicode value', offset, line, column) from exc
_NUMBER = re.compile('-?[0-9](?:_?[0-9])*(?:\\.(?:[0-9](?:_?[0-9])*)?)?(?:[eE][+-]?[0-9](?:_?[0-9])*)?[dD]?')
_DATE = re.compile('(?P<y>[0-9]{4})-(?P<m>[0-9]{2})-(?P<d>[0-9]{2})$')
_DATE_LAX = re.compile('(?P<y>[0-9]{1,4})-(?P<m>[0-9]{1,2})-(?P<d>[0-9]{1,2})$')
_TIME = re.compile('(?P<h>[0-9]{1,2}):(?P<m>[0-9]{2})(?::(?P<s>[0-9]{2})(?:\\.(?P<f>[0-9]{1,9}))?)?(?P<z>Z|[+-][0-9]{2}:[0-9]{2})?$')
_TIME_LAX = re.compile('(?P<h>[0-9]{1,2}):(?P<m>[0-9]{1,2})(?::(?P<s>[0-9]{1,2})(?:\\.(?P<f>[0-9]{1,9}))?)?(?P<z>Z|[+-][0-9]{1,2}(?::?[0-9]{1,2})?)?$')

def loads2026(text, *, options=None, constants=None, _trace=None):
    """Parse an AXON 2026 document stream into Python values."""
    options = options or Options()
    if not isinstance(text, str):
        raise TypeError('AXON input must be text')
    if _utf8_length(text) > options.max_input:
        raise Axon2026Error('resource-limit-exceeded', 'input is too large')
    if options.indented_bodies:
        text = _rewrite_indented_bodies(text)
        options = replace(options, indented_bodies=False)
    parser = Parser(text, options, constants or DEFAULT_CONSTANTS, trace=_trace is not None)
    values = parser.resolve(parser.parse_document())
    if _trace is not None:
        _trace.extend(parser.resolved_value_events)
    if parser.options.canonical_verify:
        if parser.options.scale_significant:
            parser.error('unsupported-profile-feature', 'scale-significant is incompatible with canonical verification')
        payload = text[parser.payload_start:]
        if parser.payload_start:
            payload = payload.lstrip(' \t\r\n')
        rendered = dumps2026(values, canonical=True, graph=True)
        if payload != rendered:
            parser.error('unexpected-token', 'document payload is not canonical AXON')
    return values

def load2026(source, *, options=None, constants=None, encoding='utf-8'):
    """Read AXON from a text stream and return the list of values it contains."""
    options = options or Options()
    try:
        if hasattr(source, 'read'):
            text = _read_bounded_text(source, options.max_input)
        else:
            with open(source, 'r', encoding=encoding) as stream:
                text = _read_bounded_text(stream, options.max_input)
    except UnicodeDecodeError as exc:
        raise Axon2026Error('invalid-unicode', 'input is not well-formed %s' % encoding, exc.start, 1, exc.start + 1, exc.end) from exc
    return loads2026(text, options=options, constants=constants)

def iloads2026(text, *, options=None, constants=None):
    """Incrementally parse and yield semantic values from Unicode text.

    Each complete top-level value is resolved and yielded before the following
    value is parsed.  Attribute documents remain atomic because they denote one
    mapping value.  Canonical verification is necessarily document-atomic.
    """
    options = options or Options()
    if not isinstance(text, str):
        raise TypeError('AXON input must be text')
    if _utf8_length(text) > options.max_input:
        raise Axon2026Error('resource-limit-exceeded', 'input is too large')
    if options.canonical_verify:
        yield from loads2026(text, options=options, constants=constants)
        return
    if options.indented_bodies:
        text = _rewrite_indented_bodies(text)
        options = replace(options, indented_bodies=False)
    parser = Parser(text, options, constants or DEFAULT_CONSTANTS)
    parser.skip()
    if not parser.peek():
        return
    save = parser.i
    key = parser.try_key()
    parser.i = save
    if key is not None:
        yield parser.resolve(parser.parse_document())[0]
        return
    first = True
    while True:
        parser.skip()
        if not parser.peek():
            return
        if parser.text.startswith('#_', parser.i):
            parser._discard_values()
            continue
        value = parser.parse_value()
        if first:
            header_probe = [value]
            if parser._consume_header(header_probe):
                first = False
                continue
        first = False
        yield parser.resolve([value])[0]
        parser.skip()
        if parser.peek() == ',':
            if not (parser.options.compatibility or parser.options.legacy_binding):
                parser.error('unexpected-token', 'top-level commas are not permitted in Core')
            parser.i += 1

def iload2026(source, *, options=None, constants=None, encoding='utf-8'):
    """Incrementally read, parse, and yield values from a text source."""
    options = options or Options()
    constants = constants or DEFAULT_CONSTANTS
    close = False
    if hasattr(source, 'read'):
        stream = source
    else:
        stream = open(source, 'r', encoding=encoding)
        close = True
    try:
        if options.canonical_verify or options.indented_bodies:
            text = _read_bounded_text(stream, options.max_input)
            yield from iloads2026(text, options=options, constants=constants)
            return
        buffer = ''
        eof = False
        total_bytes = 0
        anchors = {}
        total_values = 0
        total_references = 0
        stream_options = options
        first = True

        def read_more(size=64 * 1024):
            nonlocal buffer, eof, total_bytes
            allowance = max(1, options.max_input + 1 - total_bytes)
            chunk = stream.read(min(size, allowance))
            if not chunk:
                eof = True
                return False
            if not isinstance(chunk, str):
                raise TypeError('AXON input stream must return text')
            total_bytes += _utf8_length(chunk)
            if total_bytes > options.max_input:
                raise Axon2026Error('resource-limit-exceeded', 'input is too large')
            buffer += chunk
            return True
        read_more()
        while buffer or not eof:
            if not buffer and (not read_more()):
                break
            candidate = Parser(buffer, stream_options, constants)
            candidate.anchors = dict(anchors)
            candidate.count = total_values
            candidate.reference_count = total_references
            try:
                candidate.skip()
                if not candidate.peek():
                    if eof:
                        break
                    read_more()
                    continue
                if first:
                    save = candidate.i
                    key = candidate.try_key()
                    candidate.i = save
                    if key is not None:
                        if not eof:
                            buffer += _read_bounded_text(stream, options.max_input - total_bytes)
                            eof = True
                        values = loads2026(buffer, options=stream_options, constants=constants)
                        if values:
                            yield values[0]
                        return
                if candidate.text.startswith('#_', candidate.i):
                    candidate._discard_values()
                    value = _PENDING_ANCHOR
                else:
                    value = candidate.parse_value()
                candidate.skip()
                consumed = candidate.i
                if consumed == len(buffer) and (not eof):
                    read_more(1)
                    continue
            except Axon2026Error as exc:
                if exc.category == 'unexpected-end' and (not eof):
                    read_more()
                    continue
                raise
            anchors = candidate.anchors
            total_values = candidate.count
            total_references = candidate.reference_count
            buffer = buffer[consumed:]
            if value is _PENDING_ANCHOR:
                continue
            if first:
                header_probe = [value]
                if candidate._consume_header(header_probe):
                    stream_options = candidate.options
                    first = False
                    continue
            first = False
            yield candidate.resolve([value])[0]
    except UnicodeDecodeError as exc:
        raise Axon2026Error('invalid-unicode', 'input is not well-formed %s' % encoding, exc.start, 1, exc.start + 1, exc.end) from exc
    finally:
        if close:
            stream.close()

def _read_bounded_text(stream, remaining):
    chunks = []
    total = 0
    while True:
        chunk = stream.read(min(64 * 1024, remaining + 1 - total))
        if not chunk:
            break
        if not isinstance(chunk, str):
            raise TypeError('AXON input stream must return text')
        chunks.append(chunk)
        total += _utf8_length(chunk)
        if total > remaining:
            raise Axon2026Error('resource-limit-exceeded', 'input is too large')
    return ''.join(chunks)

def dumps2026(values, *, canonical=False, graph=False):
    """Write a value or stream in AXON 2026 compact form."""
    if not isinstance(values, (list, tuple)) or isinstance(values, Node):
        values = [values]
    writer = Writer(canonical=canonical, graph=graph)
    return '\n'.join(writer.write_stream(values))

def dump2026(path, values, *, canonical=False, graph=False, encoding='utf-8'):
    """Write *values* to *path* as AXON text, atomically.

The text is staged in a temporary file, flushed, fsynced, and renamed over
the target, so a failure leaves the original file untouched.
    """
    rendered = dumps2026(values, canonical=canonical, graph=graph)
    _atomic_write_text(path, rendered, encoding)

def canonical2026(value):
    """Return the Canonical AXON bytes for *value*.

Deterministic section-14 output -- sorted map keys, byte-ordered set
elements, decimal normal form, JCS float layout -- suitable for hashing,
signing, and content identifiers.
    """
    return Writer(canonical=True, graph=True).write(value).encode('utf-8')

def cid2026(value):
    """Return a CIDv1(raw, sha2-256) over canonical AXON bytes."""
    digest = sha256(canonical2026(value)).digest()
    binary = bytes((1, 85, 18, 32)) + digest
    return 'b' + b32encode(binary).decode('ascii').lower().rstrip('=')

def parse_cid2026(text):
    """Validate a CID string and return its raw 32-byte SHA-256 digest."""
    if not isinstance(text, str) or not text.startswith('b'):
        raise ValueError('CID must use lowercase base32 multibase')
    payload = text[1:]
    if not payload or payload != payload.lower() or (not re.fullmatch('[a-z2-7]+', payload)):
        raise ValueError('invalid lowercase base32 CID')
    try:
        binary = b32decode(payload.upper() + '=' * (-len(payload) % 8))
    except Exception as exc:
        raise ValueError('invalid base32 CID') from exc
    if len(binary) != 36 or binary[:4] != bytes((1, 85, 18, 32)):
        raise ValueError('CID must be v1/raw/sha2-256-256')
    canonical_payload = b32encode(binary).decode('ascii').lower().rstrip('=')
    if payload != canonical_payload:
        raise ValueError('non-canonical base32 CID')
    return binary[4:]

def _valid_absolute_uri(target):
    """Validate an ASCII RFC-3986 absolute URI without resolving it."""
    if not isinstance(target, str) or not target:
        return False
    try:
        target.encode('ascii')
    except UnicodeEncodeError:
        return False
    if any((ord(ch) <= 32 or ord(ch) == 127 for ch in target)) or any((ch in '<>"{}|\\^`' for ch in target)):
        return False
    if re.search('%(?![0-9A-Fa-f]{2})', target):
        return False
    if not re.match('^[A-Za-z][A-Za-z0-9+.-]*:', target):
        return False
    try:
        parsed = urlsplit(target)
        if parsed.scheme.lower() in ('http', 'https') and (not parsed.netloc):
            return False
        _ = parsed.port
    except ValueError:
        return False
    return True

def verify_cid2026(cid, value):
    """Check whether *cid* is the content identifier of *value*."""
    try:
        digest = parse_cid2026(cid.target if isinstance(cid, Link) else cid)
    except ValueError:
        return False
    return digest == sha256(canonical2026(value)).digest()

def migrate2026(text, *, graph=False, selective=False):
    """Read documented legacy forms and return Core text plus an event report."""
    if not isinstance(text, str):
        raise TypeError('migration input must be text')
    options = Options.compat(graph=graph)
    if __package__:
        from .cst2026 import parse_cst2026
        document = parse_cst2026(text, options=options)
        if document.diagnostics:
            loads2026(text, options=options)
        values = document.values
    else:
        values = loads2026(text, options=options)
    rendered = dumps2026(values, canonical=True, graph=graph)
    if not selective:
        return MigrationResult(rendered, _migration_events(text, rendered))
    # Selective migration runs a superlinear (SequenceMatcher) diff to derive
    # byte edits; for large inputs fall back to the linear canonical rewrite so
    # a few kilobytes cannot monopolize the process (audit H5).
    if len(text) > _SELECTIVE_MIGRATION_LIMIT:
        return MigrationResult(rendered, _migration_events(text, rendered), (), 'canonical-fallback')
    candidate = _selective_migration_candidate(text, graph=graph)
    try:
        migrated_values = loads2026(candidate, options=Options(graph=graph))
        equivalent = dumps2026(migrated_values, canonical=True, graph=graph) == rendered
    except (Axon2026Error, TypeError, ValueError):
        equivalent = False
    if equivalent:
        output = candidate
        mode = 'selective'
    else:
        output = rendered
        mode = 'canonical-fallback'
    edits = _migration_edits(text, output)
    return MigrationResult(output, _migration_events(text, rendered), edits, mode)

def migrate_selective2026(text, *, graph=False):
    """Migrate legacy AXON while preserving unaffected bytes when safe.

    The result carries byte-addressed ``edits``.  If the local repairs cannot
    be proven semantically equivalent to compatibility parsing, migration
    falls back to the existing canonical rewrite and reports that in ``mode``.
    """
    return migrate2026(text, graph=graph, selective=True)

def apply_migration_edits2026(source, edits):
    """Apply validated, non-overlapping migration edits to UTF-8 text."""
    if not isinstance(source, str):
        raise TypeError('migration source must be text')
    data = source.encode('utf-8')
    ordered = sorted(tuple(edits), key=lambda item: (item.start, item.end))
    previous_end = 0
    for edit in ordered:
        if not isinstance(edit, MigrationEdit):
            raise TypeError('migration edits must be MigrationEdit values')
        if edit.start < previous_end or edit.start < 0 or edit.end < edit.start:
            raise ValueError('migration edits overlap or have invalid bounds')
        if edit.end > len(data):
            raise ValueError('migration edit exceeds source length')
        _require_utf8_boundary(data, edit.start, 'migration edit start')
        _require_utf8_boundary(data, edit.end, 'migration edit end')
        try:
            edit.replacement.encode('utf-8')
        except UnicodeEncodeError as exc:
            raise ValueError('migration replacement is not scalar Unicode') from exc
        previous_end = edit.end
    for edit in reversed(ordered):
        data = data[:edit.start] + edit.replacement.encode('utf-8') + data[edit.end:]
    return data.decode('utf-8')

def _require_utf8_boundary(data, offset, label):
    try:
        data[:offset].decode('utf-8')
    except UnicodeDecodeError as exc:
        raise ValueError('%s is not a UTF-8 scalar boundary' % label) from exc

def _selective_migration_candidate(source, *, graph):
    from .cst2026 import migrate_indented2026, parse_cst2026
    options = Options.compat(graph=graph)
    candidate = migrate_indented2026(source, options=options)
    document = parse_cst2026(candidate, options=options)
    edits = []
    bare_temporal = re.compile('(?:[0-9]{1,5}-[0-9]{1,2}-[0-9]{1,2}(?:T[0-9].*)?|[0-9]{1,2}:[0-9].*)$')
    leading_zero = re.compile('-?0[0-9]+$')
    lax_number = re.compile('(?:\\+[0-9][0-9_]*(?:\\.[0-9_]*)?(?:[eE][+-]?[0-9_]+)?|-?[0-9][0-9_]*\\.)$')
    for token in document.tokens:
        rewrite = False
        if token.kind == 'comment' and token.text.startswith('#_'):
            # `#_` opened a comment in the legacy grammar but is Core's discard
            # token. Inserting one byte preserves the complete comment while
            # removing the lexical ambiguity (section 19.3.1).
            edits.append(MigrationEdit(
                'hash-underscore-comment', token.span.start + 1,
                token.span.start + 1, ' '))
            continue
        if token.kind in ('temporal', 'binary'):
            rewrite = True
        elif token.kind == 'atom' and (
                bare_temporal.fullmatch(token.text)
                or leading_zero.fullmatch(token.text)
                or lax_number.fullmatch(token.text)):
            rewrite = True
        elif token.kind in ('string', 'quoted-name'):
            rewrite = '\\' in token.text or '\n' in token.text or '\r' in token.text
        if not rewrite:
            continue
        try:
            values = loads2026(token.text, options=options)
            if len(values) != 1:
                continue
            replacement = dumps2026(values[0])
        except (Axon2026Error, TypeError, ValueError):
            continue
        if replacement != token.text:
            edits.append(MigrationEdit('token-normalization', token.span.start, token.span.end, replacement))
    if edits:
        candidate = apply_migration_edits2026(candidate, edits)
    return candidate

def _migration_edits(source, output):
    if source == output:
        return ()
    source_offsets = [0]
    for character in source:
        source_offsets.append(source_offsets[-1] + len(character.encode('utf-8')))
    edits = []
    matcher = SequenceMatcher(None, source, output, autojunk=False)
    for operation, left_start, left_end, right_start, right_end in matcher.get_opcodes():
        if operation == 'equal':
            continue
        edits.append(MigrationEdit('selective-rewrite', source_offsets[left_start], source_offsets[left_end], output[right_start:right_end]))
    if apply_migration_edits2026(source, edits) != output:
        raise AssertionError('migration edit derivation did not reproduce output')
    return tuple(edits)

def _migration_events(source, rendered):
    events = []

    def add(code, message, character_offset=0, *, byte_offset=None):
        if byte_offset is None:
            byte_offset = len(source[:character_offset].encode('utf-8'))
        events.append(MigrationEvent(code, message, byte_offset))
    if _rewrite_indented_bodies(source) != source:
        add('indented-body', 'rewrote an indentation-bound body with Core braces')
    for match in re.finditer('\\\\\\r?\\n', source):
        add('legacy-continuation', 'preserved the baseline continuation value with Core escapes', match.start())
    for match in re.finditer("'(?:\\\\.|[^'])*[\\r\\n](?:\\\\.|[^'])*'", source):
        add('quoted-name-newline', 'escaped a literal newline in a quoted name', match.start())
    # Detect token migrations on tokens, not the raw source. The former regexes
    # could report the `00` fields inside a temporal (or digits in a string or
    # comment) as independent leading-zero numbers.
    try:
        if not __package__:
            document = None
        else:
            from .cst2026 import parse_cst2026
            document = parse_cst2026(source, options=Options.compat())
    except (Axon2026Error, TypeError, ValueError):
        document = None
    if document is not None:
        bare_temporal = re.compile('(?:[0-9]{1,5}-[0-9]{1,2}-[0-9]{1,2}(?:T[0-9].*)?|[0-9]{1,2}:[0-9].*)$')
        leading_zero = re.compile('-?0[0-9]+$')
        trailing_point = re.compile('[+-]?[0-9][0-9_]*\\.$')
        for token in document.tokens:
            if token.kind == 'comment' and token.text.startswith('#_'):
                add('hash-underscore-comment',
                    'separated the legacy #_ comment opener from Core discard',
                    byte_offset=token.span.start)
            if token.kind == 'atom' and bare_temporal.fullmatch(token.text):
                add('bare-temporal',
                    'added the Core temporal marker and field padding',
                    byte_offset=token.span.start)
            if token.kind == 'atom' and leading_zero.fullmatch(token.text):
                add('leading-zero-number', 'removed legacy leading zeros',
                    byte_offset=token.span.start)
            if token.kind == 'atom' and trailing_point.fullmatch(token.text):
                add('trailing-point-number',
                    'added a fractional digit to a trailing-point number',
                    byte_offset=token.span.start)
            if token.kind == 'binary' and (
                    len(token.text) < 2 or not token.text.endswith('|')):
                add('legacy-binary', 'closed a legacy binary literal',
                    byte_offset=token.span.start)
            if token.kind in ('temporal', 'atom'):
                fraction = re.search(r'\.([0-9]+)', token.text)
                if fraction is not None and len(fraction.group(1)) != 6:
                    add('temporal-fraction',
                        'normalized a legacy temporal fraction to six digits',
                        byte_offset=token.span.start + len(
                            token.text[:fraction.start(1)].encode('utf-8')))
    try:
        loads2026(source, options=Options.compat(
            last_wins=False, set_dedup=False))
    except Axon2026Error as exc:
        if exc.category == 'duplicate-key':
            add('duplicate-key', 'kept the last duplicate key with a warning',
                byte_offset=exc.offset)
        elif exc.category == 'duplicate-set-element':
            add('duplicate-set-element',
                'dropped a duplicate set element with a warning',
                byte_offset=exc.offset)
    if re.search('(?<![&*A-Za-z0-9_/])[A-Za-z_][\\w/]*\\s*[\\[(]', source):
        add('legacy-binding', 'preserved the historical pending-name value shape')
    if rendered != source:
        add('core-canonicalization', 'emitted one strict deterministic Core spelling')
    return events

def _atomic_write_text(path, text, encoding):
    """Replace *path* only after encoding and durable temporary-file output."""
    path = os.fspath(path)
    directory = os.path.dirname(os.path.abspath(path)) or os.curdir
    descriptor, temporary = tempfile.mkstemp(prefix='.axon-', suffix='.tmp', dir=directory)
    try:
        with os.fdopen(descriptor, 'w', encoding=encoding, newline='\n') as stream:
            stream.write(text)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    except BaseException:
        try:
            os.unlink(temporary)
        except OSError:
            pass
        raise

def _rewrite_indented_bodies(source):
    """Insert Core braces around historical indentation-bound node bodies."""
    lines = source.splitlines(keepends=True)
    significant = []
    for index, line in enumerate(lines):
        content = line.lstrip(' \t')
        if content.strip() and (not content.startswith('#')):
            significant.append((index, _indent_width(line), content.rstrip('\r\n')))
    following = {significant[pos][0]: significant[pos + 1] if pos + 1 < len(significant) else None for pos in range(len(significant))}
    output = []
    stack = []
    for index, line in enumerate(lines):
        stripped = line.lstrip(' \t')
        if stripped.strip() and (not stripped.startswith('#')):
            indent = _indent_width(line)
            while stack and indent <= stack[-1]:
                output.append(' ' * stack.pop() + '}\n')
            next_line = following.get(index)
            code = stripped.rstrip('\r\n')
            if _indented_name(code) and next_line is not None and (next_line[1] > indent):
                newline = '\r\n' if line.endswith('\r\n') else '\n'
                output.append(line[:len(line) - len(stripped)] + code + '{' + newline)
                stack.append(indent)
                continue
        output.append(line)
    while stack:
        output.append(' ' * stack.pop() + '}\n')
    return ''.join(output)

def _indent_width(line):
    width = 0
    for ch in line:
        if ch == ' ':
            width += 1
        elif ch == '\t':
            width = (width // 8 + 1) * 8
        else:
            break
    return width

def _indented_name(text):
    if text.endswith(':'):
        text = text[:-1].rstrip()
    return bool(text) and _bare_name(text, namespace=True)

def verify_canonical2026(text):
    """Return True only when *text* is already canonical AXON 2026."""
    values = loads2026(text, options=Options(graph=True))
    rendered = dumps2026(values, canonical=True, graph=True)
    return rendered == text

def tzdb_version2026():
    """Return the tzdb provider/version used for RFC 9557 validation."""
    try:
        from importlib.metadata import version
        return 'tzdata:' + version('tzdata')
    except Exception:
        pass
    for directory in TZPATH:
        path = os.path.join(directory, 'tzdata.zi')
        try:
            with open(path, 'r', encoding='ascii') as stream:
                first = stream.readline().strip()
            if first.startswith('# version '):
                return 'system:' + first.split()[-1]
        except OSError:
            continue
    return 'system:unknown'

class Parser:

    """The AXON 2026 recursive-descent parser.

Every production is bounded by the :class:`Options` limits, so nesting
depth, value counts, and literal sizes are all capped.
    """
    def __init__(self, text, options, constants, trace=False):
        if not isinstance(text, str):
            raise TypeError('AXON 2026 input must be Unicode text')
        self.text = text
        self.n = len(text)
        self.i = 0
        self.depth = 0
        self.count = 0
        self.options = options
        self.constants = constants
        self.anchors = {}
        self.references = []
        self.reference_count = 0
        self.enabled_profiles = set()
        self.payload_start = 0
        self.trace = trace
        self.value_events = []
        self.resolved_value_events = []

    def error(self, category, message):
        prefix = self.text[:self.i]
        line = prefix.count('\n') + 1
        column = len(prefix.rsplit('\n', 1)[-1]) + 1
        raise Axon2026Error(category, message, len(prefix.encode('utf-8')), line, column)

    def peek(self, offset=0):
        j = self.i + offset
        return self.text[j] if j < self.n else ''

    def take(self):
        if self.i >= self.n:
            return ''
        ch = self.text[self.i]
        self.i += 1
        return ch

    def skip(self):
        while True:
            while self.peek() and self.peek() in ' \t\r\n':
                self.i += 1
            if self.peek() == '#' and (self.peek(1) != '_' or self.options.hash_underscore_comment or self.options.compatibility):
                while self.peek() and self.take() not in '\r\n':
                    pass
                continue
            break

    def enter(self):
        self.depth += 1
        if self.depth > self.options.max_depth:
            self.error('resource-limit-exceeded', 'maximum nesting depth exceeded')

    def leave(self):
        self.depth -= 1

    def parsed(self, value):
        self.count += 1
        if self.count > self.options.max_values:
            self.error('resource-limit-exceeded', 'maximum value count exceeded')
        return value

    def parse_document(self):
        self.skip()
        if not self.peek():
            return []
        save = self.i
        key = self.try_key()
        if key is not None:
            result = OrderedDict()
            self._put(result, key, self.parse_value())
            while True:
                self.separator(top=True)
                if not self.peek():
                    return [result]
                if self.text.startswith('#_', self.i):
                    self._discard_pairs(self.try_key, 'attribute document')
                    continue
                key = self.try_key()
                if key is None:
                    self.error('unexpected-token', 'attribute document cannot mix pairs and values')
                self._put(result, key, self.parse_value())
        self.i = save
        values = []
        while True:
            self.skip()
            if not self.peek():
                break
            if self.text.startswith('#_', self.i):
                self._discard_values()
                continue
            values.append(self.parse_value())
            if len(values) == 1:
                self._consume_header(values)
            self.skip()
            if self.peek() == ',':
                if not (self.options.compatibility or self.options.legacy_binding):
                    self.error('unexpected-token', 'top-level commas are not permitted in Core')
                self.i += 1
        return values

    def _consume_header(self, values):
        if not values or not isinstance(values[0], Node) or values[0].name != 'axon':
            return False
        header = values[0]
        edition = header.attributes.get('edition')
        if edition not in (None, '2026'):
            self.error('unsupported-profile-feature', 'unsupported AXON edition')
        required = header.attributes.get('require', [])
        if not isinstance(required, list) or any((not isinstance(name, str) for name in required)):
            self.error('unsupported-profile-feature', 'header require must be a list of strings')
        if len(required) > self.options.max_header_require:
            self.error('resource-limit-exceeded', 'header require list is too long')
        supported = {'graph', 'doc-link', 'canonical-verify', 'compat', 'scale-significant', 'cst'}
        supported.add('tz-names')
        for name in required:
            if name not in supported:
                self.error('unsupported-profile-feature', 'unsupported profile %r' % name)
            self.enabled_profiles.add(name)
        if required and (not self.options.allow_header_profiles):
            self.error('unsupported-profile-feature', 'caller policy forbids header-enabled profiles: %s' % ', '.join(sorted(required)))
        if 'graph' in self.enabled_profiles and (not self.options.graph):
            self.options = replace(self.options, graph=True)
        if 'doc-link' in self.enabled_profiles and (not self.options.doc_link):
            self.options = replace(self.options, doc_link=True)
        if 'scale-significant' in self.enabled_profiles and (not self.options.scale_significant):
            self.options = replace(self.options, scale_significant=True)
        if 'canonical-verify' in self.enabled_profiles and (not self.options.canonical_verify):
            self.options = replace(self.options, canonical_verify=True)
        if self.options.canonical_verify and self.options.scale_significant:
            self.error('unsupported-profile-feature', 'scale-significant and canonical-verify are incompatible')
        self.payload_start = self.i
        if 'compat' in self.enabled_profiles and (not self.options.compatibility):
            self.options = replace(self.options, compatibility=True, bare_temporals=True, raw_fraction=True, legacy_binding=True, indented_bodies=True, lax_numbers=True, lax_temporals=True, legacy_escapes=True, legacy_binary=True, hash_underscore_comment=True, last_wins=True, set_dedup=True, undefined_ref_sentinel=True, python_name_classes=True, lax_strings=True)
        if 'tz-names' in self.enabled_profiles and (not self.options.tz_names):
            self.options = replace(self.options, tz_names=True)
        values.pop(0)
        return True

    def separator(self, close='', top=False):
        self.skip()
        if self.peek() == ',':
            self.i += 1
            self.skip()
            if self.peek() == ',':
                self.error('unexpected-token', 'consecutive commas are not allowed')
            if not top and close and (self.peek() == close):
                return

    def _discard_count(self):
        count = 0
        while self.text.startswith('#_', self.i):
            count += 1
            self.i += 2
            self.skip()
        return count

    def _discard_values(self):
        for _ in range(self._discard_count()):
            self.parse_value()
            self.skip()

    def _discard_pairs(self, key_parser, context):
        for _ in range(self._discard_count()):
            key = key_parser()
            if key is None:
                self.error('unexpected-token', 'discard in %s requires a pair' % context)
            self.parse_value()
            self.skip()

    def parse_value(self):
        self.skip()
        start = self.i

        def done(value):
            value = self.parsed(value)
            if self.trace:
                self.value_events.append((start, self.i, value))
            return value
        ch = self.peek()
        if not ch:
            self.error('unexpected-end', 'expected a value')
        if ch == '[':
            return done(self.parse_bracket())
        if ch == '{':
            return done(self.parse_brace())
        if ch == '(':
            return done(AxonTuple(self.parse_sequence('(', ')')))
        if ch == '∅':
            self.i += 1
            return done(AxonSet())
        if ch in ('"', '`'):
            return done(self.parse_string())
        if ch == 'r' and self._at_raw_string():
            return done(self.parse_raw_string())
        if ch == '|':
            return done(self.parse_binary())
        if ch == '^':
            self.i += 1
            return done(self.parse_temporal())
        if ch == '$':
            return done(self.parse_constant())
        if ch == '&':
            return done(self.parse_anchor())
        if ch == '*':
            return done(self.parse_reference())
        if ch == '∞' or (ch == '-' and self.peek(1) == '∞') or ch == '?':
            return done(self.parse_special())
        if ch.isdigit() or ch == '-':
            if self.options.bare_temporals:
                temporal = self.try_temporal()
                if temporal is not None:
                    return done(temporal)
            return done(self.parse_number())
        if ch == "'" or self.is_name_start(ch):
            return done(self.parse_named())
        self.error('unexpected-token', 'unexpected character %r' % ch)

    def _at_raw_string(self):
        j = self.i + 1
        while j < self.n and self.text[j] == '#':
            j += 1
        return j < self.n and self.text[j] == '"'

    def parse_bracket(self):
        self.enter()
        self.i += 1
        self.skip()
        if self.text.startswith(':]', self.i):
            self.i += 2
            self.leave()
            return OrderedDict()
        if self.peek() == ']':
            self.i += 1
            self.leave()
            return []
        save = self.i
        key = self.try_key()
        if key is not None:
            result = OrderedDict()
            self._put(result, key, self.parse_value())
            while True:
                self.separator(']')
                if self.peek() == ']':
                    self.i += 1
                    self.leave()
                    return result
                if self.text.startswith('#_', self.i):
                    self._discard_pairs(self.try_key, 'ordered map')
                    continue
                key = self.try_key()
                if key is None:
                    self.error('unexpected-token', 'ordered map cannot contain bare values')
                self._put(result, key, self.parse_value())
        self.i = save
        result = self.parse_sequence_after_open(']')
        self.leave()
        return result

    def parse_brace(self):
        self.enter()
        self.i += 1
        self.skip()
        if self.peek() == '}':
            self.i += 1
            self.leave()
            return {}
        save = self.i
        key = self.try_key()
        if key is not None:
            result = {}
            self._put(result, key, self.parse_value())
            while True:
                self.separator('}')
                if self.peek() == '}':
                    self.i += 1
                    self.leave()
                    return result
                if self.text.startswith('#_', self.i):
                    self._discard_pairs(self.try_key, 'map')
                    continue
                key = self.try_key()
                if key is None:
                    self.error('unexpected-token', 'map cannot contain bare values')
                self._put(result, key, self.parse_value())
        self.i = save
        items = self.parse_sequence_after_open('}')
        result = AxonSet()
        for item in items:
            if item in result:
                if not self.options.set_dedup:
                    self.error('duplicate-set-element', 'duplicate set element')
            else:
                result.items.append(item)
        self.leave()
        return result

    def parse_sequence(self, open_ch, close_ch):
        self.enter()
        if self.take() != open_ch:
            self.error('unexpected-token', 'expected %s' % open_ch)
        result = self.parse_sequence_after_open(close_ch)
        self.leave()
        return result

    def parse_sequence_after_open(self, close):
        result = []
        while True:
            self.skip()
            if self.peek() == close:
                self.i += 1
                return result
            if not self.peek():
                self.error('unexpected-end', 'unterminated container')
            if self.text.startswith('#_', self.i):
                self._discard_values()
            else:
                result.append(self.parse_value())
                self._check_container(len(result))
            self.separator(close)

    def parse_named(self):
        quoted_name = self.peek() == "'"
        name = self.parse_name(quoted=True, namespace=True)
        save = self.i
        legacy_binding = self.options.legacy_binding or self.options.compatibility
        if legacy_binding:
            self.skip()
        else:
            while self.peek() and self.peek() in ' \t':
                self.i += 1
        if not quoted_name and name in _RESERVED_BAREWORDS:
            value = {'null': None, 'true': True, 'false': False}[name]
            if self.i == save and self.peek() and (self.peek() in '{(['):
                self.error('unexpected-token', 'reserved constant cannot have a body')
            self.i = save
            return value
        ch = self.peek()
        if ch == '{':
            return self.parse_node_brace(name)
        if legacy_binding:
            if ch == ',':
                self.error('unexpected-token', 'comma after a pending legacy node name')
            if ch:
                return None
            self.i = save
            return Node(name)
        if ch == '(':
            return self._well_known(Node(name, 'tuple', children=self.parse_sequence('(', ')')))
        if ch == '[':
            return self._well_known(Node(name, 'list', children=self.parse_sequence('[', ']')))
        self.i = save
        return Node(name)

    def parse_node_brace(self, name):
        self.enter()
        self.i += 1
        node = Node(name, 'brace')
        children_started = False
        while True:
            self.skip()
            if self.peek() == '}':
                self.i += 1
                self.leave()
                return self._well_known(node)
            if not self.peek():
                self.error('unexpected-end', 'unterminated node body')
            if self.text.startswith('#_', self.i):
                count = self._discard_count()
                for _ in range(count):
                    save = self.i
                    key = self.try_attr_key()
                    if key is not None:
                        self.parse_value()
                    else:
                        self.i = save
                        self.parse_value()
                    self.skip()
                self.separator('}')
                continue
            save = self.i
            key = self.try_attr_key()
            if key is not None:
                if children_started:
                    self.error('unexpected-token', 'node attributes must precede children')
                self._put(node.attributes, key, self.parse_value())
            else:
                self.i = save
                children_started = True
                node.children.append(self.parse_value())
                self._check_container(len(node.attributes) + len(node.children))
            self.separator('}')

    def _well_known(self, node):
        if self.options.doc_link and node.name in ('cid', 'link') and (len(node.children) == 1) and isinstance(node.children[0], str):
            target = node.children[0]
            if node.name == 'cid':
                try:
                    parse_cid2026(target)
                except ValueError as exc:
                    self.error('invalid-link', str(exc))
            elif not _valid_absolute_uri(target):
                self.error('invalid-link', 'link requires an absolute URI')
            return Link(node.name, target)
        return node

    def try_key(self):
        save = self.i
        self.skip()
        if self.peek() == '"':
            key = self.parse_string()
        elif self.is_name_start(self.peek()):
            key = self.parse_name(quoted=False, namespace=False)
            if key in _RESERVED_BAREWORDS:
                self.i = save
                return None
        else:
            self.i = save
            return None
        self.skip()
        if self.peek() != ':':
            self.i = save
            return None
        self.i += 1
        self.skip()
        return key

    def try_attr_key(self):
        save = self.i
        self.skip()
        if self.peek() == "'":
            key = self.parse_name(quoted=True, namespace=False)
        elif self.is_name_start(self.peek()):
            key = self.parse_name(quoted=False, namespace=False)
            if key in _RESERVED_BAREWORDS:
                self.i = save
                return None
        else:
            self.i = save
            return None
        self.skip()
        if self.peek() != ':':
            self.i = save
            return None
        self.i += 1
        self.skip()
        return key

    def _put(self, mapping, key, value):
        if key in mapping and (not self.options.last_wins):
            self.error('duplicate-key', 'duplicate key %r' % key)
        mapping[key] = value
        self._check_container(len(mapping))

    def _check_container(self, size):
        if size > self.options.max_container:
            self.error('resource-limit-exceeded', 'container entry limit exceeded')

    def is_name_start(self, ch):
        if self.options.python_name_classes or self.options.compatibility:
            return bool(ch) and (ch == '_' or ch.isalpha())
        return bool(ch) and (ch == '_' or ch.isidentifier())

    def is_name_continue(self, ch):
        if self.options.python_name_classes or self.options.compatibility:
            return bool(ch) and (ch == '_' or ch.isalnum())
        return bool(ch) and ('_' + ch).isidentifier()

    def parse_name(self, *, quoted, namespace):
        parts = []
        while True:
            if quoted and self.peek() == "'":
                part = self.parse_quoted_name()
            else:
                if not self.is_name_start(self.peek()):
                    self.error('invalid-name', 'expected a name')
                start = self.i
                self.i += 1
                while self.is_name_continue(self.peek()):
                    self.i += 1
                part = self.text[start:self.i]
                if len(part.encode('utf-8')) > self.options.max_bare_name:
                    self.error('resource-limit-exceeded', 'bare name segment is too long')
            parts.append(part)
            if not namespace or self.peek() != '/':
                break
            if len(parts) >= 2:
                # §9.3: a namespaced name has exactly one `/` (audit M5).
                self.error('invalid-name', 'a name may contain at most one namespace slash')
            self.i += 1
            if not (quoted and self.peek() == "'" or self.is_name_start(self.peek())):
                self.error('invalid-name', 'namespace slash requires another name segment')
        return '/'.join(parts)

    def parse_quoted_name(self):
        self.i += 1
        out = []
        byte_count = 0
        while True:
            ch = self.take()
            if not ch:
                self.error('unexpected-end', 'unterminated quoted name')
            if ch == "'":
                return ''.join(out)
            if ch in '\r\n' and (not (self.options.lax_strings or self.options.compatibility)):
                self.error('invalid-name', 'newline in quoted name')
            if ch == '\\':
                nxt = self.take()
                escapes = {"'": "'", '\\': '\\', 'n': '\n', 'r': '\r', 't': '\t'}
                if nxt in escapes:
                    decoded = escapes[nxt]
                    out.append(decoded)
                    byte_count += len(decoded.encode('utf-8'))
                elif nxt == 'u' and self.peek() == '{':
                    self.i += 1
                    start = self.i
                    while self.peek() and self.peek() != '}':
                        self.i += 1
                    raw = self.text[start:self.i]
                    if self.take() != '}' or not 1 <= len(raw) <= 6:
                        self.error('invalid-escape', 'invalid Unicode escape')
                    try:
                        cp = int(raw, 16)
                        if cp > 1114111 or 55296 <= cp <= 57343:
                            raise ValueError
                        decoded = chr(cp)
                        out.append(decoded)
                        byte_count += len(decoded.encode('utf-8'))
                    except ValueError:
                        self.error('invalid-escape', 'invalid Unicode scalar')
                else:
                    self.error('invalid-escape', 'invalid quoted-name escape')
            else:
                out.append(ch)
                byte_count += len(ch.encode('utf-8'))
            if byte_count > self.options.max_quoted_name:
                self.error('resource-limit-exceeded', 'quoted name is too long')

    def parse_string(self):
        end = self.take()
        out = []
        escapes = {'n': '\n', 'r': '\r', 't': '\t', '\\': '\\', '"': '"', '`': '`', "'": "'"}
        while True:
            ch = self.take()
            if not ch:
                self.error('unexpected-end', 'unterminated string')
            if ch == end:
                value = ''.join(out).replace('\r\n', '\n').replace('\r', '\n')
                if len(value) > self.options.max_string:
                    self.error('resource-limit-exceeded', 'string is too long')
                return value
            if ch != '\\':
                if ord(ch) < 32 and ch not in '\t\r\n' and (not (self.options.lax_strings or self.options.compatibility)):
                    self.error('invalid-escape', 'raw C0 control in string')
                out.append(ch)
                if len(out) > self.options.max_string:
                    self.error('resource-limit-exceeded', 'string is too long')
                continue
            esc = self.take()
            if esc in '\r\n':
                if esc == '\r' and self.peek() == '\n':
                    self.i += 1
                if self.options.legacy_escapes or self.options.compatibility:
                    # The legacy reader doubles the accumulated buffer on each
                    # continuation, which is exponential in the number of
                    # continuations. Enforce max_string here, not just at the
                    # closing quote, so a tiny input cannot build a
                    # multi-gigabyte buffer first (audit H1).
                    if len(out) > self.options.max_string:
                        self.error('resource-limit-exceeded', 'string is too long')
                    out.extend(list(out))
                    out.append('\\')
                continue
            if self.options.legacy_escapes or self.options.compatibility:
                out.extend(('\\', esc))
                continue
            if esc in escapes:
                out.append(escapes[esc])
                continue
            if esc == 'u' and self.peek() == '{':
                self.i += 1
                start = self.i
                while self.peek() and self.peek() != '}':
                    self.i += 1
                raw = self.text[start:self.i]
                if self.take() != '}' or not 1 <= len(raw) <= 6:
                    self.error('invalid-escape', 'invalid Unicode escape')
                try:
                    cp = int(raw, 16)
                    if 55296 <= cp <= 57343:
                        raise ValueError
                    out.append(chr(cp))
                except ValueError:
                    self.error('invalid-escape', 'invalid Unicode scalar')
                continue
            self.error('invalid-escape', 'unknown escape \\%s' % esc)

    def parse_raw_string(self):
        self.i += 1
        hashes = 0
        while self.peek() == '#':
            hashes += 1
            self.i += 1
        if self.take() != '"':
            self.error('unexpected-token', 'invalid raw string opener')
        end = '"' + '#' * hashes
        stop = self.text.find(end, self.i)
        if stop < 0:
            self.error('unexpected-end', 'unterminated raw string')
        if stop - self.i > self.options.max_string:
            self.error('resource-limit-exceeded', 'string is too long')
        value = self.text[self.i:stop]
        self.i = stop + len(end)
        return value

    def parse_number(self):
        match = _NUMBER.match(self.text, self.i)
        if not match:
            self.error('invalid-number', 'invalid number')
        raw = match.group(0)
        self.i = match.end()
        if len(raw) > self.options.max_number:
            self.error('resource-limit-exceeded', 'numeric literal is too long')
        if self.peek() and self.peek() in ':-':
            self.error('invalid-temporal', 'bare temporal is not permitted in Core')
        if self.peek() == '$':
            self.error('unexpected-token', '`$` is not a decimal suffix')
        nxt = self.peek()
        if nxt and (not (self.options.legacy_binding or self.options.compatibility)) and (nxt.isdigit() or self.is_name_start(nxt)):
            self.error('unexpected-token', 'a numeric literal must be separated from a following name or digit')
        if '__' in raw or raw.startswith('_') or raw.endswith('_'):
            self.error('invalid-number', 'misplaced numeric separator')
        if '_' in raw and (not self.options.numeric_separators):
            self.error('invalid-number', 'numeric separators are disabled')
        clean = raw.replace('_', '')
        unsigned = clean[1:] if clean.startswith('-') else clean
        integer_part = re.split('[.eEdD]', unsigned, maxsplit=1)[0]
        if len(integer_part) > 1 and integer_part.startswith('0') and (not (self.options.lax_numbers or self.options.compatibility)):
            self.error('invalid-number', 'leading zeros are not permitted')
        if clean.endswith(('D', 'd')):
            try:
                value = Decimal(clean[:-1])
            except InvalidOperation:
                self.error('invalid-number', 'invalid decimal')
            # The value model stores the base-10 exponent in a signed 32-bit
            # field (the Rust port's `Decimal::Finite.exponent: i32`). Reject
            # exponents outside that range so both runtimes agree, and so a
            # tiny literal like `1e9999999999D` cannot force a ~10 GB positional
            # canonical form (audit M1).
            exp = value.as_tuple().exponent
            if isinstance(exp, int) and not -2147483648 <= exp <= 2147483647:
                self.error('invalid-number', 'decimal exponent is out of the representable range')
            return ScaledDecimal.from_decimal(value) if self.options.scale_significant else value
        if '.' in clean or 'e' in clean.lower():
            if clean.endswith('.') and (not (self.options.lax_numbers or self.options.compatibility)):
                self.error('invalid-number', 'fraction requires digits')
            try:
                return float(clean)
            except ValueError:
                self.error('invalid-number', 'invalid float')
        return int(clean)

    def parse_special(self):
        negative = False
        if self.peek() == '-':
            negative = True
            self.i += 1
        ch = self.take()
        suffix = self.take() if self.peek() and self.peek() in 'dD$' else ''
        if suffix:
            if ch == '?':
                value = Decimal('NaN')
            else:
                value = Decimal('-Infinity' if negative else 'Infinity')
        elif ch == '?':
            value = math.nan
        else:
            value = -math.inf if negative else math.inf
        nxt = self.peek()
        if nxt and (not (self.options.legacy_binding or self.options.compatibility)) and (nxt.isdigit() or self.is_name_start(nxt)):
            self.error('unexpected-token', 'a numeric special must be separated from a following value')
        return value

    def parse_temporal(self):
        start = self.i
        while self.peek() and self.peek() not in ' \t\r\n,[]})#':
            self.i += 1
            if len(self.text[start:self.i].encode('utf-8')) > self.options.max_temporal:
                self.error('resource-limit-exceeded', 'temporal literal is too long')
        if self.peek() == '[' and (not self.options.tz_names):
            self.error('unsupported-profile-feature', 'named time zones require tz-names profile')
        if self.options.tz_names and self.peek() == '[':
            search_end = min(self.n, start + self.options.max_temporal + 1)
            close = self.text.find(']', self.i + 1, search_end)
            if close < 0:
                if self.text.find(']', self.i + 1) >= search_end:
                    self.error('resource-limit-exceeded', 'temporal literal is too long')
                self.error('invalid-temporal', 'unterminated named time-zone suffix')
            self.i = close + 1
        raw = self.text[start:self.i]
        if len(raw.encode('utf-8')) > self.options.max_temporal:
            self.error('resource-limit-exceeded', 'temporal literal is too long')
        return _parse_temporal_text(raw, self)

    def try_temporal(self):
        save = self.i
        try:
            return self.parse_temporal()
        except Axon2026Error:
            self.i = save
            return None

    def parse_binary(self):
        self.i += 1
        if self.options.legacy_binary or self.options.compatibility:
            encoded = []
            saw_padding = False
            while self.peek():
                ch = self.peek()
                if ch == '=':
                    saw_padding = True
                    while self.peek() == '=':
                        encoded.append(self.take())
                    break
                if ord(ch) <= 32:
                    self.i += 1
                    continue
                if ch not in 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/':
                    self.error('invalid-binary', 'invalid legacy Base64 character')
                encoded.append(ch)
                self.i += 1
            if not saw_padding:
                self.error('unexpected-end', 'legacy Base64 requires a padding terminator')
            try:
                value = b64decode(''.join(encoded), validate=True)
            except Exception:
                self.error('invalid-binary', 'invalid legacy Base64')
            if len(value) > self.options.max_binary:
                self.error('resource-limit-exceeded', 'binary value is too long')
            return value
        start = self.i
        while self.peek() and self.peek() != '|':
            self.i += 1
        if self.peek() != '|':
            if not (self.options.legacy_binary or self.options.compatibility):
                self.error('unexpected-end', 'binary literal requires a closing pipe')
            stop = self.i
        else:
            stop = self.i
            self.i += 1
        encoded = self.text[start:stop]
        encoded = encoded.replace(' ', '').replace('\t', '').replace('\r', '').replace('\n', '')
        if len(encoded) % 4:
            encoded += '=' * (-len(encoded) % 4)
        try:
            value = b64decode(encoded, validate=True)
        except Exception:
            self.error('invalid-binary', 'invalid Base64')
        if len(value) > self.options.max_binary:
            self.error('resource-limit-exceeded', 'binary value is too long')
        return value

    def parse_constant(self):
        self.i += 1
        name = self.parse_name(quoted=False, namespace=False)
        if name not in self.constants:
            self.error('unknown-constant', 'undefined constant %r' % name)
        return self.constants[name]

    def parse_label(self):
        start = self.i
        while self.peek() and (self.peek().isalnum() or self.peek() == '_'):
            self.i += 1
            if self.i - start > self.options.max_label:
                self.error('resource-limit-exceeded', 'anchor label is too long')
        if self.i == start:
            self.error('unexpected-token', 'expected anchor label')
        return self.text[start:self.i]

    def parse_anchor(self):
        if not self.options.graph:
            self.error('unsupported-profile-feature', 'anchors require graph profile')
        self.i += 1
        label = self.parse_label()
        if label in self.anchors:
            self.error('duplicate-anchor', 'duplicate anchor %r' % label)
        if len(self.anchors) >= self.options.max_anchors:
            self.error('resource-limit-exceeded', 'anchor limit exceeded')
        self.anchors[label] = _PENDING_ANCHOR
        self.skip()
        try:
            value = self.parse_value()
        except BaseException:
            self.anchors.pop(label, None)
            raise
        self.anchors[label] = value
        return value

    def parse_reference(self):
        if not self.options.graph:
            self.error('unsupported-profile-feature', 'references require graph profile')
        self.i += 1
        label = self.parse_label()
        self.reference_count += 1
        if self.reference_count > self.options.max_references:
            self.error('resource-limit-exceeded', 'reference limit exceeded')
        if label not in self.anchors:
            if self.options.undefined_ref_sentinel:
                return UNDEFINED
            self.error('unknown-reference', 'reference must follow its anchor %r' % label)
        ref = _Ref(label)
        self.references.append(ref)
        return ref

    def resolve(self, values):
        memo = {}
        resolving_refs = set()

        def walk(value):
            if isinstance(value, _Ref):
                if value.label not in self.anchors:
                    if self.options.undefined_ref_sentinel:
                        return UNDEFINED
                    self.error('unknown-reference', 'unknown reference %r' % value.label)
                if value.label in resolving_refs:
                    self.error('unexpected-token', 'reference-only cycle has no materialized value')
                target = self.anchors[value.label]
                if target is _PENDING_ANCHOR:
                    self.error('unexpected-token', 'anchor body did not materialize')
                resolving_refs.add(value.label)
                try:
                    return walk(target)
                finally:
                    resolving_refs.remove(value.label)
            ident = id(value)
            if ident in memo:
                return memo[ident]
            if isinstance(value, list):
                memo[ident] = value
                for i, item in enumerate(value):
                    value[i] = walk(item)
            elif isinstance(value, (tuple, AxonTuple)):
                result = AxonTuple()
                memo[ident] = result
                result.items.extend((walk(item) for item in value))
                return result
            elif isinstance(value, (dict, OrderedDict)):
                memo[ident] = value
                for key in list(value):
                    value[key] = walk(value[key])
            elif isinstance(value, AxonSet):
                memo[ident] = value
                value.items[:] = [walk(item) for item in value.items]
            elif isinstance(value, Node):
                memo[ident] = value
                for key in list(value.attributes):
                    value.attributes[key] = walk(value.attributes[key])
                value.children[:] = [walk(item) for item in value.children]
            return value
        resolved = [walk(value) for value in values]
        if self.trace:
            for start, end, value in self.value_events:
                if isinstance(value, _Ref):
                    value = walk(value)
                elif self._trace_graph_object(value):
                    value = memo.get(id(value), value)
                self.resolved_value_events.append((start, end, value))
        return resolved

    @staticmethod
    def _trace_graph_object(value):
        return isinstance(value, (list, tuple, AxonTuple, dict, OrderedDict, AxonSet, Node))

def _parse_temporal_text(raw, parser):
    zone_name = None
    if raw.endswith(']'):
        open_bracket = raw.rfind('[')
        if open_bracket < 0 or not parser.options.tz_names:
            parser.error('unsupported-profile-feature', 'named time zones require tz-names profile')
        zone_name = raw[open_bracket + 1:-1]
        raw = raw[:open_bracket]
        if not zone_name or '/' not in zone_name:
            parser.error('invalid-temporal', 'invalid IANA time-zone name')
    if 'T' in raw:
        left, right = raw.split('T', 1)
        d = _parse_date(left, parser)
        t = _parse_time(right, parser)
        if zone_name is not None:
            if t.offset_minutes is None:
                parser.error('invalid-temporal', 'named time zones require a numeric offset')
            try:
                zone = ZoneInfo(zone_name)
            except ZoneInfoNotFoundError:
                parser.error('invalid-temporal', 'unknown IANA time zone %r' % zone_name)
            local = datetime(d.year, d.month, d.day, t.hour, t.minute, t.second, t.nanosecond // 1000)
            instant = (local - timedelta(minutes=t.offset_minutes)).replace(tzinfo=timezone.utc)
            resolved = instant.astimezone(zone)
            same_wall = resolved.replace(tzinfo=None) == local
            actual = int(resolved.utcoffset().total_seconds() // 60)
            if not same_wall or actual != t.offset_minutes:
                parser.error('invalid-temporal', 'numeric offset does not match named time zone')
        return NanoDateTime(d, t, zone_name)
    if zone_name is not None:
        parser.error('invalid-temporal', 'named time zones require a datetime')
    if ':' in raw:
        return _parse_time(raw, parser)
    return _parse_date(raw, parser)

def _parse_date(raw, parser):
    match = _DATE.fullmatch(raw)
    if match is None and (parser.options.lax_temporals or parser.options.compatibility):
        match = _DATE_LAX.fullmatch(raw)
    if not match:
        parser.error('invalid-temporal', 'date must be YYYY-MM-DD')
    try:
        return date(int(match['y']), int(match['m']), int(match['d']))
    except ValueError as exc:
        parser.error('invalid-temporal', str(exc))

def _parse_time(raw, parser):
    match = _TIME.fullmatch(raw)
    lax = parser.options.lax_temporals or parser.options.compatibility
    if match is None and lax:
        match = _TIME_LAX.fullmatch(raw)
    if not match:
        parser.error('invalid-temporal', 'invalid time')
    fraction = match['f'] or ''
    if fraction and (parser.options.raw_fraction or parser.options.compatibility):
        nanoseconds = int(fraction[:6]) * 1000
    else:
        nanoseconds = int((fraction + '000000000')[:9]) if fraction else 0
    if nanoseconds > 999999999:
        parser.error('invalid-temporal', 'fraction exceeds nanosecond range')
    zone = match['z']
    offset_minutes = None
    zulu = zone == 'Z'
    if zone == 'Z':
        offset_minutes = 0
    elif zone:
        sign = 1 if zone[0] == '+' else -1
        zone_body = zone[1:]
        if ':' in zone_body:
            hours, minutes = map(int, zone_body.split(':', 1))
        elif len(zone_body) > 2:
            hours, minutes = (int(zone_body[:-2]), int(zone_body[-2:]))
        else:
            hours, minutes = (int(zone_body), 0)
        if hours > (24 if lax else 23) or minutes > 59 or (hours == 24 and minutes):
            parser.error('invalid-temporal', 'offset out of range')
        offset_minutes = sign * (hours * 60 + minutes)
    try:
        return NanoTime(int(match['h']), int(match['m']), int(match['s'] or 0), nanoseconds, offset_minutes, zulu, len(fraction) if fraction and parser.options.scale_significant else None)
    except ValueError as exc:
        parser.error('invalid-temporal', str(exc))

def _ecmascript_float(value):
    """Return ECMAScript/JCS finite-number layout from Python's shortest digits."""
    text = repr(value).lower()
    negative = text.startswith('-')
    if negative:
        text = text[1:]
    if 'e' in text:
        mantissa, exponent_text = text.split('e', 1)
        exponent = int(exponent_text)
    else:
        mantissa, exponent = (text, 0)
    if '.' in mantissa:
        integer, fraction = mantissa.split('.', 1)
    else:
        integer, fraction = (mantissa, '')
    digits = integer + fraction
    decimal_at = len(integer) + exponent
    while len(digits) > 1 and digits.startswith('0'):
        digits = digits[1:]
        decimal_at -= 1
    while len(digits) > 1 and digits.endswith('0'):
        digits = digits[:-1]
    count = len(digits)
    if 0 < decimal_at <= 21:
        if decimal_at >= count:
            result = digits + '0' * (decimal_at - count)
        else:
            result = digits[:decimal_at] + '.' + digits[decimal_at:]
    elif -6 < decimal_at <= 0:
        result = '0.' + '0' * -decimal_at + digits
    else:
        result = digits[0]
        if count > 1:
            result += '.' + digits[1:]
        exponent = decimal_at - 1
        result += 'e' + ('+' if exponent >= 0 else '') + str(exponent)
    return ('-' if negative else '') + result

class Writer:

    """Serialises values back to AXON text, compact or canonical."""
    def __init__(self, *, canonical, graph, max_depth=128):
        self.canonical = canonical
        self.graph = graph
        self.max_depth = max_depth
        self.write_depth = 0
        self.seen = {}
        self.active = set()
        self.next_label = 1
        self.counts = {}
        self.cyclic = set()
        self.labels = {}
        self.emitted = set()

    def write(self, value):
        """Return the AXON text for a single value."""
        self._prepare([value])
        return self._write(value)

    def write_stream(self, values):
        """Return the AXON text for a stream of values, one per line."""
        self._prepare(values)
        return [self._write(value) for value in values]

    def _prepare(self, values):
        self.counts.clear()
        self.cyclic.clear()
        self.labels.clear()
        self.emitted.clear()
        self.next_label = 1
        expanded = set()

        def scan(value, stack, depth):
            if not self._graph_object(value):
                return
            if depth > self.max_depth:
                raise Axon2026Error('resource-limit-exceeded', 'maximum writer nesting depth exceeded')
            ident = id(value)
            self.counts[ident] = self.counts.get(ident, 0) + 1
            if ident in stack:
                self.cyclic.add(ident)
                return
            if ident in expanded:
                return
            expanded.add(ident)
            stack.add(ident)
            for child in self._children(value):
                scan(child, stack, depth + 1)
            stack.remove(ident)
        for value in values:
            scan(value, set(), 1)
        if self.cyclic and (not self.graph):
            raise Axon2026Error('unsupported-profile-feature', 'cyclic values require graph output')

    def _graph_object(self, value):
        return isinstance(value, (list, tuple, AxonTuple, dict, OrderedDict, AxonSet, Node))

    def _children(self, value):
        if isinstance(value, Node):
            attrs = list(value.attributes.items())
            if self.canonical:
                attrs.sort(key=lambda item: self._key_sort_bytes(item[0]))
            return [item for _, item in attrs] + list(value.children)
        if isinstance(value, (dict, OrderedDict)):
            items = list(value.items())
            if self.canonical and (not isinstance(value, OrderedDict)):
                items.sort(key=lambda item: self._key_sort_bytes(item[0]))
            return [item for _, item in items]
        if isinstance(value, AxonSet):
            return list(value.items)
        return list(value)

    def _sort_preview(self, value, active=None, depth=1, seen=None):
        """Return a deterministic, label-free canonical set-order key.

        The prefix mirrors canonical AXON bytes for ordinary tree values.  A
        NUL-delimited identity suffix records local repeats plus graph-wide
        reference counts; NUL cannot occur in canonical AXON, so it preserves
        the lexical ordering of the canonical prefix while breaking graph ties
        deterministically.  Nested sets preview each member independently and
        sort those previews, avoiding any dependency on their input order.
        """
        if depth > self.max_depth:
            raise Axon2026Error('resource-limit-exceeded', 'maximum writer nesting depth exceeded')
        active = set() if active is None else active
        seen = {} if seen is None else seen
        if not self._graph_object(value):
            return self._write_unanchored(value).encode('utf-8')
        ident = id(value)
        marker = self._graph_marker(value)
        if ident in active:
            return b'\xffcycle:' + marker + b':' + str(seen.get(ident, 0)).encode('ascii')
        if ident in seen:
            return b'\xffref:' + marker + b':' + str(seen[ident]).encode('ascii')
        seen[ident] = len(seen) + 1
        active.add(ident)
        try:
            if isinstance(value, Node):
                attrs = sorted(value.attributes.items(), key=lambda item: self._key_sort_bytes(item[0]))
                name = self._node_name(value.name).encode('utf-8')
                attr_parts = [self._key(key).encode('utf-8') + b':' + self._sort_preview(item, active, depth + 1, seen) for key, item in attrs]
                child_parts = [self._sort_preview(item, active, depth + 1, seen) for item in value.children]
                if value.style == 'unit':
                    prefix = name
                elif value.style == 'tuple':
                    prefix = name + b'(' + b' '.join(child_parts) + b')'
                elif value.style == 'list':
                    prefix = name + b'[' + b' '.join(child_parts) + b']'
                else:
                    prefix = name + b'{' + b' '.join(attr_parts + child_parts) + b'}'
            elif isinstance(value, (dict, OrderedDict)):
                items = list(value.items())
                if not isinstance(value, OrderedDict):
                    items.sort(key=lambda item: self._key_sort_bytes(item[0]))
                children = [self._key(key).encode('utf-8') + b':' + self._sort_preview(item, active, depth + 1, seen) for key, item in items]
                if isinstance(value, OrderedDict):
                    prefix = b'[:]' if not children else b'[' + b' '.join(children) + b']'
                else:
                    prefix = b'{' + b' '.join(children) + b'}'
            elif isinstance(value, AxonSet):
                children = [self._sort_preview(item, set(active), depth + 1, dict(seen)) for item in value.items]
                children.sort()
                prefix = b'\xe2\x88\x85' if not children else b'{' + b' '.join(children) + b'}'
            else:
                children = [self._sort_preview(item, active, depth + 1, seen) for item in value]
                marks = (b'[', b']') if isinstance(value, list) else (b'(', b')')
                prefix = marks[0] + b' '.join(children) + marks[1]
            identity = b'\x00graph:' + marker + b':' + str(self.counts.get(ident, 1)).encode('ascii') + b':' + (b'cycle' if ident in self.cyclic else b'tree')
            return prefix + identity
        finally:
            active.remove(ident)

    @staticmethod
    def _graph_marker(value):
        if isinstance(value, Node):
            return b'node'
        if isinstance(value, OrderedDict):
            return b'ordered-map'
        if isinstance(value, dict):
            return b'map'
        if isinstance(value, AxonSet):
            return b'set'
        if isinstance(value, list):
            return b'list'
        return b'tuple'

    def _set_values_equal(self, left, right, preview):
        if isinstance(left, float) and math.isnan(left) or (isinstance(left, Decimal) and left.is_nan()):
            return False
        try:
            return _value_equal(left, right)
        except RecursionError:
            return preview == self._sort_preview(right)

    def _write(self, value):
        self.write_depth += 1
        if self.write_depth > self.max_depth:
            self.write_depth -= 1
            raise Axon2026Error('resource-limit-exceeded', 'maximum writer nesting depth exceeded')
        try:
            if self.graph and self._graph_object(value):
                ident = id(value)
                if self.counts.get(ident, 0) > 1 or ident in self.cyclic:
                    label = self.labels.get(ident)
                    if label is None:
                        label = str(self.next_label)
                        self.next_label += 1
                        self.labels[ident] = label
                    if ident in self.emitted:
                        return '*' + label
                    self.emitted.add(ident)
                    return '&' + label + ' ' + self._write_unanchored(value)
            return self._write_unanchored(value)
        finally:
            self.write_depth -= 1

    def _write_unanchored(self, value):
        if value is None:
            return 'null'
        if value is True:
            return 'true'
        if value is False:
            return 'false'
        if isinstance(value, int):
            return str(value)
        if isinstance(value, float):
            if math.isnan(value):
                return '?'
            if math.isinf(value):
                return '-∞' if value < 0 else '∞'
            if value == 0.0 and math.copysign(1.0, value) < 0:
                return '-0.0'
            text = _ecmascript_float(value)
            return text if any((c in text for c in '.e')) else text + '.0'
        if isinstance(value, ScaledDecimal):
            if self.canonical:
                raise Axon2026Error('unsupported-profile-feature', 'scale-significant values are incompatible with canonical output')
            return format(value.value, 'f') + 'D'
        if isinstance(value, Decimal):
            if value.is_nan():
                return '?D'
            if value.is_infinite():
                return '-∞D' if value.is_signed() else '∞D'
            text = format(value, 'f')
            if self.canonical and '.' in text:
                text = text.rstrip('0').rstrip('.')
            return text + 'D'
        if isinstance(value, str):
            return self._string(value)
        if isinstance(value, bytes):
            return '|' + b64encode(value).decode('ascii') + '|'
        if isinstance(value, datetime):
            return '^' + self._datetime(value)
        if isinstance(value, date):
            return '^' + value.isoformat()
        if isinstance(value, time):
            return '^' + self._time(value)
        if isinstance(value, NanoDateTime):
            result = '^' + value.calendar_date.isoformat() + 'T' + self._nano_time(value.clock_time)
            return result + ('[' + value.zone_name + ']' if value.zone_name else '')
        if isinstance(value, NanoTime):
            return '^' + self._nano_time(value)
        if isinstance(value, Link):
            return '%s(%s)' % (value.kind, self._string(value.target))
        if value is UNDEFINED:
            raise Axon2026Error('unknown-reference', 'undefined sentinel has no Core spelling')
        if isinstance(value, Node):
            return self._node(value)
        if isinstance(value, AxonSet):
            values = list(value.items)
            if self.canonical:
                decorated = sorted(((self._sort_preview(item), item) for item in values), key=lambda item: item[0])
                representatives = {}
                for preview, item in decorated:
                    for previous in representatives.get(preview, ()):
                        if self._set_values_equal(item, previous, preview):
                            raise Axon2026Error('duplicate-set-element', 'cannot encode a set containing duplicate values')
                    representatives.setdefault(preview, []).append(item)
                values = [item for _, item in decorated]
            else:
                probe = AxonSet()
                for item in values:
                    if item in probe:
                        raise Axon2026Error('duplicate-set-element', 'cannot encode a set containing duplicate values')
                    probe.items.append(item)
            items = [self._write(item) for item in values]
            return '∅' if not items else '{' + ' '.join(items) + '}'
        if isinstance(value, OrderedDict):
            if not value:
                return '[:]'
            return '[' + ' '.join((self._pair(k, v) for k, v in value.items())) + ']'
        if isinstance(value, dict):
            items = list(value.items())
            if self.canonical:
                items.sort(key=lambda item: self._key_sort_bytes(item[0]))
            return '{' + ' '.join((self._pair(k, v) for k, v in items)) + '}'
        if isinstance(value, list):
            return '[' + ' '.join((self._write(item) for item in value)) + ']'
        if isinstance(value, (tuple, AxonTuple)):
            return '(' + ' '.join((self._write(item) for item in value)) + ')'
        raise TypeError('cannot encode %s as AXON' % type(value).__name__)

    def _pair(self, key, value):
        return self._key(key) + ':' + self._write(value)

    def _key(self, key):
        if not isinstance(key, str):
            raise TypeError('AXON map keys must be strings')
        _utf8_length(key)
        return key if _bare_name(key, namespace=False) and key not in _RESERVED_BAREWORDS else self._string(key)

    def _attr_pair(self, key, value):
        return self._attr_key(key) + ':' + self._write(value)

    def _attr_key(self, key):
        if not isinstance(key, str):
            raise TypeError('AXON node attribute keys must be strings')
        _utf8_length(key)
        return key if _bare_name(key, namespace=False) and key not in _RESERVED_BAREWORDS else self._quoted_name(key)

    @staticmethod
    def _key_sort_bytes(key):
        if not isinstance(key, str):
            raise TypeError('AXON map keys must be strings')
        _utf8_length(key)
        return key.encode('utf-8')

    def _node_name(self, name):
        if not isinstance(name, str):
            raise TypeError('AXON node names must be strings')
        _utf8_length(name)
        parts = name.split('/')
        return '/'.join((part if _bare_name(part, namespace=False) and (not (len(parts) == 1 and part in _RESERVED_BAREWORDS)) else self._quoted_name(part) for part in parts))

    def _quoted_name(self, value):
        out = ["'"]
        table = {"'": "\\'", '\\': '\\\\', '\n': '\\n', '\r': '\\r', '\t': '\\t'}
        for ch in value:
            if ch in table:
                out.append(table[ch])
            elif ord(ch) < 32:
                out.append('\\u{%x}' % ord(ch))
            else:
                out.append(ch)
        out.append("'")
        return ''.join(out)

    def _node(self, node):
        name = self._node_name(node.name)
        if node.style == 'unit':
            if node.attributes or node.children:
                raise Axon2026Error('unexpected-token', 'a unit-style node cannot carry attributes or children')
            return name
        if node.style in ('tuple', 'list'):
            if node.attributes:
                raise Axon2026Error('unexpected-token', 'a sequence-style node cannot carry attributes')
            marks = ('(', ')') if node.style == 'tuple' else ('[', ']')
            return name + marks[0] + ' '.join((self._write(v) for v in node.children)) + marks[1]
        attrs = list(node.attributes.items())
        if self.canonical:
            attrs.sort(key=lambda item: self._key_sort_bytes(item[0]))
        body = [self._attr_pair(k, v) for k, v in attrs]
        body.extend((self._write(v) for v in node.children))
        return name + '{' + ' '.join(body) + '}'

    def _string(self, value):
        _utf8_length(value)
        out = ['"']
        for ch in value:
            table = {'"': '\\"', '\\': '\\\\', '\n': '\\n', '\r': '\\r', '\t': '\\t'}
            if ch in table:
                out.append(table[ch])
            elif ord(ch) < 32:
                out.append('\\u{%x}' % ord(ch))
            else:
                out.append(ch)
        out.append('"')
        return ''.join(out)

    def _time(self, value):
        text = '%02d:%02d:%02d' % (value.hour, value.minute, value.second)
        if value.microsecond:
            text += '.' + ('%06d' % value.microsecond).rstrip('0')
        if value.tzinfo is not None:
            offset = value.utcoffset()
            if offset is not None:
                total_seconds = offset.total_seconds()
                seconds = int(total_seconds)
                if total_seconds != seconds or seconds % 60:
                    raise Axon2026Error('invalid-temporal', 'AXON offsets must be an exact whole number of minutes')
                sign = '+' if seconds >= 0 else '-'
                seconds = abs(seconds)
                text += '%s%02d:%02d' % (sign, seconds // 3600, seconds // 60 % 60)
        return text

    def _datetime(self, value):
        return value.date().isoformat() + 'T' + self._time(value.timetz())

    def _nano_time(self, value):
        text = '%02d:%02d:%02d' % (value.hour, value.minute, value.second)
        if value.fraction_digits is not None:
            text += '.' + ('%09d' % value.nanosecond)[:value.fraction_digits]
        elif value.nanosecond:
            text += '.' + ('%09d' % value.nanosecond).rstrip('0')
        if value.offset_minutes is not None:
            if value.zulu:
                text += 'Z'
            else:
                minutes = value.offset_minutes
                sign = '+' if minutes >= 0 else '-'
                minutes = abs(minutes)
                text += '%s%02d:%02d' % (sign, minutes // 60, minutes % 60)
        return text

def _bare_name(value, *, namespace):
    parts = value.split('/') if namespace else [value]
    if not parts or any((not part for part in parts)):
        return False
    for part in parts:
        if not (part[0] == '_' or part[0].isidentifier()):
            return False
        if any((not ('_' + ch).isidentifier() for ch in part[1:])):
            return False
    return True

def _value_equal(left, right):
    if type(left) is not type(right):
        return False
    return left == right
__all__ = ['Axon2026Error', 'Options', 'Node', 'AxonSet', 'AxonTuple', 'Link', 'MigrationEvent', 'MigrationEdit', 'MigrationResult', 'ScaledDecimal', 'NanoTime', 'NanoDateTime', 'UNDEFINED', 'loads2026', 'load2026', 'iloads2026', 'iload2026', 'dumps2026', 'dump2026', 'canonical2026', 'cid2026', 'parse_cid2026', 'verify_cid2026', 'migrate2026', 'migrate_selective2026', 'apply_migration_edits2026', 'verify_canonical2026', 'tzdb_version2026']
