use super::*;

#[test]
fn j_object_and_array_serialize_with_no_extra_whitespace_like_json_stringify() {
    let j = J::Obj(vec![
        ("a", J::UInt(1)),
        ("b", J::Arr(vec![J::Str("x".to_string()), J::UInt(2)])),
    ]);
    assert_eq!(j.to_json_string(), r#"{"a":1,"b":["x",2]}"#);
}

#[test]
fn j_string_escapes_control_chars_and_quotes_like_json_stringify() {
    let j = J::Str("a\"b\\c\nd".to_string());
    assert_eq!(j.to_json_string(), r#""a\"b\\c\nd""#);
}
