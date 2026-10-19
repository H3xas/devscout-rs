//! The export's header: what the facts were read from, what this release
//! extracts, and how fresh the index was when it was read.
//!
//! Provenance is deliberately three objects rather than one flat list. Source
//! inputs (the commit, the scope, the files that were already dirty when the
//! index was built, and the content digests it recorded) are what a reader
//! compares against its own checkout; analysis inputs (the extractor's version,
//! the schema versions, the configuration digests, the admitted compiler-fact
//! artifact) are what a reader compares against its own toolchain. Keeping them
//! apart is what lets another tool map each one onto its own snapshot identity
//! rather than guess at a merged set.
//!
//! Nothing machine-local is written: no absolute path, no user or host name,
//! no export-time clock. The record's own `owner` is an absolute path in the
//! publishing worktree's Git directory and never reaches the stream.

use serde::Serialize;

use crate::freshness::record::RECORD_VERSION;
use crate::graph::GRAPH_SCHEMA_VERSION;

use super::snapshot::Snapshot;

/// The stream's `format` discriminator, on the header and the trailer alike.
pub(crate) const FORMAT: &str = "devscout-facts";
/// The contract version this release writes.
pub(crate) const SCHEMA_VERSION: u32 = 1;

const EXTRACTED: &str = "extracted";
const NOT_EXTRACTED: &str = "not-extracted";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileDigest {
    path: String,
    // Absent for a path recorded as dirty whose bytes the index kept no digest
    // of -- a file deleted before it was indexed.
    digest: Option<String>,
}

/// The source inputs the facts were read from.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceProvenance {
    indexed_head: Option<String>,
    scope: Vec<String>,
    dirty_at_map: Vec<FileDigest>,
    file_count: u64,
    // The digest of the whole per-file digest table, so a reader can compare
    // one identity instead of thousands without losing the ability to compare
    // any single one: the table itself is still the export's input, this is a
    // receipt for it.
    file_digests: String,
}

/// The analysis inputs the facts depend on.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisProvenance {
    devscout_version: &'static str,
    graph_schema_version: u32,
    fragment_generation: u32,
    index_state_version: u32,
    config_inputs: Vec<FileDigest>,
    // The admitted artifact's own digest as it was read, or `none`.
    compiler_facts: String,
    semantic_lane: String,
}

/// The publishing run's identity, which is what tells two exports of the same
/// tree apart without either one carrying a clock of its own.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IndexProvenance {
    generation: String,
    indexed_at: String,
}

#[derive(Serialize)]
struct Provenance {
    source: SourceProvenance,
    analysis: AnalysisProvenance,
    index: IndexProvenance,
}

#[derive(Serialize, Clone, Copy)]
pub(super) struct LanguageCoverage {
    csharp: &'static str,
    typescript: &'static str,
    rust: &'static str,
}

// What this release extracts, per family and language -- a property of the
// build, not of the repository: an extracted family with no facts at all still
// reads `extracted`, which is how a consumer tells "this repository has none"
// from "this release cannot see them".
#[derive(Serialize)]
pub(super) struct Coverage {
    definition: LanguageCoverage,
    edge: LanguageCoverage,
    #[serde(rename = "publish-site")]
    publish_site: LanguageCoverage,
    #[serde(rename = "handler-registration")]
    handler_registration: LanguageCoverage,
    #[serde(rename = "di-registration")]
    di_registration: LanguageCoverage,
    #[serde(rename = "unresolved-import")]
    unresolved_import: LanguageCoverage,
    #[serde(rename = "test-file")]
    test_file: LanguageCoverage,
    #[serde(rename = "transport-address")]
    transport_address: LanguageCoverage,
    #[serde(rename = "http-route")]
    http_route: LanguageCoverage,
    #[serde(rename = "http-client")]
    http_client: LanguageCoverage,
}

