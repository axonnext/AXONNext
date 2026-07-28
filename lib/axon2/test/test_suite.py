# coding: utf-8
"""Aggregate suite over the historical test modules that exist in this tree."""
import unittest

from axon2.test import (
    test_int, test_float, test_decimal, test_string, test_constant,
    test_datetime, test_element, test_mapping, test_sequence,
    test_unsafe_loads, test_crossref, test_base64, test_errors,
)

_MODULES = (
    test_int, test_float, test_decimal, test_string, test_constant,
    test_datetime, test_element, test_mapping, test_sequence,
    test_unsafe_loads, test_crossref, test_base64, test_errors,
)

def suite():
    loader = unittest.defaultTestLoader
    s = unittest.TestSuite()
    for mod in _MODULES:
        s.addTests(loader.loadTestsFromModule(mod))
    return s

if __name__ == '__main__':
    unittest.TextTestRunner().run(suite())
