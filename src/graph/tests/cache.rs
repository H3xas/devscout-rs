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
    assert_eq!(SUPERSEDED_CACHE_FILES.len(), 44, "v1..v22 pairs");
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
            base_type_args: OrderedMap::new(),
            property_message_args: Vec::new(),
            array_message_bases: Vec::new(),
            end_line: 1,
        }],
        usings: vec![],
        refs: vec![],
        names: vec![],
        registrations: vec![],
        publishes: vec![],
        handler_registrations: vec![],
    };
    let mut fresh = HashMap::new();
    fresh.insert("src/A.cs".to_string(), AnyFragment::Cs(fragment));
    let graph_files = vec![GraphFile {
        rel: "src/A.cs".to_string(),
        mtime: 222,
    }];
    rebuild_graph(&dir, &graph_files, &fresh, true, None).unwrap();

    assert!(
        graph_dir(&dir).join("fragments-v23.json").exists(),
        "the v23 payload cache is what gets written"
    );
    assert!(
        graph_dir(&dir).join("fragments-index-v23.json").exists(),
        "and its mtime-only index alongside it"
    );
    for stale in SUPERSEDED_CACHE_FILES {
        assert!(
            !graph_dir(&dir).join(stale).exists(),
            "{stale} must be deleted -- rename IS the invalidation"
        );
    }
}

// --- The v23 cache generation --------------------------

#[test]
fn fragments_cache_v23_supersedes_v22() {
    let dir = temp_dir("fragments-cache-v23-paths");
    assert_eq!(
        fragments_cache_path(&dir),
        graph_dir(&dir).join("fragments-v23.json")
    );
    assert_eq!(
        fragments_index_path(&dir),
        graph_dir(&dir).join("fragments-index-v23.json")
    );
    assert!(
        SUPERSEDED_CACHE_FILES.contains(&"fragments-v22.json"),
        "v22 joined the superseded list when the v23 bump landed"
    );
    assert!(
        SUPERSEDED_CACHE_FILES.contains(&"fragments-index-v22.json"),
        "its index pairs with it, same as every other generation"
    );

    let dir = temp_dir("rebuild-v23");
    fs::create_dir_all(graph_dir(&dir)).unwrap();
    fs::write(graph_dir(&dir).join("fragments-v22.json"), b"{}").unwrap();
    fs::write(graph_dir(&dir).join("fragments-index-v22.json"), b"{}").unwrap();

    let fragment = Fragment {
        defs: vec![],
        usings: vec![],
        refs: vec![],
        names: vec![],
        registrations: vec![],
        publishes: vec![],
        handler_registrations: vec![],
    };
    let mut fresh = HashMap::new();
    fresh.insert("src/A.cs".to_string(), AnyFragment::Cs(fragment));
    let graph_files = vec![GraphFile {
        rel: "src/A.cs".to_string(),
        mtime: 1,
    }];
    rebuild_graph(&dir, &graph_files, &fresh, true, None).unwrap();

    assert!(graph_dir(&dir).join("fragments-v23.json").exists());
    assert!(graph_dir(&dir).join("fragments-index-v23.json").exists());
    assert!(
        !graph_dir(&dir).join("fragments-v22.json").exists(),
        "the v22 pair is deleted -- rename IS the invalidation"
    );
    assert!(!graph_dir(&dir).join("fragments-index-v16.json").exists());
}

// --- The rename as the bus-hop invalidation ------------------------------

// Cache payloads here are built as JSON rather than as struct literals
// because the entry under test is one an OLDER generation wrote: it has to
// be missing a field the current type has, which no literal can express.
fn cs_def(id: &str, name: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": name,
        "namespace": "App.Bus",
        "kind": "class",
        "line": 1,
        "methods": [],
    })
}

