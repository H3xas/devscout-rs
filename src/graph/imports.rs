// The imported-edges artifact: a validated, versioned cross-repo edge export
// loaded by the `import-edges` verb and read back by `impact`. Lives beside
// `graph.json` under the same graph directory, but is its own file with its
// own schema -- importing one never touches `graph.json` or bumps
// `GRAPH_SCHEMA_VERSION`.
//
// The wire shape this module parses is the one the producer actually writes:
// an envelope `{schemaVersion, format, tool, provenance, edges}` where each
// edge end is `{repo, ref, file, line}` (the message end of a `publishes`/
// `consumes`/`enqueues` triple carries `repo: "message"` with no file or
// line) and `provenance` is a block -- `id`, `producer`, `formatVersion` --
// carried once in the envelope, not per record. Only `id` is ever compared;
// nothing here recomputes or verifies the digest it names.
//
// Validation is loud and total: a malformed file, a format or schema-version
// mismatch, a record missing `kind`/`from`/`to`, or a kind outside the fixed
// set below is rejected with a message naming the offending value, and
// rejection never touches whatever artifact already exists on disk -- the
// caller only calls `write_imported_edges` once `parse_imported_edges`
// returns `Ok`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::paths::{atomic_write_json, graph_dir};

/// The export format `import-edges` accepts.
pub const IMPORTED_EDGES_FORMAT: &str = "flowtrace-edges";

/// The export schema version `import-edges` accepts.
pub const IMPORTED_EDGES_SCHEMA_VERSION: u64 = 1;

/// The closed set of edge kinds a record may carry. Fixed by the producer's
/// own contract -- adding a kind here is out of scope for the importer.
pub const IMPORTED_EDGE_KINDS: &[&str] = &["calls", "consumes", "enqueues", "publishes", "tests"];

/// One end of an imported edge.
///
/// Every field but `repo` is optional: the message end of a
/// `publishes`/`consumes`/`enqueues` triple carries no file or line, and a
/// producer that does not track a reference name leaves `ref` absent. Joined
/// to this graph on `file` (repo-relative) plus `repo`, never on `ref` --
/// `ref` is carried through for display only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeEnd {
    /// The repo id this end belongs to, or `"message"` for a fileless
    /// message node. Absent (rather than a hard parse failure) when a
    /// producer omits it -- an end with no repo simply never matches the
    /// mapped repo id.
    #[serde(default)]
    pub repo: Option<String>,
    /// The producer's own symbol reference, display-only.
    #[serde(rename = "ref", default)]
    pub reference: Option<String>,
    /// The repo-relative file this end sits in, absent on a message end.
    #[serde(default)]
    pub file: Option<String>,
    /// The line this end sits at, absent on a message end.
    #[serde(default)]
    pub line: Option<u64>,
}

/// One joined edge from the export.
///
/// A `kind` in [`IMPORTED_EDGE_KINDS`], its two ends, and the join key a
/// `publishes` record shares with the `consumes`/`enqueues` record on the
/// other side of the same message node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportRecord {
    /// The kind value.
    pub kind: String,
    /// The from value.
    pub from: EdgeEnd,
    /// The to value.
    pub to: EdgeEnd,
    /// The message join key, present on `publishes`/`consumes`/`enqueues`
    /// records and absent (rather than compared) on every other kind.
    #[serde(default)]
    pub key: Option<String>,
}

/// The export's provenance: who produced it, in which format version, and
/// the digest an importer keys equality on. Carried once in the envelope,
/// never per record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    /// The digest identifying this export's generation. Compared for
    /// equality only -- never recomputed or verified here.
    pub id: String,
    /// The producer name and version, display-only.
    #[serde(default)]
    pub producer: String,
    /// The producer's own format version, display-only (already checked
    /// against [`IMPORTED_EDGES_SCHEMA_VERSION`] at the envelope level).
    #[serde(rename = "formatVersion", default)]
    pub format_version: u64,
}

/// The validated artifact `import-edges` writes and `impact` reads.
///
/// Carries the repo id the operator declared as "this repo" at import time,
/// the export's provenance, and every accepted record. A re-import always
/// overwrites this file wholesale -- there is no merge, so an edge the
/// newest export no longer states is simply gone from the next `impact`,
/// whether or not the provenance id changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportedEdges {
    /// This artifact's own schema version (distinct from the export's own
    /// `schemaVersion`, already checked and not re-stored).
    pub schema_version: u64,
    /// The repo id from `--repo <id>` that names "this repo" inside the
    /// export -- never inferred.
    pub mapped_repo: String,
    /// The export's provenance block.
    pub provenance: Provenance,
    /// Every accepted record, in file order.
    pub edges: Vec<ImportRecord>,
}

