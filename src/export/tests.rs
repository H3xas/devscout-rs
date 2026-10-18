//! Unit tests for the pieces that need no repository: the evidence mapping, the
//! refusal vocabulary, the coverage table, and the stream's own ordering and
//! digest over a hand-built index.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::freshness::record::{self, ArtifactIdentity, Artifacts, Record};
use crate::graph::{
    Candidate, Edge, EdgesByKind, Graph, HeuristicByTier, HeuristicTier, Percent1,
    SemanticProvenance, Stats, GRAPH_SCHEMA_VERSION,
};

use super::evidence;
use super::refusal::ExportRefusal;
use super::snapshot;
use super::stream;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn scratch(label: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("scout-export-{label}-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).expect("create scratch dir");
    fs::canonicalize(&dir).expect("canonicalize scratch dir")
}

fn git(root: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git must run");
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

fn empty_stats() -> Stats {
    Stats {
        def_count: 0,
        file_count: 0,
        edges_by_kind: EdgesByKind::default(),
        ambiguous_count: 0,
        ambiguous_pct: Percent1::zero(),
        unresolved_external_count: 0,
        heuristic_edge_count: 0,
        test_def_count: 0,
        ts: None,
        heuristic_by_tier: HeuristicByTier::default(),
        bus_vocabulary_derived: None,
        semantic: None,
        lane: None,
    }
}

/// A repository root carrying one published, empty index: a record, its digest
/// table, a graph and an empty fragment cache, all written by this build's own
/// writers so the identities the record certifies are the ones on disk.
pub(super) fn indexed_root(label: &str) -> PathBuf {
    let root = scratch(label);
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "test@example.com"]);
    git(&root, &["config", "user.name", "test"]);

    let manifest = record::state_dir(&root).join("manifest.json");
    fs::create_dir_all(record::state_dir(&root)).expect("create state dir");
    fs::write(&manifest, b"{}").expect("write manifest");

    let graph = Graph {
        schema_version: GRAPH_SCHEMA_VERSION,
        built_at_head: None,
        defs: Vec::new(),
        edges: Vec::new(),
        stats: empty_stats(),
        names: Vec::new(),
        units: Vec::new(),
        unresolved_imports: Vec::new(),
        transport: Vec::new(),
        template_unsupported: Vec::new(),
    };
    crate::graph::write_graph(&root, &graph).expect("write graph");
    crate::graph::write_fragments_cache(&root, &crate::graph::OrderedMap::default())
        .expect("write fragments");

    let generation = record::new_generation("test");
    record::write_digests(&root, &generation, &BTreeMap::new()).expect("write digests");
    let published = Record {
        format: record::RECORD_FORMAT.to_string(),
        version: record::RECORD_VERSION,
        state: "published".to_string(),
        owner: record::owner_of(&root).expect("the scratch root is a git repository"),
        generation,
        indexed_head: None,
        scope: vec![".".to_string()],
        indexed_at: "2026-01-01T00:00:00Z".to_string(),
        file_count: 0,
        dirty_at_map: Some(Vec::new()),
        config_inputs: BTreeMap::new(),
        artifacts: Artifacts {
            graph: ArtifactIdentity::of(&crate::graph::graph_json_path(&root)),
            manifest: ArtifactIdentity::of(&manifest),
        },
    };
    record::write_record(&root, &published).expect("write record");
    root
}

fn member_edge(tier: Option<HeuristicTier>, source: Option<SemanticProvenance>) -> Edge {
    match (tier, source) {
        (Some(tier), None) => Edge::uses_member(
            "a.cs".to_string(),
            1,
            "T.M".to_string(),
            "b.cs".to_string(),
            Some("M".to_string()),
            Some(tier),
        ),
        (None, Some(source)) => Edge::uses_member_semantic(
            "a.cs".to_string(),
            1,
            "T.M".to_string(),
            "b.cs".to_string(),
            Some("M".to_string()),
            source,
            None,
        ),
        (None, None) => Edge::uses_member(
            "a.cs".to_string(),
            1,
            "T.M".to_string(),
            "b.cs".to_string(),
            Some("M".to_string()),
            None,
        ),
        (Some(_), Some(_)) => panic!("an edge is either tiered or compiler-sourced, never both"),
    }
}

fn ctor_di(resolution: &str) -> Edge {
    Edge::CtorDi {
        from_file: "a.cs".to_string(),
        from_line: 1,
        iface: "IService".to_string(),
        resolution: resolution.to_string(),
        args: None,
        to: None,
        candidates: Vec::new(),
    }
}

