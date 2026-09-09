use super::*;

#[test]
fn parse_int_js_matches_node_parseint_on_checked_values() {
    assert_eq!(parse_int_js("3"), Some(3));
    assert_eq!(parse_int_js("3abc"), Some(3));
    assert_eq!(parse_int_js("-5"), Some(-5));
    assert_eq!(parse_int_js("  7"), Some(7));
    assert_eq!(parse_int_js(""), None);
    assert_eq!(parse_int_js("abc"), None);
    assert_eq!(parse_int_js("-"), None);
}
