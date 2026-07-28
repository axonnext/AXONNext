========
axonnext
========

``axonnext`` is a dual-licensed (MIT OR Apache-2.0) Python library
for `AXON <https://github.com/intellimath/pyaxon>`_. 
AXON is eXtended Object Notation. It's a simple text based format for interchanging
objects, documents and data.
It tries to combine the best of `JSON <http://www.json.org>`_,
`XML <http://www.w3.org/XML/>`_ and `YAML <http://www.yaml.org>`_.

Project lineage
---------------

* The original ``pyaxon`` was first published on Bitbucket (Mercurial), which
  Atlassian has since retired; its `GitHub mirror
  <https://github.com/intellimath/pyaxon>`_ remains the historical upstream.

The AXON 2026 / AXON Next continuation is developed at
`github.com/axonnext/AXONNext <https://github.com/axonnext/AXONNext>`_, under
dual-licensed MIT OR Apache-2.0, retaining the original author's MIT copyright.

Installation
------------

``axonnext`` supports Python 3.10, 3.11, 3.12, and 3.13.

It can be installed via pip::

	pip install axonnext
	
It can be installed from sources::

		python -m pip install .

Quick start
-----------

First import `axon2` module::

	>>> import axon2

Load and dump lists, dicts, tuples::

	>>> from decimal import Decimal
	>>> from datetime import datetime, time, date
		>>> text = axon2.dumps([['abc абв', 1, 3.14, True],
		...                    [datetime(2026, 7, 11, 13, 8, 37, 78189), Decimal('3.14')]])
		>>> print(text)
		["abc абв" 1 3.14 true]
		[^2026-07-11T13:08:37.078189 3.14D]
		
		>>> vals = [{'id':1, 'nickname':'nick', 'time':time(12, 31, 34), 'text':'hello!'},
		...         {'id':2, 'nickname':'mark', 'time':time(12, 32, 3), 'text':'hi!'}]
	>>> text = axon2.dumps(vals)
	>>> print(text)
	{id:1 nickname:"nick" time:^12:31:34 text:"hello!"}
	{id:2 nickname:"mark" time:^12:32:03 text:"hi!"}
	>>> text = axon2.dumps(vals, pretty=1)
	>>> print(text)
	{ id: 1
	  nickname: "nick"
	  time: ^12:31:34
	  text: "hello!"}
	{ id: 2
	  nickname: "mark"
	  time: ^12:32:03
	  text: "hi!"}
	>>> vals == axon2.loads(text)
	True
	  
	>>> vals = [[{'a':1, 'b':2, 'c':3}, {'a':[1,2,3], 'b':(1,2,3), 'c':{1,2,3}}]]
	>>> text = axon2.dumps(vals)
	>>> print(text)
	[{a:1 b:2 c:3} {a:[1 2 3] b:(1 2 3) c:{1 2 3}}]
	>>> text = axon2.dumps(vals, pretty=1)
	>>> print(text)
	[ { a: 1
	    b: 2
	    c: 3}
	  { a: [1 2 3]
	    b: (1 2 3)
	    c: {1 2 3}}]
	>>> vals == axon2.loads(text)
	True

Dump, load objects in "safe" mode::
	
    >>> vals = axon2.loads('person{name:"nick" age:32 email:"nail@example.com"}')
    >>> print(type(vals[0]))
    <class 'axon2._objects.Node'>
    >>> print(vals[0])
    person{name: 'nick', age: 32, email: 'nail@example.com'}

    >>> text = axon2.dumps(vals)
    >>> print(text)
    person{name:"nick" age:32 email:"nail@example.com"}
    >>> text = axon2.dumps(vals, pretty=1)
    >>> print(text)
    person
      name: "nick"
      age: 32
      email: "nail@example.com"
    >>> text = axon2.dumps(vals, pretty=1, braces=1)
    >>> print(text)
    person {
      name: "nick"
      age: 32
      email: "nail@example.com"}