#[test]
fn fact_export_ac_3_the_evidence_mapping_states_the_tier_a_guess_came_from() {
    assert_eq!(
        evidence::edge_evidence(&member_edge(Some(HeuristicTier::Guess), None)),
        "heuristic-guess"
    );
    assert_eq!(
        evidence::edge_evidence(&member_edge(Some(HeuristicTier::Ext), None)),
        "heuristic-ext"
    );
    // A flagged edge that names no tier is stated as the weaker word rather
    // than as the tier it might have been.
    assert_eq!(
        evidence::edge_evidence(&Edge::UsesType {
            from_file: "a.cs".to_string(),
            from_line: 1,
            to: "T".to_string(),
            to_file: "b.cs".to_string(),
            heuristic: true,
        }),
        "heuristic"
    );
    assert_eq!(
        evidence::edge_evidence(&member_edge(None, None)),
        "exact",
        "a precise member reference is never exported as a guess"
    );
}

#[test]
fn fact_export_ac_3_a_compiler_sourced_edge_names_its_source() {
    assert_eq!(
        evidence::edge_evidence(&member_edge(None, Some(SemanticProvenance::Semantic))),
        "semantic"
    );
    assert_eq!(
        evidence::edge_evidence(&member_edge(
            None,
            Some(SemanticProvenance::SemanticDiscovered)
        )),
        "semantic-discovered"
    );
}

#[test]
fn fact_export_ac_3_every_edge_kind_and_resolution_word_maps_onto_one_evidence_class() {
    let inherited = Edge::Inherits {
        from_file: "a.cs".to_string(),
        from_line: 1,
        to: "B".to_string(),
        to_file: "b.cs".to_string(),
        heuristic: false,
    };
    assert_eq!(evidence::edge_evidence(&inherited), "exact");
    assert_eq!(
        evidence::edge_evidence(&Edge::Imports {
            from_file: "a.cs".to_string(),
            from_line: 1,
            target: "System.Text".to_string(),
        }),
        "declared",
        "a C# using names a namespace and resolves to no file"
    );
    assert_eq!(
        evidence::edge_evidence(&Edge::Ambiguous {
            origin: "a.cs".to_string(),
            from_file: "a.cs".to_string(),
            from_line: 1,
            raw: "Load".to_string(),
            candidates: vec![Candidate {
                id: "T.Load".to_string(),
                file: "b.cs".to_string(),
            }],
            candidate_count: 1,
        }),
        "ambiguous"
    );
    assert_eq!(evidence::edge_evidence(&ctor_di("plain")), "exact");
    assert_eq!(evidence::edge_evidence(&ctor_di("closed")), "exact");
    assert_eq!(evidence::edge_evidence(&ctor_di("open-generic")), "exact");
    assert_eq!(evidence::edge_evidence(&ctor_di("ambiguous")), "ambiguous");
    assert_eq!(
        evidence::edge_evidence(&ctor_di("unresolved")),
        "unresolved"
    );
    assert_eq!(evidence::edge_evidence(&ctor_di("infra")), "unresolved");
    assert_eq!(
        evidence::edge_evidence(&ctor_di("a-word-a-later-release-adds")),
        "unresolved",
        "an unknown resolution word must never be stated as a bound injection"
    );
    assert_eq!(
        evidence::edge_evidence(&Edge::BusHop {
            from_file: "a.cs".to_string(),
            from_line: 1,
            message: "OrderPlaced".to_string(),
            to: "Handler".to_string(),
            to_file: "b.cs".to_string(),
            evidence: "base-arg".to_string(),
        }),
        "derived"
    );
}

#[test]
fn fact_export_ac_3_every_evidence_word_the_mapping_produces_is_in_the_closed_vocabulary() {
    let produced = [
        evidence::edge_evidence(&inherited()),
        evidence::edge_evidence(&member_edge(Some(HeuristicTier::Ext), None)),
        evidence::edge_evidence(&member_edge(Some(HeuristicTier::Guess), None)),
        evidence::edge_evidence(&member_edge(None, Some(SemanticProvenance::Semantic))),
        evidence::edge_evidence(&member_edge(
            None,
            Some(SemanticProvenance::SemanticDiscovered),
        )),
        evidence::edge_evidence(&member_edge(None, None)),
        evidence::edge_evidence(&ctor_di("ambiguous")),
        evidence::edge_evidence(&ctor_di("infra")),
        evidence::edge_evidence(&Edge::Imports {
            from_file: "a.cs".to_string(),
            from_line: 1,
            target: "N".to_string(),
        }),
        evidence::edge_evidence(&Edge::BusHop {
            from_file: "a.cs".to_string(),
            from_line: 1,
            message: "M".to_string(),
            to: "H".to_string(),
            to_file: "b.cs".to_string(),
            evidence: "base-arg".to_string(),
        }),
        evidence::DECLARED,
        evidence::CONVENTION,
        evidence::UNRESOLVED,
    ];
    for word in produced {
        assert!(
            evidence::VOCABULARY.contains(&word),
            "{word} is not a documented evidence word"
        );
    }
    let mut unique: Vec<&str> = evidence::VOCABULARY.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        evidence::VOCABULARY.len(),
        "the vocabulary repeats a word"
    );
}

