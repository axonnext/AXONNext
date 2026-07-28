import cython
import axon2.errors as errors
from axon2.types import builtins
import datetime
import_datetime()
try:
    from datetime import timezone as timezone_cls
except:
    timezone_cls = timezone
try:
    from base64 import encodebytes, decodebytes
except:
    from base64 import encodestring as encodebytes, decodestring as decodebytes
try:
    import _decimal
except:
    import decimal as _decimal
default_decimal_context = _decimal.getcontext()
_str2decimal = default_decimal_context.create_decimal
_decimal2str = default_decimal_context.to_eng_string
from _weakref import proxy as _proxy

class Undefined:

    def __repr__(self):
        return '??'

    def __str__(self):
        return '??'
c_undefined = Undefined()
undef = c_undefined

def _error_unsupported_comparison(self):
    return TypeError('This type of comparison is not supported by %r' % type(self))

def _error_incomparable_types(self, other):
    return TypeError('Types %r and %r are not comparable' % (type(self), type(other)))
c_undescore = '_'
empty_name = ''
name_cache = {empty_name: empty_name, c_undescore: c_undescore}

def clear_all_names():
    name_cache.clear()
    name_cache[empty_name] = empty_name
    name_cache[c_undescore] = c_undescore

def as_name(name):
    return c_as_name(c_as_unicode(name))

def as_unicode(o):
    return c_as_unicode(o)

def as_list(o):
    return c_as_list(o)

def as_dict(o):
    return c_as_dict(o)

def as_tuple(o):
    return c_as_tuple(o)
c_constants = {c_as_name(c_as_unicode('NaN')): float('nan'), c_as_name(c_as_unicode('NaND')): _decimal.Decimal(float('nan')), c_as_name(c_as_unicode('Inf')): float('inf'), c_as_name(c_as_unicode('NegInf')): float('-inf')}
reserved_name_dict = {'null': None, 'true': True, 'false': False}
empty_odict = OrderedDict([])
empty_list = []

class Attribute(object):

    def __init__(self, name, val):
        self.name = c_as_name(name)
        self.val = val

    def __getitem__(self, index):
        if index == 0:
            return self.name
        elif index == 1:
            return self.val
        else:
            raise IndexError('Index out of range: ' + str(index))

    def __iter__(self):
        yield self.name
        yield self.val

    def __repr__(self):
        return self.name + ':' + repr(self.val)

def attribute(name, val):
    a = Attribute.__new__(Attribute)
    a.name = c_as_unicode(name)
    a.val = val
    return a

def c_new_attribute(name, val):
    a = Attribute.__new__(Attribute)
    a.name = name
    a.val = val
    return a

class KeyVal(object):

    def __init__(self, key, val):
        self.key = key
        self.val = val

    def __getitem__(self, index):
        if index == 0:
            return self.key
        elif index == 1:
            return self.val
        else:
            raise IndexError('Index out of range: ' + str(index))

    def __iter__(self):
        yield self.key
        yield self.val

    def __repr__(self):
        return repr(self.key) + ':' + repr(self.val)

def keyval(key, val):
    a = KeyVal.__new__(KeyVal)
    a.key = c_as_unicode(key)
    a.val = val
    return a

def c_new_keyval(key, val):
    a = KeyVal.__new__(KeyVal)
    a.key = key
    a.val = val
    return a

class Node(object):

    @property
    def __tag__(self):
        return self.name

    @property
    def __attrs__(self):
        return self.attrs

    @property
    def __vals__(self):
        return self.vals

    def __init__(self, name, attrs=None, vals=None):
        self.name = c_as_name(name)
        if attrs is None:
            self.attrs = None
        elif type(attrs) == OrderedDict:
            self.attrs = attrs
        else:
            self.attrs = OrderedDict(attrs)
        if vals is None:
            self.vals = None
        elif type(vals) is list:
            self.vals = vals
        else:
            self.vals = list(vals)

    def __getattr__(self, name):
        if name.startswith('__'):
            return self.__getattribute__(name)
        else:
            if self.attrs is not None:
                val = self.attrs.get(name, c_undefined)
            else:
                val = c_undefined
            if val is c_undefined:
                raise AttributeError('Undefined name: ' + self.name)
            return val

    def __setattr__(self, name, val):
        if name.startswith('__'):
            object.__setattr__(self, name, val)
        else:
            self.attrs[name] = val

    def __getitem__(self, index):
        if self.vals is None:
            raise errors.error_no_children(Node)
        return self.vals[index]

    def __setitem__(self, index, val):
        if self.vals is None:
            raise errors.error_no_children(Node)
        self.vals[index] = val

    def __delitem__(self, index):
        if self.vals is None:
            raise errors.error_no_children(Node)
        del self.vals[index]

    def __iadd__(self, vals):
        if self.vals is None:
            self.vals = []
        self.vals.extend(vals)
        return self

    def __iter__(self):
        if self.vals is None:
            return iter([])
        else:
            return iter(self.vals)

    def __nonzero__(self):
        return self.attrs or self.vals

    def __richcmp__(self, other, op):
        if type(self) is Node:
            v = self.name == other.name and self.attrs == other.attrs and (self.vals == other.vals)
            if op == 2:
                return v
            elif op == 3:
                return not v
            else:
                raise _error_unsupported_comparison(self)
        else:
            raise _error_incomparable_types(self, other)

    def __repr__(self):
        attrs = self.attrs
        vals = self.vals
        name = self.name
        if attrs:
            attrs_text = ', '.join([str(name) + ': ' + repr(attrs[name]) for name in attrs])
        else:
            attrs_text = ''
        if vals:
            vals_text = ', '.join([repr(x) for x in vals])
        else:
            vals_text = ''
        sp = ' ' if attrs and vals else ''
        return name + '{' + attrs_text + sp + vals_text + '}'

    def __reduce__(self):
        return (node, (self.name, self.attrs, self.vals))

