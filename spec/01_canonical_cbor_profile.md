# Canonical CBOR Profile for Binary AXON

**Status:** Proposed Extension for AXON 2026
**Inspiration:** ASN.1 DER (Distinguished Encoding Rules)

## Motivation
While AXON 2026 defines a "Canonical Text" form, many enterprise and cryptographic use-cases (e.g., PKI, digital signatures, blockchain) require a deterministic *binary* representation. ASN.1 DER guarantees that any specific data structure has exactly *one* valid byte representation. 

The existing **Binary AXON** specification uses a CBOR profile, but CBOR inherently allows multiple valid encodings for the same data (e.g., a short integer can be encoded as 1 byte, 2 bytes, 4 bytes, or 8 bytes, and map keys can be ordered arbitrarily). 

This proposal defines a **Canonical CBOR Profile** for AXON that acts similarly to ASN.1 DER, removing all encoding ambiguities.

## Specification Rules

To produce Canonical Binary AXON, the encoder MUST adhere to the following deterministic rules:

### 1. Minimal Encoding
Integers, lengths of strings, lengths of bytes, and array/map item counts MUST be encoded using the shortest possible CBOR byte sequence. Padding or using larger integer sizes than necessary is strictly forbidden. 
- Example: The value `24` must be encoded as `18 18` (1-byte following unsigned integer) and never as `19 00 18` (2-byte unsigned integer).

### 2. Strict Map Key Sorting
All maps and object properties MUST be sorted by their binary representation. 
- Keys are sorted lexicographically by their encoded CBOR byte sequence.
- If two keys have different lengths, the shorter byte sequence precedes the longer one, padding with `0x00` if necessary for the tie-break, in accordance with RFC 7049 (Section 3.9).

### 3. Definite Lengths Only
Indefinite-length encoding for byte strings, text strings, arrays, and maps is strictly forbidden. The encoder MUST pre-calculate the size and use the definite-length CBOR token.

### 4. Deterministic Float Encoding
Floating-point numbers MUST be encoded in the smallest float size (half-precision `float16`, single-precision `float32`, or double-precision `float64`) that preserves the value without loss of precision. NaN values MUST be encoded using the exact bit pattern `0x7E00` (`float16` NaN).

### 5. Tag Consistency
AXON Node names and references MUST be encoded using the agreed-upon Binary AXON tags strictly. No alias tags or custom extension tags may be injected into a Canonical representation.

## Cryptographic Guarantees
By enforcing these rules, any two AXON encoders given the exact same semantic AXON graph will produce an identical SHA-256 hash. This unlocks native integration with cryptographic suites without needing a custom serialisation bridge.