fn message_and_consumer() -> Vec<(String, AnyFragment)> {
    let consumer = serde_json::json!({
        "id": "App.Bus.Handler",
        "name": "Handler",
        "namespace": "App.Bus",
        "kind": "class",
        "line": 1,
        "methods": [],
        "bases": ["IConsumer"],
        "baseGenericArgs": {"IConsumer": ["LoanRequested"]},
    });
    vec![
        (
            "Bus/Messages.cs".to_string(),
            cs_fragment(serde_json::json!({
                "defs": [cs_def("App.Bus.LoanRequested", "LoanRequested")],
                "usings": [],
                "refs": [],
            })),
        ),
        (
            "Bus/Consumers.cs".to_string(),
            cs_fragment(serde_json::json!({
                "defs": [consumer],
                "usings": [],
                "refs": [],
            })),
        ),
    ]
}

fn publisher_with_publish() -> serde_json::Value {
    serde_json::json!({
        "defs": [cs_def("App.Bus.Publisher", "Publisher")],
        "usings": [],
        "refs": [],
        "publishes": [{
            "verb": "Publish",
            "message": "LoanRequested",
            "namespace": "App.Bus",
            "line": 10,
        }],
    })
}

// The same file as the previous generation recorded it: parsed, so its
// mtime is current and the entry looks reusable, but carrying no publish
// key at all because the field did not exist when it was written.
fn publisher_without_publishes() -> serde_json::Value {
    serde_json::json!({
        "defs": [cs_def("App.Bus.Publisher", "Publisher")],
        "usings": [],
        "refs": [],
    })
}

fn cs_fragment(value: serde_json::Value) -> AnyFragment {
    serde_json::from_value(value).unwrap()
}

fn plant_cache_pair(dir: &std::path::Path, payload: &str, index: &str, generation: &str) {
    fs::write(
        graph_dir(dir).join(format!("fragments{generation}.json")),
        payload,
    )
    .unwrap();
    fs::write(
        graph_dir(dir).join(format!("fragments-index{generation}.json")),
        index,
    )
    .unwrap();
}

fn bus_files() -> Vec<GraphFile> {
    ["Bus/Messages.cs", "Bus/Consumers.cs", "Bus/Publisher.cs"]
        .iter()
        .map(|rel| GraphFile {
            rel: (*rel).to_string(),
            mtime: 7,
        })
        .collect()
}

fn fresh_bus_fragments() -> HashMap<String, AnyFragment> {
    let mut fresh: HashMap<String, AnyFragment> = message_and_consumer().into_iter().collect();
    fresh.insert(
        "Bus/Publisher.cs".to_string(),
        cs_fragment(publisher_with_publish()),
    );
    fresh
}

fn rebuilt(dir: &std::path::Path) -> Graph {
    match rebuild_graph(dir, &bus_files(), &fresh_bus_fragments(), true, None).unwrap() {
        RebuildOutcome::Rebuilt(g) => g,
        RebuildOutcome::NotRebuilt => unreachable!("changed is true"),
    }
}

fn hop_count(g: &Graph) -> usize {
    g.edges
        .iter()
        .filter(|e| matches!(e, Edge::BusHop { .. }))
        .count()
}

#[test]
fn a_pre_bump_fragment_cache_cannot_leave_a_supported_hop_unfound() {
    let stale = serde_json::to_string(&serde_json::json!({
        "Bus/Publisher.cs": {"mtime": 7, "fragment": publisher_without_publishes()},
    }))
    .unwrap();
    let index = r#"{"Bus/Publisher.cs":7}"#;

    let dir = temp_dir("bus-hop-pre-bump-cache");
    fs::create_dir_all(graph_dir(&dir)).unwrap();
    plant_cache_pair(&dir, &stale, index, "-v21");
    let g = rebuilt(&dir);
    assert_eq!(
        hop_count(&g),
        1,
        "the publish fact comes from this run's extraction, never from the older generation"
    );
    assert_eq!(g.stats.edges_by_kind.bus_hop, Some(1));
    assert!(!graph_dir(&dir).join("fragments-v21.json").exists());

    // The control: the identical entry sitting at the CURRENT generation is
    // reused on its matching mtime and the hop goes missing with nothing on
    // disk to show for it. That is the loss the rename prevents, so this
    // half fails the moment the rename is reverted.
    let reused = temp_dir("bus-hop-current-generation-cache");
    fs::create_dir_all(graph_dir(&reused)).unwrap();
    plant_cache_pair(&reused, &stale, index, "-v23");
    let g = rebuilt(&reused);
    assert_eq!(hop_count(&g), 0);
    assert_eq!(g.stats.edges_by_kind.bus_hop, None);
}

