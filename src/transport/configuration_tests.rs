use super::*;

#[test]
fn transport_facts_ac_4_settings_parse_with_comments_and_trailing_commas() {
    let json = parse("{ // a comment\n \"A\": { /* b */ \"B\": \"x\", }, }").expect("parses");
    assert!(matches!(lookup(&json, "a:b"), Found::Text(t) if t == "x"));
    assert!(matches!(lookup(&json, "a:c"), Found::Missing));
    assert!(parse("{ \"A\": ").is_none());
}

#[test]
fn transport_facts_ac_4_ambiguous_and_non_string_keys_are_unreadable() {
    let twice = parse("{\"a\": \"x\", \"A\": \"y\"}").expect("parses");
    assert!(matches!(lookup(&twice, "a"), Found::Unreadable));
    let number = parse("{\"a\": 5}").expect("parses");
    assert!(matches!(lookup(&number, "a"), Found::Unreadable));
}
