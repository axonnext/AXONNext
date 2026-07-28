import sys

class AxonError(Exception):

    """Error raised by the legacy (pre-2026) loader and dumper.

Carries the message, an optional ``(line, position)`` pair, and the source
chunk where processing stopped.
    """
    def __init__(self, msg, pos=None, chunk=''):
        self.msg = msg
        self.pos = pos
        self.chunk = chunk

    def __str__(self):
        text = 'ERROR:: ' + self.msg
        if self.pos is not None:
            lnum, pos = self.pos
            text += ' line:%s pos:%s' % (lnum, pos)
        if self.chunk:
            text += ' chunk: %s' % self.chunk
        return text

def error(self, msg):
    """
Raises a context-aware exception, enriched with the line and position information.
"""
    e = AxonError(msg, (self.lnum, self.pos), self.line[self.pos:self.pos + 16])
    if sys.flags.debug:
        self.errto.write(str(e))
    raise e

def error_unexpected_end_complex_value(self):
    """
Raises an error indicating an unexpected termination while parsing a complex value.
"""
    error(self, 'Unexpected end inside complex value')

def error_unexpected_end_list(self):
    """
Raises an error when a list ends abruptly without proper closure.
"""
    error(self, 'Unexpected end inside list')

def error_unexpected_end_odict(self):
    """
Raises an error when an ordered dictionary terminates unexpectedly.
"""
    error(self, 'Unexpected end inside ordered dict')

def error_unexpected_end_tuple(self):
    """
Raises an error indicating an incomplete tuple structure.
"""
    error(self, 'Unexpected end inside tuple')

def error_unexpected_end_string(self):
    """
Raises an error for a string literal that lacks a closing quote.
"""
    error(self, 'Unexpected end of the string')

def error_unexpected_keyval(self):
    """
Raises an error when a key-value pair is encountered in an invalid context.
"""
    error(self, 'Unexpected key:val')

def error_expected_keyval(self):
    """
Raises an error when a key-value pair is strictly required but missing.
"""
    error(self, 'Expected key:val')

def error_expected_key(self):
    """
Raises an error indicating that a valid key identifier is expected.
"""
    error(self, 'Expected key')

def error_expected_named_constant(self):
    """
Raises an error when a reserved named constant is expected.
"""
    error(self, 'Expected named constant')

def error_getnumber(self):
    """
Raises an error for malformed or unrecognised numeric literals.
"""
    error(self, 'Invalid number')

def error_getint_part(self):
    """
Raises an error regarding an invalid integer component within a number.
"""
    error(self, 'Invalid int part or value')

def error_invalid_date(self):
    """
Raises an error for date values that violate formatting or logical constraints.
"""
    error(self, 'Invalid date')

def error_invalid_time(self):
    """
Raises an error for time values with invalid hours, minutes, or seconds.
"""
    error(self, 'Invalid time')

def error_invalid_datetime(self):
    """
Raises an error for malformed combined date and time structures.
"""
    error(self, 'Invalid datetime')

def error_invalid_value(self, vtype=''):
    """
Raises a generic error for invalid values, optionally specifying the expected type.
"""
    error(self, 'Invalid %s value' % vtype.__name__)

def error_invalid_odict(self, vtype=''):
    """
Raises an error for structurally invalid ordered dictionaries.
"""
    error(self, 'Invalid ordered dict')

def error_invalid_value_with_prefix(self, prefix):
    """
Raises an error for values carrying an unrecognised or invalid prefix.
"""
    error(self, "Invalid value with prefix '%s'" % prefix)

def error_dict_value(self):
    """
Raises an error indicating that standard dictionaries must exclusively contain key-value pairs.
"""
    error(self, 'dict can contain only key:value pairs')

def error_unexpected_attribute(self, name):
    """
Raises an error when an unrecognised attribute is attached to a node.
"""
    error(self, 'Unexpected attribute %s:?' % name)

def error_expected_attribute(self):
    """
Raises an error when a required attribute is missing from a node.
"""
    error(self, 'Expected attribute')

def error_unexpected_value(self, context=''):
    """
Raises an error for unexpected values within a specific context.
"""
    error(self, 'Unexpected value: %s' % context)

def error_end_item(self):
    """
Raises an error indicating that a valid container closure or delimiter is required.
"""
    error(self, "Expected space character, '[', '}' or ')' here")

def error_star(self):
    """
Raises an error regarding the invalid placement or usage of the asterisk symbol.
"""
    error(self, 'Invalid usage of symbol *')

def error_undefined_name(self, name):
    """
Raises an error when attempting to reference an undefined identifier.
"""
    error(self, 'Undefined name %r' % name)

def error_indentation(self, idn):
    """
Raises an error highlighting a discrepancy in the structural indentation.
"""
    error(self, 'Invalid indentation: current position=%s ident position=%s' % (self.pos, idn))

def error_expected_name(self):
    """
Raises an error indicating that a valid identifier name is strictly required.
"""
    error(self, 'Expected name here')

def error_expected_same_name(self, name):
    """
Raises an error when an element name does not match the expected identifier.
"""
    error(self, "Element must have same name as '%s'" % name)

def error_expected_complex_value(self):
    """
Raises an error when a structural complex value is anticipated.
"""
    error(self, 'Expected complex here')

def error_expected_label(self):
    """
Raises an error indicating the absence of a required value label.
"""
    error(self, 'Expected label of the value')

def error2(msg):
    """
Raises a generic AxonError without positional context, typically for internal faults.
"""
    e = AxonError(msg)
    if sys.flags.debug:
        sys.stderr.write(str(e))
    raise e

def error_no_handler(name):
    """
Raises an error indicating that no registered handler exists for the specified name.
"""
    error2('Handler for name <%s> is not registered' % name)

def error_no_reducer(tp):
    """
Raises an error when attempting to reduce an unsupported or unrecognised type.
"""
    error2('There is no reducer for this type: %r' % tp)

def error_reducer_wrong_type(tp):
    """
Raises an error when a reducer yields an incorrect or unexpected type.
"""
    error2('Reducer return wrong type: %r' % tp)

def error_no_attributes(tp):
    """
Raises an error indicating that the specified type does not support attributes.
"""
    error2('The type %r does not contain attributes' % tp)

def error_no_children(tp):
    """
Raises an error indicating that the specified type cannot contain child values.
"""
    error2('The type %r does not contain child values' % tp)