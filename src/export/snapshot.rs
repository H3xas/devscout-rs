//! The one consistent read of the index a `devscout export` run is built from.
//!
//! Everything the export publishes comes from a single published generation, or
//! the run refuses. A repository whose index a `map` is rewriting while the
//! export reads it would otherwise publish a header from one generation beside
//! facts from another, and a consumer has no way to notice that from the bytes.
//!
//! The read is bracketed: the record is read first, every artifact's identity
//! is checked against the identities the record certifies, the query layer's
//! own freshness validation runs at the end (which re-reads the record and
//! re-stats the two artifacts it certifies), and then this module re-reads the
//! record and every identity it took itself. Any difference is `index-changed`.
//!
//! The fragment cache is read through the strict reader rather than the
//! fail-open one `map` uses: an export that silently dropped every C# messaging
//! fact of a repository would be worse than a refusal, and the owner it checks
//! against is the record's, not the exporting worktree's.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use crate::freshness::record::{
    self, ArtifactIdentity, DigestReadError, DigestTable, Record, RecordState,
};
use crate::freshness::validate;
use crate::freshness::{FreshnessState, UnknownReason};
use crate::graph::{self, FragMessaging, Graph, StrictCacheError, GRAPH_SCHEMA_VERSION};
use crate::query::{self, TestVia};

use super::refusal::ExportRefusal;

/// Everything one export publishes, read from one published index generation.
pub(crate) struct Snapshot {
    /// The published record the generation is certified by.
    pub(crate) record: Record,
    /// Every indexed path's content digest, from the record's own generation.
    pub(crate) digests: DigestTable,
    /// The repository graph.
    pub(crate) graph: Graph,
    /// Each cached file's messaging facts, in the cache's own file order.
    pub(crate) messaging: Vec<(String, FragMessaging)>,
    /// Which files the test classification places in the test set, and which
    /// vouch put each one there.
    pub(crate) test_via: BTreeMap<String, TestVia>,
    /// The digest of the admitted compiler-fact artifact as read, absent when
    /// there is none.
    pub(crate) compiler_facts: Option<String>,
    /// The fragment generation this build's cache path carries.
    pub(crate) fragment_generation: u32,
    /// The freshness observed at export time, in the query vocabulary.
    pub(crate) freshness: FreshnessState,
    /// The freshness warning line, `None` when stderr stays silent.
    pub(crate) freshness_warning: Option<String>,
}

/// Reads the index at `root` as it stands now.
pub(crate) fn load(root: &Path, invocation_dir: &Path) -> Result<Snapshot, ExportRefusal> {
    load_bracketed(root, invocation_dir, &mut |_| {})
}

/// [`load`] with a seam between the reads and the bracket's closing check, for
/// the unit case where the index is replaced while an export runs. Production
/// passes a no-op.
pub(crate) fn load_bracketed(
    root: &Path,
    invocation_dir: &Path,
    between: &mut dyn FnMut(&Path),
) -> Result<Snapshot, ExportRefusal> {
    let begin = validate::begin(root, invocation_dir);
    let record = published_record(root)?;
    let digests = digest_table(root, &record)?;
    let graph_path = graph::graph_json_path(root);
    let manifest_path = validate::manifest_artifact_path(root);
    let expected_graph = record.artifacts.graph.ok_or_else(|| {
        ExportRefusal::ArtifactMissing("the index certifies no graph.json".to_string())
    })?;

    if !graph_path.is_file() {
        return Err(ExportRefusal::ArtifactMissing("graph.json".to_string()));
    }
    // The artifact is read and parsed BEFORE its identity is compared with the
    // record's: a truncated or half-written graph is named as the unreadable
    // artifact it is, rather than only as an identity that moved.
    let bytes = fs::read(&graph_path).map_err(|_| {
        ExportRefusal::ArtifactUnreadable("graph.json could not be read".to_string())
    })?;
    let graph: Graph = serde_json::from_slice(&bytes)
        .map_err(|_| ExportRefusal::ArtifactUnreadable("graph.json does not parse".to_string()))?;
    if graph.schema_version != GRAPH_SCHEMA_VERSION {
        return Err(ExportRefusal::UnknownGeneration(format!(
            "graph.json is schema version {}, this build writes {GRAPH_SCHEMA_VERSION}",
            graph.schema_version
        )));
    }
    if ArtifactIdentity::of(&graph_path) != Some(expected_graph)
        || ArtifactIdentity::of(&manifest_path) != record.artifacts.manifest
    {
        return Err(ExportRefusal::IndexChanged(
            "graph.json or manifest.json is not the artifact the index record certifies"
                .to_string(),
        ));
    }

    let messaging = read_messaging(root, &record)?;
    let fragment_identity = ArtifactIdentity::of(&graph::fragments_cache_path(root));
    let test_via = classify_test_files(root, &graph, &messaging);
    let compiler_identity = ArtifactIdentity::of(&graph::compiler_facts_json_path(root));
    let compiler_facts = read_compiler_facts(root)?;

    between(root);
    let validation = begin.finish();
    confirm_unchanged(
        root,
        &record,
        &graph_path,
        &manifest_path,
        fragment_identity,
        compiler_identity,
        &validation,
    )?;

    Ok(Snapshot {
        record,
        digests,
        graph,
        messaging,
        test_via,
        compiler_facts,
        fragment_generation: graph::fragment_generation(),
        freshness: validation.state,
        freshness_warning: validation.warning,
    })
}

