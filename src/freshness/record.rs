//! The index's freshness record: who published it, from which bytes, and which
//! two artifacts it certifies.
//!
//! The record keeps the 0.9.0 file name (`index-state.json`) but not its shape.
//! A 0.9.0 binary reads the new shape as "no freshness record", and a 0.9.0
//! `map` overwrites it with the legacy shape, which this build likewise refuses
//! to certify -- so neither binary can ever certify the other's index, and the
//! recovery on both sides is one `map`. Anything malformed, unrecognised or
//! missing a required field certifies nothing: the safe direction is always
//! `unknown`, never `fresh`.
//!
//! The two artifacts are named by **file identity** (`len`, `modified_ns`, and
//! on Unix `dev`/`ino`) rather than by a token written into them. Every artifact
//! write goes through an atomic rename, which installs a new inode, so a later
//! writer -- an older build, or another worktree -- changes the identity without
//! having to cooperate with this format at all.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::repo::{git_common_dir, git_dir_for, scout_dir};

/// The record's `format` discriminator.
pub const RECORD_FORMAT: &str = "devscout-index-state";
/// The record's on-disk version.
pub const RECORD_VERSION: u32 = 2;
/// The digest table's `format` discriminator.
pub const DIGESTS_FORMAT: &str = "devscout-index-digests";

/// The identity of one artifact file, as the filesystem reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactIdentity {
    /// The file's length in bytes.
    pub len: u64,
    /// The file's modification time in nanoseconds since the Unix epoch.
    pub modified_ns: u64,
    /// The containing device, on platforms that expose one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev: Option<u64>,
    /// The inode, on platforms that expose one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ino: Option<u64>,
}

impl ArtifactIdentity {
    /// The identity of the file at `path`, or `None` when it is absent or
    /// unreadable.
    #[must_use]
    pub fn of(path: &Path) -> Option<ArtifactIdentity> {
        let meta = fs::metadata(path).ok()?;
        let modified_ns = modified_ns(&meta);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Some(ArtifactIdentity {
                len: meta.len(),
                modified_ns,
                dev: Some(meta.dev()),
                ino: Some(meta.ino()),
            })
        }
        #[cfg(not(unix))]
        {
            Some(ArtifactIdentity {
                len: meta.len(),
                modified_ns,
                dev: None,
                ino: None,
            })
        }
    }
}

fn modified_ns(meta: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let secs = meta.mtime().max(0) as u64;
        let nanos = meta.mtime_nsec().max(0) as u64;
        secs.saturating_mul(1_000_000_000).saturating_add(nanos)
    }
    #[cfg(not(unix))]
    {
        meta.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos().min(u64::MAX as u128) as u64)
            .unwrap_or(0)
    }
}

/// The two artifacts a published generation certifies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifacts {
    /// `graph.json`'s identity, absent when the index has no graph.
    #[serde(default)]
    pub graph: Option<ArtifactIdentity>,
    /// `manifest.json`'s identity.
    #[serde(default)]
    pub manifest: Option<ArtifactIdentity>,
}

/// A published freshness record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// Always [`RECORD_FORMAT`].
    pub format: String,
    /// Always [`RECORD_VERSION`].
    pub version: u32,
    /// Always `"published"`.
    pub state: String,
    /// The publishing worktree's canonical Git administrative directory.
    pub owner: String,
    /// This publication's unique generation token.
    pub generation: String,
    /// The `HEAD` the index was built from, `null` when the map saw no commit.
    pub indexed_head: Option<String>,
    /// The directories this map covered.
    pub scope: Vec<String>,
    /// When the record was published.
    pub indexed_at: String,
    /// How many manifest entries the index holds.
    pub file_count: u64,
    /// The relevant paths already changed when `map` read them. Absent when the
    /// run could not establish them, which makes the record uncertifiable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dirty_at_map: Option<Vec<String>>,
    /// Each configuration input's recorded content digest.
    pub config_inputs: BTreeMap<String, String>,
    /// The identities of the artifacts this record certifies.
    pub artifacts: Artifacts,
}

/// The record a `map` leaves behind while it runs, so a query racing it (or
/// reading after a crash) never certifies a half-written generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    /// Always [`RECORD_FORMAT`].
    pub format: String,
    /// Always [`RECORD_VERSION`].
    pub version: u32,
    /// Always `"in-progress"`.
    pub state: String,
    /// The worktree running the map.
    pub owner: String,
    /// When the map started.
    pub started_at: String,
}

