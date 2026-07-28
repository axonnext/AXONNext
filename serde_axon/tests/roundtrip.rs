//! Serde round-trip: derived Rust types must survive to_string -> from_str.
use serde::{Deserialize, Serialize};
use serde_axon::{from_str, to_string, to_string_canonical};

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Point {
    x: i32,
    y: i32,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Nested {
    name: String,
    tags: Vec<String>,
    point: Point,
    ratio: f64,
    on: bool,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
enum Shape {
    Unit,
    Circle(f64),
    Rect { w: i32, h: i32 },
    Triple(i32, i32, i32),
}

fn rt<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug>(v: T) {
    let s = to_string(&v).unwrap();
    let back: T = from_str(&s).unwrap_or_else(|e| panic!("from_str({s:?}) failed: {e}"));
    assert_eq!(v, back, "round-trip mismatch via {s:?}");
    // canonical form must also read back
    let sc = to_string_canonical(&v).unwrap();
    let back2: T =
        from_str(&sc).unwrap_or_else(|e| panic!("from_str(canonical {sc:?}) failed: {e}"));
    assert_eq!(v, back2, "canonical round-trip mismatch via {sc:?}");
}

#[test]
fn primitives() {
    rt(42i32);
    rt(-7i64);
    rt(3.5f64);
    rt(true);
    rt(false);
    rt("hello".to_string());
    rt(String::new());
    rt(Option::<i32>::None);
    rt(Some(5i32));
}

#[test]
fn structs_and_seqs() {
    rt(Point { x: 1, y: -2 });
    rt(vec![1i32, 2, 3]);
    rt((1i32, "two".to_string(), 3.0f64));
    rt(Nested {
        name: "n".into(),
        tags: vec!["a".into(), "b".into()],
        point: Point { x: 10, y: 20 },
        ratio: 0.5,
        on: true,
    });
}

#[test]
fn enums_external_tagging() {
    rt(Shape::Unit);
    rt(Shape::Circle(2.5));
    rt(Shape::Rect { w: 3, h: 4 });
    rt(Shape::Triple(1, 2, 3));
    rt(vec![
        Shape::Unit,
        Shape::Circle(1.0),
        Shape::Rect { w: 1, h: 1 },
    ]);
}

#[test]
fn maps() {
    use std::collections::BTreeMap;
    let mut m = BTreeMap::new();
    m.insert("a".to_string(), 1i32);
    m.insert("b".to_string(), 2);
    rt(m);
}