// Which of the three record states this root is in, mapped onto the refusal it
// means for an export. A graph with no record is not "no index": the bytes are
// there, only the certification is missing, and that is the one case a reader
// can fix with `map` alone.
fn published_record(root: &Path) -> Result<Record, ExportRefusal> {
    match record::read_record(root) {
        RecordState::Published(record) => Ok(*record),
        RecordState::Missing => Err(if graph::graph_json_path(root).is_file() {
            ExportRefusal::NoIndexState(
                "a graph.json is here with no published index record".to_string(),
            )
        } else {
            ExportRefusal::NoIndex("this repository has no index".to_string())
        }),
        RecordState::Uncertifiable => Err(ExportRefusal::LegacyIndex(
            "the index record is not a shape this build certifies".to_string(),
        )),
        RecordState::InProgress(_) => Err(ExportRefusal::IndexChanged(
            "a `map` is running in this repository".to_string(),
        )),
    }
}

fn digest_table(root: &Path, record: &Record) -> Result<DigestTable, ExportRefusal> {
    let table = match record::read_digests_strict(root) {
        Ok(table) => table,
        Err(DigestReadError::Missing) => {
            return Err(ExportRefusal::LegacyIndex(
                "the index has no per-file digest table".to_string(),
            ))
        }
        Err(DigestReadError::Unreadable) => {
            return Err(ExportRefusal::ArtifactUnreadable(
                "the per-file digest table does not parse".to_string(),
            ))
        }
        Err(DigestReadError::UnknownFormat) => {
            return Err(ExportRefusal::UnknownGeneration(
                "the per-file digest table is of an unknown format".to_string(),
            ))
        }
        Err(DigestReadError::Changed) => {
            return Err(ExportRefusal::IndexChanged(
                "the per-file digest table changed while it was read".to_string(),
            ))
        }
    };
    if table.generation != record.generation {
        return Err(ExportRefusal::IndexChanged(
            "the per-file digest table belongs to another `map` run".to_string(),
        ));
    }
    Ok(table)
}

fn read_messaging(
    root: &Path,
    record: &Record,
) -> Result<Vec<(String, FragMessaging)>, ExportRefusal> {
    match graph::read_fragments_messaging(root, &record.owner) {
        Ok(entries) => Ok(entries),
        Err(StrictCacheError::Missing) => Err(if graph::superseded_cache_present(root) {
            ExportRefusal::UnknownGeneration(
                "only a fragment cache of another generation is here".to_string(),
            )
        } else {
            ExportRefusal::ArtifactMissing("the fragment cache".to_string())
        }),
        Err(StrictCacheError::Unreadable) => Err(ExportRefusal::ArtifactUnreadable(
            "the fragment cache does not parse".to_string(),
        )),
        Err(StrictCacheError::ForeignOwner) => Err(ExportRefusal::IndexChanged(
            "the fragment cache was written by another worktree".to_string(),
        )),
        Err(StrictCacheError::Changed) => Err(ExportRefusal::IndexChanged(
            "the fragment cache changed while it was read".to_string(),
        )),
    }
}

// The graph's own input files are the fragment cache's keys, so the cache's
// file set is also the file set the test classification runs over: a file the
// index never extracted cannot be vouched for either way.
fn classify_test_files(
    root: &Path,
    graph: &Graph,
    messaging: &[(String, FragMessaging)],
) -> BTreeMap<String, TestVia> {
    let index = query::load_graph_index(graph, root);
    messaging
        .iter()
        .filter_map(|(file, _)| query::test_via(&index, file).map(|via| (file.clone(), via)))
        .collect()
}

fn read_compiler_facts(root: &Path) -> Result<Option<String>, ExportRefusal> {
    match fs::read(graph::compiler_facts_json_path(root)) {
        Ok(bytes) => Ok(Some(format!("sha256:{}", record::sha256_hex(&bytes)))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(ExportRefusal::ArtifactUnreadable(
            "the admitted compiler-fact artifact could not be read".to_string(),
        )),
    }
}

// The bracket's closing half: the query layer's validation re-reads the record
// and re-stats the two artifacts it certifies, and this module then re-reads
// the record and the identities of everything it read itself.
fn confirm_unchanged(
    root: &Path,
    record: &Record,
    graph_path: &Path,
    manifest_path: &Path,
    fragment_identity: Option<ArtifactIdentity>,
    compiler_identity: Option<ArtifactIdentity>,
    validation: &validate::Validation,
) -> Result<(), ExportRefusal> {
    if let FreshnessState::Unknown { reason } = &validation.state {
        if matches!(
            reason,
            UnknownReason::IndexChanged | UnknownReason::NoIndexState
        ) {
            return Err(ExportRefusal::IndexChanged(
                "the index moved while the export ran".to_string(),
            ));
        }
    }
    match record::read_record(root) {
        RecordState::Published(end) if end.generation == record.generation => {}
        _ => {
            return Err(ExportRefusal::IndexChanged(
                "the index was republished while the export ran".to_string(),
            ))
        }
    }
    if ArtifactIdentity::of(graph_path) != record.artifacts.graph
        || ArtifactIdentity::of(manifest_path) != record.artifacts.manifest
        || ArtifactIdentity::of(&graph::fragments_cache_path(root)) != fragment_identity
        || ArtifactIdentity::of(&graph::compiler_facts_json_path(root)) != compiler_identity
    {
        return Err(ExportRefusal::IndexChanged(
            "an index artifact changed while the export ran".to_string(),
        ));
    }
    Ok(())
}
