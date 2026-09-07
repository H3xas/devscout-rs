use super::*;

// --- OrderedMap: order + round-trip -------------------------------

#[test]
fn ordered_map_preserves_insertion_order_through_json_round_trip() {
    let mut m: OrderedMap<i64> = OrderedMap::new();
    m.insert("z.cs".to_string(), 3);
    m.insert("a.cs".to_string(), 1);
    m.insert("m.cs".to_string(), 2);
    let json = serde_json::to_string(&m).unwrap();
    assert_eq!(json, r#"{"z.cs":3,"a.cs":1,"m.cs":2}"#);

    let reparsed: OrderedMap<i64> = serde_json::from_str(&json).unwrap();
    let keys: Vec<&String> = reparsed.iter().map(|(k, _)| k).collect();
    assert_eq!(keys, vec!["z.cs", "a.cs", "m.cs"]);
}

#[test]
fn ordered_map_insert_overwrite_keeps_original_position() {
    let mut m: OrderedMap<i64> = OrderedMap::new();
    m.insert("a".to_string(), 1);
    m.insert("b".to_string(), 2);
    m.insert("a".to_string(), 99);
    let keys: Vec<&String> = m.iter().map(|(k, _)| k).collect();
    assert_eq!(keys, vec!["a", "b"]);
    assert_eq!(m.get("a"), Some(&99));
}

// --- Percent1: number-shaped formatting -------------------------

#[test]
fn percent1_whole_value_serializes_without_decimal_point() {
    let p = Percent1::from_ratio(2, 10); // 20.0 -> "20"
    assert_eq!(serde_json::to_string(&p).unwrap(), "20");
}

#[test]
fn percent1_fractional_value_serializes_with_one_decimal_digit() {
    let p = Percent1::from_ratio(1, 3); // 33.333... -> round to 33.3
    assert_eq!(serde_json::to_string(&p).unwrap(), "33.3");
}

#[test]
fn percent1_zero_denominator_is_zero() {
    assert_eq!(Percent1::from_ratio(0, 0), Percent1::zero());
    assert_eq!(serde_json::to_string(&Percent1::zero()).unwrap(), "0");
}
