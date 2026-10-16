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

#[test]
fn transport_facts_ac_4_a_malformed_scalar_or_escape_makes_the_file_unreadable() {
    for bad in [
        "{\"Region\": \"us-east-1\", \"Broken\": tru}",
        "{\"a\": nul}",
        "{\"a\": 01}",
        "{\"a\": 1.}",
        "{\"a\": .5}",
        "{\"a\": +1}",
        "{\"a\": -}",
        "{\"a\": 1e}",
        "{\"a\": \"\\q\"}",
        "{\"a\": \"\\u12\"}",
        "{\"a\": \"\\ud83d\"}",
        "{\"a\": \"tab\there\"}",
    ] {
        assert!(parse(bad).is_none(), "{bad}");
    }
    let good = parse(
        "{\"t\": true, \"f\": false, \"n\": null, \"x\": -1.5e+3, \"z\": 0, \"s\": \"\\u0041\\/\\ud83d\\ude00\"}",
    )
    .expect("parses");
    assert!(matches!(lookup(&good, "s"), Found::Text(t) if t == "A/\u{1f600}"));
}
