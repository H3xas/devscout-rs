use super::*;
use std::collections::HashMap;
use std::fs;

// --- rebuild_graph: unchanged path never touches the graph -----------

#[test]
fn rebuild_graph_skips_when_unchanged_and_graph_already_exists() {
    let dir = temp_dir("rebuild-unchanged");
    fs::create_dir_all(graph_dir(&dir)).unwrap();
    fs::write(graph_json_path(&dir), b"{\"schema_version\":3,\"built_at_head\":null,\"defs\":[],\"edges\":[],\"stats\":{\"def_count\":0,\"file_count\":0,\"edges_by_kind\":{\"inherits\":0,\"uses-type\":0,\"imports\":0,\"uses-member\":0},\"ambiguous_count\":0,\"ambiguous_pct\":0,\"unresolved_external_count\":0}}").unwrap();
    let outcome = rebuild_graph(&dir, &[], &HashMap::new(), false, None, None).unwrap();
    assert!(matches!(outcome, RebuildOutcome::NotRebuilt));
}

// The other half of that fast path: "nothing changed" is not enough on
// its own. A graph.json written by a build that predates the current
// schema is missing facts every reader now expects, so it is rebuilt
// even though not one fragment moved -- the ONLY thing separating this
// case from the one above is the version in its first bytes.
#[test]
fn rebuild_graph_rebuilds_when_the_existing_graph_carries_an_older_schema_version() {
    let dir = temp_dir("rebuild-old-schema");
    fs::create_dir_all(graph_dir(&dir)).unwrap();
    fs::write(graph_json_path(&dir), b"{\"schema_version\":1,\"built_at_head\":null,\"defs\":[],\"edges\":[],\"stats\":{\"def_count\":0,\"file_count\":0,\"edges_by_kind\":{\"inherits\":0,\"uses-type\":0,\"imports\":0,\"uses-member\":0},\"ambiguous_count\":0,\"ambiguous_pct\":0,\"unresolved_external_count\":0}}").unwrap();
    let outcome = rebuild_graph(&dir, &[], &HashMap::new(), false, None, None).unwrap();
    let RebuildOutcome::Rebuilt(graph) = outcome else {
        panic!("an older-schema graph must be rebuilt on the unchanged path");
    };
    assert_eq!(graph.schema_version, GRAPH_SCHEMA_VERSION);
    assert!(
        fs::read_to_string(graph_json_path(&dir))
            .unwrap()
            .starts_with(r#"{"schema_version":3,"#),
        "and the rebuilt artifact carries the current version on disk"
    );
}

// A truncated or unreadable artifact answers the same way an older one
// does -- rebuild -- rather than being trusted or panicking.
#[test]
fn rebuild_graph_rebuilds_when_the_existing_graph_is_too_short_to_carry_a_version() {
    let dir = temp_dir("rebuild-truncated");
    fs::create_dir_all(graph_dir(&dir)).unwrap();
    fs::write(graph_json_path(&dir), b"{").unwrap();
    assert!(!graph_schema_is_current(&dir));
    assert!(matches!(
        rebuild_graph(&dir, &[], &HashMap::new(), false, None, None).unwrap(),
        RebuildOutcome::Rebuilt(_)
    ));
}

#[test]
fn rebuild_graph_reuses_a_cached_fragment_at_matching_mtime() {
    let dir = temp_dir("rebuild-cache-hit");
    let fragment = Fragment {
        defs: vec![FragDef {
            id: "A".into(),
            name: "A".into(),
            namespace: "".into(),
            kind: "class".into(),
            line: 1,
            methods: vec![],
            properties: vec![],
            fields: vec![],
            method_returns: OrderedMap::new(),
            extension_methods: vec![],
            bases: vec![],
            type_params: vec![],
            base_generic_args: OrderedMap::new(),
            test_methods: vec![],
            property_types: OrderedMap::new(),
            field_types: OrderedMap::new(),
            method_return_args: OrderedMap::new(),
            non_public_methods: vec![],
            method_arities: OrderedMap::new(),
            method_params: OrderedMap::new(),
            override_methods: vec![],
            end_line: 1,
        }],
        usings: vec![],
        refs: vec![],
        names: vec![],
        registrations: vec![],
    };
    // First build: nothing cached, comes from fresh_fragments.
    let mut fresh = HashMap::new();
    fresh.insert("A.cs".to_string(), AnyFragment::Cs(fragment.clone()));
    let graph_files = vec![GraphFile {
        rel: "A.cs".to_string(),
        mtime: 111,
    }];
    let first = rebuild_graph(&dir, &graph_files, &fresh, true, None, None).unwrap();
    assert!(matches!(first, RebuildOutcome::Rebuilt(_)));

    // Second build: same mtime, EMPTY fresh_fragments -- must reuse the
    // cache, not silently drop the file from the graph.
    let empty: HashMap<String, AnyFragment> = HashMap::new();
    let second = rebuild_graph(&dir, &graph_files, &empty, true, None, None).unwrap();
    match second {
        RebuildOutcome::Rebuilt(g) => {
            assert_eq!(g.defs.len(), 1, "cached fragment must still be used")
        }
        RebuildOutcome::NotRebuilt => panic!("changed=true must always rebuild"),
    }
}
