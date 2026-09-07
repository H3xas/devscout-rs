use super::*;

#[test]
fn seed_kind_str_matches_js_string_literals() {
    assert_eq!(seed_kind_str(SeedKind::File), "file");
    assert_eq!(seed_kind_str(SeedKind::Symbol), "symbol");
}
