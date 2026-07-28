//! Robustness + parity coverage for the surfaces the 2026-07-18 audit flagged:
//! Binary AXON decoder hardening (H1), Binary encode input validation (L2), the
//! `EnvelopeStream` fail-closed count check (M1), and the `scientific` array
//! extractors (L1). The stream outcomes are pinned to the reference
//! (`axon.stream.EnvelopeStream` over `iloads2026`), which was run to produce
//! them.

use serde_axon::binary::{self, decode};
use serde_axon::scientific::{ArrayDtype, ArrayOrder, ScientificArray};
use serde_axon::stream::EnvelopeStream;
use serde_axon::value::{Node, NodeStyle};
use serde_axon::{Category, Decimal, Value, from_str_value, iter_stream};

// ---- H1: decoder never panics/aborts on malformed input ------------------- //

#[test]
fn binary_decode_rejects_malformed_without_panicking() {
    // Each of these crashed the process before the fix (overflow / capacity
    // overflow / stack overflow). They must now return `Err`, not panic -- and a
    // panic here would fail the test rather than take the process down.
    let cases: &[(&str, Vec<u8>)] = &[
        (
            "bytestr huge length",
            vec![0x5B, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
        ),
        (
            "textstr huge length",
            vec![0x7B, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
        ),
        (
            "array huge count",
            vec![0x9B, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
        ),
        (
            "map huge count",
            vec![0xBB, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
        ),
        ("truncated head", vec![0x18]),
        ("empty input", vec![]),
    ];
    for (name, bytes) in cases {
        assert!(
            decode(bytes).is_err(),
            "{name}: expected Err on malformed input"
        );
    }
}

#[test]
fn binary_decode_bounds_deep_nesting() {
    // 100k nested arrays-of-1 overflowed the stack before the depth guard.
    let mut deep = alloc_nested_arrays(100_000);
    deep.push(0xF6); // innermost value: null
    assert!(
        decode(&deep).is_err(),
        "deeply nested input must be rejected, not overflow the stack"
    );
}

fn alloc_nested_arrays(n: usize) -> Vec<u8> {
    vec![0x81u8; n] // major 4, additional-info 1: array of one element
}

#[test]
fn binary_roundtrip_stays_byte_identical() {
    // Valid documents must still round-trip, and canonical encoding must be
    // stable through a decode (encode == re-encode of the decoded value).
    for text in [
        "[1 2 3]",
        "point{x:1 y:2}",
        "[[[[[1]]]]]",
        "(1 (2 (3)))",
        "[a:1 b:2]",
        "{1 2 3}",
    ] {
        let v = from_str_value(text).expect("parse");
        let enc = binary::encode_canonical(&v).expect("encode");
        let decoded = decode(&enc).expect("decode");
        let reenc = binary::encode_canonical(&decoded).expect("re-encode");
        assert_eq!(enc, reenc, "{text}: canonical bytes changed across decode");
    }
}

// ---- Direct-API depth guards (2026-07-27 audit F6) ------------------------ //

/// Build a `Value` nested `depth` levels deep without recursing. A caller can
/// construct this without going through the parser, so the public APIs that
/// walk a `Value` need their own bound rather than relying on the parser's.
fn nested_value(depth: usize) -> Value {
    let mut value = Value::Null;
    for _ in 0..depth {
        value = Value::List(alloc_vec([value]));
    }
    value
}

/// `resolve_refs`, `GraphDocument::from_value` and `binary::encode` recursed
/// with only cycle guards, so a caller-built deep value aborted the process on
/// stack exhaustion instead of returning an error. Each must now report
/// `resource-limit-exceeded`; a regression here shows up as a crashed test
/// binary rather than a failed assertion.
#[test]
fn direct_value_apis_reject_depth_beyond_the_section_16_bound() {
    let deep = nested_value(500);

    let resolved = deep.resolve_refs();
    assert!(
        resolved.is_err(),
        "resolve_refs must bound its own recursion"
    );
    assert_eq!(
        resolved.unwrap_err().category(),
        Category::ResourceLimitExceeded
    );

    let graph = serde_axon::GraphDocument::from_value(&deep);
    assert!(
        graph.is_err(),
        "graph lowering must bound its own recursion"
    );
    assert_eq!(
        graph.unwrap_err().category(),
        Category::ResourceLimitExceeded
    );

    // encode runs graph normalisation before the depth-checked encoder, so the
    // guard has to hold on that pre-pass too.
    assert!(binary::encode(&deep).is_err());
    assert!(binary::encode_canonical(&deep).is_err());

    // Ordinary depths keep working.
    let shallow = nested_value(8);
    assert!(shallow.resolve_refs().is_ok());
    assert!(serde_axon::GraphDocument::from_value(&shallow).is_ok());
    assert!(binary::encode_canonical(&shallow).is_ok());
}

// ---- L2: encode validates integer / decimal text -------------------------- //

#[test]
fn binary_encode_rejects_invalid_int_text() {
    // A hand-built, malformed Value::Int must error, not underflow-panic.
    assert!(binary::encode(&Value::Int("12x3".into())).is_err());
    assert!(binary::encode(&Value::Int("".into())).is_err());
    assert!(binary::encode(&Value::Int("-".into())).is_err());
    // A malformed decimal mantissa likewise.
    let bad = Value::Decimal(Decimal::Finite {
        mantissa: "9z".into(),
        exponent: -1,
    });
    assert!(binary::encode(&bad).is_err());
    // Well-formed ints still encode.
    assert!(binary::encode(&Value::Int("-1000000000000000000000000".into())).is_ok());
}

// ---- M1: EnvelopeStream fail-closed count validation ---------------------- //

fn run_envelope(doc: &str) -> Result<usize, ()> {
    let mut count = 0;
    for item in EnvelopeStream::new(iter_stream(doc)) {
        item.map_err(|_| ())?;
        count += 1;
    }
    Ok(count)
}

#[test]
fn envelope_stream_matches_reference() {
    // Outcomes pinned to axon.stream.EnvelopeStream over iloads2026.
    assert_eq!(run_envelope("StreamStart 1 2 StreamEnd{count:2}"), Ok(2));
    assert_eq!(run_envelope("StreamStart 1 2 StreamEnd"), Ok(2)); // no count -> no check
    assert_eq!(run_envelope("StreamStart 1 2 StreamEnd{count:3}"), Err(())); // mismatch
    assert_eq!(
        run_envelope("StreamStart 1 2 StreamEnd{count:\"2\"}"),
        Err(())
    ); // non-int -> fail-closed
    assert_eq!(run_envelope("1 2 3"), Err(())); // data outside envelope
    assert_eq!(run_envelope("StreamStart 1 2"), Err(())); // no StreamEnd
    assert_eq!(run_envelope("StreamStart StreamEnd{count:0}"), Ok(0));
    assert_eq!(
        run_envelope("StreamStart 1 StreamEnd StreamStart 2 StreamEnd{count:1}"),
        Ok(2)
    );

    // The reference uses Python numeric equality rather than an integer-only
    // check. Explicit null is the same sentinel as an absent count.
    assert_eq!(run_envelope("StreamStart 1 StreamEnd{count:true}"), Ok(1));
    assert_eq!(run_envelope("StreamStart 1 StreamEnd{count:1.0}"), Ok(1));
    assert_eq!(run_envelope("StreamStart 1 StreamEnd{count:1e0}"), Ok(1));
    assert_eq!(run_envelope("StreamStart 1 StreamEnd{count:1D}"), Ok(1));
    assert_eq!(run_envelope("StreamStart 1 StreamEnd{count:1.00D}"), Ok(1));
    assert_eq!(run_envelope("StreamStart StreamEnd{count:false}"), Ok(0));
    assert_eq!(run_envelope("StreamStart StreamEnd{count:-0.0}"), Ok(0));
    assert_eq!(run_envelope("StreamStart StreamEnd{count:-0D}"), Ok(0));
    assert_eq!(run_envelope("StreamStart 1 2 StreamEnd{count:null}"), Ok(2));
    assert_eq!(run_envelope("StreamStart 1 StreamEnd{count:1.5}"), Err(()));
    assert_eq!(run_envelope("StreamStart 1 StreamEnd{count:0.1D}"), Err(()));
    assert_eq!(run_envelope("StreamStart 1 StreamEnd{count:?}"), Err(()));
    assert_eq!(run_envelope("StreamStart StreamEnd{count:-1}"), Err(()));
    assert_eq!(
        run_envelope("StreamStart StreamEnd{count:999999999999999999999999999999999999999999}"),
        Err(())
    );
}

fn assert_fused<I: core::iter::FusedIterator>(_iterator: &I) {}

#[test]
fn envelope_stream_implicit_session_errors_once_then_is_done() {
    let mut abrupt = EnvelopeStream::new(iter_stream("StreamStart"));
    assert_fused(&abrupt);
    let error = abrupt.next().unwrap().unwrap_err();
    assert_eq!(error.category(), Category::UnexpectedEnd);
    assert!(abrupt.next().is_none());
    assert!(abrupt.next().is_none());

    let mut outside = EnvelopeStream::new(iter_stream("0 StreamStart 1 StreamEnd"));
    assert_eq!(
        outside.next().unwrap().unwrap_err().message,
        "Received data outside of a stream envelope: 0"
    );
    assert!(
        outside.next().is_none(),
        "records after an error must be hidden from the failed session"
    );
    let mut resumed = outside.session();
    assert_eq!(resumed.next().unwrap().unwrap(), Value::Int("1".into()));
    assert!(resumed.next().is_none());

    let mut nested = EnvelopeStream::new(iter_stream(
        "StreamStart 1 StreamStart 2 StreamEnd{count:2}",
    ));
    assert_eq!(nested.next().unwrap().unwrap(), Value::Int("1".into()));
    assert!(nested.next().unwrap().is_err());
    assert!(nested.next().is_none());

    let mut mismatch = EnvelopeStream::new(iter_stream("StreamStart 1 StreamEnd{count:2}"));
    assert!(mismatch.next().unwrap().is_ok());
    assert!(mismatch.next().unwrap().is_err());
    assert!(mismatch.next().is_none());

    let mut float_mismatch = EnvelopeStream::new(iter_stream("StreamStart 1 StreamEnd{count:2.0}"));
    assert!(float_mismatch.next().unwrap().is_ok());
    assert_eq!(
        float_mismatch.next().unwrap().unwrap_err().message,
        "StreamEnd count mismatch: expected 2.0, got 1"
    );

    let mut parse_error = EnvelopeStream::new(iter_stream("StreamStart 1 @ StreamEnd"));
    assert!(parse_error.next().unwrap().is_ok());
    assert!(parse_error.next().unwrap().is_err());
    assert!(parse_error.next().is_none());
}

fn stream_marker(name: &str, count: Option<Value>) -> Value {
    Value::Node(Box::new(Node {
        name: name.into(),
        style: if count.is_some() {
            NodeStyle::Brace
        } else {
            NodeStyle::Unit
        },
        attributes: count
            .into_iter()
            .map(|value| ("count".into(), value))
            .collect(),
        children: Vec::new(),
    }))
}

#[test]
fn envelope_stream_new_sessions_resume_shared_python_state() {
    let values = vec![
        Ok(Value::Int("0".into())),
        Ok(stream_marker("StreamStart", None)),
        Ok(Value::Int("1".into())),
        Ok(stream_marker("StreamEnd", Some(Value::Int("1".into())))),
    ];
    let mut outside = EnvelopeStream::new(values.into_iter());
    {
        let mut first = outside.session();
        assert_fused(&first);
        assert!(first.next().unwrap().is_err());
        assert!(first.next().is_none());
    }
    assert!(!outside.in_stream());
    {
        let mut resumed = outside.session();
        assert_eq!(resumed.next().unwrap().unwrap(), Value::Int("1".into()));
        assert!(resumed.next().is_none());
    }

    let values = vec![
        Ok(stream_marker("StreamStart", None)),
        Ok(Value::Int("1".into())),
        Ok(stream_marker("StreamStart", None)),
        Ok(Value::Int("2".into())),
        Ok(stream_marker("StreamEnd", Some(Value::Int("2".into())))),
    ];
    let mut nested = EnvelopeStream::new(values.into_iter());
    {
        let mut first = nested.session();
        assert_eq!(first.next().unwrap().unwrap(), Value::Int("1".into()));
        assert!(first.next().unwrap().is_err());
        assert!(first.next().is_none());
    }
    assert!(nested.in_stream());
    assert_eq!(nested.record_count(), 1);
    {
        let mut resumed = nested.iter();
        assert_eq!(resumed.next().unwrap().unwrap(), Value::Int("2".into()));
        assert!(resumed.next().is_none());
    }

    let values = vec![
        Ok(stream_marker("StreamStart", None)),
        Ok(Value::Int("1".into())),
        Ok(stream_marker("StreamEnd", Some(Value::Int("2".into())))),
        Ok(Value::Int("2".into())),
        Ok(stream_marker("StreamEnd", Some(Value::Int("2".into())))),
    ];
    let mut mismatch = EnvelopeStream::new(values.into_iter());
    {
        let mut first = mismatch.session();
        assert!(first.next().unwrap().is_ok());
        assert!(first.next().unwrap().is_err());
    }
    assert!(mismatch.in_stream());
    assert_eq!(mismatch.record_count(), 1);
    {
        let mut resumed = mismatch.session();
        assert_eq!(resumed.next().unwrap().unwrap(), Value::Int("2".into()));
        assert!(resumed.next().is_none());
    }

    let values = vec![
        Ok(stream_marker("StreamStart", None)),
        Ok(Value::Int("1".into())),
        Err(serde_axon::Error::new(
            Category::UnexpectedToken,
            "recoverable upstream error",
            0,
            0,
        )),
        Ok(Value::Int("2".into())),
        Ok(stream_marker("StreamEnd", Some(Value::Int("2".into())))),
    ];
    let mut upstream_error = EnvelopeStream::new(values.into_iter());
    {
        let mut first = upstream_error.session();
        assert!(first.next().unwrap().is_ok());
        assert!(first.next().unwrap().is_err());
    }
    assert!(upstream_error.in_stream());
    {
        let mut resumed = upstream_error.session();
        assert_eq!(resumed.next().unwrap().unwrap(), Value::Int("2".into()));
        assert!(resumed.next().is_none());
    }

    let values = vec![
        Ok(stream_marker("StreamStart", None)),
        Ok(Value::Int("1".into())),
    ];
    let mut abrupt = EnvelopeStream::new(values.into_iter());
    {
        let mut first = abrupt.session();
        assert!(first.next().unwrap().is_ok());
        assert!(first.next().unwrap().is_err());
        assert!(first.next().is_none());
    }
    assert!(abrupt.in_stream());
    for _ in 0..2 {
        let mut resumed = abrupt.session();
        assert_eq!(
            resumed.next().unwrap().unwrap_err().category(),
            Category::UnexpectedEnd
        );
        assert!(resumed.next().is_none());
    }
}

// ---- L1: scientific array extractors -------------------------------------- //

fn array_node(dtype: &str, data: Vec<u8>, shape: usize) -> Value {
    array_node_with(
        Value::List(alloc_vec([Value::Int(shape.to_string())])),
        Some(Value::String(dtype.to_string())),
        Some(Value::Bytes(data)),
        None,
        NodeStyle::Brace,
        Vec::new(),
    )
}

fn array_node_with(
    shape: Value,
    dtype: Option<Value>,
    data: Option<Value>,
    order: Option<Value>,
    style: NodeStyle,
    children: Vec<Value>,
) -> Value {
    let mut attributes = vec![("shape".to_string(), shape)];
    if let Some(dtype) = dtype {
        attributes.push(("type".to_string(), dtype));
    }
    if let Some(data) = data {
        attributes.push(("data".to_string(), data));
    }
    if let Some(order) = order {
        attributes.push(("order".to_string(), order));
    }
    Value::Node(Box::new(Node {
        name: "Array".into(),
        style,
        attributes,
        children,
    }))
}

fn alloc_vec<T, const N: usize>(items: [T; N]) -> Vec<T> {
    items.into_iter().collect()
}

#[test]
fn scientific_extracts_typed_vectors() {
    // f32
    let mut f32_data = Vec::new();
    f32_data.extend_from_slice(&1.5f32.to_ne_bytes());
    f32_data.extend_from_slice(&(-2.0f32).to_ne_bytes());
    let arr = ScientificArray::from_value(array_node("f32", f32_data, 2)).unwrap();
    assert_eq!(arr.dtype(), Some(ArrayDtype::F32));
    assert_eq!(arr.shape(), vec![2]);
    assert_eq!(arr.to_f32_vec().unwrap(), vec![1.5, -2.0]);
    // dtype mismatch is an error, not a misread
    assert!(arr.to_i32_vec().is_err());

    // f64
    let mut f64_data = Vec::new();
    f64_data.extend_from_slice(&3.25f64.to_ne_bytes());
    let arr = ScientificArray::from_value(array_node("f64", f64_data, 1)).unwrap();
    assert_eq!(arr.to_f64_vec().unwrap(), vec![3.25]);

    // i32
    let mut i32_data = Vec::new();
    i32_data.extend_from_slice(&(-7i32).to_ne_bytes());
    i32_data.extend_from_slice(&42i32.to_ne_bytes());
    let arr = ScientificArray::from_value(array_node("i32", i32_data, 2)).unwrap();
    assert_eq!(arr.to_i32_vec().unwrap(), vec![-7, 42]);

    // u8
    let arr = ScientificArray::from_value(array_node("u8", vec![1, 2, 3], 3)).unwrap();
    assert_eq!(arr.to_u8_vec().unwrap(), vec![1, 2, 3]);

    // ragged data length is rejected
    let arr = ScientificArray::from_value(array_node("f32", vec![0, 1, 2], 1)).unwrap();
    assert!(arr.to_f32_vec().is_err());
}

#[test]
fn scientific_dtype_registry_and_extended_extractors() {
    for (tag, dtype, width) in [
        ("u8", ArrayDtype::U8, 1),
        ("u16", ArrayDtype::U16, 2),
        ("u32", ArrayDtype::U32, 4),
        ("u64", ArrayDtype::U64, 8),
        ("i8", ArrayDtype::I8, 1),
        ("i16", ArrayDtype::I16, 2),
        ("i32", ArrayDtype::I32, 4),
        ("i64", ArrayDtype::I64, 8),
        ("f16", ArrayDtype::F16, 2),
        ("f32", ArrayDtype::F32, 4),
        ("f64", ArrayDtype::F64, 8),
        ("c64", ArrayDtype::C64, 8),
        ("c128", ArrayDtype::C128, 16),
    ] {
        assert_eq!(ArrayDtype::from_tag(tag), Some(dtype));
        assert_eq!(dtype.tag(), tag);
        assert_eq!(dtype.element_size(), width);
    }
    assert_eq!(ArrayDtype::from_tag("F32"), None);

    let f16 = ScientificArray::from_value(array_node("f16", 0x3c00u16.to_ne_bytes().to_vec(), 1))
        .unwrap();
    assert_eq!(f16.to_f16_bits_vec().unwrap(), vec![0x3c00]);

    let mut c64_bytes = Vec::new();
    c64_bytes.extend_from_slice(&1.5f32.to_ne_bytes());
    c64_bytes.extend_from_slice(&(-2.0f32).to_ne_bytes());
    let c64 = ScientificArray::from_value(array_node("c64", c64_bytes, 1)).unwrap();
    assert_eq!(c64.to_c64_components_vec().unwrap(), vec![(1.5, -2.0)]);

    let mut c128_bytes = Vec::new();
    c128_bytes.extend_from_slice(&3.25f64.to_ne_bytes());
    c128_bytes.extend_from_slice(&(-4.5f64).to_ne_bytes());
    let c128 = ScientificArray::from_value(array_node("c128", c128_bytes, 1)).unwrap();
    assert_eq!(c128.to_c128_components_vec().unwrap(), vec![(3.25, -4.5)]);
}

#[test]
fn scientific_checked_shape_and_layout_reject_malformed_arrays() {
    let invalid_shapes = [
        Value::String("not-a-list".into()),
        Value::List(vec![Value::String("2".into())]),
        Value::List(vec![Value::Int(format!("{}0", usize::MAX))]),
    ];
    for shape in invalid_shapes {
        let array = ScientificArray::from_value(array_node_with(
            shape,
            Some(Value::String("u8".into())),
            Some(Value::Bytes(Vec::new())),
            None,
            NodeStyle::Brace,
            Vec::new(),
        ))
        .unwrap();
        assert!(array.try_shape().is_err());
        assert!(array.validate_layout().is_err());
        assert!(array.validate_strict_layout().is_err());
    }

    // numpy treats one arbitrary negative dimension as inferred, including for
    // an empty payload. The strict extension still rejects it.
    let inferred = ScientificArray::from_value(array_node_with(
        Value::List(vec![Value::Int("-7".into())]),
        Some(Value::String("u8".into())),
        Some(Value::Bytes(Vec::new())),
        None,
        NodeStyle::Brace,
        Vec::new(),
    ))
    .unwrap();
    assert_eq!(inferred.resolved_shape().unwrap(), vec![0]);
    assert!(inferred.validate_strict_layout().is_err());

    let mismatch = ScientificArray::from_value(array_node_with(
        Value::List(vec![Value::Int("2".into()), Value::Int("2".into())]),
        Some(Value::String("f32".into())),
        Some(Value::Bytes(vec![0; 8])),
        None,
        NodeStyle::Brace,
        Vec::new(),
    ))
    .unwrap();
    assert!(mismatch.validate_layout().is_err());
    assert!(mismatch.to_f32_vec().is_err());

    let overflow = ScientificArray::from_value(array_node_with(
        Value::List(vec![
            Value::Int(usize::MAX.to_string()),
            Value::Int("2".into()),
        ]),
        Some(Value::String("u8".into())),
        Some(Value::Bytes(Vec::new())),
        None,
        NodeStyle::Brace,
        Vec::new(),
    ))
    .unwrap();
    assert!(overflow.validate_layout().is_err());
    assert_eq!(
        overflow.validate_strict_layout().unwrap_err().category(),
        Category::ResourceLimitExceeded
    );

    let automatic = ScientificArray::from_value(array_node_with(
        Value::List(vec![Value::Int("1".into())]),
        Some(Value::String("u8".into())),
        Some(Value::Bytes(vec![1])),
        Some(Value::String("A".into())),
        NodeStyle::Brace,
        Vec::new(),
    ))
    .unwrap();
    assert_eq!(automatic.numpy_layout().unwrap().order(), ArrayOrder::C);
    assert!(automatic.validate_strict_layout().is_err());

    let non_string_order = ScientificArray::from_value(array_node_with(
        Value::List(vec![Value::Int("1".into())]),
        Some(Value::String("u8".into())),
        Some(Value::Bytes(vec![1])),
        Some(Value::Int("1".into())),
        NodeStyle::Brace,
        Vec::new(),
    ))
    .unwrap();
    assert!(non_string_order.validate_layout().is_err());

    let missing_type = ScientificArray::from_value(array_node_with(
        Value::List(vec![Value::Int("0".into())]),
        None,
        Some(Value::Bytes(Vec::new())),
        None,
        NodeStyle::Brace,
        Vec::new(),
    ))
    .unwrap();
    assert!(missing_type.validate_layout().is_ok());
    assert_eq!(
        missing_type.numpy_layout().unwrap().dtype(),
        ArrayDtype::F32
    );
    assert!(missing_type.validate_strict_layout().is_err());

    let missing_data = ScientificArray::from_value(array_node_with(
        Value::List(vec![Value::Int("0".into())]),
        Some(Value::String("u8".into())),
        None,
        None,
        NodeStyle::Brace,
        Vec::new(),
    ))
    .unwrap();
    assert!(missing_data.validate_layout().is_ok());
    assert!(missing_data.numpy_layout().unwrap().data().is_empty());
    assert!(missing_data.validate_strict_layout().is_err());

    let missing_shape = ScientificArray::from_value(Value::Node(Box::new(Node {
        name: "Array".into(),
        style: NodeStyle::Brace,
        attributes: vec![
            ("type".into(), Value::String("u8".into())),
            ("data".into(), Value::Bytes(Vec::new())),
        ],
        children: Vec::new(),
    })))
    .unwrap();
    assert!(missing_shape.try_shape().is_err());
    assert!(missing_shape.validate_layout().is_err());

    for (dtype, data) in [
        (Value::String("unknown".into()), Value::Bytes(vec![1])),
        (
            Value::String("u8".into()),
            Value::String("not-bytes".into()),
        ),
    ] {
        let array = ScientificArray::from_value(array_node_with(
            Value::List(vec![Value::Int("1".into())]),
            Some(dtype),
            Some(data),
            None,
            NodeStyle::Brace,
            Vec::new(),
        ))
        .unwrap();
        assert!(array.validate_layout().is_err());
    }

    let wrong_style = ScientificArray::from_value(array_node_with(
        Value::List(vec![Value::Int("0".into())]),
        Some(Value::String("u8".into())),
        Some(Value::Bytes(Vec::new())),
        None,
        NodeStyle::Unit,
        Vec::new(),
    ))
    .unwrap();
    assert!(wrong_style.validate_layout().is_ok());
    assert!(wrong_style.validate_strict_layout().is_err());

    let with_child = ScientificArray::from_value(array_node_with(
        Value::List(vec![Value::Int("0".into())]),
        Some(Value::String("u8".into())),
        Some(Value::Bytes(Vec::new())),
        None,
        NodeStyle::Brace,
        vec![Value::Null],
    ))
    .unwrap();
    assert!(with_child.validate_layout().is_ok());
    assert!(with_child.validate_strict_layout().is_err());
}

#[test]
fn scientific_layout_accepts_scalar_zero_and_fortran_and_roundtrips_node() {
    let scalar_bytes = 7u32.to_ne_bytes().to_vec();
    let scalar_value = array_node_with(
        Value::List(Vec::new()),
        Some(Value::String("u32".into())),
        Some(Value::Bytes(scalar_bytes)),
        None,
        NodeStyle::Brace,
        Vec::new(),
    );
    let scalar = ScientificArray::from_value(scalar_value.clone()).unwrap();
    assert_eq!(scalar.try_shape().unwrap(), Vec::<usize>::new());
    assert!(scalar.validate_layout().is_ok());
    assert_eq!(scalar.as_node().name, "Array");
    assert_eq!(scalar.into_value(), scalar_value);

    let zero = ScientificArray::from_value(array_node_with(
        Value::List(vec![Value::Int("4".into()), Value::Int("0".into())]),
        Some(Value::String("f64".into())),
        Some(Value::Bytes(Vec::new())),
        Some(Value::String("F".into())),
        NodeStyle::Brace,
        Vec::new(),
    ))
    .unwrap();
    assert_eq!(zero.order(), "F");
    assert!(zero.validate_layout().is_ok());
    assert!(zero.to_f64_vec().unwrap().is_empty());
}

#[test]
fn scientific_numpy_defaults_shape_forms_orders_and_ignored_structure_match_reference() {
    let missing_shape = ScientificArray::from_value(Value::Node(Box::new(Node {
        name: "Array".into(),
        style: NodeStyle::Unit,
        attributes: vec![
            ("type".into(), Value::String("u8".into())),
            ("data".into(), Value::Bytes(vec![7])),
        ],
        children: vec![Value::String("ignored".into())],
    })))
    .unwrap();
    let layout = missing_shape.numpy_layout().unwrap();
    assert_eq!(layout.shape(), &[]);
    assert_eq!(layout.dtype(), ArrayDtype::U8);
    assert_eq!(missing_shape.to_u8_vec().unwrap(), vec![7]);
    assert!(missing_shape.validate_strict_layout().is_err());

    let missing_type = ScientificArray::from_value(array_node_with(
        Value::List(vec![Value::Int("1".into())]),
        None,
        Some(Value::Bytes(1.5f32.to_ne_bytes().to_vec())),
        None,
        NodeStyle::Brace,
        Vec::new(),
    ))
    .unwrap();
    assert_eq!(
        missing_type.numpy_layout().unwrap().dtype(),
        ArrayDtype::F32
    );
    assert_eq!(missing_type.to_f32_vec().unwrap(), vec![1.5]);

    let missing_data = ScientificArray::from_value(array_node_with(
        Value::List(vec![Value::Int("0".into())]),
        Some(Value::String("u16".into())),
        None,
        None,
        NodeStyle::Brace,
        Vec::new(),
    ))
    .unwrap();
    assert!(missing_data.to_u16_vec().unwrap().is_empty());

    for (shape, expected) in [
        (Value::Null, vec![2]),
        (Value::Int("2".into()), vec![2]),
        (Value::Tuple(vec![Value::Int("2".into())]), vec![2]),
        (Value::List(vec![Value::Int("-7".into())]), vec![2]),
        (
            Value::List(vec![Value::Int("1".into()), Value::Int("-2".into())]),
            vec![1, 2],
        ),
    ] {
        let array = ScientificArray::from_value(array_node_with(
            shape,
            Some(Value::String("u8".into())),
            Some(Value::Bytes(vec![1, 2])),
            None,
            NodeStyle::Brace,
            Vec::new(),
        ))
        .unwrap();
        assert_eq!(array.resolved_shape().unwrap(), expected);
    }

    for shape in [
        Value::List(vec![Value::Int("-1".into()), Value::Int("-2".into())]),
        Value::List(vec![Value::Int("0".into()), Value::Int("-1".into())]),
    ] {
        let array = ScientificArray::from_value(array_node_with(
            shape,
            Some(Value::String("u8".into())),
            Some(Value::Bytes(Vec::new())),
            None,
            NodeStyle::Brace,
            Vec::new(),
        ))
        .unwrap();
        assert!(array.validate_layout().is_err());
    }

    for (order, expected) in [
        (None, ArrayOrder::C),
        (Some(Value::Null), ArrayOrder::C),
        (Some(Value::String("c".into())), ArrayOrder::C),
        (Some(Value::String("A".into())), ArrayOrder::C),
        (Some(Value::String("f".into())), ArrayOrder::F),
    ] {
        let array = ScientificArray::from_value(array_node_with(
            Value::List(vec![Value::Int("1".into())]),
            Some(Value::String("u8".into())),
            Some(Value::Bytes(vec![1])),
            order,
            NodeStyle::Brace,
            Vec::new(),
        ))
        .unwrap();
        assert_eq!(array.numpy_layout().unwrap().order(), expected);
    }
}

#[test]
fn scientific_native_constructors_cover_every_dtype_and_numeric_f16() {
    let arrays = vec![
        ScientificArray::from_u8_vec(vec![1], vec![1], ArrayOrder::C).unwrap(),
        ScientificArray::from_u16_vec(vec![1], vec![2], ArrayOrder::C).unwrap(),
        ScientificArray::from_u32_vec(vec![1], vec![3], ArrayOrder::C).unwrap(),
        ScientificArray::from_u64_vec(vec![1], vec![4], ArrayOrder::C).unwrap(),
        ScientificArray::from_i8_vec(vec![1], vec![-1], ArrayOrder::C).unwrap(),
        ScientificArray::from_i16_vec(vec![1], vec![-2], ArrayOrder::C).unwrap(),
        ScientificArray::from_i32_vec(vec![1], vec![-3], ArrayOrder::C).unwrap(),
        ScientificArray::from_i64_vec(vec![1], vec![-4], ArrayOrder::C).unwrap(),
        ScientificArray::from_f16_bits_vec(vec![1], vec![0x3e00], ArrayOrder::C).unwrap(),
        ScientificArray::from_f32_vec(vec![1], vec![1.5], ArrayOrder::C).unwrap(),
        ScientificArray::from_f64_vec(vec![1], vec![2.5], ArrayOrder::C).unwrap(),
        ScientificArray::from_c64_components_vec(vec![1], vec![(1.0, -2.0)], ArrayOrder::C)
            .unwrap(),
        ScientificArray::from_c128_components_vec(vec![1], vec![(3.0, -4.0)], ArrayOrder::F)
            .unwrap(),
    ];
    let expected = [
        ArrayDtype::U8,
        ArrayDtype::U16,
        ArrayDtype::U32,
        ArrayDtype::U64,
        ArrayDtype::I8,
        ArrayDtype::I16,
        ArrayDtype::I32,
        ArrayDtype::I64,
        ArrayDtype::F16,
        ArrayDtype::F32,
        ArrayDtype::F64,
        ArrayDtype::C64,
        ArrayDtype::C128,
    ];
    for (array, dtype) in arrays.iter().zip(expected) {
        assert_eq!(array.numpy_layout().unwrap().dtype(), dtype);
        assert!(array.validate_layout().is_ok());
    }
    assert_eq!(arrays[8].to_f16_vec().unwrap(), vec![1.5]);
    assert_eq!(
        arrays[11].to_c64_components_vec().unwrap(),
        vec![(1.0, -2.0)]
    );
    assert_eq!(
        arrays[12].to_c128_components_vec().unwrap(),
        vec![(3.0, -4.0)]
    );
    assert_eq!(arrays[12].numpy_layout().unwrap().order(), ArrayOrder::F);
    assert_eq!(arrays[12].as_node().attributes.len(), 4);

    assert!(
        ScientificArray::from_native_bytes(
            vec![2],
            ArrayDtype::U32,
            1u32.to_ne_bytes().to_vec(),
            ArrayOrder::C,
        )
        .is_err()
    );
}

// ---- M4 (2026-07-19 audit): regex grammar matches CPython's parser -------- //

/// Every verdict below was produced by running the same pattern through
/// CPython 3.13 `re.compile` -- the pyaxon schema `pattern` authority.
#[test]
fn regex_grammar_matches_python_accept_reject_verdicts() {
    use serde_axon::regex::safe_regex_search;

    let accepted = [
        "(?P<x>a)(?P<y>b)",
        "(?a:a)",
        "(?u:a)",
        "(?a)x",
        "(?i-s:a)",
        "(?P<_x1>a)",
        "(?P<x1>a)",
        "(?P<\u{03a9}>a)",
        "(?ii:a)",
        "(?a-i:x)",
        "(?aa:a)",
        "(?uu:a)",
    ];
    for pattern in accepted {
        assert!(
            safe_regex_search(pattern, "ax").is_ok(),
            "python accepts {pattern:?}"
        );
    }

    let rejected = [
        "(?P<1>a)",
        "(?P<a-b>a)",
        "(?P<1x>a)",
        "(?P<x>a)(?P<x>b)",
        "(?au:a)",
        "(?ua:a)",
        "(?-a:a)",
        "(?-u:a)",
        "(?i-a:a)",
        "x(?a)",
        "(?-i)x",
        "(?-:a)",
        "(?i-:a)",
        "(?i-ii:a)",
        "(?i-si:a)",
    ];
    for pattern in rejected {
        assert!(
            safe_regex_search(pattern, "ax").is_err(),
            "python rejects {pattern:?}"
        );
    }
}

// ---- M6 (2026-07-19 audit): writer refuses values its parser rejects ------ //

/// Contradictory caller-built nodes and duplicate containers must fail loudly
/// instead of silently dropping data or emitting unloadable text. Verdicts and
/// messages are pinned to `axon.v2026.dumps2026` after the same fix there.
#[test]
fn writer_rejects_contradictory_nodes_and_duplicate_containers() {
    use serde_axon::writer::to_string;

    let unit_with_children = Value::Node(Box::new(Node {
        name: "a".into(),
        style: NodeStyle::Unit,
        attributes: vec![],
        children: vec![Value::Int("1".into())],
    }));
    assert!(to_string(&unit_with_children).is_err());

    let tuple_with_attrs = Value::Node(Box::new(Node {
        name: "a".into(),
        style: NodeStyle::Tuple,
        attributes: vec![("x".into(), Value::Int("1".into()))],
        children: vec![Value::Int("2".into())],
    }));
    assert!(to_string(&tuple_with_attrs).is_err());

    let dup_attrs = Value::Node(Box::new(Node {
        name: "a".into(),
        style: NodeStyle::Brace,
        attributes: vec![
            ("x".into(), Value::Int("1".into())),
            ("x".into(), Value::Int("2".into())),
        ],
        children: vec![],
    }));
    assert!(to_string(&dup_attrs).is_err());

    let dup_set = Value::Set(vec![Value::Int("1".into()), Value::Int("1".into())]);
    assert!(to_string(&dup_set).is_err());

    let dup_map = Value::Map(vec![
        (Value::String("k".into()), Value::Int("1".into())),
        (Value::String("k".into()), Value::Int("2".into())),
    ]);
    assert!(to_string(&dup_map).is_err());

    // Healthy shapes still emit exactly what the reference emits.
    let ok_node = Value::Node(Box::new(Node {
        name: "a".into(),
        style: NodeStyle::Brace,
        attributes: vec![("x".into(), Value::Int("1".into()))],
        children: vec![Value::Int("2".into())],
    }));
    assert_eq!(to_string(&ok_node).unwrap(), "a{x:1 2}");
    let ok_set = Value::Set(vec![Value::Int("1".into()), Value::Int("2".into())]);
    assert_eq!(to_string(&ok_set).unwrap(), "{1 2}");
}
