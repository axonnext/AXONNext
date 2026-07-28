from axon2._objects import defname, factory, type_factory
from axon2._objects import convert, reset_factory, reset_type_factory
from axon2._objects import undef, as_unicode, as_list, as_dict, as_tuple, as_name
from axon2._dumper import reduce, dump_as_str
import axon2
import xml.etree.ElementTree as etree
import json

@reduce(etree.Element)
def reduce_ElementTree(e):
    """Reduce an ElementTree element to an AXON node.

Attributes become node attributes; child elements are interleaved with their
tail text so the document order is preserved.
    """
    attrs = {axon2.as_unicode(name): val for name, val in e.attrib.items()}
    if e.text:
        vals = [e.text]
    else:
        vals = []
    for child in list(e):
        vals.append(child)
        if child.tail:
            vals.append(child.tail)
    if len(attrs) == 0:
        attrs = None
    if len(vals) == 0:
        vals = None
    return axon2.node(axon2.as_unicode(e.tag), attrs, vals)
del reduce_ElementTree

def xml2axon(from_, to_=None, pretty=1, braces=0):
    """
    Convert from `XML` to `AXON`.
    
    :from:
        The path of input file with `XML` or `XML` string.
        
    :to:
        The path of output file with `XML`` (default is `None`). 
        If `to` is valid path then result of convertion to `AXON` will write to the file.      
    
    :result:
        If `to` is `None` then return string with `AXON`, else return `None`.
    """
    _text = from_.lstrip()
    if _text.startswith('<'):
        root = etree.fromstring(from_)
    else:
        tree = etree.parse(from_)
        root = tree.getroot()
    if to_ is None:
        return axon2.dumps([root], pretty=pretty, braces=braces)
    else:
        axon2.dump(to_, [root], pretty=pretty, braces=braces)

def json2axon(from_, to_=None, pretty=1, braces=1):
    """
    Convert from `JSON` to `AXON`.
    
    :from:
        The path of input file with `JSON` or `JSON` string.
        
    :to:
        The path of output file with `JSON` (default is `None`). 
        If `to` is valid path then result of convertion to `AXON` will write to the file.      
    
    :result:
        If `to` is `None` then return string with `AXON`, else return `None`.
    """
    text = from_.lstrip()
    if text.startswith('[') or text.startswith('{'):
        val = json.loads(from_)
    else:
        with open(from_, 'r', encoding='utf-8') as stream:
            val = json.load(stream)
    if to_ is None:
        return axon2.dumps([val], pretty=pretty, braces=braces)
    else:
        axon2.dump(to_, [val], pretty=pretty, braces=braces)