// `pub(super)`: the unit tests assert the whole table, including the families
// this release declares it cannot see.
pub(super) fn coverage() -> Coverage {
    let all = LanguageCoverage {
        csharp: EXTRACTED,
        typescript: EXTRACTED,
        rust: EXTRACTED,
    };
    let csharp_only = LanguageCoverage {
        csharp: EXTRACTED,
        typescript: NOT_EXTRACTED,
        rust: NOT_EXTRACTED,
    };
    let typescript_only = LanguageCoverage {
        csharp: NOT_EXTRACTED,
        typescript: EXTRACTED,
        rust: NOT_EXTRACTED,
    };
    let none = LanguageCoverage {
        csharp: NOT_EXTRACTED,
        typescript: NOT_EXTRACTED,
        rust: NOT_EXTRACTED,
    };
    Coverage {
        definition: all,
        edge: all,
        publish_site: csharp_only,
        handler_registration: csharp_only,
        di_registration: csharp_only,
        unresolved_import: typescript_only,
        test_file: all,
        transport_address: csharp_only,
        http_route: none,
        http_client: none,
    }
}

// Every digest in the stream is spelled this way, whether the index recorded
// the bare hex or the export computed it.
fn spelled(digest: String) -> String {
    format!("sha256:{digest}")
}

fn filed(path: &str, digest: Option<&String>) -> FileDigest {
    FileDigest {
        path: path.to_string(),
        digest: digest.map(|d| spelled(d.clone())),
    }
}

fn source_provenance(snapshot: &Snapshot) -> SourceProvenance {
    let record = &snapshot.record;
    let config_inputs = &record.config_inputs;
    let dirty_at_map = record
        .dirty_at_map
        .clone()
        .unwrap_or_default()
        .iter()
        .map(|path| {
            let digest = snapshot
                .digests
                .files
                .get(path)
                .or_else(|| config_inputs.get(path));
            filed(path, digest)
        })
        .collect();
    // The table's own digest is taken over the pairs the stream names, each
    // already spelled the one way, so a reader can recompute it from the
    // facts' own vocabulary rather than from the index's storage spelling.
    let pairs: Vec<(String, String)> = snapshot
        .digests
        .files
        .iter()
        .map(|(path, digest)| (path.clone(), spelled(digest.clone())))
        .collect();
    let table = serde_json::to_string(&pairs).expect("a digest table always serializes");
    SourceProvenance {
        indexed_head: record.indexed_head.clone(),
        scope: record.scope.clone(),
        dirty_at_map,
        file_count: record.file_count,
        file_digests: spelled(crate::freshness::record::sha256_hex(table.as_bytes())),
    }
}

fn analysis_provenance(snapshot: &Snapshot) -> AnalysisProvenance {
    AnalysisProvenance {
        devscout_version: env!("CARGO_PKG_VERSION"),
        graph_schema_version: GRAPH_SCHEMA_VERSION,
        fragment_generation: snapshot.fragment_generation,
        index_state_version: RECORD_VERSION,
        config_inputs: snapshot
            .record
            .config_inputs
            .iter()
            .map(|(path, digest)| filed(path, Some(digest)))
            .collect(),
        compiler_facts: snapshot
            .compiler_facts
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        semantic_lane: snapshot
            .graph
            .stats
            .lane
            .as_ref()
            .map_or_else(|| "none".to_string(), |lane| lane.state().to_string()),
    }
}

fn provenance(snapshot: &Snapshot) -> Provenance {
    Provenance {
        source: source_provenance(snapshot),
        analysis: analysis_provenance(snapshot),
        index: IndexProvenance {
            generation: snapshot.record.generation.clone(),
            indexed_at: snapshot.record.indexed_at.clone(),
        },
    }
}

/// The header line, without its terminating newline.
///
/// Composed from separately serialized parts rather than as one struct because
/// `freshness` is the query layer's own `--json` shape: rendering it here would
/// be a second spelling of a vocabulary that already has one, and the two would
/// drift the first time a state gains a field.
pub(crate) fn line(snapshot: &Snapshot) -> String {
    format!(
        "{{\"format\":{},\"schemaVersion\":{SCHEMA_VERSION},\"provenance\":{},\"coverage\":{},\"freshness\":{}}}",
        serde_json::to_string(FORMAT).expect("a constant string always serializes"),
        serde_json::to_string(&provenance(snapshot)).expect("provenance always serializes"),
        serde_json::to_string(&coverage()).expect("the coverage table always serializes"),
        crate::cli::freshness_json(&snapshot.freshness).to_json_string(),
    )
}
