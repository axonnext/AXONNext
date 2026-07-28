import struct
try:
    import numpy as np
except ImportError:
    np = None
from axon2.v2026 import Node
_AXON_TO_NUMPY = {'u8': 'uint8', 'u16': 'uint16', 'u32': 'uint32', 'u64': 'uint64', 'i8': 'int8', 'i16': 'int16', 'i32': 'int32', 'i64': 'int64', 'f16': 'float16', 'f32': 'float32', 'f64': 'float64', 'c64': 'complex64', 'c128': 'complex128'}
if np is not None:
    _NUMPY_TO_AXON = {getattr(np, v): k for k, v in _AXON_TO_NUMPY.items()}
else:
    _NUMPY_TO_AXON = {}

def to_numpy(node):
    """Convert an AXON Array Node to a numpy ndarray.
    
    If numpy is not installed, raises an ImportError.
    """
    if np is None:
        raise ImportError('numpy is required to decode AXON Arrays to ndarray')
    if not isinstance(node, Node) or node.name != 'Array':
        raise ValueError('Expected an AXON Array Node')
    shape = node.attributes.get('shape', [])
    dtype_str = node.attributes.get('type', 'f32')
    data_bytes = node.attributes.get('data', b'')
    order = node.attributes.get('order', 'C')
    np_type_str = _AXON_TO_NUMPY.get(dtype_str)
    if not np_type_str:
        raise ValueError(f'Unsupported AXON array type: {dtype_str}')
    # The AXON Array wire payload is defined as little-endian regardless of the
    # host (audit H11); read it as little-endian, then return a native-order
    # array so downstream numpy ops behave normally. On little-endian hosts this
    # is identical to the historical native read.
    little = np.dtype(np_type_str).newbyteorder('<')
    arr = np.frombuffer(data_bytes, dtype=little).reshape(shape, order=order)
    return arr.astype(np.dtype(np_type_str))

def from_numpy(arr):
    """Convert a numpy ndarray to an AXON Array Node.
    
    If numpy is not installed, raises an ImportError.
    """
    if np is None:
        raise ImportError('numpy is required to encode ndarray to AXON Arrays')
    if not isinstance(arr, np.ndarray):
        arr = np.array(arr)
    if not arr.flags.c_contiguous and (not arr.flags.f_contiguous):
        arr = np.ascontiguousarray(arr)
    order = 'F' if arr.flags.f_contiguous and (not arr.flags.c_contiguous) else 'C'
    dtype_type = arr.dtype.type
    if dtype_type not in _NUMPY_TO_AXON:
        raise ValueError(f'Unsupported numpy dtype: {arr.dtype}')
    dtype_str = _NUMPY_TO_AXON[dtype_type]
    node = Node('Array', style='brace')
    node.attributes['shape'] = list(arr.shape)
    node.attributes['type'] = dtype_str
    # Serialise little-endian regardless of the source array's byte order, so a
    # big-endian (e.g., '>i4') array is not silently corrupted on round-trip
    # (audit H11).
    node.attributes['data'] = arr.astype(arr.dtype.newbyteorder('<')).tobytes(order=order)
    if order == 'F':
        node.attributes['order'] = 'F'
    return node