/// What the record file at a root currently holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordState {
    /// No record file at all. Distinct from every other arm because stderr
    /// stays silent for it: a repo that was never mapped says nothing.
    Missing,
    /// A `map` is running, or was interrupted, at this root.
    InProgress(Marker),
    /// A record this build can certify.
    Published(Box<Record>),
    /// A record file that exists but cannot certify anything: the 0.9.0 shape,
    /// another `format`/`version`, a missing required field, or unparseable
    /// bytes.
    Uncertifiable,
}

/// The record's directory, beside the manifest and the graph.
#[must_use]
pub fn state_dir(root: &Path) -> PathBuf {
    match git_common_dir(root) {
        Some(common) => common.join("scout"),
        None => scout_dir(root),
    }
}

/// The record file's path.
#[must_use]
pub fn record_path(root: &Path) -> PathBuf {
    state_dir(root).join("index-state.json")
}

/// The digest table's path.
#[must_use]
pub fn digest_path(root: &Path) -> PathBuf {
    state_dir(root).join("index-digests.json")
}

/// The `map` lock file's path.
#[must_use]
pub fn lock_path(root: &Path) -> PathBuf {
    state_dir(root).join("map.lock")
}

/// The canonical Git administrative directory naming the worktree that owns an
/// index at `root`, or `None` for a root with no Git directory.
///
/// Canonicalized so a symlinked root (macOS `/tmp` against `/private/tmp`)
/// cannot split one worktree's identity in two. No Git process is spawned.
#[must_use]
pub fn owner_of(root: &Path) -> Option<String> {
    let dir = git_dir_for(root)?;
    let canonical = fs::canonicalize(&dir).unwrap_or(dir);
    Some(canonical.to_string_lossy().into_owned())
}

// The digest table's `{format, version, generation, files}` shape. `files`
// holds every indexed path's SHA-256, written once per map and opened by a query
// only when it has at least one candidate to compare.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawDigests {
    format: String,
    version: u32,
    generation: String,
    files: BTreeMap<String, String>,
}

/// A map's whole content-digest table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestTable {
    /// The generation whose record this table belongs to.
    pub generation: String,
    /// Each indexed path's SHA-256, lowercase hex.
    pub files: BTreeMap<String, String>,
}

/// Reads the digest table, `None` when it is absent or in any way unusable.
#[must_use]
pub fn read_digests(root: &Path) -> Option<DigestTable> {
    read_digests_strict(root).ok()
}

/// Why a strict read of the digest table found nothing usable. A caller that
/// republishes what it read needs the three apart: a table that was never
/// written, one that is corrupt, and one of another format all send a reader
/// somewhere different.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DigestReadError {
    /// No digest table file at all.
    Missing,
    /// The file is there but does not parse.
    Unreadable,
    /// The file parses but is not this format and version.
    UnknownFormat,
    /// The file's identity changed between the reads bracketing the parse.
    Changed,
}

/// Reads and validates the digest table, naming what stopped it.
pub(crate) fn read_digests_strict(root: &Path) -> Result<DigestTable, DigestReadError> {
    let path = digest_path(root);
    let before = ArtifactIdentity::of(&path);
    let text = fs::read_to_string(&path).map_err(|_| DigestReadError::Missing)?;
    let raw: RawDigests = serde_json::from_str(&text).map_err(|_| DigestReadError::Unreadable)?;
    if raw.format != DIGESTS_FORMAT || raw.version != 1 {
        return Err(DigestReadError::UnknownFormat);
    }
    if before.is_none() || before != ArtifactIdentity::of(&path) {
        return Err(DigestReadError::Changed);
    }
    Ok(DigestTable {
        generation: raw.generation,
        files: raw.files,
    })
}

/// Writes the digest table, by atomic rename like every other artifact.
pub fn write_digests(
    root: &Path,
    generation: &str,
    files: &BTreeMap<String, String>,
) -> io::Result<()> {
    let raw = RawDigests {
        format: DIGESTS_FORMAT.to_string(),
        version: 1,
        generation: generation.to_string(),
        files: files.clone(),
    };
    write_atomic(&digest_path(root), &raw)
}

// The tolerant view of a record file: every field optional, so a legacy or
// half-written file deserializes and is then judged, rather than failing to
// parse for a reason the caller cannot tell apart from a corrupt file.
#[derive(Debug, Deserialize)]
struct RawRecord {
    format: Option<String>,
    version: Option<u32>,
    state: Option<String>,
    owner: Option<String>,
    generation: Option<String>,
    indexed_head: Option<String>,
    scope: Option<Vec<String>>,
    indexed_at: Option<String>,
    file_count: Option<u64>,
    dirty_at_map: Option<Vec<String>>,
    config_inputs: Option<BTreeMap<String, String>>,
    artifacts: Option<Artifacts>,
    started_at: Option<String>,
}

