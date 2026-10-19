use super::*;
use crate::ngtemplate::GapReason;

fn components(src: &str) -> Vec<TsComponent> {
    let units = crate::parse::utf16_units(src);
    let grammar = crate::parse::ts_grammar_for("a.ts").expect("ts grammar");
    let tree = crate::parse::parse_ts_js(&units, grammar).expect("fixture parses");
    extract_ts_fragment(tree.root_node(), &crate::parse::utf16_bytes(&units)).components
}

fn member_rows(c: &TsComponent) -> Vec<(&str, &str, usize)> {
    c.members
        .iter()
        .map(|m| (m.name.as_str(), m.kind.as_str(), m.line))
        .collect()
}

#[test]
fn angular_templates_ac_1_members_keep_kinds_and_first_lines() {
    let src = "@Component({ templateUrl: './a.html' })\n\
               export class Panel {\n\
               static registry = 1;\n\
               title = '';\n\
               #secret = 2;\n\
               'quoted' = 3;\n\
               [computed] = 4;\n\
               get open() { return true; }\n\
               set open(v) {}\n\
               save(): void;\n\
               save(x?: number) {}\n\
               static make() {}\n\
               constructor(private svc: object, readonly id: string, plain: number) {}\n\
               }";
    let found = components(src);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "Panel");
    assert_eq!(
        member_rows(&found[0]),
        [
            ("title", "field", 4),
            ("#secret", "field", 5),
            ("quoted", "field", 6),
            ("open", "property", 8),
            ("save", "method", 10),
            ("svc", "field", 13),
            ("id", "field", 13),
        ]
    );
    assert_eq!(
        found[0].template_url,
        Some(TsTemplateUrl {
            spec: "./a.html".into(),
            line: 1
        })
    );
}

#[test]
fn angular_templates_ac_1_only_a_component_decorator_with_an_object_counts() {
    let src = "@Injectable({}) export class Service { a = 1; }\n\
               @Component('x') export class NotObject { b = 1; }\n\
               @ns.Component({}) export class Dotted { c = 1; }\n\
               export class Plain { d = 1; }\n\
               @Component({}) class Local { e = 1; }\n\
               export @Component({}) class Late { f = 1; }";
    let names: Vec<String> = components(src).into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["Local", "Late"]);
}

#[test]
fn angular_templates_ac_3_an_inline_template_counts_lines_from_its_string() {
    let src = "@Component({\n  template: `\n<p>{{ first }}</p>\n<b\\n>{{ second }}</b>`,\n})\nexport class Inline {}";
    let found = components(src);
    let scan = found[0].inline.as_ref().expect("inline scan");
    let reads: Vec<(&str, usize)> = scan
        .reads
        .iter()
        .map(|r| (r.name.as_str(), r.line))
        .collect();
    assert_eq!(reads, [("first", 3), ("second", 4)]);
}

#[test]
fn angular_templates_ac_3_escapes_are_undone_without_moving_lines() {
    let src = "@Component({ template: '<p title=\\'{{ a }}\\'>\\\n{{ b }}</p>' })\nexport class Escaped {}";
    let found = components(src);
    let scan = found[0].inline.as_ref().expect("inline scan");
    let reads: Vec<(&str, usize)> = scan
        .reads
        .iter()
        .map(|r| (r.name.as_str(), r.line))
        .collect();
    assert_eq!(reads, [("a", 1), ("b", 2)]);
}

#[test]
fn angular_templates_ac_7_a_value_that_is_not_one_literal_is_a_gap() {
    let src = "const PATH = './a.html';\n\
               @Component({\n\
               templateUrl: PATH,\n\
               template: `<p>${PATH}</p>`,\n\
               })\n\
               export class Gaps {}\n\
               @Component({ templateUrl: './a' + '.html', template })\n\
               export class More {}";
    let found = components(src);
    let gaps = |c: &TsComponent| -> Vec<(usize, GapReason)> {
        c.gaps.iter().map(|g| (g.line, g.reason)).collect()
    };
    assert_eq!(
        gaps(&found[0]),
        [
            (3, GapReason::TemplateUrlNonLiteral),
            (4, GapReason::InlineTemplateSubstitution)
        ]
    );
    assert_eq!(
        gaps(&found[1]),
        [
            (7, GapReason::TemplateUrlNonLiteral),
            (7, GapReason::InlineTemplateSubstitution)
        ]
    );
    assert!(found[0].template_url.is_none() && found[0].inline.is_none());
}

#[test]
fn angular_templates_ac_1_a_file_without_a_component_serializes_as_before() {
    let units = crate::parse::utf16_units("export class Plain { a = 1; }");
    let grammar = crate::parse::ts_grammar_for("a.ts").expect("ts grammar");
    let tree = crate::parse::parse_ts_js(&units, grammar).expect("fixture parses");
    let fragment = extract_ts_fragment(tree.root_node(), &crate::parse::utf16_bytes(&units));
    let json = serde_json::to_string(&fragment).unwrap();
    assert!(!json.contains("components"), "{json}");
}