def c_new_node(name, attrs, vals):
    node = Node.__new__(Node)
    node.name = name
    node.attrs = attrs
    node.vals = vals
    return node

def node(name, attrs=None, vals=None):
    """
    Factory function for creating node.

    :param name:

        name of the sequence.

    :param sequence:

        python sequence containing values.
    """
    if attrs is not None:
        if len(attrs) == 0:
            _attrs = None
        else:
            _attrs = OrderedDict(attrs)
    else:
        _attrs = None
    if vals is not None and len(vals) == 0:
        _vals = None
    else:
        _vals = c_as_list(vals)
    return c_new_node(c_as_name(name), _attrs, _vals)

def dict_as_sequence_factory(items):
    return dict(items)

class FactoryRegister:

    def __init__(self):
        self.reset()
        self.reset_types()

    def reset(self):
        if self.c_factory_dict is None:
            self.c_factory_dict = {}
        else:
            self.c_factory_dict.clear()
        self.c_factory_dict['dict'] = dict_as_sequence_factory

    def reset_types(self):
        if self.c_type_factory_dict is None:
            self.c_type_factory_dict = {}
        else:
            self.c_type_factory_dict.clear()

    def factory(self, name, factory_func=None):
        name = c_as_unicode(name)
        if factory_func is None:

            def _factory(factory_func, name=name):
                self.c_factory_dict[name] = factory_func
                return factory_func
            return _factory
        else:
            self.c_factory_dict[name] = factory_func

    def defname(self, name, val):
        name = c_as_unicode(name)
        c_constants[name] = val

    def type(self, tp, factory_func=None):
        if factory_func is None:

            def _factory(factory_func, tp=tp):
                self.c_type_factory_dict[tp] = factory_func
                return factory_func
            return _factory
        else:
            self.c_type_factory_dict[tp] = factory_func

    def convert(self, ob, to):
        caller = self.c_type_factory_dict.get(to, None)
        otype = type(ob)
        if caller is None:
            raise errors.error2("Object %s can't be converted to %s" % (otype, to))
        if otype is Node:
            return caller(ob.sequence)
        elif otype is dict:
            return caller(ob)
        elif otype is list:
            return caller(ob)
        elif otype is tuple:
            return caller(ob)
        else:
            raise errors.error2('Object %s do not support convertion' % otype)
default_factory_register = FactoryRegister()
factory = default_factory_register.factory
defname = default_factory_register.defname
type_factory = default_factory_register.type
convert = default_factory_register.convert
reset_factory = default_factory_register.reset
reset_type_factory = default_factory_register.reset_types

def dict_as_node(d):
    return c_new_node(c_as_name('dict'), None, [(k, v) for k, v in d.items()])

class Builder:

    def create_node(self, name, attrs, vals):
        return self.node(name, attrs, vals)

class SafeBuilder(Builder):

    def create_node(self, name, attrs, vals):
        return c_new_node(name, attrs, vals)

class StrictBuilder(Builder):

    @cython.locals(register=FactoryRegister)
    def __init__(self, register=default_factory_register):
        self.register = register
        self.c_factory_dict = register.c_factory_dict

    def create_node(self, name, attrs, vals):
        handler = self.c_factory_dict.get(name)
        if handler is None:
            errors.error_no_handler(name)
        else:
            return handler(attrs, vals)

class MixedBuilder(Builder):

    @cython.locals(register=FactoryRegister)
    def __init__(self, register=default_factory_register):
        self.register = register
        self.c_factory_dict = register.c_factory_dict

    def create_node(self, name, attrs, vals):
        handler = self.c_factory_dict.get(name)
        if handler is None:
            return c_new_node(name, attrs, vals)
        else:
            return handler(attrs, vals)
_inf = float('inf')
_ninf = float('-inf')
_nan = float('nan')
_decimal_inf = _str2decimal('Infinity')
_decimal_ninf = _str2decimal('-Infinity')
_decimal_nan = _str2decimal('NaN')
tz_dict = {}

