.. _api:

===
API
===


.. module:: axon

This part of the documentation covers interfaces of *axon*.


-------
Classes
-------


.. autoclass:: Loader
   :members:

.. autoclass:: Dumper
   :members:
   :inherited-members:

.. autoclass:: Node
   :members:
   :inherited-members:

---------
Functions
---------

Loading and dumping
-------------------

.. autofunction:: display

.. autofunction:: load

.. autofunction:: loads

.. autofunction:: iload

.. autofunction:: iloads

.. autofunction:: dump

.. autofunction:: dumps

AXON 2026 edition
-----------------

The 2026 engine. ``Options`` selects the conformance profiles (Core, Graph,
Doc-Link, ``tz-names``) and the ``compat`` legacy-reader bundle; the ``*2026``
functions mirror the historical ``load``/``dump`` family.

.. autoclass:: Options
   :members:

Loading and dumping
^^^^^^^^^^^^^^^^^^^^

.. autofunction:: loads2026
.. autofunction:: load2026
.. autofunction:: iloads2026
.. autofunction:: iload2026
.. autofunction:: dumps2026
.. autofunction:: dump2026

Canonical form and content identifiers
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

.. autofunction:: canonical2026
.. autofunction:: verify_canonical2026
.. autofunction:: cid2026
.. autofunction:: parse_cid2026
.. autofunction:: verify_cid2026

Migration
^^^^^^^^^

.. autofunction:: migrate2026
.. autofunction:: migrate_selective2026

Factory functions for safe mode complex values
----------------------------------------------

.. autofunction:: node


----------
Exceptions
----------


.. class:: LoadExit

   Exception for exit load AXON representation from file or text.
