# Streaming AXON Standard

**Status:** Proposed Extension for AXON 2026
**Inspiration:** EDIFACT (Electronic Data Interchange)

## Motivation
EDIFACT thrives in supply chain logistics because its delimited segments allow processors to parse gigabytes of sequential transactions without buffering the entire document into RAM. 

Currently, standard data formats like JSON require an outer array (`[ {...}, {...} ]`) to house multiple objects, forcing strict parsers to construct an entire DOM tree in memory. While JSON Lines (NDJSON) solves this, it breaks multi-line formatting. 

AXON inherently supports multiple top-level values separated by whitespace. The **Streaming AXON Standard** formalises how systems should emit and consume continuous, unbounded data streams (such as logs, ledgers, or B2B transaction envelopes).

## Specification

### 1. Unbounded Top-Level Stream
A Streaming AXON document is a sequence of discrete top-level AXON values (typically Map or Brace Nodes). The stream is intentionally *not* enclosed in a global `[ ]` list.

```axon
Transaction { id: 1 amount: 100.5D }
Transaction { id: 2 amount: 25.0D }
Transaction { id: 3 amount: 99.9D }
```

### 2. Envelope Segments
To group related streams (analogous to EDIFACT's Interchange Headers `UNB`), Streaming AXON defines formal "Envelope" marker nodes.

- **`StreamStart{ type:"..." version:1 }`**: Marks the beginning of a related batch of records.
- **`StreamEnd{ count: 150 checksum:"..." }`**: Marks the end of a batch, allowing the parser to verify completeness.

```axon
StreamStart { type:"Orders" date:^2026-07-15 }
Order { id: "A1" qty: 5 }
Order { id: "A2" qty: 10 }
StreamEnd { count: 2 }
```

### 3. Parsing Requirements (Yielding vs Loading)
Compliant parsers implementing the Streaming Standard MUST provide an iterator/generator API (e.g., `from_stream(buffer)`) that parses a single top-level value, yields it to the consumer application, and aggressively releases it from memory before parsing the next. 

This enables `O(1)` memory overhead regardless of the AXON document's size (which may reach terabytes in big-data applications).