class SimpleBuilder:

    def create_int(self, text):
        n = c_unicode_length(text)
        num_buffer = PyBytes_FromStringAndSize(cython.NULL, n + 1)
        buf = PyBytes_AS_STRING(num_buffer)
        for i in range(n):
            buf[i] = c_unicode_char(text, i)
        buf[n] = 0
        return c_int_fromstring(buf)

    def create_float(self, text):
        n = c_unicode_length(text)
        num_buffer = PyBytes_FromStringAndSize(cython.NULL, n)
        buf = PyBytes_AS_STRING(num_buffer)
        for i in range(n):
            buf[i] = c_unicode_char(text, i)
        return c_float_fromstring(num_buffer)

    def create_decimal(self, text):
        return _str2decimal(text)

    def create_time(self, h, m, s, ms, tz):
        return time_new(h, m, s, ms, tz)

    def create_timedelta(self, d, s, ms):
        return timedelta_new(d, s, ms)

    def create_date(self, y, m, d):
        return date_new(y, m, d)

    def create_datetime(self, y, M, d, h, m, s, ms, tz):
        return datetime_new(y, M, d, h, m, s, ms, tz)

    def create_tzinfo(self, minutes):
        o_minutes = minutes
        tzinfo = tz_dict.get(o_minutes, None)
        if tzinfo is None:
            tzinfo = timezone_cls(datetime.timedelta(minutes=o_minutes))
            tz_dict[o_minutes] = tzinfo
        return tzinfo

    def create_inf(self):
        return _inf

    def create_ninf(self):
        return _ninf

    def create_nan(self):
        return _nan

    def create_decimal_inf(self):
        return _decimal_inf

    def create_decimal_ninf(self):
        return _decimal_ninf

    def create_decimal_nan(self):
        return _decimal_nan

    def create_binary(self, text):
        return decodebytes(text.encode('ascii'))

class StringReader:

    def __init__(self, text):
        self.buffer = text
        self.pos = 0
        self.n = len(text)

    def readline(self):
        buffer = self.buffer
        pos = self.pos
        pos0 = self.pos
        n = self.n
        if pos >= n:
            return ''
        while 1:
            if pos >= n:
                line = c_unicode_substr(buffer, pos0, pos)
                break
            ch = c_unicode_char(buffer, pos)
            if ch == '\n':
                pos += 1
                if pos >= n:
                    line = c_unicode_substr(buffer, pos0, pos)
                    break
                ch = c_unicode_char(buffer, pos)
                if ch == '\r':
                    pos += 1
                line = c_unicode_substr(buffer, pos0, pos)
                break
            elif ch == '\r':
                pos += 1
                if pos >= n:
                    line = c_unicode_substr(buffer, pos0, pos)
                    break
                ch = c_unicode_char(buffer, pos)
                if ch == '\n':
                    pos += 1
                line = c_unicode_substr(buffer, pos0, pos)
                break
            else:
                pos += 1
        self.pos = pos
        return line

    def close(self):
        self.pos = self.n

class StringWriter:

    def __init__(self):
        self.blocks = []
        self.items = []
        self.n = 0

    def write(self, item):
        if self.n > 256:
            self.blocks.append(''.join(self.items))
            self.items = []
            self.n = 0
        self.items.append(item)
        self.n += 1

    def getvalue(self):
        if self.items:
            self.blocks.append(''.join(self.items))
            self.items = []
            self.n = 0
        return ''.join(self.blocks)

    def close(self):
        self.items = []
        self.blocks = []

class timezone(tzinfo):
    """Fixed offset in minutes east from UTC."""

    def __init__(self, offset, name=None):
        self.offset = offset
        self.name = name

    def utcoffset(self, dt):
        return self.offset

    def tzname(self, dt):
        seconds = self.offset.seconds + self.offset.days * 24 * 60 * 60
        if seconds < 0:
            seconds = -seconds
            sign = '-'
        else:
            sign = '+'
        minutes, seconds = builtins.divmod(seconds, 60)
        hours, minutes = builtins.divmod(minutes, 60)
        if minutes:
            return 'UTC%s%02d:%02d' % (sign, hours, minutes)
        else:
            return 'UTC%s%02d' % (sign, hours)

    def dst(self, dt):
        return None

    def __richcmp__(self, other, op):
        if not isinstance(other, tzinfo):
            raise TypeError('Invalid type: expected `tzinfo` instance')
        if op == 0:
            return self.offset < other.offset
        elif op == 1:
            return self.offset <= other.offset
        elif op == 2:
            return self.offset == other.offset
        elif op == 3:
            return self.offset != other.offset
        elif op == 4:
            return self.offset > other.offset
        else:
            return self.offset >= other.offset

    def __str__(self):
        return self.tzname(None)

    def __repr__(self):
        if self.name:
            return 'timezone(%r, %s)' % (self.offset, self.name)
        else:
            return 'timezone(%r)' % (self.offset,)