/// This artifact's own schema version.
pub const IMPORTED_EDGES_ARTIFACT_SCHEMA_VERSION: u64 = 1;

fn field_str<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(Value::as_str)
}

/// Parses and validates a cross-repo edge export.
///
/// Names `mapped_repo` (the operator's `--repo <id>`) in the returned
/// artifact. `Err` carries a message naming the offending value --
/// malformed JSON, a `format` or `schemaVersion` mismatch, a record missing
/// `kind`/`from`/`to`, or a kind outside [`IMPORTED_EDGE_KINDS`] -- and is
/// the ONLY failure mode: nothing here writes to disk, so a caller that
/// never reaches `Ok` never touches whatever artifact already exists.
pub fn parse_imported_edges(bytes: &[u8], mapped_repo: &str) -> Result<ImportedEdges, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|e| format!("malformed JSON: {e}"))?;
    let obj = value
        .as_object()
        .ok_or_else(|| "malformed JSON: expected an object at the top level".to_string())?;

    let format = field_str(obj, "format").unwrap_or("<missing>");
    if format != IMPORTED_EDGES_FORMAT {
        return Err(format!(
            "unsupported format \"{format}\", expected \"{IMPORTED_EDGES_FORMAT}\""
        ));
    }
    let schema_version = obj.get("schemaVersion").and_then(Value::as_u64);
    if schema_version != Some(IMPORTED_EDGES_SCHEMA_VERSION) {
        let got = obj
            .get("schemaVersion")
            .map(ToString::to_string)
            .unwrap_or_else(|| "<missing>".to_string());
        return Err(format!(
            "unsupported schemaVersion {got}, expected {IMPORTED_EDGES_SCHEMA_VERSION}"
        ));
    }

    let provenance_value = obj
        .get("provenance")
        .ok_or_else(|| "missing \"provenance\" block".to_string())?;
    let provenance: Provenance = serde_json::from_value(provenance_value.clone())
        .map_err(|e| format!("malformed \"provenance\" block: {e}"))?;

    let edges_value = obj
        .get("edges")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing \"edges\" array".to_string())?;

    let mut edges = Vec::with_capacity(edges_value.len());
    for (i, raw) in edges_value.iter().enumerate() {
        let rec_obj = raw
            .as_object()
            .ok_or_else(|| format!("edges[{i}] is not an object"))?;
        let kind =
            field_str(rec_obj, "kind").ok_or_else(|| format!("edges[{i}] missing \"kind\""))?;
        if !IMPORTED_EDGE_KINDS.contains(&kind) {
            let allowed = IMPORTED_EDGE_KINDS.join("|");
            return Err(format!(
                "edges[{i}] has kind \"{kind}\", not one of {allowed}"
            ));
        }
        let from_value = rec_obj
            .get("from")
            .ok_or_else(|| format!("edges[{i}] missing \"from\""))?;
        let to_value = rec_obj
            .get("to")
            .ok_or_else(|| format!("edges[{i}] missing \"to\""))?;
        let from: EdgeEnd = serde_json::from_value(from_value.clone())
            .map_err(|e| format!("edges[{i}].from is malformed: {e}"))?;
        let to: EdgeEnd = serde_json::from_value(to_value.clone())
            .map_err(|e| format!("edges[{i}].to is malformed: {e}"))?;
        let key = field_str(rec_obj, "key").map(str::to_string);
        edges.push(ImportRecord {
            kind: kind.to_string(),
            from,
            to,
            key,
        });
    }

    Ok(ImportedEdges {
        schema_version: IMPORTED_EDGES_ARTIFACT_SCHEMA_VERSION,
        mapped_repo: mapped_repo.to_string(),
        provenance,
        edges,
    })
}

/// Path to the imported-edges artifact for `root`, beside `graph.json` under
/// the same graph directory.
pub fn imported_edges_json_path(root: &Path) -> PathBuf {
    graph_dir(root).join("imported-edges.json")
}

/// Reads the imported-edges artifact for `root`, or `None` when absent or
/// unreadable.
///
/// The same fail-open convention [`super::read_graph`] follows, since this
/// artifact is auxiliary and its absence is the ordinary "no import
/// configured" state, not an error.
pub fn read_imported_edges(root: &Path) -> Option<ImportedEdges> {
    let text = fs::read_to_string(imported_edges_json_path(root)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Writes the imported-edges artifact for `root`, atomically.
///
/// The one and only writer: `import-edges` calls this after
/// [`parse_imported_edges`] returns `Ok`, always as a wholesale replace --
/// there is no merge with whatever this file already held.
pub fn write_imported_edges(root: &Path, data: &ImportedEdges) -> io::Result<()> {
    atomic_write_json(&imported_edges_json_path(root), data)
}
