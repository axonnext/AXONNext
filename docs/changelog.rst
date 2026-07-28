History of changes
------------------

**1.0.0**

#. First stable release of the AXON 2026 continuation (AXON Next v1). It
   promotes the ``0.11.0a2``-``0.11.0a13`` development iterations recorded below
   to a 1.0 release; no language, canonical, profile, or conformance behaviour
   changed from the final pre-release.
#. AXON Schema remediation (audit finding L1): the validator now rejects unknown
   schema keywords as an ``invalid-schema`` issue instead of silently ignoring
   them, so a mistyped constraint (for example ``minimun`` for ``minimum``) is a
   loud error rather than a false pass.  The check mirrors the existing unknown
   ``kind`` rejection and applies to both dict-form and node-form schemas;
   recognised constraints, structural keywords, and non-semantic annotation
   keywords are unchanged.

**0.11.0a12**

#. Corrected prerelease continuity: this remediation follows the existing
   ``0.11.0a11`` release and is packaged, identified, and verified as
   ``0.11.0a12`` throughout.
#. Fixed an audit-discovered native legacy-loader crash on an anchored value
   followed by an end-of-file comment; the path now raises ``AxonError`` and is
   protected by an isolated regression plus a 100,000-input native fuzz pass.
#. Removed the inherited upstream-push helper and incomplete duplicate AppVeyor
   gate, corrected Git build/binary exclusions, and marked original repository
   links as project lineage pending the new continuation repository.
#. Retained AXON 2026 revision 5 without any semantic, canonical, profile, or
   conformance change.

**0.11.0a10**

#. Finalised the AXON 2026 revision 5 specification and executable normative
   requirement registry.
#. Added the full RFC 8785 Appendix B binary64 corpus, an independent V8 gate,
   deterministic generated-value, mutation, malformed-input, and canonical
   graph-isomorphism suites.
#. Added CST expected-token sets and byte-addressed fix-its, selective migration
   edits, governed local-only schema modules, deterministic bounded validation
   reports, and multi-platform wheel/install workflows.
#. Prevented distinct empty tuples from acquiring false shared graph identity,
   categorised non-scalar Unicode, and restored measured legacy escape
   behaviour under its isolated compatibility flag.
#. Closed the exhaustive follow-up audit: repaired generated-C ordered-map
   crashes and stale historical registries, bounded adversarial schema regexes
   and deep CST recovery, made CST semantic traces profile-exact, validated
   UTF-8 edit boundaries and canonical keys, and expanded the machine registry
   to 131 requirements, 125 vectors, all 16 error categories, and a frozen
   64-clause specification fingerprint index.
#. Closed the final independent audit findings: made shared-identity set graphs
   deterministic, rejected duplicate programmatic sets, made CST trace
   attachment near-linear, restored standard keys/items/values view equality,
   fixed reserved-name and attribute-key canonical spelling, and verified a
   source-only generated-C build.
#. Promoted the binary64 V8 audit into the platform matrix with 32,752
   exponent-edge and 100,000 deterministic random finite values per job.

**0.11.0a9**

#. Remediated the exhaustive 2026 audit: exact canonical name identity,
   pre-bound graphs, cycle-safe sets, faithful compatibility/migration, complete
   resource categories, incremental streams, byte diagnostics, safe schemas,
   JCS-threshold floats, canonical CIDs, strict links, and atomic output.
#. Expanded the CST with grammar productions, trivia attachment, semantic
   traceability, cycle-safe LSP traversal, and linear token positioning.
#. Restored a green full discovery gate, executable README examples, valid
   fixtures, modern Windows CI, generated-C PEP 517 builds, and complete source
   distribution reference material.

**0.11.0a7**

#. Implemented semantic ``scale-significant`` decimals and temporal fraction
   widths, including the required rejection by Canonical AXON.
#. Implemented the ``canonical-verify`` document profile and its composition
   conflict with ``scale-significant``.
#. Completed local Document Link generation, parsing and verification using
   CIDv1, the permanent ``raw`` multicodec, SHA2-256 multihash, and lowercase
   base32 multibase over canonical AXON bytes; added URI validation.

