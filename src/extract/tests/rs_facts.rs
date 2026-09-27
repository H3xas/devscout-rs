use super::*;

fn fragment(src: &str) -> RsFragment {
    extract_rs_file(src).fragment
}

fn uses(src: &str) -> Vec<(String, Option<usize>, String, String, usize)> {
    fragment(src)
        .uses
        .into_iter()
        .map(|u| (u.scope, u.block, u.path, u.name, u.line))
        .collect()
}

fn leaf(
    scope: &str,
    block: Option<usize>,
    path: &str,
    name: &str,
    line: usize,
) -> (String, Option<usize>, String, String, usize) {
    (scope.into(), block, path.into(), name.into(), line)
}

#[test]
fn use_trees_flatten_into_one_leaf_per_bound_name() {
    let src = "use a::b;\nuse c::{d, e::{f, g::*}, self};\nuse h::{i as j, k as _};\nuse ::l::m;\nuse super::super::n;\nuse crate::o::*;\nuse p::{self as q};\n";
    assert_eq!(
        uses(src),
        vec![
            leaf("", None, "a::b", "b", 1),
            leaf("", None, "c::d", "d", 2),
            leaf("", None, "c::e::f", "f", 2),
            leaf("", None, "c::e::g", "*", 2),
            leaf("", None, "c", "c", 2),
            leaf("", None, "h::i", "j", 3),
            leaf("", None, "::l::m", "m", 4),
            leaf("", None, "super::super::n", "n", 5),
            leaf("", None, "crate::o", "*", 6),
            leaf("", None, "p", "q", 7),
        ]
    );
}

#[test]
fn a_use_in_a_block_names_its_block_and_the_block_its_parent() {
    let src = "mod m {\n    fn f() {\n        use a::b;\n        {\n            use c::d;\n        }\n        let x = 1;\n    }\n    use e::g;\n}\nfn h() { let y = 2; }\n";
    let frag = fragment(src);
    assert_eq!(
        frag.uses
            .iter()
            .map(|u| (u.scope.clone(), u.block, u.path.clone()))
            .collect::<Vec<_>>(),
        vec![
            ("m".to_string(), Some(0), "a::b".to_string()),
            ("m".to_string(), Some(1), "c::d".to_string()),
            ("m".to_string(), None, "e::g".to_string()),
        ]
    );
    assert_eq!(
        frag.blocks,
        vec![RsBlock { parent: None }, RsBlock { parent: Some(0) }],
        "the block of `h` holds no use and is not recorded"
    );
}

#[test]
fn uses_in_macros_attributes_and_function_body_modules_are_not_recorded() {
    let src = "macro_rules! m { () => { use a::b; } }\n#[cfg(feature = \"x\")]\nuse c::d;\nfn f() { mod inner { use e::g; } }\n";
    assert_eq!(uses(src), vec![leaf("", None, "c::d", "d", 3)]);
}

#[test]
fn module_declarations_are_told_apart_from_inline_modules() {
    let src = "mod a;\nmod b { mod c; mod d {} }\npub(crate) mod r#e;\n";
    let frag = fragment(src);
    assert_eq!(frag.mod_files, vec!["a", "b::c", "e"]);
    assert!(frag.items.iter().any(|i| i.path == "b::d"));
}

#[test]
fn tuple_and_unit_structs_are_constructors_and_braced_ones_are_not() {
    let src = "struct T(u8);\nstruct U;\nstruct B { a: u8 }\nmod m { pub struct V(pub u8, u16); }\nenum E { A(u8) }\n";
    assert_eq!(fragment(src).ctors, vec!["T", "U", "m::V"]);
}

#[test]
fn test_functions_are_marked_by_their_attribute() {
    let src = "#[test]\nfn a() {}\n#[tokio::test(flavor = \"x\")]\nasync fn b() {}\n#[test_case]\nfn c() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    // a note between\n    #[should_panic]\n    fn d() {\n        #[test]\n        fn nested() {}\n    }\n}\nfn e() {}\n";
    assert_eq!(fragment(src).tests, vec!["a", "b", "tests::d"]);
}

#[test]
fn impl_blocks_record_their_self_type_path_and_items() {
    let src = "impl Lamp { fn a() {} }\nimpl<T> crate::m::Lamp<T> { fn b() {} }\nimpl Show for &mut Lamp { fn a() {} }\nimpl dyn Show { fn c() {} }\nimpl Show for (u8, u8) { fn d() {} }\nmod inner { impl super::Lamp { const K: u8 = 1; type T = u8; } }\n";
    let frag = fragment(src);
    let impls: Vec<(String, String, Vec<String>)> = frag
        .impls
        .iter()
        .map(|i| (i.scope.clone(), i.self_type.clone(), i.items.clone()))
        .collect();
    assert_eq!(
        impls,
        vec![
            ("".into(), "Lamp".into(), vec!["Lamp::a".into()]),
            ("".into(), "crate::m::Lamp".into(), vec!["Lamp::b".into()]),
            ("".into(), "Lamp".into(), vec!["Lamp::a-1".into()]),
            ("".into(), "Show".into(), vec!["Show::c".into()]),
            (
                "inner".into(),
                "super::Lamp".into(),
                vec!["inner::Lamp::K".into(), "inner::Lamp::T".into()]
            ),
        ],
        "a tuple self type writes no record"
    );
    assert!(frag.items.iter().any(|i| i.path == "_::d"));
}

#[test]
fn a_file_with_no_facts_serializes_as_items_only() {
    let json = serde_json::to_string(&fragment("fn a() { let x = 1; }\n")).unwrap();
    assert_eq!(
        json,
        r#"{"rs":1,"items":[{"name":"a","kind":"fn","path":"a","line":1,"nameLine":1,"endLine":1}]}"#
    );
}
