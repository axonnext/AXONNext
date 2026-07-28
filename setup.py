# coding: utf-8

# The MIT License (MIT)
# 
# Copyright (c) <2011-2016> <Shibzukhov Zaur, szport at gmail dot com>
# 
# Permission is hereby granted, free of charge, to any person obtaining a copy
# of this software and associated documentation files (the "Software"), to deal
# in the Software without restriction, including without limitation the rights
# to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
# copies of the Software, and to permit persons to whom the Software is
# furnished to do so, subject to the following conditions:
# 
# The above copyright notice and this permission notice shall be included in
# all copies or substantial portions of the Software.
# 
# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
# OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
# THE SOFTWARE.

from io import open
import os

#from distutils.core import setup
from setuptools import setup

from setuptools.command.build_py import build_py as _build_py

class build_py(_build_py):
    def find_package_modules(self, package, package_dir):
        modules = _build_py.find_package_modules(self, package, package_dir)
        py_ext_modules = []
        for ext in self.distribution.ext_modules:
            for src in ext.sources:
                if src.endswith('.py'):
                    py_ext_modules.append(src)
        if py_ext_modules:
            modules = [m for m in modules if m[2] not in py_ext_modules]
        return modules

# Extension build config. Uses setuptools (distutils was removed in Python
# 3.12) and supports CPython 3.10-3.13. The checked-in generated C is produced
# by a modern Cython (3.x) so it compiles against the 3.13 C-API; set
# PYAXON_USE_CYTHON=1 to regenerate it from the Cython sources.
from setuptools import Extension
from setuptools.command.build_ext import build_ext

_ext_names = [
    ("axon2._objects", "lib/axon2/_objects"),
    ("axon2._loader", "lib/axon2/_loader"),
    ("axon2._dumper", "lib/axon2/_dumper"),
    ("axon2.odict", "lib/axon2/odict"),
]
# The odict module is a genuine .pyx; the other three are Cython-augmented .py.
_cython_source = {"axon2.odict": "lib/axon2/odict.pyx"}

use_cython = os.environ.get('PYAXON_USE_CYTHON') == '1'
if use_cython:
    try:
        from Cython.Build import cythonize
    except ImportError as exc:
        raise RuntimeError(
            'PYAXON_USE_CYTHON=1 requires an installed Cython toolchain') from exc
    _sources = [
        Extension(name, [_cython_source.get(name, base + ".py")])
        for name, base in _ext_names
    ]
    ext_modules = cythonize(
        _sources,
        compiler_directives={'language_level': '3'},
    )
else:
    ext_modules = [
        Extension(name, sources=[base + ".c"]) for name, base in _ext_names
    ]

long_description = open('README.rst', encoding='utf-8').read()


setup(
    name = 'axonnext',
    version = '1.0.0',
    description = 'Python reference implementation for AXON and the AXON 2026 edition',
    author = 'Michael Lauzon',
    author_email = 'axonnext@gmail.com',
    # Original AXON / pyaxon author (retained in LICENSE_original.txt and NOTICE):
    #   Zaur Shibzukhov <szport@gmail.com>
    license = "MIT OR Apache-2.0",
    license_files = ("LICENSE-MIT", "LICENSE-APACHE", "LICENSE_original.txt"),
    cmdclass = {'build_ext': build_ext, 'build_py': build_py},
    ext_modules = ext_modules,
    package_dir = {'': 'lib'},
    packages = ['axon2', 'axon2.test', 'axon2.test.benchmark'],
    scripts = ['bin/xml2axon', 'bin/json2axon'],
    project_urls = {
        'Repository': 'https://github.com/axonnext/AXONNext',
        # Historical upstream (project lineage), retained per the MIT license.
        'Original pyaxon repository': 'https://github.com/intellimath/pyaxon',
    },
    long_description = long_description,
    platforms = 'Linux, Mac OS X, Windows',
    keywords = ['Object Notation', 'Serialization', 'Configuration'],
    classifiers = [
        'Development Status :: 5 - Production/Stable',
        'License :: OSI Approved :: MIT License',
        'License :: OSI Approved :: Apache Software License',
        'Intended Audience :: Developers',
        'Intended Audience :: Information Technology',
        'Programming Language :: Python :: 3',
        'Programming Language :: Python :: 3.10',
        'Programming Language :: Python :: 3.11',
        'Programming Language :: Python :: 3.12',
        'Programming Language :: Python :: 3.13',
        'Operating System :: OS Independent',
        'Topic :: Software Development :: Libraries :: Python Modules'
    ],
)