**0.11.0a6**

#. Retracted the premature Python freeze designation after a specification-to-
   implementation audit identified substantial remaining edition work.
#. Made ``doc-link`` truthful and profile-gated, added individually addressable
   compatibility options, and implemented isolated raw-fraction, legacy escape,
   binary, comment, numeric, binding, and Python-name controls.
#. Expanded the CST from a flat lossless token list to a nested delimiter/
   production tree with multiple structural diagnostics and recovery.

**0.11.0a5**

#. Freeze-review corrections for DST folds/gaps, repeated schema-value paths,
   Core newline binding, temporal token ownership, and formatter stream boundaries.
#. Added regression coverage plus 500 seeded generated-value formatter/parser/
   canonical round trips.

**0.11.0a4**

#. Completed the remaining Python edition tooling: CST-preserving migration of
   historical indentation-bound bodies, comment-preserving structural formatting,
   and transport-neutral diagnostics, hover, symbols, and formatting edits for LSPs.
#. Added the ``tz-names`` profile with RFC 9557 bracketed IANA zones, required
   numeric offsets, instant-specific offset validation, nanosecond preservation,
   and an API that declares the active tzdb provider/version.
#. Added AXON Schema dialect identification, local JSON-Pointer-style ``$ref``,
   external registry references, cycle-safe resolution, and unresolved-reference
   diagnostics.

**0.11.0a3**

#. AXON 2026 edition fixes: the ``axon{require:[...]}`` document header now
   enables supported profiles (policy-gated by ``Options.allow_header_profiles``)
   instead of failing downstream; a numeric literal may no longer be immediately
   followed by a name or digit in Core (``1__0`` is an error, not ``1`` plus a
   node); bare-name classes now follow UAX #31 via ``str.isidentifier``.
#. Historical test suite made ``unittest discover``-importable
   (``test_suite`` package imports; ``test_odict`` stdlib ``forget`` shim).
#. ``requirements.txt`` corrected; version bumped.

**0.11.0a2**

#. First AXON 2026 edition milestone: pure-Python ``axon.v2026`` beside the
   untouched 0.9 Cython reader; historical build restored on Python 3.12
   (Cython 0.29.37 plus three ``collections.abc`` renames).

**0.9**

1. Now date/time/datetime values have prefix ``^`` in order to be more explicit 
   (``12:00`` --> ``^12:00``, ``2010-12-31`` --> ``^2010-12-31``).
   This change is important for adding support for more wider range of simple type values
   for keys in dicts in the next version ``0.10``.
   In 0.9.x loading of old notation of date/time/datetime values are allowed. 
   In ``0.10`` old notation will be removed. 
   In order to convert ``pyaxon`` text in safe mode to new date/time/datetime notation ``0.9``::

      import axon
	  
	  axon.dumps(axon.loads(text))
	  axon.dump(path, axon.load(path, text))
	  
2. Add syntax reprsenting sets. For example::
	  
	  {1 2 3}
	  {"a" "b" "c" "d"}

**0.8.2**

1. Fix dump of toplevel dict when sort=1.

**0.8.1**

1. Now node objects support access to subnodes using attribute access.
2. Now pyaxon support continues integration via appveyor.

**0.8**

1. Now name of complex value in formatted form without {} hasn't suffix ':'. For example::

      person
         name: "Alex"
         age: 36
		
   instead of::

       person:
          name: "Alex"
          age: 36

2. Introduce used defined constants using ``defname(name, value)`` function.
   Such names in ``AXON`` message always have ``$`` prefix (for example, ``$one``, ``$PI``).
3. Attributes of the ``Node`` objects are ``axon.OrederDict`` instance now to preserve order
   of attributes.
4. Introduce new syntax for oredered dict: ``[... key:val ...]`` and ``[:]`` for empty ordered dict.
   Later ``<>``-syntax for ordered dicts will be removed.
