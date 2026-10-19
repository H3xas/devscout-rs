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
use std::io::{self, Write};
use std::path::{Path, PathBuf};
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
    #[serde(rename = "transport-address")]
    transport_address: usize,
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
            transport_address: counts[7],
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

/// How many temporary names one run may try before it gives up. Every attempt
/// is an exclusive create, so a name that is already taken costs an attempt
/// rather than a write.
const TMP_ATTEMPTS: u64 = 64;

/// Writes `bytes` to `path` through a temporary file in the same directory and
/// a rename, so a reader sees either the previous file or the complete new one
/// and never a partial write.
///
/// The temporary file is removed on ANY failure, the rename included, and only
/// when this run created it -- an entry that was already at the name belongs to
/// whoever put it there. The target's directory is never created: `--out` into
/// a path that does not exist is a usage-level mistake, not a request to build
/// the tree.
pub(crate) fn write_out(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("export.ndjson")
        .to_string();
    write_out_via(path, bytes, tmp_candidates(dir, name))
}

/// One candidate temporary name per attempt, in the target's own directory so
/// the final rename stays same-filesystem.
fn tmp_candidates(dir: Option<&Path>, name: String) -> impl Iterator<Item = PathBuf> {
    let dir = dir.unwrap_or_else(|| Path::new(".")).to_path_buf();
    let pid = std::process::id();
    (0..TMP_ATTEMPTS).map(move |_| {
        let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        dir.join(format!(".{name}.tmp.{pid}.{counter}"))
    })
}

/// Writes through the first candidate name this run can create exclusively.
///
/// The name is predictable -- a pid and a counter, the shape the repository's
/// other atomic writers use -- so another local actor sharing the output
/// directory can put an entry there first. Exclusive creation is what makes
/// that harmless: an existing entry is skipped, never opened, so a symlink
/// planted at the name is not followed and an unrelated file is not truncated,
/// and a leftover from a crashed run does not block the export forever.
pub(super) fn write_out_via(
    path: &Path,
    bytes: &[u8],
    names: impl Iterator<Item = PathBuf>,
) -> io::Result<()> {
    for tmp in names {
        let mut file = match create_exclusive(&tmp) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let write = file.write_all(bytes).and_then(|()| file.flush());
        drop(file);
        if let Err(error) = write {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
        if let Err(error) = fs::rename(&tmp, path) {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "every temporary name beside the output is taken",
    ))
}

fn create_exclusive(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}
