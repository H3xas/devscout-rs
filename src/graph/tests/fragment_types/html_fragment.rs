use super::*;
use crate::ngtemplate::scan_template;

#[test]
fn angular_templates_ac_8_a_template_fragment_round_trips_as_its_own_shape() {
    let fragment = HtmlFragment::from_scan(scan_template("{{ a }}\n@if (b) {", 1));
    let any = AnyFragment::from(fragment.clone());
    let json = serde_json::to_string(&any).unwrap();
    assert_eq!(
        json,
        r#"{"html":1,"reads":[{"name":"a","line":1}],"gaps":[{"line":2,"reason":"control-flow-block"}]}"#
    );
    assert_eq!(
        serde_json::from_str::<AnyFragment>(&json).unwrap(),
        AnyFragment::Html(fragment)
    );
    let empty = serde_json::to_string(&AnyFragment::from(HtmlFragment::from_scan(scan_template(
        "<p></p>", 1,
    ))))
    .unwrap();
    assert_eq!(empty, r#"{"html":1}"#);
    assert!(matches!(
        serde_json::from_str::<AnyFragment>(&empty).unwrap(),
        AnyFragment::Html(_)
    ));
}

#[test]
fn angular_templates_ac_8_other_shapes_never_read_as_a_template() {
    for json in [
        r#"{"ts":1,"defs":[],"imports":[],"reexports":[],"refs":[]}"#,
        r#"{"defs":[],"usings":[],"refs":[],"names":[]}"#,
        r#"{"rs":1,"items":[]}"#,
    ] {
        assert!(!matches!(
            serde_json::from_str::<AnyFragment>(json).unwrap(),
            AnyFragment::Html(_)
        ));
    }
    assert!(serde_json::from_str::<AnyFragment>(r#"{"reads":[]}"#).is_err());
}
