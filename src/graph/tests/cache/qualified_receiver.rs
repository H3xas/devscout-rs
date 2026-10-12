use super::*;

const QUALIFIED_RECEIVER_SITE: &str = "src/App/Runner.cs";

fn qualified_receiver_sources() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "src/Rabbit/ConfigurationHostSettings.cs",
            "\nnamespace App.Rabbit.Configuration\n{\n    public class ConfigurationHostSettings\n    {\n        public class ConfigurationBatchSettings\n        {\n            public bool Enabled { get; set; }\n        }\n    }\n}\n",
        ),
        (
            "src/Bench/ConfigurationBatchSettings.cs",
            "\nnamespace App.Bench;\n\npublic class ConfigurationBatchSettings\n{\n    public bool Enabled { get; set; }\n}\n",
        ),
        (
            QUALIFIED_RECEIVER_SITE,
            "\nusing App.Bench;\n\nnamespace App.Rabbit.Configuration\n{\n    internal class Runner\n    {\n        readonly ConfigurationHostSettings.ConfigurationBatchSettings _settings;\n\n        public bool Enabled\n        {\n            set => _settings.Enabled = value;\n        }\n    }\n}\n",
        ),
    ]
}

fn qualified_receiver_fresh() -> HashMap<String, AnyFragment> {
    qualified_receiver_sources()
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

fn qualified_receiver_files() -> Vec<GraphFile> {
    qualified_receiver_sources()
        .into_iter()
        .map(|(rel, _)| GraphFile {
            rel: rel.to_string(),
            mtime: 7,
        })
        .collect()
}

/// The site fragment as the PREVIOUS generation wrote it: every key the
/// qualifier added removed, so its receiver facts read as the bare tail the
/// old extractor recorded.
fn qualified_receiver_payload_without_the_qualifier() -> String {
    fn strip(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("qualifiedType");
                map.remove("receiverTypeQualified");
                for child in map.values_mut() {
                    strip(child);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    strip(item);
                }
            }
            _ => {}
        }
    }
    let fragment = qualified_receiver_fresh()
        .remove(QUALIFIED_RECEIVER_SITE)
        .expect("site fragment present");
    let mut entry = serde_json::json!({"mtime": 7, "fragment": fragment});
    strip(&mut entry["fragment"]);
    serde_json::to_string(&serde_json::json!({ QUALIFIED_RECEIVER_SITE: entry })).unwrap()
}

fn qualified_receiver_targets(g: &Graph) -> Vec<String> {
    g.edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                to,
                member,
                heuristic: false,
                ..
            } if member.as_deref() == Some("Enabled") => Some(to.clone()),
            _ => None,
        })
        .collect()
}

fn rebuild_qualified_receiver(dir: &std::path::Path) -> Graph {
    match rebuild_graph(
        dir,
        &qualified_receiver_files(),
        &qualified_receiver_fresh(),
        true,
        None,
        None,
    )
    .unwrap()
    {
        RebuildOutcome::Rebuilt(g) => g,
        RebuildOutcome::NotRebuilt => unreachable!("changed is true"),
    }
}

#[test]
fn a_prior_generation_cache_cannot_leave_a_qualified_receiver_on_its_bare_tail() {
    let index = r#"{"src/App/Runner.cs":7}"#;
    let stale = qualified_receiver_payload_without_the_qualifier();

    let warm = temp_dir("qualified-receiver-warm-v32-cache");
    fs::create_dir_all(graph_dir(&warm)).unwrap();
    plant_cache_pair(&warm, &stale, index, "-v32");
    let warm_graph = rebuild_qualified_receiver(&warm);
    assert_eq!(
        qualified_receiver_targets(&warm_graph),
        vec!["App.Rabbit.Configuration.ConfigurationHostSettings+ConfigurationBatchSettings"],
        "the v32 payload is not read, so this run re-parses the source and keeps the qualifier"
    );
    assert!(!graph_dir(&warm).join("fragments-v32.json").exists());

    let cold = temp_dir("qualified-receiver-cold-cache");
    fs::create_dir_all(graph_dir(&cold)).unwrap();
    let cold_graph = rebuild_qualified_receiver(&cold);
    assert_eq!(
        qualified_receiver_targets(&warm_graph),
        qualified_receiver_targets(&cold_graph),
        "a run that ignored a prior-generation cache agrees with a cold one"
    );

    // The control: the same payload at the CURRENT generation is reused on
    // its matching mtime, and the receiver falls back to its bare tail --
    // which the file's own `using App.Bench;` answers with the decoy
    // sharing that tail. That is the loss the rename prevents, so this half
    // fails the moment the generation bump is reverted.
    let reused = temp_dir("qualified-receiver-current-generation-cache");
    fs::create_dir_all(graph_dir(&reused)).unwrap();
    plant_cache_pair(&reused, &stale, index, "-v33");
    let reused_graph = rebuild_qualified_receiver(&reused);
    assert_eq!(
        qualified_receiver_targets(&reused_graph),
        vec!["App.Bench.ConfigurationBatchSettings"],
        "reusing the tail-only entry at the current generation is exactly the wrong target the \
         rename prevents"
    );
}
