//! The export stream: canonical order, the payload digest, the trailer, and
//! the two places its bytes can go.
//!
//! Everything is assembled in memory before a byte is emitted. That is what
//! makes a refusal total -- the snapshot is already read, so a run that cannot
//! finish never writes a partial stream to stdout or a half-written file to
//! `--out` -- and it is what lets the payload digest be computed over the exact
//! bytes the facts are sent as, in the order they are sent.
//!
//! Canonical order is a total order over the facts, not over the index's own
//! traversal: family rank, then file, then line, then the serialized line. Two
//! exports of one unchanged index therefore agree byte for byte, and two facts
//! that differ in nothing keep both their lines, adjacent.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

use crate::freshness::record::sha256_hex;

use super::facts::{self, Fact, FAMILY_ORDER};
use super::header;
use super::snapshot::Snapshot;

/// Per-family counts in [`FAMILY_ORDER`].
pub(crate) type Counts = [usize; FAMILY_ORDER.len()];

// The counts object, one key per family in rank order. A map would sort its
// keys (serde_json without `preserve_order`), and a `Vec` of pairs would
// serialize as an array, so the order is held by the field declarations.
#[derive(Serialize)]
struct CountedFamilies {
    definition: usize,
    edge: usize,
    #[serde(rename = "publish-site")]
    publish_site: usize,
    #[serde(rename = "handler-registration")]
    handler_registration: usize,
    #[serde(rename = "di-registration")]
    di_registration: usize,
    #[serde(rename = "unresolved-import")]
    unresolved_import: usize,
    #[serde(rename = "test-file")]
    test_file: usize,
}

impl CountedFamilies {
    fn of(counts: &Counts) -> CountedFamilies {
        CountedFamilies {
            definition: counts[0],
            edge: counts[1],
            publish_site: counts[2],
            handler_registration: counts[3],
            di_registration: counts[4],
            unresolved_import: counts[5],
            test_file: counts[6],
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Trailer {
    trailer: &'static str,
    counts: CountedFamilies,
    facts: usize,
    payload_digest: String,
}

/// A rendered stream, ready to be written as one unit.
pub(crate) struct Stream {
    header: String,
    /// Every fact line, in canonical order, each already terminated by `\n`.
    body: Vec<u8>,
    trailer: String,
}

impl Stream {
    /// The whole stream's bytes: header, facts, trailer, one `\n` each.
    pub(crate) fn bytes(&self) -> Vec<u8> {
        let mut bytes =
            Vec::with_capacity(self.header.len() + self.body.len() + self.trailer.len() + 2);
        bytes.extend_from_slice(self.header.as_bytes());
        bytes.push(b'\n');
        bytes.extend_from_slice(&self.body);
        bytes.extend_from_slice(self.trailer.as_bytes());
        bytes.push(b'\n');
        bytes
    }
}

fn canonical(facts: &mut [Fact]) {
    facts.sort_by(|left, right| {
        left.rank
            .cmp(&right.rank)
            .then_with(|| left.file.as_bytes().cmp(right.file.as_bytes()))
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.json.as_bytes().cmp(right.json.as_bytes()))
    });
}

fn count(facts: &[Fact]) -> Counts {
    let mut counts = [0usize; FAMILY_ORDER.len()];
    for fact in facts {
        counts[fact.rank] += 1;
    }
    counts
}

/// Renders one snapshot as a stream. No clock is read, so two calls over one
/// unchanged snapshot produce identical bytes.
pub(crate) fn render(snapshot: &Snapshot) -> Stream {
    let mut facts = facts::collect(snapshot);
    canonical(&mut facts);
    let counts = count(&facts);
    let mut body = Vec::new();
    for fact in &facts {
        body.extend_from_slice(fact.json.as_bytes());
        body.push(b'\n');
    }
    let trailer = serde_json::to_string(&Trailer {
        trailer: header::FORMAT,
        counts: CountedFamilies::of(&counts),
        facts: facts.len(),
        payload_digest: format!("sha256:{}", sha256_hex(&body)),
    })
    .expect("a trailer always serializes");
    Stream {
        header: header::line(snapshot),
        body,
        trailer,
    }
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Writes `bytes` to `path` through a temporary file in the same directory and
/// a rename, so a reader sees either the previous file or the complete new one
/// and never a partial write.
///
/// The temporary file is removed on ANY failure, the rename included, and the
/// target's directory is never created: `--out` into a path that does not exist
/// is a usage-level mistake, not a request to build the tree.
pub(crate) fn write_out(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("export.ndjson");
    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".{name}.tmp.{}.{counter}", std::process::id()));
    if let Err(error) = fs::write(&tmp, bytes) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    if let Err(error) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    Ok(())
}
