from collections import OrderedDict
__all__ = ['OrderedDict', 'c_new_odict']

def c_new_odict(items=()):
    """Build an :class:`OrderedDict` from *items*.

Pure-Python stand-in for the compiled ``odict`` constructor.
    """
    return OrderedDict(items)