use super::*;
use crate::graph::OrderedMap;

fn object(entries: &[(&str, OrderedValue)]) -> OrderedValue {
    let mut map = OrderedMap::new();
    for (key, value) in entries {
        map.insert((*key).into(), value.clone());
    }
    OrderedValue::Object(map)
}

fn path(value: &str) -> OrderedValue {
    OrderedValue::String(value.into())
}

fn targets(answer: MapMatch) -> Vec<String> {
    if let MapMatch::Targets(targets) = answer {
        targets
    } else {
        vec![]
    }
}

#[test]
fn module_resolution_ac_3_conditions_follow_manifest_order() {
    let map = object(&[
        ("default", path("./fallback.js")),
        ("import", path("./esm.js")),
    ]);
    assert_eq!(
        targets(exports(&map, ".", "import", Mode::Node16, &[])),
        vec!["./fallback.js"]
    );
    let reversed = object(&[
        ("import", path("./esm.js")),
        ("default", path("./fallback.js")),
    ]);
    assert_eq!(
        targets(exports(&reversed, ".", "import", Mode::Node16, &[])),
        vec!["./esm.js"]
    );
}

#[test]
fn module_resolution_ac_3_subpath_patterns_and_null_exclusions() {
    let map = object(&[
        ("./blocked", OrderedValue::Null),
        ("./features/*", path("./src/features/*.js")),
        ("./features/private/*", path("./src/private/*.js")),
    ]);
    assert_eq!(
        targets(exports(
            &map,
            "./features/private/a",
            "import",
            Mode::Bundler,
            &[]
        )),
        vec!["./src/private/a.js"]
    );
    assert!(matches!(
        exports(&map, "./blocked", "import", Mode::Bundler, &[]),
        MapMatch::NoCondition
    ));
    assert!(matches!(
        exports(&map, "./missing", "import", Mode::Bundler, &[]),
        MapMatch::NotMatched
    ));
}

#[test]
fn module_resolution_ac_3_node_and_custom_conditions() {
    let map = object(&[
        ("node", path("./node.js")),
        ("browser", path("./browser.js")),
    ]);
    assert_eq!(
        targets(exports(&map, ".", "import", Mode::NodeNext, &[])),
        vec!["./node.js"]
    );
    assert_eq!(
        targets(exports(
            &map,
            ".",
            "import",
            Mode::Bundler,
            &["browser".into()]
        )),
        vec!["./browser.js"]
    );
    let types = object(&[
        ("types", path("./index.d.ts")),
        ("default", path("./index.js")),
    ]);
    assert_eq!(
        targets(exports(&types, ".", "import", Mode::Bundler, &[])),
        vec!["./index.js"]
    );
}