5. Extend ``AXON`` for converting text into ``axon.OrderedDict`` object  
   and dumping instances of ``collections.MutableMapping`` to text containing sequence 
   of ``key:val`` pairs. 
   For example::

		name: "Alex"
		age: 32
		email: "mail@example.com"
		
   will converted to ``axon.OrderedDict([('name','Alex'), ('age',32), ('email','mail@example.com')])``. 
		
6. ``pyaxon`` now builds with MSVC.


**0.7**

1. Safe mode loading/dumping on named complex values are based on general ``Node`` objects.
2. Attributes in safe mode are represented as ``Attribute`` objects.
3. Named complex values are now sensitive to an order of containing values and attributes.
4. The protocol for unsafe loading/dumping of named complex values is changed.
5. Old safe mode loading/dumping are still here in the ``mode='safe_old'``

**0.6**

1. Use compiled `decimal` module when possible.
2. Add syntax "< ... key:value ... >" to AXON in order to load/dump ordered dicts.
3. Add cython implementation of ordered dict `axon.odict`.
   (API compatible with `collections.OrderedDict`).
4. Fix bug with number-like string keys in dicts.

**0.5.11**

1. Add ability to dump custom class objects as dict, list or tuple.
2. Add support (`axon.convert`) to convertion of safely loaded objects to given type.
3. Fix several bugs.

Special credit to sbant.

**0.5.10**

1. Make error messages in loader more useful.
2. Refactoring of comment handling with addition of some tests.
3. Fix crossreference issue with unsafe mode of loading/dumping.
4. Add windows installers.

**0.5.9**

1. Some errors with processing of comment lines are fixed.
2. It's possible now to use "d"/"D" suffix instead of "$" to indicate decimal values.
3. Fix problem with mixing of tabs ('\t') with other spacing characters.
4. Fix example of AXON in index.rst to use "d/D" suffix for decimal values.

**0.5.8**

1. Fix 2.7/3.3 compatibility error with reading from files.
2. Pretty dumping now is more compact in simple cases.
3. Now default pretty dumping mode (``pretty=1``) is indented without braces (like YAML);
   new parameter ``braces=1`` with ``pretty=1`` specifies formatted mode with braces (like JSON).

**0.5.7**

1. Refine indentation control when loading complex objects in indented form.
2. Restore support of names as quoted strings a.k.a. ``'the name'``.
3. Make ``date/time/datetime`` creation code compatible with pure python mode.
4. Add ``hsize`` parameter in pretty dumping mode. It specifies maximum number of
   simple data items in the line.
5. Add more tests by examples.

**0.5.6**

1. Fix support for decimal ``Infinity`` and ``NaN``.
2. Fix support for ``base64`` in ``python2.7``.
3. Add support for complex names like ``a.b.c.d``.

**0.5.5**

1. Make creation of custom builders of atomic values easier too (in ``cython`` only).
2. Make creation of custom object builders easier (both in ``cython`` and ``python``).
   This allows you to implement custom import/export for data in ``XML`` and ``YAML``
   representation.
3. Add plotting of results to simple benchmark script.

**0.5.4**

1. Make internal timezone class (for ``python2.7``) compatible with datetime.timezone class (for ``python3.2`` and higher).
2. Make creation of custom object builders (both safe and unsafe) easier (in ``cython`` only).

**0.5.3**

1. Dumping is now faster.

**0.5.2**

1. Refactor setup.py so that .py sources of extensions dosn't installed.
2. Ensuire that attribute names and keys loads and dumps correctly.
3. Add explicit flag (``use_cython``) in order to decide when to use cython compiler.

**0.5.1**

1. Add notebook with performance comparisons with ``JSON`` and ``YAML``.
2. Refactor setup.py so that project could be installed with/without ``Cython`` installation.
3. Some improvements with introductory notebooks.
4. Make project uploadable to ``PyPI`` by ``setup.py``.



**0.5**

   First public release of ``pyaxon``.

0.11.0a8 (2026-07-10)
=======================

* Partial audit-remediation attempt. The later exhaustive 2026-07-11 audit
  demonstrated that canonical names, graph edge cases, compatibility fidelity,
  CST completeness, limits, packaging, and the full historical gate were still
  unresolved. This release must not be treated as a freeze baseline; a9
  supersedes it.