fn inherited() -> Edge {
    Edge::Inherits {
        from_file: "a.cs".to_string(),
        from_line: 1,
        to: "B".to_string(),
        to_file: "b.cs".to_string(),
        heuristic: false,
    }
}

#[test]
fn fact_export_ac_3_the_coverage_table_states_what_this_release_extracts() {
    let text = serde_json::to_string(&super::header::coverage()).expect("coverage serializes");
    let coverage: serde_json::Value = serde_json::from_str(&text).expect("coverage parses");
    for family in ["definition", "edge", "test-file"] {
        for language in ["csharp", "typescript", "rust"] {
            assert_eq!(
                coverage[family][language], "extracted",
                "{family}/{language}"
            );
        }
    }
    for family in [
        "publish-site",
        "handler-registration",
        "di-registration",
        "transport-address",
    ] {
        assert_eq!(coverage[family]["csharp"], "extracted", "{family}");
        assert_eq!(coverage[family]["typescript"], "not-extracted", "{family}");
        assert_eq!(coverage[family]["rust"], "not-extracted", "{family}");
    }
    assert_eq!(coverage["unresolved-import"]["typescript"], "extracted");
    assert_eq!(coverage["unresolved-import"]["csharp"], "not-extracted");
    assert_eq!(coverage["unresolved-import"]["rust"], "not-extracted");
    for family in ["http-route", "http-client"] {
        for language in ["csharp", "typescript", "rust"] {
            assert_eq!(
                coverage[family][language], "not-extracted",
                "{family}/{language}: this release extracts none of them"
            );
        }
    }
}

#[test]
fn fact_export_ac_6_every_refusal_variant_renders_one_line_naming_the_reason_and_map() {
    for refusal in ExportRefusal::every_variant() {
        let line = refusal.line();
        assert!(!line.contains('\n'), "a refusal is one stderr line: {line}");
        assert!(
            line.contains(&format!("({})", refusal.reason())),
            "the reason word is stated: {line}"
        );
        assert!(
            line.contains("devscout map"),
            "every refusal names the one recovery: {line}"
        );
    }
}

#[test]
fn fact_export_ac_1_an_index_with_no_record_and_no_graph_refuses_with_no_index() {
    let root = scratch("bare");
    let refusal = snapshot::load(&root, &root)
        .map(|_| ())
        .expect_err("a bare directory has no index");
    assert_eq!(refusal.reason(), "no-index");
}

#[test]
fn fact_export_ac_6_a_record_removed_mid_run_refuses_as_index_changed() {
    let root = indexed_root("mid-run");
    assert!(
        snapshot::load(&root, &root).is_ok(),
        "the hand-built index is readable"
    );
    let refusal = snapshot::load_bracketed(&root, &root, &mut |root| {
        fs::remove_file(record::record_path(root)).expect("remove the record");
    })
    .map(|_| ())
    .expect_err("a record that disappears under the run cannot be published");
    assert_eq!(refusal.reason(), "index-changed");
}

#[test]
fn fact_export_ac_5_two_renders_of_one_index_agree_byte_for_byte() {
    let root = indexed_root("determinism");
    let first = snapshot::load(&root, &root).expect("the hand-built index is readable");
    let second = snapshot::load(&root, &root).expect("the hand-built index is readable");
    assert_eq!(
        stream::render(&first).bytes(),
        stream::render(&second).bytes(),
        "an unchanged index must render once, not twice"
    );
}

