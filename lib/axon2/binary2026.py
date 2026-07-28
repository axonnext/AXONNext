"""Binary AXON -- reference codec over the AXON 2026 value model.

A CBOR (RFC 8949) profile: every document is well-formed CBOR. Standard tags are
reused where they exist (2/3 bignum, 4 decimal, 258 set, 28/29 shared/reference,
42 CID, 32 URI); a provisional AXON block covers the rest (720 node, 724 tuple,
725 ordered map, 727 temporal, 731 decimal-special).

This module fixes the audit findings of AUDIT_AXON2026_BINARY_12-07-2026_1104:

* M1 -- ``encode(..., canonical=True)`` emits Canonical Binary AXON: shortest-form
  floats (half/single/double), map/attribute keys sorted by encoded key bytes,
  and decimals in the section-14 normal form (trailing zeros removed).
* M2 -- shared and cyclic values are encoded with CBOR tags 28/29 (value sharing);
  ``encode`` carries a depth guard and detects cycles instead of recursing without
  bound.
* L1 -- CID vs URI links are resolved by the ``Link.kind`` field only; no base32
  prefix guessing.
* L2 -- a decoder ships here, so the round-trip guarantee is demonstrable, not
  merely asserted.
"""
import math
import struct
from collections import OrderedDict
from datetime import date as _pydate, time as _pytime, datetime as _pydatetime
from decimal import Decimal
from axon2.v2026 import Node, AxonSet, AxonTuple, Link, NanoTime, NanoDateTime, ScaledDecimal
__all__ = ['encode', 'decode', 'DEFAULT_MAX_DEPTH']
T_NODE, T_TUPLE, T_OMAP, T_TEMP, T_DECSP = (720, 724, 725, 727, 731)
T_BIGPOS, T_BIGNEG, T_DECFRAC = (2, 3, 4)
T_SET, T_SHARE, T_SHAREREF, T_CID, T_URI = (258, 28, 29, 42, 32)
DEFAULT_MAX_DEPTH = 128

class BinaryAxonError(Exception):
    """Raised for malformed, truncated, or over-deep Binary AXON input.

Also raised for values that have no binary encoding, so callers never see a
bare ``IndexError`` or ``struct.error`` from this codec.
    """
    pass

def _head(major, n):
    if n < 24:
        return bytes([major << 5 | n])
    if n < 256:
        return bytes([major << 5 | 24, n])
    if n < 65536:
        return bytes([major << 5 | 25]) + struct.pack('>H', n)
    if n < 4294967296:
        return bytes([major << 5 | 26]) + struct.pack('>I', n)
    return bytes([major << 5 | 27]) + struct.pack('>Q', n)

