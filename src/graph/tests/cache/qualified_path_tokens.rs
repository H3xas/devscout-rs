use super::*;

const SITE: &str = "src/App/Skip.cs";

// The receiver's own declared type is written with whitespace around its
// dot, so the prior generation recorded the raw text `Outbox .
// OutboxSendEndpoint` as the written path. Nothing matches that text, so the
// ladder falls back to the tail identifier and the file's own
// `using InMemoryOutbox;` answers with the same-named sibling.
fn sources() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "src/Middleware/Outbox/OutboxSendEndpoint.cs",
            "\nnamespace App.Middleware.Outbox;\n\npublic class OutboxSendEndpoint\n{\n    public IEndpoint Endpoint { get; }\n}\n",
        ),
        (
            "src/Middleware/InMemoryOutbox/OutboxSendEndpoint.cs",
            "\nnamespace App.Middleware.InMemoryOutbox;\n\npublic class OutboxSendEndpoint\n{\n    public IEndpoint Endpoint { get; }\n}\n",
        ),
        (
            SITE,
            "\nnamespace App.Middleware\n{\n    using InMemoryOutbox;\n\n    public static class SkipExtensions\n    {\n        public static void Skip(IEndpoint endpoint)\n        {\n            if (endpoint is Outbox . OutboxSendEndpoint outboxEndpoint)\n            {\n                var found = outboxEndpoint.Endpoint;\n            }\n        }\n    }\n}\n",
        ),
    ]
}

fn fresh() -> HashMap<String, AnyFragment> {
    sources()
        .into_iter()
        .map(|(rel, src)| {
            (
                rel.to_string(),
                AnyFragment::Cs(crate::graph::fragment_from_extraction(
                    &crate::extract::extract(src),
                )),
            )
        })
        .collect()
}

fn files() -> Vec<GraphFile> {
    sources()
        .into_iter()
        .map(|(rel, _)| GraphFile {
            rel: rel.to_string(),
            mtime: 7,
        })
        .collect()
}

/// The site fragment as the PRIOR generation wrote it: both written-path
/// fields hold the source text the type was written with, trivia included.
fn payload_with_the_raw_path_text() -> String {
    fn rewrite(value: &mut serde_json::Value, hits: &mut usize) {
        match value {
            serde_json::Value::Object(map) => {
                for key in ["qualifiedType", "receiverTypeQualified"] {
                    if let Some(serde_json::Value::String(written)) = map.get_mut(key) {
                        if written == "Outbox.OutboxSendEndpoint" {
                            *written = "Outbox . OutboxSendEndpoint".to_string();
                            *hits += 1;
                        }
                    }
                }
                for child in map.values_mut() {
                    rewrite(child, hits);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    rewrite(item, hits);
                }
            }
            _ => {}
        }
    }
    let fragment = fresh().remove(SITE).expect("site fragment present");
    let mut entry = serde_json::json!({"mtime": 7, "fragment": fragment});
    let mut hits = 0;
    rewrite(&mut entry["fragment"], &mut hits);
    assert!(hits > 0, "the site records a written path to rewrite");
    serde_json::to_string(&serde_json::json!({ SITE: entry })).unwrap()
}

fn edge_targets(g: &Graph, heuristic: bool) -> Vec<String> {
    g.edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                to,
                member,
                heuristic: edge_heuristic,
                ..
            } if *edge_heuristic == heuristic && member.as_deref() == Some("Endpoint") => {
                Some(to.clone())
            }
            _ => None,
        })
        .collect()
}

fn rebuild(dir: &std::path::Path) -> Graph {
    match rebuild_graph(dir, &files(), &fresh(), true, None, None).unwrap() {
        RebuildOutcome::Rebuilt(g) => g,
        RebuildOutcome::NotRebuilt => unreachable!("changed is true"),
    }
}

#[test]
fn qualified_path_tokens_ac_6_a_prior_generation_cache_is_re_extracted() {
    let index = r#"{"src/App/Skip.cs":7}"#;
    let stale = payload_with_the_raw_path_text();

    let warm = temp_dir("qualified-path-tokens-warm-v33-cache");
    fs::create_dir_all(graph_dir(&warm)).unwrap();
    plant_cache_pair(&warm, &stale, index, "-v33");
    let warm_graph = rebuild(&warm);
    assert_eq!(
        edge_targets(&warm_graph, false),
        vec!["App.Middleware.Outbox.OutboxSendEndpoint"],
        "the v33 payload is not read, so this run re-parses the source and binds the written path"
    );
    assert!(!graph_dir(&warm).join("fragments-v33.json").exists());

    let cold = temp_dir("qualified-path-tokens-cold-cache");
    fs::create_dir_all(graph_dir(&cold)).unwrap();
    let cold_graph = rebuild(&cold);
    assert_eq!(
        edge_targets(&warm_graph, false),
        edge_targets(&cold_graph, false),
        "a run that ignored a prior-generation cache agrees with a cold one"
    );

    // The control: the same payload at the CURRENT generation is reused on
    // its matching mtime, and the receiver falls back to its bare tail --
    // which the file's own `using InMemoryOutbox;` answers with the sibling
    // sharing that tail. That is the loss the rename prevents, so this half
    // fails the moment the generation bump is reverted.
    let reused = temp_dir("qualified-path-tokens-current-generation-cache");
    fs::create_dir_all(graph_dir(&reused)).unwrap();
    plant_cache_pair(&reused, &stale, index, "-v36");
    let reused_graph = rebuild(&reused);
    assert!(
        edge_targets(&reused_graph, false).is_empty(),
        "reusing the raw-text entry at the current generation loses the precise edge, which is \
         the loss the rename prevents"
    );
    assert_eq!(
        edge_targets(&reused_graph, true),
        vec![
            "App.Middleware.InMemoryOutbox.OutboxSendEndpoint",
            "App.Middleware.Outbox.OutboxSendEndpoint"
        ],
        "the raw text matches neither written path, so the receiver degrades to a guess at both \
         same-named types"
    );
}