#[test]
fn fact_export_ac_4_the_payload_digest_covers_the_fact_lines_alone() {
    let root = indexed_root("digest");
    let loaded = snapshot::load(&root, &root).expect("the hand-built index is readable");
    let rendered = stream::render(&loaded).bytes();
    let text = String::from_utf8(rendered).expect("the stream is UTF-8");
    let mut lines = text.lines();
    let header = lines.next().expect("a header line");
    assert!(header.starts_with("{\"format\":\"devscout-facts\",\"schemaVersion\":1,"));
    let trailer = text.lines().last().expect("a trailer line");
    let trailer: serde_json::Value = serde_json::from_str(trailer).expect("the trailer parses");
    let body: String = lines
        .take_while(|line| !line.starts_with("{\"trailer\""))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(trailer["facts"], 0);
    assert_eq!(
        trailer["payloadDigest"],
        format!("sha256:{}", record::sha256_hex(body.as_bytes()))
    );
    assert_eq!(trailer["counts"]["definition"], 0);
}

// The temporary name beside `--out` is predictable -- a pid and a counter, the
// same shape the repository's other atomic writers use -- so a local actor can
// put an entry there first. Following that entry would write through it: onto a
// symlink's target, or over an unrelated file. Every candidate is therefore
// created exclusively, and one that already exists is skipped rather than
// opened, so a leftover or a planted name costs an attempt, never a write.

#[test]
fn fact_export_ac_1_an_occupied_temporary_name_is_never_opened_or_truncated() {
    let dir = scratch("tmp-taken");
    let target = dir.join("stream.ndjson");
    fs::write(&target, b"the previous stream\n").expect("write the destination");
    // Every name this run can pick is already taken by an unrelated file.
    let pid = std::process::id();
    let mut planted = Vec::new();
    for counter in 0..512u64 {
        let path = dir.join(format!(".stream.ndjson.tmp.{pid}.{counter}"));
        fs::write(&path, format!("unrelated {counter}\n")).expect("plant a file");
        planted.push(path);
    }
    let error = stream::write_out(&target, b"the new stream\n")
        .expect_err("every temporary name is taken, so nothing may be written");
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    for path in &planted {
        let content = fs::read_to_string(path).expect("the planted file is still readable");
        assert!(
            content.starts_with("unrelated "),
            "{} was truncated: {content}",
            path.display()
        );
    }
    assert_eq!(
        fs::read(&target).expect("read the destination"),
        b"the previous stream\n",
        "the destination keeps its previous bytes"
    );
    assert_eq!(
        fs::read_dir(&dir).expect("read the directory").count(),
        planted.len() + 1,
        "this run leaves no temporary entry of its own"
    );
}

#[test]
fn fact_export_ac_1_an_occupied_temporary_name_does_not_block_the_next_one() {
    let dir = scratch("tmp-reuse");
    let stale = dir.join(".stream.ndjson.tmp.claim.0");
    fs::write(&stale, b"a crashed run's leftover\n").expect("plant the leftover");
    let free = dir.join(".stream.ndjson.tmp.claim.1");
    let target = dir.join("stream.ndjson");
    fs::write(&target, b"the previous stream\n").expect("write the destination");
    stream::write_out_via(
        &target,
        b"the new stream\n",
        vec![stale.clone(), free.clone()].into_iter(),
    )
    .expect("the next free name is used");
    assert_eq!(
        fs::read(&stale).expect("read the leftover"),
        b"a crashed run's leftover\n",
        "an entry this run did not create is left alone"
    );
    assert_eq!(
        fs::read(&target).expect("read the destination"),
        b"the new stream\n"
    );
    assert!(
        !free.exists(),
        "the used name was renamed onto the destination"
    );
}

#[cfg(unix)]
#[test]
fn fact_export_ac_1_a_temporary_name_that_is_a_symlink_is_skipped_not_followed() {
    let dir = scratch("tmp-symlink");
    let victim = dir.join("sentinel.txt");
    fs::write(&victim, b"PREEXISTING USER DATA\n").expect("write the sentinel");
    let taken = dir.join(".stream.ndjson.tmp.claim.0");
    std::os::unix::fs::symlink(&victim, &taken).expect("plant the symlink");
    let free = dir.join(".stream.ndjson.tmp.claim.1");
    let target = dir.join("stream.ndjson");
    stream::write_out_via(
        &target,
        b"the new stream\n",
        vec![taken.clone(), free.clone()].into_iter(),
    )
    .expect("the next free name is used");
    assert_eq!(
        fs::read(&victim).expect("read the sentinel"),
        b"PREEXISTING USER DATA\n",
        "the symlink's target is untouched"
    );
    assert!(
        fs::symlink_metadata(&taken)
            .expect("stat the planted entry")
            .file_type()
            .is_symlink(),
        "the planted entry is left as it was"
    );
    assert_eq!(
        fs::read(&target).expect("read the destination"),
        b"the new stream\n"
    );
    assert!(
        !free.exists(),
        "the used name was renamed onto the destination"
    );
}