def _bignum(i):
    tag, v = (T_BIGPOS, i) if i >= 0 else (T_BIGNEG, -1 - i)
    body = v.to_bytes((v.bit_length() + 7) // 8 or 1, 'big')
    return _head(6, tag) + _head(2, len(body)) + body

def _enc_int(i):
    if i >= 0:
        return _head(0, i) if i < 2 ** 64 else _bignum(i)
    return _head(1, -1 - i) if -1 - i < 2 ** 64 else _bignum(i)

def _tag(t, payload):
    return _head(6, t) + payload

def _enc_text(s):
    b = s.encode('utf-8')
    return _head(3, len(b)) + b

def _enc_bytes(b):
    return _head(2, len(b)) + bytes(b)

def _enc_float_shortest(v):
    """M1: shortest of half/single/double that round-trips (dCBOR rule)."""
    if v != v:
        return b'\xf9~\x00'
    if v == math.inf:
        return b'\xf9|\x00'
    if v == -math.inf:
        return b'\xf9\xfc\x00'
    d = struct.pack('>d', v)
    try:
        f = struct.pack('>f', v)
    except (OverflowError, struct.error):
        return b'\xfb' + d
    if struct.unpack('>f', f)[0] == v:
        h = _double_to_half(v)
        if h is not None and _half_to_double(h) == v:
            return b'\xf9' + struct.pack('>H', h)
        return b'\xfa' + f
    return b'\xfb' + d

def _double_to_half(v):
    try:
        return struct.unpack('>H', struct.pack('>e', v))[0]
    except (OverflowError, struct.error):
        return None

def _half_to_double(h):
    return struct.unpack('>e', struct.pack('>H', h))[0]

def _enc_float_full(v):
    if v != v:
        return b'\xf9~\x00'
    if v == math.inf:
        return b'\xf9|\x00'
    if v == -math.inf:
        return b'\xf9\xfc\x00'
    return b'\xfb' + struct.pack('>d', v)

def _tz_slot(off, zulu, zone):
    if zone is not None:
        return [off or 0, zone]
    if zulu:
        return 'Z'
    return None if off is None else off

def _decimal_tuple(d, canonical):
    sign, digits, exp = d.as_tuple()
    mant = int(''.join(map(str, digits)))
    if canonical:
        while mant % 10 == 0 and mant != 0 and (exp < 0):
            mant //= 10
            exp += 1
    if sign:
        mant = -mant
    return (exp, mant)

class _Encoder:

    def __init__(self, canonical, max_depth):
        self.canonical = canonical
        self.max_depth = max_depth
        self.active = {}
        self.shared = {}
        self.next_index = 0

    def enc(self, v, depth):
        if depth > self.max_depth:
            raise BinaryAxonError('maximum encoding depth %d exceeded' % self.max_depth)
        container = isinstance(v, (list, tuple, dict, Node, AxonSet, AxonTuple))
        if container:
            vid = id(v)
            if vid in self.shared:
                return _tag(T_SHAREREF, _enc_int(self.shared[vid]))
            if vid in self.active:
                return _tag(T_SHAREREF, _enc_int(self.active[vid]))
        if v is None:
            return b'\xf6'
        if isinstance(v, bool):
            return b'\xf5' if v else b'\xf4'
        if isinstance(v, int):
            return _enc_int(v)
        if isinstance(v, float):
            return _enc_float_shortest(v) if self.canonical else _enc_float_full(v)
        if isinstance(v, (ScaledDecimal, Decimal)):
            d = v.value if isinstance(v, ScaledDecimal) else v
            if d.is_nan():
                return _tag(T_DECSP, _enc_int(2))
            if d.is_infinite():
                return _tag(T_DECSP, _enc_int(0 if d > 0 else 1))
            exp, mant = _decimal_tuple(d, self.canonical)
            return _tag(T_DECFRAC, _head(4, 2) + _enc_int(exp) + _enc_int(mant))
        if isinstance(v, str):
            return _enc_text(v)
        if isinstance(v, (bytes, bytearray)):
            return _enc_bytes(v)
        if isinstance(v, (NanoTime, NanoDateTime, _pydate, _pytime, _pydatetime)):
            return self._temporal(v, depth)
        if isinstance(v, Link):
            if getattr(v, 'kind', None) == 'cid':
                return _tag(T_CID, _enc_bytes(b'\x00' + str(v.target).encode()))
            return _tag(T_URI, _enc_text(str(v.target)))
        return self._container(v, depth)

    def _container(self, v, depth):
        vid = id(v)
        is_shared = vid in self._referenced
        if is_shared:
            idx = self.next_index
            self.next_index += 1
            self.active[vid] = idx
            body = self._container_body(v, depth)
            del self.active[vid]
            self.shared[vid] = idx
            return _tag(T_SHARE, body)
        self.active[vid] = None
        body = self._container_body(v, depth)
        del self.active[vid]
        return body

    def _container_body(self, v, depth):
        if isinstance(v, AxonSet):
            elems = list(v)
            if self.canonical:
                # Encode elements in a share-independent canonical order so a
                # shared element's tag-28 index does not depend on set iteration
                # order (audit H1); the final byte sort below fixes emission
                # order.
                elems.sort(key=_order_key)
            items = [self.enc(x, depth + 1) for x in elems]
            if self.canonical:
                items.sort()
            return _tag(T_SET, _head(4, len(items)) + b''.join(items))
        if isinstance(v, AxonTuple):
            return _tag(T_TUPLE, self._array(list(v), depth))
        if isinstance(v, tuple):
            return _tag(T_TUPLE, self._array(list(v), depth))
        if isinstance(v, Node):
            return self._node(v, depth)
        if isinstance(v, OrderedDict):
            pairs = [_head(4, 2) + self.enc(k, depth + 1) + self.enc(val, depth + 1) for k, val in v.items()]
            return _tag(T_OMAP, _head(4, len(pairs)) + b''.join(pairs))
        if isinstance(v, dict):
            return self._map(v, depth)
        if isinstance(v, list):
            return self._array(v, depth)
        raise BinaryAxonError('no binary encoding for %r' % type(v).__name__)

    def _array(self, items, depth):
        return _head(4, len(items)) + b''.join((self.enc(x, depth + 1) for x in items))

    def _map(self, d, depth):
        items = list(d.items())
        if self.canonical:
            # Encode values in canonical (sorted-key) order so any shared value's
            # tag-28 index is assigned in emission order, not dict-insertion
            # order -- otherwise semantically identical maps produce different
            # "canonical" bytes (audit H1). Keys are scalars, so encoding them
            # for the sort key has no share side effects.
            items.sort(key=lambda kv: self.enc(kv[0], depth + 1))
        entries = [(self.enc(k, depth + 1), self.enc(val, depth + 1)) for k, val in items]
        return _head(5, len(entries)) + b''.join((k + val for k, val in entries))

    def _node(self, v, depth):
        bk = {'unit': 0, 'brace': 1, 'tuple': 2, 'list': 3}[v.style]
        if bk == 0:
            payload = b'\xf6'
        elif bk == 1:
            attrs = list(v.attributes.items())
            if self.canonical:
                # See _map: encode attribute values in sorted-key order so shared
                # values get deterministic tag-28 indices (audit H1).
                attrs.sort(key=lambda kv: _enc_text(kv[0]))
            attr_entries = [(_enc_text(k), self.enc(val, depth + 1)) for k, val in attrs]
            attr_map = _head(5, len(attr_entries)) + b''.join((k + val for k, val in attr_entries))
            children = self._array(list(v.children), depth)
            payload = _head(4, 2) + attr_map + children
        else:
            payload = self._array(list(v.children), depth)
        return _tag(T_NODE, _head(4, 3) + _enc_int(bk) + _enc_text(v.name) + payload)

    def _temporal(self, v, depth):
        if isinstance(v, NanoDateTime):
            d, t = (v.calendar_date, v.clock_time)
            a = [2, d.year, d.month, d.day, t.hour, t.minute, t.second, t.nanosecond, _tz_slot(t.offset_minutes, t.zulu, v.zone_name)]
        elif isinstance(v, NanoTime):
            a = [1, v.hour, v.minute, v.second, v.nanosecond, _tz_slot(v.offset_minutes, v.zulu, None)]
        elif isinstance(v, _pydatetime):
            off = None if v.tzinfo is None else int(v.utcoffset().total_seconds() // 60)
            a = [2, v.year, v.month, v.day, v.hour, v.minute, v.second, v.microsecond * 1000, off]
        elif isinstance(v, _pydate):
            a = [0, v.year, v.month, v.day]
        else:
            off = None if v.tzinfo is None else int(v.utcoffset().total_seconds() // 60)
            a = [1, v.hour, v.minute, v.second, v.microsecond * 1000, off]
        return _tag(T_TEMP, self._array(a, depth))

def _order_key(v):
    """A deterministic, share-independent canonical byte key for ordering set
    elements before encoding, so a shared element's tag-28 index is independent
    of set iteration order.  Encodes with an empty referenced set (everything
    inlined); a pathological cyclic element yields a stable empty key."""
    probe = _Encoder(True, DEFAULT_MAX_DEPTH)
    probe._referenced = frozenset()
    try:
        return probe.enc(v, 0)
    except BinaryAxonError:
        return b''

def _find_referenced(root, max_depth=DEFAULT_MAX_DEPTH):
    """First pass: ids of containers reachable by more than one path OR on a cycle.
    Only these need a tag-28 mark, keeping canonical output minimal.

    The traversal is depth-bounded so that pathologically deep input raises the
    public ``BinaryAxonError`` here, rather than the interpreter's
    ``RecursionError`` before the encoder's own depth guard runs."""
    seen = set()
    referenced = set()

    def children(v):
        if isinstance(v, dict):
            return list(v.keys()) + list(v.values())
        if isinstance(v, (list, tuple, AxonTuple, AxonSet)):
            return list(v)
        if isinstance(v, Node):
            return list(v.attributes.keys()) + list(v.attributes.values()) + list(v.children)
        return []

    def visit(v, ancestors, depth):
        if not isinstance(v, (list, tuple, dict, Node, AxonSet, AxonTuple)):
            return
        if depth > max_depth:
            raise BinaryAxonError('maximum encoding depth %d exceeded' % max_depth)
        vid = id(v)
        if vid in ancestors:
            referenced.add(vid)
            return
        if vid in seen:
            referenced.add(vid)
            return
        seen.add(vid)
        ancestors = ancestors | {vid}
        for c in children(v):
            visit(c, ancestors, depth + 1)
    visit(root, frozenset(), 0)
    return referenced

def encode(value, canonical=False, max_depth=DEFAULT_MAX_DEPTH):
    """Encode an AXON value to Binary AXON bytes.

    canonical=True produces Canonical Binary AXON (deterministic; CID-ready).
    Shared and cyclic values are encoded with CBOR tags 28/29. Raises
    BinaryAxonError past max_depth.
    """
    enc = _Encoder(canonical, max_depth)
    enc._referenced = _find_referenced(value, max_depth)
    return enc.enc(value, 0)

class _Decoder:

    def __init__(self, buf, max_depth=DEFAULT_MAX_DEPTH):
        """Bounds- and depth-checked reader.  Every malformed or truncated input
        raises the public ``BinaryAxonError`` (audit H3), never a bare
        ``IndexError``/``struct.error``/``UnicodeDecodeError``/``RecursionError``."""
        self.b = buf
        self.i = 0
        self.shares = []
        self.max_depth = max_depth

    def _need(self, n):
        """Require *n* more bytes or raise the public unexpected-end error."""
        if self.i + n > len(self.b):
            raise BinaryAxonError('unexpected end of binary input')

    def _count(self, n):
        """A container declaring *n* elements needs at least *n* more bytes;
        reject a count exceeding the remaining input before iterating."""
        if n > len(self.b) - self.i:
            raise BinaryAxonError('container length exceeds remaining input')
        return n

    def _u(self, n):
        self._need(n)
        v = int.from_bytes(self.b[self.i:self.i + n], 'big')
        self.i += n
        return v

    def _head(self):
        self._need(1)
        ib = self.b[self.i]
        self.i += 1
        major, ai = (ib >> 5, ib & 31)
        if ai < 24:
            return (major, ai, ai)
        if ai == 24:
            return (major, ai, self._u(1))
        if ai == 25:
            return (major, ai, self._u(2))
        if ai == 26:
            return (major, ai, self._u(4))
        if ai == 27:
            return (major, ai, self._u(8))
        raise BinaryAxonError('indefinite/reserved additional-info %d' % ai)

    def item(self, depth=0):
        if depth > self.max_depth:
            raise BinaryAxonError('maximum decoding depth %d exceeded' % self.max_depth)
        self._need(1)
        ib = self.b[self.i]
        major = ib >> 5
        if major == 7:
            ai = ib & 31
            self.i += 1
            if ai == 20:
                return False
            if ai == 21:
                return True
            if ai == 22:
                return None
            if ai == 25:
                return _half_to_double(self._u(2))
            if ai == 26:
                self._need(4)
                v = struct.unpack('>f', self.b[self.i:self.i + 4])[0]
                self.i += 4
                return v
            if ai == 27:
                self._need(8)
                v = struct.unpack('>d', self.b[self.i:self.i + 8])[0]
                self.i += 8
                return v
            raise BinaryAxonError('unsupported simple/float ai %d' % ai)
        major, ai, val = self._head()
        if major == 0:
            return val
        if major == 1:
            return -1 - val
        if major == 2:
            self._need(val)
            b = self.b[self.i:self.i + val]
            self.i += val
            return bytes(b)
        if major == 3:
            self._need(val)
            b = self.b[self.i:self.i + val]
            self.i += val
            try:
                return b.decode('utf-8')
            except UnicodeDecodeError as exc:
                raise BinaryAxonError('invalid UTF-8 in text string') from exc
        if major == 4:
            return [self.item(depth + 1) for _ in range(self._count(val))]
        if major == 5:
            return {self.item(depth + 1): self.item(depth + 1) for _ in range(self._count(val))}
        if major == 6:
            return self._tagged(val, depth)
        raise BinaryAxonError('unexpected major %d' % major)

    def _tagged(self, t, depth):
        if t in (T_BIGPOS, T_BIGNEG):
            raw = self.item(depth + 1)
            if not isinstance(raw, (bytes, bytearray)):
                raise BinaryAxonError('bignum payload must be a byte string')
            mag = int.from_bytes(raw, 'big')
            return mag if t == T_BIGPOS else -1 - mag
        if t == T_DECFRAC:
            arr = self.item(depth + 1)
            if not isinstance(arr, list) or len(arr) != 2 or not all(isinstance(x, int) for x in arr):
                raise BinaryAxonError('decfrac must be [exp, mant] of integers')
            exp, mant = arr
            return Decimal(mant).scaleb(exp)
        if t == T_DECSP:
            code = self.item(depth + 1)
            try:
                return Decimal({0: 'Infinity', 1: '-Infinity', 2: 'NaN'}[code])
            except (KeyError, TypeError):
                raise BinaryAxonError('unknown decimal-special code %r' % (code,))
        if t == T_TEMP:
            return _decode_temporal(self.item(depth + 1))
        if t == T_NODE:
            arr = self.item(depth + 1)
            if not isinstance(arr, list) or len(arr) != 3:
                raise BinaryAxonError('node must be [bk, name, payload]')
            bk, name, payload = arr
            style = {0: 'unit', 1: 'brace', 2: 'tuple', 3: 'list'}.get(bk)
            if style is None or not isinstance(name, str):
                raise BinaryAxonError('invalid node body kind/name')
            if bk == 0:
                return Node(name, style, OrderedDict(), [])
            if bk == 1:
                if not isinstance(payload, list) or len(payload) != 2 or not isinstance(payload[0], dict):
                    raise BinaryAxonError('brace node payload must be [attrs, children]')
                return Node(name, style, OrderedDict(payload[0]), list(payload[1]))
            return Node(name, style, OrderedDict(), list(payload))
        if t == T_TUPLE:
            return tuple(self.item(depth + 1))
        if t == T_OMAP:
            arr = self.item(depth + 1)
            result = OrderedDict()
            for pair in arr:
                if not isinstance(pair, list) or len(pair) != 2:
                    raise BinaryAxonError('ordered-map entry must be a 2-array')
                result[pair[0]] = pair[1]
            return result
        if t == T_SET:
            return AxonSet(list(self.item(depth + 1)))
        if t == T_CID:
            return Link('cid', self.item(depth + 1))
        if t == T_URI:
            return Link('uri', self.item(depth + 1))
        if t == T_SHARE:
            idx = len(self.shares)
            self.shares.append(None)
            v = self.item(depth + 1)
            self.shares[idx] = v
            return v
        if t == T_SHAREREF:
            return _Ref(self.item(depth + 1))
        raise BinaryAxonError('unsupported Binary AXON tag %d' % t)

def _decode_tz(slot):
    """Reverse ``_tz_slot``: return ``(offset_minutes, zulu, zone_name)``."""
    if slot is None:
        return (None, False, None)
    if slot == 'Z':
        return (0, True, None)
    if isinstance(slot, list):
        if len(slot) != 2:
            raise BinaryAxonError('named-zone slot must be [offset, name]')
        return (slot[0], False, slot[1])
    if isinstance(slot, int) and not isinstance(slot, bool):
        return (slot, False, None)
    raise BinaryAxonError('invalid time-zone slot %r' % (slot,))

def _decode_temporal(arr):
    """Reconstruct the public temporal type from a tag-727 field array."""
    if not isinstance(arr, list) or not arr:
        raise BinaryAxonError('temporal must be a non-empty array')
    kind = arr[0]
    try:
        if kind == 0:
            if len(arr) != 4:
                raise BinaryAxonError('date must be [0, y, m, d]')
            return _pydate(arr[1], arr[2], arr[3])
        if kind == 1:
            if len(arr) != 6:
                raise BinaryAxonError('time must be [1, h, mi, s, ns, tz]')
            off, zulu, _ = _decode_tz(arr[5])
            return NanoTime(arr[1], arr[2], arr[3], arr[4], off, zulu)
        if kind == 2:
            if len(arr) != 9:
                raise BinaryAxonError('datetime must be [2, y, m, d, h, mi, s, ns, tz]')
            off, zulu, zone = _decode_tz(arr[8])
            clock = NanoTime(arr[4], arr[5], arr[6], arr[7], off, zulu)
            return NanoDateTime(_pydate(arr[1], arr[2], arr[3]), clock, zone)
    except (TypeError, ValueError) as exc:
        raise BinaryAxonError('invalid temporal value: %s' % exc) from exc
    raise BinaryAxonError('unknown temporal kind %r' % (kind,))

class _Ref:

    def __init__(self, index):
        self.index = index

def decode(buf, max_depth=DEFAULT_MAX_DEPTH):
    """Decode Binary AXON bytes to the public value model. Shared references
    (tags 28/29) are resolved to the same object; cycles are reconstructed.

    Decoding is bounds- and depth-checked (audit H3) and reconstructs public
    ``Node``/``AxonSet``/``Link`` and temporal values rather than private
    wrappers (audit H2)."""
    d = _Decoder(buf, max_depth)
    v = d.item(0)
    if d.i != len(buf):
        raise BinaryAxonError('trailing bytes: consumed %d of %d' % (d.i, len(buf)))
    seen = {}
    for idx, sv in enumerate(d.shares):
        d.shares[idx] = _resolve_inplace(sv, d.shares, seen)
    return _resolve_inplace(v, d.shares, seen)

def _resolve_inplace(v, shares, seen):
    """Replace _Ref markers with their shared object. Mutable containers (list,
    dict, set, node) are resolved in place so identity and cycles are preserved;
    an immutable tuple that transitively contains a _Ref is rebuilt (its shared
    members still resolve to the same instances)."""
    if isinstance(v, _Ref):
        if not 0 <= v.index < len(shares):
            raise BinaryAxonError('reference to an undefined share %r' % (v.index,))
        return shares[v.index]
    vid = id(v)
    if vid in seen:
        return seen[vid]
    if isinstance(v, list):
        seen[vid] = v
        for i, x in enumerate(v):
            v[i] = _resolve_inplace(x, shares, seen)
        return v
    if isinstance(v, dict):
        seen[vid] = v
        for k in list(v.keys()):
            v[k] = _resolve_inplace(v[k], shares, seen)
        return v
    if isinstance(v, AxonSet):
        seen[vid] = v
        v.items = [_resolve_inplace(x, shares, seen) for x in v.items]
        return v
    if isinstance(v, Node):
        seen[vid] = v
        v.children = [_resolve_inplace(x, shares, seen) for x in v.children]
        for k in list(v.attributes.keys()):
            v.attributes[k] = _resolve_inplace(v.attributes[k], shares, seen)
        return v
    if isinstance(v, tuple):
        rebuilt = tuple((_resolve_inplace(x, shares, seen) for x in v))
        seen[vid] = rebuilt
        return rebuilt
    return v
