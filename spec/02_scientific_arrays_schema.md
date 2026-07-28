# Scientific Arrays Schema Pattern

**Status:** Proposed Extension for AXON 2026
**Inspiration:** HDF5 (Hierarchical Data Format)

## Motivation
Scientific and machine learning workloads frequently manage massive multidimensional arrays (tensors, matrices, volumes). HDF5 solves this by providing a hierarchical structure capable of hosting virtually unlimited multidimensional chunks of binary data.

AXON 2026 natively supports robust binary blobs (`|base64...|`), but the standard language lacks a unified semantic convention for declaring the *shape* and *data type* of these blobs when they represent numerical arrays. 

This proposal defines a standard Schema Pattern to bring HDF5-like scientific arrays to AXON.

## Specification

The `ScientificArray` pattern is defined as a tagged AXON Node with specific attributes, acting as an envelope for the raw binary data.

### Node Structure

```axon
Array {
  shape: [dim1 dim2 ... dimN]
  type: "datatype"
  data: |binary_payload|
}
```

### Attributes

1. **`shape` (List of Integers)**
   Defines the multidimensional bounds of the array. For example, `[1920 1080 3]` represents an image tensor.
2. **`type` (String)**
   Defines the primitive numerical type stored in the binary payload. Standard identifiers include:
   - `"u8"`, `"u16"`, `"u32"`, `"u64"` (unsigned integers)
   - `"i8"`, `"i16"`, `"i32"`, `"i64"` (signed integers)
   - `"f16"`, `"f32"`, `"f64"` (IEEE floats)
   - `"c64"`, `"c128"` (complex numbers)
3. **`data` (Binary)**
   The native AXON binary literal (`|...|`) containing the raw, closely-packed bytes in little-endian order.
4. **`order` (Optional String)**
   Either `"C"` (row-major, default) or `"F"` (column-major/Fortran).

### Example
A 2x2 matrix of 32-bit floats:

```axon
Array {
  shape: [2 2]
  type: "f32"
  data: |AAAAgAAAAIAAAACAAAAAgA==|
}
```

## Zero-Copy Deserialisation
Because the binary payload is stored contiguously, low-level parsers (like `serde_axon` in Rust or C++ ports) can provide zero-copy views directly into memory. The parser simply returns a pointer to the decoded bytes along with the `shape` and `type` attributes, enabling instant loading into scientific libraries like NumPy or PyTorch without parsing overhead.
