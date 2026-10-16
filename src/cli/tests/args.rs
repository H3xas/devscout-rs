use super::*;

use super::super::args::{parse_pick, Pick};

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

fn args_of(value: &[&str]) -> Vec<String> {
    value.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn candidate_sections_ac_7_parse_pick_accepts_one_positive_integer_or_the_word_all() {
    assert_eq!(parse_pick(&args_of(&["Stow"])), Ok(None));
    assert_eq!(
        parse_pick(&args_of(&["Stow", "--pick", "3"])),
        Ok(Some(Pick::Nth(3)))
    );
    assert_eq!(
        parse_pick(&args_of(&["Stow", "--pick", "all"])),
        Ok(Some(Pick::All))
    );
    // A second `--pick` is not a second narrowing: the first one counts, the
    // same as every other repeated flag on these verbs.
    assert_eq!(
        parse_pick(&args_of(&["--pick", "1", "--pick", "all"])),
        Ok(Some(Pick::Nth(1)))
    );
}

#[test]
fn candidate_sections_ac_7_parse_pick_refuses_every_other_value_before_the_graph_is_read() {
    for value in ["", "0", "-1", "All", "ALL", "two", "1,2", "all ", " all"] {
        assert_eq!(
            parse_pick(&args_of(&["Stow", "--pick", value])),
            Err(()),
            "{value:?} is not a positive integer and not exactly `all`"
        );
    }
    assert_eq!(
        parse_pick(&args_of(&["Stow", "--pick"])),
        Err(()),
        "a missing value is a usage error, never an absent flag"
    );
}
