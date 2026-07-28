def c_unicode_length(text):
    return len(text)

def c_unicode_char(text, i):
    return text[i] if 0 <= i < len(text) else '\x00'

def c_unicode_substr(text, start, end):
    return text[start:end]

def c_object_to_unicode(o):
    return str(o)

def c_int_tostring(o):
    return str(o)

def c_int_fromstring(text):
    return int(text)

def c_int_fromlong(val):
    return int(val)

def c_int_fromint(val):
    return int(val)

def c_float_fromstring(text):
    return float(text)

def PyUnicode_FromOrdinal(ordinal):
    return chr(ordinal)

def current_char(self):
    line = self.line
    pos = self.pos
    return line[pos] if pos < len(line) else '\x00'

def skip_char(self):
    self.pos += 1

def next_char(self):
    self.pos += 1
    line = self.line
    pos = self.pos
    return line[pos] if pos < len(line) else '\x00'

def get_chunk(self, pos0):
    return self.line[pos0:self.pos]

def dict_get(op, key, default):
    return op.get(key, default)

def c_as_unicode(ob):
    if type(ob) is str:
        return ob
    if isinstance(ob, str):
        return str(ob)
    raise TypeError('The type of the object should be `str`.')

def c_as_list(ob):
    if ob is None:
        return []
    return ob if type(ob) is list else list(ob)

def c_as_dict(ob):
    if ob is None:
        return {}
    return ob if type(ob) is dict else dict(ob)

def c_as_tuple(ob):
    if ob is None:
        return ()
    return ob if type(ob) is tuple else tuple(ob)