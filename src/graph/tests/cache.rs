use super::*;
use std::collections::HashMap;
use std::fs;

// --- The v13 cache generation --------------------------

#[test]
fn rebuild_graph_writes_the_v13_caches_and_deletes_every_superseded_generation() {
    let dir = temp_dir("rebuild-v13");
    fs::create_dir_all(graph_dir(&dir)).unwrap();
    // The v9 pair joined this list when fragments gained def
    // `typeParams`/`baseGenericArgs` and the `ctor-param` ref kind, so a v9
    // fragment read back today carries none and every ctor-DI fact would be
    // missing from the graph. The v10 pair joined when a markup
    // fragment gained the `x:Class` def and its element/binding refs, so a
    // v10 markup fragment read back carries names only and every XAML
    // declaration and instantiation would be missing from the graph. The v11
    // pair joined when a `.ts/.tsx/.js/.jsx` file gained a reference
    // fragment where it previously had none at all, so a v11 cache read back
    // would leave every TS/JS rel looking like a file the worker never saw.
    // This side records no TS fragment yet, but the cache generation is
    // shared -- two writers putting two generations into one git dir would
    // have each delete the other's cache on every map. The v12 pair joined
    // when defs gained propertyTypes and refs
    // gained receiverPropertyOwner and the receiverCallOwner/
    // receiverCallMember pair, so a v12 fragment read back carries none and
    // every property hop and every var-from-invocation receiver would
    // silently stay unresolved.
    assert_eq!(SUPERSEDED_CACHE_FILES.len(), 40, "v1..v20 pairs");
    for stale in SUPERSEDED_CACHE_FILES {
        fs::write(graph_dir(&dir).join(stale), b"{}").unwrap();
    }

    let fragment = Fragment {
        defs: vec![FragDef {
            id: "App.A".into(),
            name: "A".into(),
            namespace: "App".into(),
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
    let mut fresh = HashMap::new();
    fresh.insert("src/A.cs".to_string(), AnyFragment::Cs(fragment));
    let graph_files = vec![GraphFile {
        rel: "src/A.cs".to_string(),
        mtime: 222,
    }];
    rebuild_graph(&dir, &graph_files, &fresh, true, None).unwrap();

    assert!(
        graph_dir(&dir).join("fragments-v21.json").exists(),
        "the v21 payload cache is what gets written"
    );
    assert!(
        graph_dir(&dir).join("fragments-index-v21.json").exists(),
        "and its mtime-only index alongside it"
    );
    for stale in SUPERSEDED_CACHE_FILES {
        assert!(
            !graph_dir(&dir).join(stale).exists(),
            "{stale} must be deleted -- rename IS the invalidation"
        );
    }
}

// --- The v21 cache generation --------------------------

#[test]
fn fragments_cache_v21_supersedes_v20() {
    let dir = temp_dir("fragments-cache-v21-paths");
    assert_eq!(
        fragments_cache_path(&dir),
        graph_dir(&dir).join("fragments-v21.json")
    );
    assert_eq!(
        fragments_index_path(&dir),
        graph_dir(&dir).join("fragments-index-v21.json")
    );
    assert!(
        SUPERSEDED_CACHE_FILES.contains(&"fragments-v20.json"),
        "v20 joined the superseded list when the v21 bump landed"
    );
    assert!(
        SUPERSEDED_CACHE_FILES.contains(&"fragments-index-v20.json"),
        "its index pairs with it, same as every other generation"
    );

    let dir = temp_dir("rebuild-v21");
    fs::create_dir_all(graph_dir(&dir)).unwrap();
    fs::write(graph_dir(&dir).join("fragments-v20.json"), b"{}").unwrap();
    fs::write(graph_dir(&dir).join("fragments-index-v20.json"), b"{}").unwrap();

    let fragment = Fragment {
        defs: vec![],
        usings: vec![],
        refs: vec![],
        names: vec![],
        registrations: vec![],
    };
    let mut fresh = HashMap::new();
    fresh.insert("src/A.cs".to_string(), AnyFragment::Cs(fragment));
    let graph_files = vec![GraphFile {
        rel: "src/A.cs".to_string(),
        mtime: 1,
    }];
    rebuild_graph(&dir, &graph_files, &fresh, true, None).unwrap();

    assert!(graph_dir(&dir).join("fragments-v21.json").exists());
    assert!(graph_dir(&dir).join("fragments-index-v21.json").exists());
    assert!(
        !graph_dir(&dir).join("fragments-v20.json").exists(),
        "the v20 pair is deleted -- rename IS the invalidation"
    );
    assert!(!graph_dir(&dir).join("fragments-index-v16.json").exists());
}