Dump, load objects in unsafe mode::

		>>> class Person:
		...     def __init__(self, name, age, email):
		...         self.name = name
		...         self.age = age
		...         self.email = email
		...     def __str__(self):
		...         return "Person(name=%r, age=%r, email=%r)" % (self.name, self.age, self.email)
		>>> @axon2.reduce(Person)
		... def reduce_Person(p):
		...     return axon2.node('person', {'name':p.name, 'age':p.age, 'email':p.email})
		>>> @axon2.factory('person')
		... def factory_Person(attrs, vals):
		...     return Person(name=attrs['name'], age=attrs['age'], email=attrs['email'])
	
	>>> p = Person('nick', 32, 'mail@example.com')
	>>> text = axon2.dumps([p])
	>>> print(text)
	person{name:"nick" age:32 email:"mail@example.com"}
	>>> val = axon2.loads(text, mode='strict')[0]
	>>> print(val)
	Person(name='nick', age=32, email='mail@example.com')
	
Features
--------

1. Provide simple API for loading and dumping of objects in textual AXON format.
2. Provide safe loading and dumping by default.
3. Provide unsafe loading and dumping of objects on the base of registration of factory/reduce callables.
4. Provide a way for fully controlled by application/framework/library unsafe loading and dumping.
5. It's sufficiently fast so as to be useful.

AXON 2026 edition
-----------------

This repository carries the final AXON 2026 revision 5 edition. The Python
implementation is the normative reference, and the Rust port (``serde_axon``)
is active and held to byte-for-byte parity. The historical API remains
available, while the edition API is explicit::

    >>> from axon2 import loads2026, dumps2026, canonical2026, Options
    >>> loads2026('point{label:"p" 10 20}')
    [Node('point', brace, attrs=OrderedDict({'label': 'p'}), children=[10, 20])]
    >>> dumps2026([{'b': 2, 'a': 1}], canonical=True)
    '{a:1 b:2}'

The 2026 engine adds strict duplicate detection, numeric separators, corrected
fractional-second semantics, standard string escapes and raw strings,
namespaced names, tuple/list node bodies, terminated Base64 literals, discard
syntax, document headers, resource limits, deterministic output, and an
opt-in graph profile with identity and cycle support.  ``Options.compat()``
provides documented legacy-reading behaviour for migrations.

The Python reference also includes a lossless CST with byte-addressed repairs,
selective migration edits, local-only governed schema modules, deterministic
canonical CIDs, and an executable language-neutral conformance registry. The
ordinary release gate runs the complete normative vectors, RFC 8785 Appendix B,
checked malformed inputs, seeded mutation tests, generated round trips, and
canonical graph-isomorphism tests. The registry fingerprints all 64
RFC-2119-bearing specification lines, and the platform workflow differentially
checks 132,752 finite binary64 values against V8 in addition to Appendix B.

Internationalisation
---------------------

International text is a first-class concern. Source text must be well-formed
UTF-8 (rejected as ``invalid-unicode`` otherwise); bare names and keys follow
the Unicode identifier rules (UAX #31), so non-ASCII keys in Greek, Cyrillic,
Arabic, or CJK scripts are legal unquoted; string values are Unicode scalars
written literally, with a single braced ``\u{...}`` escape that rejects
surrogate code points -- none of JSON's surrogate-pair hazards. Canonical AXON
orders map keys by their UTF-8 bytes, so an international document has one
stable byte sequence, and therefore one stable CID, in every implementation;
temporals keep local, offset, and ``Z`` distinct; and numbers are
locale-neutral (always ``.``, no digit grouping).

By contrast, JSON escapes non-ASCII with 16-bit surrogate pairs and defines no
canonical form in its base standard; TOML mandates UTF-8 but keeps bare keys
ASCII-only; YAML is Unicode-capable but ambiguous. One deliberate difference:
canonical AXON does not silently NFC-normalise string values, so normalise
upstream if you need content-address equality across differently-composed text.