/// Reads what the record file at `root` currently holds.
#[must_use]
pub fn read_record(root: &Path) -> RecordState {
    let Ok(text) = fs::read_to_string(record_path(root)) else {
        return RecordState::Missing;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return RecordState::Uncertifiable;
    };
    // `indexed_head` is legitimately null in a repository with no commit, so
    // presence has to be read off the raw object: `Option` cannot tell an
    // absent key from a null one.
    if value.get("state").and_then(|s| s.as_str()) == Some("published")
        && value.get("indexed_head").is_none()
    {
        return RecordState::Uncertifiable;
    }
    let Ok(raw) = serde_json::from_value::<RawRecord>(value) else {
        return RecordState::Uncertifiable;
    };
    if raw.format.as_deref() != Some(RECORD_FORMAT) || raw.version != Some(RECORD_VERSION) {
        return RecordState::Uncertifiable;
    }
    match raw.state.as_deref() {
        Some("in-progress") => match (raw.owner, raw.started_at) {
            (Some(owner), Some(started_at)) => RecordState::InProgress(Marker {
                format: RECORD_FORMAT.to_string(),
                version: RECORD_VERSION,
                state: "in-progress".to_string(),
                owner,
                started_at,
            }),
            _ => RecordState::Uncertifiable,
        },
        Some("published") => {
            let indexed_head = raw.indexed_head;
            let (Some(owner), Some(generation), Some(scope), Some(indexed_at)) =
                (raw.owner, raw.generation, raw.scope, raw.indexed_at)
            else {
                return RecordState::Uncertifiable;
            };
            let (Some(file_count), Some(dirty_at_map)) = (raw.file_count, raw.dirty_at_map) else {
                return RecordState::Uncertifiable;
            };
            let (Some(config_inputs), Some(artifacts)) = (raw.config_inputs, raw.artifacts) else {
                return RecordState::Uncertifiable;
            };
            RecordState::Published(Box::new(Record {
                format: RECORD_FORMAT.to_string(),
                version: RECORD_VERSION,
                state: "published".to_string(),
                owner,
                generation,
                indexed_head,
                scope,
                indexed_at,
                file_count,
                dirty_at_map: Some(dirty_at_map),
                config_inputs,
                artifacts,
            }))
        }
        _ => RecordState::Uncertifiable,
    }
}

/// Writes the in-progress marker, by atomic rename.
pub fn write_marker(root: &Path, owner: &str) -> io::Result<()> {
    let marker = Marker {
        format: RECORD_FORMAT.to_string(),
        version: RECORD_VERSION,
        state: "in-progress".to_string(),
        owner: owner.to_string(),
        started_at: crate::hookio::iso8601_now(),
    };
    write_atomic(&record_path(root), &marker)
}

/// Publishes a record, by atomic rename. Always the last artifact `map` writes.
pub fn write_record(root: &Path, record: &Record) -> io::Result<()> {
    write_atomic(&record_path(root), record)
}