#[test]
fn a_rebuild_with_no_publish_site_writes_no_bus_hop_counter_at_all() {
    let dir = temp_dir("bus-hop-counter-absent");
    let files: Vec<GraphFile> = message_and_consumer()
        .iter()
        .map(|(rel, _)| GraphFile {
            rel: rel.clone(),
            mtime: 7,
        })
        .collect();
    let fresh: HashMap<String, AnyFragment> = message_and_consumer().into_iter().collect();
    let g = match rebuild_graph(&dir, &files, &fresh, true, None).unwrap() {
        RebuildOutcome::Rebuilt(g) => g,
        RebuildOutcome::NotRebuilt => unreachable!("changed is true"),
    };

    assert_eq!(hop_count(&g), 0);
    assert_eq!(g.stats.edges_by_kind.bus_hop, None);
    let written = fs::read_to_string(graph_dir(&dir).join("graph.json")).unwrap();
    assert!(
        !written.contains("bus-hop"),
        "an absent key, not a zero -- a zero would move every bus-free artifact's bytes"
    );
}

// --- The v22 generation carried no array fact ----------------------------

fn array_message_and_consumers() -> Vec<(String, AnyFragment)> {
    let array_consumer = serde_json::json!({
        "id": "App.Bus.ArrayHandler",
        "name": "ArrayHandler",
        "namespace": "App.Bus",
        "kind": "class",
        "line": 1,
        "methods": [],
        "bases": ["IConsumer"],
        "baseGenericArgs": {"IConsumer": ["Foo"]},
        "arrayMessageBases": ["IConsumer"],
    });
    let single_consumer = serde_json::json!({
        "id": "App.Bus.SingleHandler",
        "name": "SingleHandler",
        "namespace": "App.Bus",
        "kind": "class",
        "line": 1,
        "methods": [],
        "bases": ["IConsumer"],
        "baseGenericArgs": {"IConsumer": ["Foo"]},
    });
    vec![
        (
            "Bus/Messages.cs".to_string(),
            cs_fragment(serde_json::json!({
                "defs": [cs_def("App.Bus.Foo", "Foo")],
                "usings": [],
                "refs": [],
            })),
        ),
        (
            "Bus/Consumers.cs".to_string(),
            cs_fragment(serde_json::json!({
                "defs": [array_consumer, single_consumer],
                "usings": [],
                "refs": [],
            })),
        ),
    ]
}

fn array_publisher_with_publish() -> serde_json::Value {
    serde_json::json!({
        "defs": [cs_def("App.Bus.Publisher", "Publisher")],
        "usings": [],
        "refs": [],
        "publishes": [{
            "verb": "Publish",
            "message": "Foo[]",
            "namespace": "App.Bus",
            "line": 10,
        }],
    })
}

// The same consumer file as a v22 cache would have recorded it: parsed, so
// its mtime is current and the entry looks reusable, but with no
// `arrayMessageBases` key at all -- v22 never wrote one, so `ArrayHandler`
// reads back indistinguishable from `SingleHandler`.
fn array_consumers_without_array_message_bases() -> serde_json::Value {
    serde_json::json!({
        "defs": [
            {
                "id": "App.Bus.ArrayHandler",
                "name": "ArrayHandler",
                "namespace": "App.Bus",
                "kind": "class",
                "line": 1,
                "methods": [],
                "bases": ["IConsumer"],
                "baseGenericArgs": {"IConsumer": ["Foo"]},
            },
            {
                "id": "App.Bus.SingleHandler",
                "name": "SingleHandler",
                "namespace": "App.Bus",
                "kind": "class",
                "line": 1,
                "methods": [],
                "bases": ["IConsumer"],
                "baseGenericArgs": {"IConsumer": ["Foo"]},
            },
        ],
        "usings": [],
        "refs": [],
    })
}

