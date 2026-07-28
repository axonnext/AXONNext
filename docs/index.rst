
..
    =================================
    AXON is eXtended Object Notation
    =================================

.. Contents:
..
.. .. toctree::
..    :maxdepth: 2
..

.. Indices and tables
.. ==================
..
.. * :ref:`genindex`
.. * :ref:`modindex`
.. * :ref:`search`

What is AXON
------------

``pyaxon`` is the reference implementation of the **AXON 2026** edition
(AXON Next). It adds an explicit 2026 edition API (``loads2026`` / ``dumps2026``
/ ``canonical2026``, documented in the API reference) while preserving the
historical interface unchanged. See `AXON 2026 edition`_ below for what the
edition adds.

AXON is eXtended Object Notation (``AXON``). It's a simple text based format
for interchanging of objects, documents and data.
There is a `railroad diagram <ebnf/index.html>`_
in order to describe `AXON`.

It tries to combine the best of `JSON <http://www.json.org>`_,
`XML <http://www.w3.org/XML/>`_ and `YAML <http://www.yaml.org>`_.

``AXON`` is designed as a text based language for data exchange in the first place.

It combines in itself:

* **simplicity** of ``JSON``,

* **extensibility** of ``XML``,

* **readability** of ``YAML``.

Creation of ``AXON`` had the following objectives:

* Overcoming lack of support of date/time, decimal and binary data in ``JSON``.

* Overcoming inability to represent in ``JSON`` complex data
  with cross-references natively.

* Extension of ``JSON`` for native support of named/taged data structures
  (typed complex data, elements of documents etc.) in order to act in
  cases where ``XML`` is more suitable than ``JSON``.
  
* Support both ``JSON``-style and ``YAML``-style of formatting of ``AXON`` messages.

* Removing ``','`` as mandatory character-separator for items in containers.

* Saving relative simplicity of the language compared to ``JSON``.

``AXON`` is designed as text based format that has compact form and
formatted form in both ``JSON/C`` and ``YAML/Python`` style for ease of developers.

``AXON`` is an object notation for data, which are composed from atomic values
by several rules of composition:

.. raw:: html

    <table>
    <style type="text/css">
    table {
     /*border: 1px solid black;*/
    }
    td, th {
     padding: 5px;
    }
    th {
     text-align: left;
     background: black;
     color: white;
    }
    </style>
    <thead><th>Name</th><th>Rule</th><th>Example</th></thead>
    <tr><td>list</td><td>[ <b>V</b> ... <b>V</b> ]</td>
    <td><pre>
    [1 3.14 3.25D ∞ -∞ ?]
    </pre></td></tr>

    <tr><td>tuple</td><td>( <b>V</b> ... <b>V</b> )</td>
    <td><pre>
    (true ^12:00 ^2001-12-31 ^2001-12-31T12:00)
    </pre></td></tr>

    <tr><td>set</td><td>{ <b>V</b> ... <b>V</b> }</td>
    <td><pre>
    {"a" "b" "c" "d" "e" "f"}
    </pre></td></tr>

    <tr><td>dict</td><td>{ <b>K</b>:<b>V</b> ... <b>K</b>:<b>V</b> }</td>
    <td><pre>
    {alpha:1 beta:2 gamma:3 "other chars":4}
    </pre></td></tr>

    <tr><td>ordered dict</td><td>[ <b>K</b>:<b>V</b> ... <b>K</b>:<b>V</b> ]</td>
    <td><pre>
    [alpha:1 beta:2 gamma:3 "other chars":4]
    </pre></td></tr>
   
    <tr><td>node</td><td><b>N</b> { <b>N</b>:<b>V</b> ... <b>N</b>:<b>V</b> <b>V</b> ... <b>V</b> }</td>
    <td><pre>
    greek {alpha:123 beta:212 gamma:322}
    primes {2 3 5 7 11 13 17 19 23}
    tree {id:1 leaf{id:2 "AAA"} leaf{id:3 "BBB"}}
    </pre></td></tr>
    </table>

where **N** denotes a *name*, **K** denotes a *key*, **V** denotes a *value*.

Here is an example of ``AXON`` message:

.. raw:: html

    <table>
    <tr><th>statement form</th><th>formatted expression form</th></tr>
    <tr>
    <td><pre>
	axon
	  name: "AXON is eXtended Object Notation"
	  short_name: "AXON"
	  python_library: "pyaxon"
	  atomic_values
	    int: [0 -1 17]
	    float: [3.1428 1.5e-17]
	    decimal: [10D 1000.35D -1.25E+6D]
	    bool: [true false]
	    string: "abc абв 中文本"
	    multiline_string: "one
	two
	three"
	    date: ^2012-12-31
	    time: [^12:30:34 ^12:35:12.000120 ^12:35+03]
	    datetime: [^2012-12-31T12:30 ^2012-12-31T12:35+03]
	    binary: |QVhPTiBpcyBlWHRlbmRlZCBPYmplY3QgTm90YXRpb24=

	complex_values
	    list: ["one" "two" "three"]
	    dict: {
	      one: 1
	      three: 3
	      two: 2}
	    odered_dict: [
	      one: 1
	      three: 3
	      two: 2]
	    tuple: ("nodes" "edges")
	    set: {"a" "b" "c"}
	    node: person
	      name: "Alex"
	      age: 32
    </pre></td>
    <td><pre>
	axon {
	  name: "AXON is eXtended Object Notation"
	  short_name: "AXON"
	  python_library: "pyaxon"
	  atomic_values {
	    int: [0 -1 17]
	    float: [3.1428 1.5e-17]
	    decimal: [10D 1000.35D -1.25E+6D]
	    bool: [true false]
	    string: "abc абв 中文本"
	    multiline_string: "one
	two
	three"
	    date: ^2012-12-31
	    time: [^12:30:34 ^12:35:12.000120 ^12:35+03]
	    datetime: [2012-12-31T12:30 2012-12-31T12:35+03]
	    binary: |QVhPTiBpcyBlWHRlbmRlZCBPYmplY3QgTm90YXRpb24=
	}
	complex_values {
	    list: ["one" "two" "three"]
	    dict: {
	      one: 1
	      three: 3
	      two: 2}
	    odered_dict: [
	      one: 1
	      three: 3
	      two: 2]
	    tuple: ("nodes" "edges")
	    set: {"a" "b" "c"}
	    node: person {
	      name: "Alex"
	      age: 32}}}
    </pre></td>
    </tr>
    <tr><th colspan=2>compact expression form</th></tr>
    <tr><td colspan=2><pre>
	axon{name:"AXON is eXtended Object Notation" short_name:"AXON" python_library:"pyaxon"
	atomic_values{int:[0 -1 17] float:[3.1428 1.5e-17] decimal:[10D 1000.35D -1.25E+6D] 
	bool:[true false] string:"abc абв 中文本" multiline_string:"one
	two
	three" date:^2012-12-31 time:[^12:30:34 ^12:35:12.000120 ^12:35+03]
	datetime:[^2012-12-31T12:30 ^2012-12-31T12:35+03]
	binary:|QVhPTiBpcyBlWHRlbmRlZCBPYmplY3QgTm90YXRpb24=
	} complex_values{list:["one" "two" "three"] dict:{one:1 three:3 two:2}
	odered_dict:[one:1 two:2 three:3] tuple:("nodes" "edges")
	set:{"a" "b" "c"} node:person{name:"Alex" age:32}}}
    </pre></td></tr>
    </table>                    



AXON 2026 edition
-----------------

AXON Next is the **2026 edition** of AXON: a faithful continuation of the
original ``pyaxon`` that keeps everything above and adds a modern, safe,
deterministic layer. The historical API (``loads`` / ``dumps``) is unchanged;
the 2026 engine is reached through the explicit ``*2026`` API (see the API
reference). What the edition adds on top of baseline AXON:

* **Exact numeric types** -- integers, floats and arbitrary-precision decimals
  (``19.99D``) are distinct; ``∞``, ``-∞`` and ``?`` (NaN) are first class.
* **Native temporals** with nanosecond precision
  (``^2026-07-17T00:30:00-06:00``), plus named IANA zones under the ``tz-names``
  profile.
* **Named, namespaced nodes** -- ``ns/name{...}`` (for example
  ``geo/point{lat:53.5 lon:-113.5}``) so independent vocabularies compose
  without collision.
* **Strict safety by default** -- duplicate keys and set elements are rejected,
  and every parser enforces mandatory resource limits.
* **Deterministic Canonical AXON** -- one value always yields one byte sequence,
  enabling stable checksums, signatures, cache keys and content identifiers
  (**CIDs**).
* **Graph profile** -- ``&label`` / ``*label`` express shared and cyclic
  structure that a plain tree cannot.
* **Document Link profile** -- ``cid("...")`` / ``link("...")`` produce
  content-addressed and URI ``Link`` values.
* **Ergonomics** -- comments, optional commas, raw strings (``r"..."``), the
  ``#_`` discard token and ``axon{edition:"2026"}`` document headers.
* **More** -- streaming, a lossless concrete syntax tree, an AXON Schema
  companion, a compact Binary AXON encoding, and a ``compat`` reader that loads
  pre-2026 documents losslessly.

The language is defined by the AXON 2026 specification; because ``compat`` reads
legacy AXON losslessly, older documents keep working unchanged.


Python axonnext library
-----------------------

`axonnext <https://pypi.org/project/axonnext>`_ is a
dual-licensed (MIT OR Apache-2.0)
`Python <http://www.python.org>`_ library for ``AXON`` -- the reference
implementation of the **AXON 2026** edition (AXON Next). Example notebooks ship
in the ``examples/`` directory, and the release history is in ``changelog.rst``.

**Lineage.** AXON and ``pyaxon`` were originally created by intellimath
(Zaur Shibzukhov); the historical upstream remains at
`github.com/intellimath/pyaxon <https://github.com/intellimath/pyaxon>`_ and is
retained here under its original MIT licence; AXON Next as a whole is dual-licensed
MIT OR Apache-2.0.

API Reference
-------------

.. toctree::
   :maxdepth: 2

   api
   changelog