/// Removes the record and the digest table, for a root that must not carry a
/// certification (a root with no Git administrative directory, or a map that
/// finds its record no longer its own).
pub fn remove_state(root: &Path) -> io::Result<()> {
    for path in [record_path(root), digest_path(root)] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

static GENERATION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fresh generation token: 32 lowercase hex characters, unique per
/// publication because the publication time, the pid and a process counter all
/// feed the digest.
#[must_use]
pub fn new_generation(owner: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let counter = GENERATION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let seed = format!("{nanos}:{}:{counter}:{owner}", std::process::id());
    let digest = Sha256::digest(seed.as_bytes());
    let mut out = String::with_capacity(32);
    for byte in digest.iter().take(16) {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The lowercase-hex SHA-256 of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest.iter() {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

// Atomic write: a uniquely named temp file in the target's own directory, then
// a rename, so a concurrent reader sees either the old file or the new one and
// never a partial write.
fn write_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    let mut bytes = bytes;
    bytes.push(b'\n');
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("state");
    let counter = GENERATION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.tmp.{}.{counter}", std::process::id()));
    if let Err(e) = fs::write(&tmp, &bytes) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn scratch(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "scout-freshness-record-{label}-{}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample() -> Record {
        Record {
            format: RECORD_FORMAT.to_string(),
            version: RECORD_VERSION,
            state: "published".to_string(),
            owner: "/tmp/repo/.git".to_string(),
            generation: new_generation("/tmp/repo/.git"),
            indexed_head: Some("abc".to_string()),
            scope: vec![".".to_string()],
            indexed_at: "2026-01-01T00:00:00Z".to_string(),
            file_count: 2,
            dirty_at_map: Some(vec!["src/a.cs".to_string()]),
            config_inputs: BTreeMap::new(),
            artifacts: Artifacts {
                graph: None,
                manifest: None,
            },
        }
    }

    #[test]
    fn a_missing_record_is_missing() {
        assert_eq!(read_record(&scratch("missing")), RecordState::Missing);
    }

    #[test]
    fn a_record_round_trips() {
        let dir = scratch("roundtrip");
        let r = sample();
        write_record(&dir, &r).unwrap();
        assert_eq!(read_record(&dir), RecordState::Published(Box::new(r)));
    }

    #[test]
    fn the_legacy_shape_is_uncertifiable() {
        let dir = scratch("legacy");
        fs::create_dir_all(state_dir(&dir)).unwrap();
        fs::write(
            record_path(&dir),
            r#"{"head": "abc", "dirty": false, "indexed_at": "x", "file_count": 1, "dirty_indexed_files": []}"#,
        )
        .unwrap();
        assert_eq!(read_record(&dir), RecordState::Uncertifiable);
    }

    #[test]
    fn a_missing_required_field_is_uncertifiable() {
        let dir = scratch("missing-field");
        fs::create_dir_all(state_dir(&dir)).unwrap();
        let mut value = serde_json::to_value(sample()).unwrap();
        value.as_object_mut().unwrap().remove("owner");
        fs::write(record_path(&dir), value.to_string()).unwrap();
        assert_eq!(read_record(&dir), RecordState::Uncertifiable);
    }

    #[test]
    fn corrupt_bytes_are_uncertifiable_not_missing() {
        let dir = scratch("corrupt");
        fs::create_dir_all(state_dir(&dir)).unwrap();
        fs::write(record_path(&dir), "{not json").unwrap();
        assert_eq!(read_record(&dir), RecordState::Uncertifiable);
    }

    #[test]
    fn a_marker_reads_as_in_progress() {
        let dir = scratch("marker");
        write_marker(&dir, "/tmp/repo/.git").unwrap();
        match read_record(&dir) {
            RecordState::InProgress(m) => assert_eq!(m.owner, "/tmp/repo/.git"),
            other => panic!("expected a marker, got {other:?}"),
        }
    }

    #[test]
    fn generations_are_unique_and_are_32_hex_characters() {
        let a = new_generation("owner");
        let b = new_generation("owner");
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn rewriting_the_record_changes_nothing_about_how_it_is_written_but_a_new_file_gets_a_new_inode(
    ) {
        let dir = scratch("identity");
        let path = dir.join("artifact.json");
        fs::write(&path, b"one").unwrap();
        let first = ArtifactIdentity::of(&path).unwrap();
        fs::write(&path, b"two").unwrap();
        let rewritten = ArtifactIdentity::of(&path).unwrap();
        assert_ne!(first, rewritten, "a rewrite must be visible");
    }

    #[test]
    fn the_digest_table_round_trips_and_rejects_another_format() {
        let dir = scratch("digests");
        let mut files = BTreeMap::new();
        files.insert("src/a.cs".to_string(), sha256_hex(b"a"));
        write_digests(&dir, "gen", &files).unwrap();
        let read = read_digests(&dir).unwrap();
        assert_eq!(read.generation, "gen");
        assert_eq!(read.files, files);

        fs::write(digest_path(&dir), r#"{"format":"other","version":1}"#).unwrap();
        assert!(read_digests(&dir).is_none());
    }

    #[test]
    fn removing_state_clears_the_record_and_digest_table() {
        let dir = scratch("remove");
        write_record(&dir, &sample()).unwrap();
        write_digests(&dir, "gen", &BTreeMap::new()).unwrap();
        remove_state(&dir).unwrap();
        assert_eq!(read_record(&dir), RecordState::Missing);
        assert!(read_digests(&dir).is_none());
    }

    #[test]
    fn sha256_hex_is_lowercase_and_64_characters() {
        let hex = sha256_hex(b"hello");
        assert_eq!(hex.len(), 64);
        assert_eq!(
            hex,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