fn array_bus_files() -> Vec<GraphFile> {
    ["Bus/Messages.cs", "Bus/Consumers.cs", "Bus/Publisher.cs"]
        .iter()
        .map(|rel| GraphFile {
            rel: (*rel).to_string(),
            mtime: 7,
        })
        .collect()
}

fn fresh_array_bus_fragments() -> HashMap<String, AnyFragment> {
    let mut fresh: HashMap<String, AnyFragment> =
        array_message_and_consumers().into_iter().collect();
    fresh.insert(
        "Bus/Publisher.cs".to_string(),
        cs_fragment(array_publisher_with_publish()),
    );
    fresh
}

fn array_handler_names(g: &Graph) -> Vec<String> {
    let mut names: Vec<String> = g
        .edges
        .iter()
        .filter_map(|e| match e {
            Edge::BusHop { to, .. } => Some(
                to.rsplit_once('.')
                    .map_or(to.clone(), |(_, t)| t.to_string()),
            ),
            _ => None,
        })
        .collect();
    names.sort();
    names
}

#[test]
fn a_warm_v22_cache_is_rebuilt_rather_than_read_without_array_facts() {
    let stale = serde_json::to_string(&serde_json::json!({
        "Bus/Consumers.cs": {"mtime": 7, "fragment": array_consumers_without_array_message_bases()},
    }))
    .unwrap();
    let index = r#"{"Bus/Consumers.cs":7}"#;

    let dir = temp_dir("bus-array-warm-v22-cache");
    fs::create_dir_all(graph_dir(&dir)).unwrap();
    plant_cache_pair(&dir, &stale, index, "-v22");
    let g = match rebuild_graph(
        &dir,
        &array_bus_files(),
        &fresh_array_bus_fragments(),
        true,
        None,
    )
    .unwrap()
    {
        RebuildOutcome::Rebuilt(g) => g,
        RebuildOutcome::NotRebuilt => unreachable!("changed is true"),
    };
    assert_eq!(
        array_handler_names(&g),
        vec!["ArrayHandler".to_string()],
        "a fresh v23 extraction, never the warm v22 payload, is what decides the array \
         consumer's own identity"
    );
    assert!(!graph_dir(&dir).join("fragments-v22.json").exists());

    // The control: the SAME degraded entry, planted at the CURRENT v23
    // generation and reused on its matching mtime, loses the array bit --
    // both handlers then read as plain `Foo` consumers, so the `Foo[]`
    // publish's own identity (rank 1) matches NEITHER of them and the
    // array route silently disappears. That is the loss the rename
    // prevents, so this half fails the moment the rename is reverted.
    let reused = temp_dir("bus-array-current-generation-cache");
    fs::create_dir_all(graph_dir(&reused)).unwrap();
    plant_cache_pair(&reused, &stale, index, "-v23");
    let g = match rebuild_graph(
        &reused,
        &array_bus_files(),
        &fresh_array_bus_fragments(),
        true,
        None,
    )
    .unwrap()
    {
        RebuildOutcome::Rebuilt(g) => g,
        RebuildOutcome::NotRebuilt => unreachable!("changed is true"),
    };
    assert_eq!(
        array_handler_names(&g),
        Vec::<String>::new(),
        "reusing a degraded entry at the current generation is exactly the loss the rename \
         prevents: the array bit is gone, so the array publish's own identity (rank 1) \
         matches neither handler's now-plain (rank 0) identity, and the route silently \
         disappears rather than reaching ArrayHandler"
    );
}
