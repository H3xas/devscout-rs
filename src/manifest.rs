// Nav manifest read/write, `find` token scoring, `scope_for`, and the crate's
// one `git` shell-out (`git_head`).
//
// The manifest is completely schema-agnostic: read and write are JSON parse /
// pretty-print with a path in front, nothing more. A manifest can carry unknown
// top-level fields, and a read must round-trip them unchanged -- a fixed-schema
// struct would silently drop them. So the manifest body here is `Value`, a small
// ordered-JSON tree (below) that is Serialize/Deserialize via serde_json's
// Deserializer / Serializer traits but keeps object-key insertion order in a
// `Vec` instead of the default `Value`'s (unordered, `BTreeMap`-backed without
// the `preserve_order` cargo feature) map type. Order is not cosmetic:
// `find_in_manifest`'s pools sort by tokens matched, then inbound-edge count,
// so full ties resolve by manifest on-disk key order -- see
// `find_in_manifest_detailed`'s doc comment for the full chain.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};

use crate::repo::{entry_for, git_common_dir, scout_dir, RegistryEntry, RegistryError};

// ---------------------------------------------------------------------------
// Ordered JSON value -- the manifest's actual "schema" (i.e. none). Numbers
// delegate to `serde_json::Number` so integer-vs-float formatting is preserved
// (mtime values are always integers in practice; the general case is handled
// anyway since nothing here assumes otherwise). Objects are
// `Vec<(String, Value)>`, not a map, so read-then-write and read-then-score
// both preserve on-disk key order without relying on any HashMap/BTreeMap
// iteration order.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn object(entries: Vec<(&str, Value)>) -> Value {
        Value::Object(
            entries
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    pub fn string(s: impl Into<String>) -> Value {
        Value::String(s.into())
    }

    pub fn array(items: Vec<Value>) -> Value {
        Value::Array(items)
    }

    pub fn number(n: impl Into<serde_json::Number>) -> Value {
        Value::Number(n.into())
    }

    /// Value for `key`, or `None` -- a missing key, or `self` not being an
    /// object at all. Reading a field off a non-object is `None`, not an error.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object()?
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    pub fn as_object(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Object(entries) => Some(entries.as_slice()),
            _ => None,
        }
    }

    /// `Some` only for the `String` variant -- deliberately does NOT coerce
    /// numbers or bools to text. `find_in_manifest`'s `hay` build and its
    /// `source` default both rely on this narrower behavior; a non-string
    /// `purpose`/`source` (not produced by any real manifest writer) is simply
    /// treated as absent.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ValueVisitor;

        impl<'de> Visitor<'de> for ValueVisitor {
            type Value = Value;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON value")
            }

            fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
                Ok(Value::Bool(v))
            }
            fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
                Ok(Value::Number(v.into()))
            }
            fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
                Ok(Value::Number(v.into()))
            }
            fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
                Ok(Value::Number(
                    serde_json::Number::from_f64(v).unwrap_or_else(|| 0.into()),
                ))
            }
            fn visit_str<E>(self, v: &str) -> Result<Value, E> {
                Ok(Value::String(v.to_owned()))
            }
            fn visit_string<E>(self, v: String) -> Result<Value, E> {
                Ok(Value::String(v))
            }
            fn visit_unit<E>(self) -> Result<Value, E> {
                Ok(Value::Null)
            }
            fn visit_none<E>(self) -> Result<Value, E> {
                Ok(Value::Null)
            }
            fn visit_some<D2>(self, deserializer: D2) -> Result<Value, D2::Error>
            where
                D2: Deserializer<'de>,
            {
                Value::deserialize(deserializer)
            }
            fn visit_seq<A>(self, mut seq: A) -> Result<Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(Value::Array(items))
            }
            // Document order, not sorted: `MapAccess::next_entry` yields
            // entries in the order serde_json's Deserializer encounters
            // them in the source text, independent of what `Value`'s OWN
            // (BTreeMap-backed) object type would have done -- we never
            // construct that type, we build this `Vec` instead.
            fn visit_map<A>(self, mut map: A) -> Result<Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut entries = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, Value>()? {
                    entries.push((k, v));
                }
                Ok(Value::Object(entries))
            }
        }

        deserializer.deserialize_any(ValueVisitor)
    }
}

impl Serialize for Value {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Value::Null => serializer.serialize_unit(),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::Number(n) => n.serialize(serializer),
            Value::String(s) => serializer.serialize_str(s),
            Value::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            // Iterates the `Vec` in stored order -- this, not any property
            // of `serde_json`'s formatter, is what makes output key order
            // match input key order end to end.
            Value::Object(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (k, v) in entries {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Manifest path resolution.
// ---------------------------------------------------------------------------

// The shared, git-common-dir-keyed manifest location when `root` is inside a git
// repo (worktree-shared: every linked worktree of one repo resolves to the same
// path), else `legacy_manifest_path(root)`. Kept private deliberately.
fn manifest_path(root: &Path) -> PathBuf {
    match git_common_dir(root) {
        Some(common) => common.join("scout").join("manifest.json"),
        None => legacy_manifest_path(root),
    }
}

/// `<root>/.scout/manifest.json`, still consulted on read so pre-existing
/// enriched manifests are not orphaned by the shared-path move.
pub fn legacy_manifest_path(root: &Path) -> PathBuf {
    scout_dir(root).join("manifest.json")
}

// ---------------------------------------------------------------------------
// Errors -- a corrupt manifest file surfaces as `Result::Err` from
// `read_manifest`, and a corrupt registry surfaces from `scope_for` (propagated
// from `entry_for`) -- the same error-return discipline repo.rs uses for
// `RegistryError`.
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ManifestError {
    /// The manifest file could not be read (permission, or the file vanishing
    /// in a race after the existence check -- the same "safer, not narrower"
    /// TOCTOU behavior documented on `repo::git_dir_for`).
    Io { path: PathBuf, detail: String },
    /// The manifest file is present but not valid JSON.
    InvalidJson { path: PathBuf, detail: String },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::Io { path, detail } => {
                write!(f, "failed to read manifest at {}: {detail}", path.display())
            }
            ManifestError::InvalidJson { path, detail } => {
                write!(
                    f,
                    "manifest at {} is not valid JSON: {detail}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ManifestError {}

fn read_json_value(path: &Path) -> Result<Value, ManifestError> {
    let text = fs::read_to_string(path).map_err(|e| ManifestError::Io {
        path: path.to_path_buf(),
        detail: e.to_string(),
    })?;
    serde_json::from_str::<Value>(&text).map_err(|e| ManifestError::InvalidJson {
        path: path.to_path_buf(),
        detail: e.to_string(),
    })
}

/// Parses the shared-path manifest if present; else falls back to the legacy
/// `<root>/.scout/manifest.json` if THAT is present; else `Ok(None)`. A read has
/// no side effects (no migrate/copy/delete). Returns `Err` on a corrupt file.
pub fn read_manifest(root: &Path) -> Result<Option<Value>, ManifestError> {
    let path = manifest_path(root);
    if path.exists() {
        return read_json_value(&path).map(Some);
    }
    let legacy = legacy_manifest_path(root);
    if legacy.exists() {
        return read_json_value(&legacy).map(Some);
    }
    Ok(None)
}

/// Creates the manifest's parent directory, then writes `value` as 2-space
/// pretty-printed JSON (`serde_json::to_string_pretty`). The write is atomic:
/// tmp-file + rename, so a reader racing a writer never observes a half-written
/// `manifest.json`.
pub fn write_manifest(root: &Path, value: &Value) -> io::Result<()> {
    let path = manifest_path(root);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_name = format!(
        "{}.tmp.{}.{suffix}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("manifest.json"),
        std::process::id(),
    );
    let tmp_path = path.with_file_name(tmp_name);

    if let Err(e) = fs::write(&tmp_path, json.as_bytes()) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }
    if let Err(e) = fs::rename(&tmp_path, &path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `find` scoring.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct FindHit {
    pub path: String,
    /// The entry's `purpose`, passed through as-is and NOT defaulted the way
    /// `source` is. `None` when the key is absent; an explicit JSON `null`
    /// purpose also collapses to `None` (not produced by any real manifest
    /// writer).
    pub purpose: Option<String>,
    /// The entry's `source`, defaulting to `"heuristic"` -- always resolved,
    /// never absent in the output (an absent key and an explicit `null` both
    /// default to `"heuristic"`).
    pub source: String,
}

#[derive(Debug)]
pub enum FindError {
    Manifest(ManifestError),
    /// The manifest has no object-valued `entries` field -- the realistic case
    /// being a manifest missing its `entries` key entirely. A non-object
    /// `entries` (e.g. a string or array), which no real manifest writer
    /// produces, collapses to this same error rather than being silently scored
    /// as zero entries.
    EntriesNotObject,
}

impl From<ManifestError> for FindError {
    fn from(e: ManifestError) -> Self {
        FindError::Manifest(e)
    }
}

impl fmt::Display for FindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FindError::Manifest(e) => write!(f, "{e}"),
            FindError::EntriesNotObject => write!(f, "manifest has no \"entries\" object"),
        }
    }
}

impl std::error::Error for FindError {}

struct Scored {
    path: String,
    purpose: Option<String>,
    source: String,
    hits: usize,
    /// The entry's file's precise inbound-edge count, from the ranking map the
    /// caller passes (see `query::file_inbound_counts` for what feeds it). A
    /// caller without a graph passes an empty map, so every entry reads 0 and
    /// the sort below degrades to today's order.
    inbound: usize,
}

/// AND across whitespace-split, lowercased tokens over `path + ' ' + purpose`;
/// when no entry matches every token, fall back to entries matching ANY token,
/// ranked by hit count descending.
///
/// Reads every entry's inbound-edge count at 0 -- see
/// `find_in_manifest_detailed` for the ranked variant.
pub fn find_in_manifest(root: &Path, query: &str) -> Result<Vec<FindHit>, FindError> {
    Ok(find_in_manifest_detailed(root, query, &HashMap::new())?.hits)
}

/// `find_in_manifest_detailed`'s return shape: the hits plus which pool
/// answered.
pub struct FindResult {
    pub hits: Vec<FindHit>,
    pub fallback: bool,
}

/// Same search as `find_in_manifest`, plus WHICH pool answered: `fallback: true`
/// means no entry matched every token and the OR pool is speaking. The CLI caps
/// the two pools differently (`cmd_find`); the flat `find_in_manifest` stays for
/// callers that only want hits.
///
/// Ranking, applied to WHICHEVER pool answers and BEFORE either cap: by tokens
/// matched descending, then by the entry's file's inbound-edge count
/// (`inbound`) descending. No third sort key -- `sort_by` is documented stable,
/// so a full tie keeps the manifest's on-disk key order, which is already
/// deterministic (see the module header). In the AND pool the primary key is
/// constant by construction (every entry there matched every token), so the
/// inbound tie-break does all the ranking work; in the OR pool it breaks the
/// hit-count ties the old sort left in manifest order. An empty `inbound` map
/// (no graph available) reads 0 everywhere and returns today's order byte for
/// byte.
///
/// Tie stability of the input order itself: `scored` is built by iterating
/// `entries` -- this module's ordered `Value::Object` `Vec`, populated by
/// `Value`'s `Deserialize` impl above in exactly the JSON text's key order (see
/// its `visit_map` doc comment) -- via a single `.map()` that does not reorder.
/// So JSON text order flows through to `scored` order and, because the sort
/// preserves ties, to pool order.
///
/// Returns `Ok(FindResult{..})` with no hits for no manifest or no tokens;
/// `Err` on a corrupt manifest file or a missing `entries`.
pub fn find_in_manifest_detailed(
    root: &Path,
    query: &str,
    inbound: &HashMap<String, usize>,
) -> Result<FindResult, FindError> {
    let manifest = match read_manifest(root)? {
        Some(m) => m,
        None => {
            return Ok(FindResult {
                hits: Vec::new(),
                fallback: false,
            })
        }
    };

    let tokens: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect();
    if tokens.is_empty() {
        return Ok(FindResult {
            hits: Vec::new(),
            fallback: false,
        });
    }

    let entries = manifest
        .get("entries")
        .and_then(Value::as_object)
        .ok_or(FindError::EntriesNotObject)?;

    let scored: Vec<Scored> = entries
        .iter()
        .map(|(path, e)| {
            let purpose = e.get("purpose").and_then(Value::as_str).map(String::from);
            let source = e
                .get("source")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_else(|| "heuristic".to_string());
            let hay = format!("{path} {}", purpose.as_deref().unwrap_or("")).to_lowercase();
            let hits = tokens.iter().filter(|t| hay.contains(t.as_str())).count();
            Scored {
                path: path.clone(),
                purpose,
                source,
                hits,
                inbound: inbound.get(path).copied().unwrap_or(0),
            }
        })
        .collect();

    let full: Vec<&Scored> = scored.iter().filter(|s| s.hits == tokens.len()).collect();
    let fallback = full.is_empty();
    let mut pool: Vec<&Scored> = if !full.is_empty() {
        full
    } else {
        scored.iter().filter(|s| s.hits > 0).collect()
    };
    // Both pools rank identically: tokens first, inbound second, stability
    // third. One comparator, applied once, keeps that rule in exactly one place.
    pool.sort_by(|a, b| b.hits.cmp(&a.hits).then_with(|| b.inbound.cmp(&a.inbound)));

    let hits = pool
        .into_iter()
        .map(|s| FindHit {
            path: s.path.clone(),
            purpose: s.purpose.clone(),
            source: s.source.clone(),
        })
        .collect();
    Ok(FindResult { hits, fallback })
}

// ---------------------------------------------------------------------------
// `git_head` -- the crate's ONE `git` shell-out that lives in this module (see
// repo.rs's header comment: its git helpers never shell out).
// ---------------------------------------------------------------------------

/// The repo's `HEAD` commit via `git -C <root> rev-parse HEAD`, trimmed. `None`
/// on any failure (spawn failure, non-zero exit, e.g. no commits yet or not a
/// repo).
pub fn git_head(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    Some(stdout.trim().to_string())
}

// ---------------------------------------------------------------------------
// `scope_for`.
// ---------------------------------------------------------------------------

/// The directory scope for `root`: explicit dirs win if non-empty; else the
/// registry entry's `scope` if non-empty; else `["."]`. A corrupt registry
/// surfaces as `Err`, propagated straight from `repo::entry_for`.
pub fn scope_for(
    root: &Path,
    explicit_dirs: Option<&[String]>,
) -> Result<Vec<String>, RegistryError> {
    if let Some(dirs) = explicit_dirs {
        if !dirs.is_empty() {
            return Ok(dirs.to_vec());
        }
    }
    let entry: Option<RegistryEntry> = entry_for(root)?;
    if let Some(e) = entry {
        if !e.scope.is_empty() {
            return Ok(e.scope);
        }
    }
    Ok(vec![".".to_string()])
}

// ---------------------------------------------------------------------------
// Index freshness. `index-state.json` sits beside `manifest.json` (same
// git-common-dir-vs-legacy-`.scout` split). It is never compared byte-identical
// against anything the way manifest.json / graph.json are -- `indexed_at` is
// wall-clock, so no two runs could ever produce the same bytes regardless of
// intent.
// ---------------------------------------------------------------------------

fn index_state_dir(root: &Path) -> PathBuf {
    match git_common_dir(root) {
        Some(common) => common.join("scout"),
        None => scout_dir(root),
    }
}

pub fn index_state_path(root: &Path) -> PathBuf {
    index_state_dir(root).join("index-state.json")
}

/// Writes the index-state sidecar. Written on every successful `devscout map`,
/// the same cadence `manifest.json`'s own `built_at_head` refreshes on.
/// `dirty_indexed_files` -- the indexed files git ALREADY considered changed at
/// index time (a repo mapped before its source was ever `git add`ed is the
/// common case, not an edge case) -- is the baseline `freshness_warning` diffs
/// against, so those same files staying exactly as dirty as they were is never
/// mistaken for having changed SINCE the map. Sorted so two writes of the same
/// set are byte-identical.
pub fn write_index_state(
    root: &Path,
    head: Option<String>,
    dirty: bool,
    dirty_indexed_files: &[String],
    file_count: i64,
) -> io::Result<()> {
    let path = index_state_path(root);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut sorted = dirty_indexed_files.to_vec();
    sorted.sort();
    let value = Value::object(vec![
        ("head", head.map(Value::string).unwrap_or(Value::Null)),
        ("dirty", Value::Bool(dirty)),
        ("indexed_at", Value::string(crate::hookio::iso8601_now())),
        ("file_count", Value::number(file_count)),
        (
            "dirty_indexed_files",
            Value::array(sorted.into_iter().map(Value::string).collect()),
        ),
    ]);
    let json = serde_json::to_string_pretty(&value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(&path, format!("{json}\n"))
}

/// Tolerant read of the index-state sidecar -- `None` on a missing OR corrupt
/// sidecar (an index built by a build that predates this sidecar), never a panic
/// and never an `Err` a caller has to handle.
pub fn read_index_state(root: &Path) -> Option<Value> {
    let text = fs::read_to_string(index_state_path(root)).ok()?;
    serde_json::from_str::<Value>(&text).ok()
}

// Cap on parsed `git status` lines.
const STATUS_LINE_CAP: usize = 5000;

// A capped, scoped `git status --porcelain` -- cheap on a large repo mapped with
// a narrow scope -- as the set of repo-relative paths it reports as changed
// (modified, staged, or untracked; a rename's OLD and NEW path both count).
// `--untracked-files=all` matters: without it, git collapses an
// entirely-untracked directory to one `?? dir/` line instead of one line per
// file inside it, and the path-set intersection below would never match anything
// for a freshly-mapped, not-yet-`git add`-ed tree. `None` on any failure (no git
// binary, not a repo), distinct from an empty set (git ran and found nothing) so
// callers never mistake "could not check" for "confirmed clean".
fn git_status_paths(root: &Path, scope: &[String]) -> Option<HashSet<String>> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(root)
        .args(["status", "--porcelain", "--untracked-files=all", "--"]);
    for s in scope {
        cmd.arg(s);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let mut paths = HashSet::new();
    for line in text
        .split('\n')
        .filter(|l| !l.is_empty())
        .take(STATUS_LINE_CAP)
    {
        if line.len() < 3 {
            continue;
        }
        for part in line[3..].split(" -> ") {
            paths.insert(part.to_string());
        }
    }
    Some(paths)
}

/// Index-time convenience over `git_status_paths` -- a plain bool, fail-open to
/// `false` (a non-git root, or git being unavailable, is not "dirty"; it is
/// unknown, and `false` is the safer default to persist for it).
pub fn is_working_tree_dirty(root: &Path, scope: &[String]) -> bool {
    git_status_paths(root, scope)
        .map(|p| !p.is_empty())
        .unwrap_or(false)
}

/// The indexed files currently dirty -- `git_status_paths` intersected with the
/// manifest's own entries -- sorted for a stable, comparable list. Called once
/// at index time (the baseline `write_index_state` stores) and once at query
/// time (what `freshness_warning` diffs the baseline against).
pub fn dirty_indexed_files_at(
    root: &Path,
    scope: &[String],
    indexed_files: &HashSet<String>,
) -> Vec<String> {
    let Some(changed) = git_status_paths(root, scope) else {
        return Vec::new();
    };
    let mut out: Vec<String> = changed
        .into_iter()
        .filter(|p| indexed_files.contains(p))
        .collect();
    out.sort();
    out
}

/// Query-time check for `find`/`refs`/`impact`: cheap enough to run on every
/// call (one `git rev-parse HEAD`, one scoped `git status --porcelain`, the same
/// two costs `devscout map` itself already pays). Returns the one-line stderr
/// warning, or `None` when there is nothing to say: no index-state.json (never
/// mapped since this feature shipped, or the root was not a git repo when last
/// mapped), git unavailable or this root not a repo, or the index is genuinely
/// fresh.
///
/// "Changed" is deliberately NOT "currently dirty and indexed" -- a repo mapped
/// before its source was ever `git add`ed would then show every file as changed
/// FOREVER, indistinguishable from a genuinely fresh index. It is
/// "dirty-and-indexed now but wasn't at index time" -- the set
/// `write_index_state` recorded as the baseline. The one gap this leaves: a file
/// already dirty at index time that is edited AGAIN afterward stays in both sets
/// and is not counted (a known, accepted limitation, not silently swallowed).
pub fn freshness_warning(root: &Path) -> Option<String> {
    let state = read_index_state(root)?;
    let stored_head = state.get("head")?.as_str()?.to_string();

    let current_head = git_head(root)?;

    let manifest = read_manifest(root).ok().flatten();
    let indexed_files: HashSet<String> = manifest
        .as_ref()
        .and_then(|m| m.get("entries"))
        .and_then(|v| v.as_object())
        .map(|entries| entries.iter().map(|(k, _)| k.clone()).collect())
        .unwrap_or_default();
    let scope: Vec<String> = manifest
        .as_ref()
        .and_then(|m| m.get("scoped_dirs"))
        .and_then(|v| match v {
            Value::Array(items) => Some(
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .filter(|v: &Vec<String>| !v.is_empty())
        .unwrap_or_else(|| vec![".".to_string()]);

    let current_dirty = dirty_indexed_files_at(root, &scope, &indexed_files);
    let baseline_dirty: HashSet<String> = state
        .get("dirty_indexed_files")
        .and_then(|v| match v {
            Value::Array(items) => Some(
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    let changed_count = current_dirty
        .iter()
        .filter(|p| !baseline_dirty.contains(*p))
        .count();

    if current_head == stored_head && changed_count == 0 {
        return None;
    }

    let repo_id = root.file_name().and_then(|n| n.to_str()).unwrap_or("repo");
    let indexed_short = &stored_head[..stored_head.len().min(7)];
    let head_short = &current_head[..current_head.len().min(7)];
    Some(format!(
        "devscout: index for {repo_id} is stale (indexed at {indexed_short}, HEAD {head_short}; {changed_count} changed files) — rebuild with devscout map"
    ))
}

// ---------------------------------------------------------------------------
// Unit tests -- pure-function / single-process coverage (Value ser/de, path
// resolution on plain non-git dirs, write+read round trip, find_in_manifest
// scoring logic, scope_for). Ordering, byte-identical write, and git_head
// against a real repo are covered in the integration suite.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    // Serializes this module's own SCOUT_REGISTRY-mutating tests against each
    // other (process-global env var, tests run as threads within one binary).
    // repo.rs's own #[cfg(test)] unit tests never touch this var, so no
    // cross-binary race exists -- this lock only needs to cover this module's
    // own tests.
    static REGISTRY_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = env::temp_dir().join(format!(
            "scout-manifest-rs-{prefix}-{}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    // -- Value ser/de --------------------------------------------------

    #[test]
    fn value_round_trips_object_array_scalars_preserving_key_order() {
        let text = r#"{"z": 1, "a": [1, -2, "s", true, false, null], "m": {"k1": 1, "k0": 2}}"#;
        let v: Value = serde_json::from_str(text).unwrap();
        match &v {
            Value::Object(fields) => {
                assert_eq!(fields[0].0, "z");
                assert_eq!(fields[1].0, "a");
                assert_eq!(fields[2].0, "m");
            }
            other => panic!("expected object, got {other:?}"),
        }
        // Nested object key order preserved too (not re-sorted "k0","k1").
        let m = v.get("m").unwrap().as_object().unwrap();
        assert_eq!(m[0].0, "k1");
        assert_eq!(m[1].0, "k0");
    }

    #[test]
    fn value_pretty_print_matches_json_stringify_indent_two_shape() {
        let v = Value::object(vec![
            ("a", Value::number(1)),
            ("b", Value::array(vec![Value::string("x")])),
        ]);
        let out = serde_json::to_string_pretty(&v).unwrap();
        assert_eq!(out, "{\n  \"a\": 1,\n  \"b\": [\n    \"x\"\n  ]\n}");
    }

    #[test]
    fn value_empty_object_and_array_have_no_interior_whitespace() {
        assert_eq!(
            serde_json::to_string_pretty(&Value::object(vec![])).unwrap(),
            "{}"
        );
        assert_eq!(
            serde_json::to_string_pretty(&Value::array(vec![])).unwrap(),
            "[]"
        );
    }

    #[test]
    fn value_unicode_round_trips_unescaped() {
        let v: Value = serde_json::from_str(r#""café 😀""#).unwrap();
        assert_eq!(v, Value::string("café 😀"));
        // Ordinary non-ASCII text is not escaped.
        assert_eq!(serde_json::to_string_pretty(&v).unwrap(), "\"café 😀\"");
    }

    // -- manifest_path / legacy_manifest_path ---------------------------

    #[test]
    fn a_non_git_root_uses_the_legacy_scout_path() {
        let root = unique_temp_dir("plain");
        assert_eq!(manifest_path(&root), legacy_manifest_path(&root));
        assert_eq!(
            legacy_manifest_path(&root),
            root.join(".scout").join("manifest.json")
        );
    }

    // -- read_manifest / write_manifest ---------------------------------

    #[test]
    fn read_manifest_is_none_when_nothing_exists() {
        let root = unique_temp_dir("missing");
        assert!(read_manifest(&root).unwrap().is_none());
    }

    #[test]
    fn read_manifest_errs_on_corrupt_json() {
        let root = unique_temp_dir("corrupt");
        fs::create_dir_all(root.join(".scout")).unwrap();
        fs::write(root.join(".scout").join("manifest.json"), "{not valid json").unwrap();
        assert!(matches!(
            read_manifest(&root),
            Err(ManifestError::InvalidJson { .. })
        ));
    }

    #[test]
    fn write_then_read_round_trips_on_a_plain_root() {
        let root = unique_temp_dir("roundtrip");
        let obj = Value::object(vec![
            ("built_at_head", Value::string("abc")),
            ("scoped_dirs", Value::array(vec![Value::string("src")])),
            (
                "entries",
                Value::object(vec![(
                    "src/a.ts",
                    Value::object(vec![
                        ("purpose", Value::string("does A")),
                        ("mtime", Value::number(1)),
                    ]),
                )]),
            ),
        ]);
        write_manifest(&root, &obj).unwrap();
        assert_eq!(read_manifest(&root).unwrap(), Some(obj));
    }

    #[test]
    fn write_manifest_leaves_no_tmp_file_behind() {
        let root = unique_temp_dir("notmp");
        write_manifest(&root, &Value::object(vec![])).unwrap();
        let dir = legacy_manifest_path(&root).parent().unwrap().to_path_buf();
        let stray: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(stray.is_empty(), "stray tmp files: {stray:?}");
    }

    // -- find_in_manifest -------------------------------------------------

    fn sample_manifest() -> Value {
        Value::object(vec![
            ("built_at_head", Value::string("h")),
            ("scoped_dirs", Value::array(vec![])),
            (
                "entries",
                Value::object(vec![
                    (
                        "src/GroupRepository.cs",
                        Value::object(vec![
                            ("purpose", Value::string("Mongo CRUD for groups")),
                            ("mtime", Value::number(1)),
                        ]),
                    ),
                    (
                        "src/Other.cs",
                        Value::object(vec![
                            ("purpose", Value::string("unrelated")),
                            ("mtime", Value::number(1)),
                        ]),
                    ),
                ]),
            ),
        ])
    }

    #[test]
    fn find_matches_path_or_purpose_case_insensitively() {
        let root = unique_temp_dir("find-case");
        write_manifest(&root, &sample_manifest()).unwrap();
        let hits = find_in_manifest(&root, "group").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "src/GroupRepository.cs");
        assert_eq!(find_in_manifest(&root, "MONGO").unwrap().len(), 1);
    }

    #[test]
    fn find_returns_empty_for_zero_hits_and_missing_manifest() {
        let root = unique_temp_dir("find-zero");
        write_manifest(&root, &sample_manifest()).unwrap();
        assert!(find_in_manifest(&root, "nonexistentzzz")
            .unwrap()
            .is_empty());
        let missing_root = unique_temp_dir("find-missing");
        assert!(find_in_manifest(&missing_root, "anything")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn find_defaults_missing_source_to_heuristic() {
        let root = unique_temp_dir("find-source");
        let m = Value::object(vec![(
            "entries",
            Value::object(vec![(
                "a.ts",
                Value::object(vec![("purpose", Value::string("does A"))]),
            )]),
        )]);
        write_manifest(&root, &m).unwrap();
        let hits = find_in_manifest(&root, "does").unwrap();
        assert_eq!(hits[0].source, "heuristic");
    }

    #[test]
    fn find_errs_when_entries_is_missing() {
        let root = unique_temp_dir("find-no-entries");
        write_manifest(&root, &Value::object(vec![("built_at_head", Value::Null)])).unwrap();
        assert!(matches!(
            find_in_manifest(&root, "x"),
            Err(FindError::EntriesNotObject)
        ));
    }

    #[test]
    fn find_or_pool_sorts_by_hits_descending_stably_on_ties() {
        // Three entries with 0, 1, and 2 of the two tokens hitting; two
        // entries tie at 1 hit each -- their relative order must be
        // preserved from manifest key order (insertion order).
        let root = unique_temp_dir("find-tie");
        let m = Value::object(vec![(
            "entries",
            Value::object(vec![
                (
                    "first.ts",
                    Value::object(vec![("purpose", Value::string("alpha only"))]),
                ),
                (
                    "second.ts",
                    Value::object(vec![("purpose", Value::string("alpha beta"))]),
                ),
                (
                    "third.ts",
                    Value::object(vec![("purpose", Value::string("beta only"))]),
                ),
                (
                    "fourth.ts",
                    Value::object(vec![("purpose", Value::string("neither"))]),
                ),
            ]),
        )]);
        write_manifest(&root, &m).unwrap();
        let hits = find_in_manifest(&root, "alpha beta").unwrap();
        // full match (both tokens) wins outright: only "second.ts" hits both.
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "second.ts");

        // Force the OR-pool path: a token that ONLY "second.ts" and
        // "third.ts" partially match (one hit each), no entry hits both.
        let hits2 = find_in_manifest(&root, "beta zzz").unwrap();
        // "second.ts" (beta) and "third.ts" (beta) tie at 1 hit; manifest
        // order is second.ts then third.ts -- stable sort must keep it.
        let paths: Vec<&str> = hits2.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(paths, vec!["second.ts", "third.ts"]);
    }

    fn find_ranked(root: &Path, query: &str, inbound: &[(&str, usize)]) -> Vec<String> {
        let map: HashMap<String, usize> =
            inbound.iter().map(|(p, n)| (p.to_string(), *n)).collect();
        find_in_manifest_detailed(root, query, &map)
            .unwrap()
            .hits
            .into_iter()
            .map(|h| h.path)
            .collect()
    }

    #[test]
    fn find_and_pool_ranks_by_inbound_count_on_the_constant_token_key() {
        // Both entries match BOTH tokens of the query -- the AND pool's primary
        // key is constant by construction -- so the inbound tie-break does all
        // the work: the widely referenced file outranks the island despite its
        // later manifest position.
        let root = unique_temp_dir("find-and-inbound");
        let m = Value::object(vec![(
            "entries",
            Value::object(vec![
                (
                    "src/Island.cs",
                    Value::object(vec![("purpose", Value::string("widget ledger store"))]),
                ),
                (
                    "src/Hub.cs",
                    Value::object(vec![("purpose", Value::string("widget ledger hub"))]),
                ),
            ]),
        )]);
        write_manifest(&root, &m).unwrap();
        let paths = find_ranked(&root, "widget ledger", &[("src/Hub.cs", 7)]);
        assert_eq!(paths, vec!["src/Hub.cs", "src/Island.cs"]);

        // The ranking reads the map, not the manifest: flipping which file
        // carries the references flips the order.
        let flipped = find_ranked(&root, "widget ledger", &[("src/Island.cs", 3)]);
        assert_eq!(flipped, vec!["src/Island.cs", "src/Hub.cs"]);
    }

    #[test]
    fn find_or_pool_ranks_tokens_matched_before_inbound_count() {
        // OR pool: a three-token query no entry matches completely.
        // "rich.cs" matched TWO tokens but carries zero references;
        // "hub.cs" matched one token but is referenced 99 times. Tokens are
        // the primary key, so the extra text match wins and popularity cannot
        // buy the weaker text match the top row.
        let root = unique_temp_dir("find-or-tokens-first");
        let m = Value::object(vec![(
            "entries",
            Value::object(vec![
                (
                    "src/hub.cs",
                    Value::object(vec![("purpose", Value::string("alpha only"))]),
                ),
                (
                    "src/rich.cs",
                    Value::object(vec![("purpose", Value::string("alpha beta"))]),
                ),
            ]),
        )]);
        write_manifest(&root, &m).unwrap();
        let r = find_in_manifest_detailed(
            &root,
            "alpha beta zeta",
            &[("src/hub.cs".to_string(), 99)].into_iter().collect(),
        )
        .unwrap();
        assert!(
            r.fallback,
            "no entry matches every token: the OR pool answers"
        );
        let paths: Vec<&str> = r.hits.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(paths, vec!["src/rich.cs", "src/hub.cs"], "tokens primary");

        // Same pool, equal hit counts: NOW inbound decides.
        let m2 = Value::object(vec![(
            "entries",
            Value::object(vec![
                (
                    "src/plain.cs",
                    Value::object(vec![("purpose", Value::string("alpha here"))]),
                ),
                (
                    "src/popular.cs",
                    Value::object(vec![("purpose", Value::string("alpha too"))]),
                ),
            ]),
        )]);
        write_manifest(&root, &m2).unwrap();
        let paths2 = find_ranked(
            &root,
            "alpha zeta",
            &[("src/popular.cs", 9), ("src/plain.cs", 1)],
        );
        assert_eq!(paths2, vec!["src/popular.cs", "src/plain.cs"]);
    }

    #[test]
    fn find_full_tie_keeps_manifest_order() {
        // Every key equal -- tokens AND a populated inbound map reading the
        // same count everywhere -- so the comparator returns Equal throughout
        // and stability must hand back exactly the manifest's on-disk order.
        let root = unique_temp_dir("find-full-tie");
        let m = Value::object(vec![(
            "entries",
            Value::object(vec![
                (
                    "src/first.cs",
                    Value::object(vec![("purpose", Value::string("twin widget"))]),
                ),
                (
                    "src/second.cs",
                    Value::object(vec![("purpose", Value::string("twin widget"))]),
                ),
                (
                    "src/third.cs",
                    Value::object(vec![("purpose", Value::string("twin widget"))]),
                ),
            ]),
        )]);
        write_manifest(&root, &m).unwrap();
        let tied = [
            ("src/first.cs", 4usize),
            ("src/second.cs", 4),
            ("src/third.cs", 4),
        ];
        let paths = find_ranked(&root, "twin widget", &tied);
        assert_eq!(paths, vec!["src/first.cs", "src/second.cs", "src/third.cs"]);
    }

    #[test]
    fn find_with_an_empty_map_returns_todays_order() {
        // No graph, no ranking: the AND pool keeps manifest iteration order,
        // and the OR pool still sorts by hits alone with manifest-order ties --
        // byte for byte what `find_in_manifest` answered before the map
        // existed.
        let root = unique_temp_dir("find-empty-map");

        // AND pool: both full matches sit on equal keys (every count reads 0),
        // so stability hands back manifest order -- "second" stays ahead of
        // "first", as on disk.
        let m = Value::object(vec![(
            "entries",
            Value::object(vec![
                (
                    "src/second.cs",
                    Value::object(vec![("purpose", Value::string("alpha beta"))]),
                ),
                (
                    "src/first.cs",
                    Value::object(vec![("purpose", Value::string("beta alpha"))]),
                ),
                (
                    "src/partial.cs",
                    Value::object(vec![("purpose", Value::string("alpha only"))]),
                ),
            ]),
        )]);
        write_manifest(&root, &m).unwrap();
        assert_eq!(
            find_ranked(&root, "alpha beta", &[]),
            vec!["src/second.cs", "src/first.cs"]
        );

        // OR pool: no entry matches every token of the three-token query.
        // Hits primary: the two-hit entry leads; the one-hit ties keep
        // manifest order behind it; the zero-hit entry stays out. Paths carry
        // no token substrings -- the haystack includes the path, and this
        // test wants the PURPOSE text deciding every hit count.
        let m2 = Value::object(vec![(
            "entries",
            Value::object(vec![
                (
                    "src/a.cs",
                    Value::object(vec![("purpose", Value::string("alpha four"))]),
                ),
                (
                    "src/b.cs",
                    Value::object(vec![("purpose", Value::string("beta five"))]),
                ),
                (
                    "src/c.cs",
                    Value::object(vec![("purpose", Value::string("alpha beta six"))]),
                ),
                (
                    "src/d.cs",
                    Value::object(vec![("purpose", Value::string("seven"))]),
                ),
            ]),
        )]);
        write_manifest(&root, &m2).unwrap();
        assert_eq!(
            find_ranked(&root, "alpha beta gamma", &[]),
            vec!["src/c.cs", "src/a.cs", "src/b.cs"]
        );
    }

    // -- git_head -----------------------------------------------------

    #[test]
    fn git_head_is_none_outside_a_repo() {
        let root = unique_temp_dir("nogit-head");
        assert_eq!(git_head(&root), None);
    }

    // -- scope_for ------------------------------------------------------

    #[test]
    fn scope_for_prefers_explicit_dirs_then_registry_then_dot() {
        let _guard = REGISTRY_ENV_LOCK.lock().unwrap();
        let scoped = unique_temp_dir("scope-scoped");
        let unscoped = unique_temp_dir("scope-unscoped");
        let reg_dir = unique_temp_dir("scope-reg");
        let reg_path = reg_dir.join("repos.json");
        fs::write(
            &reg_path,
            format!(
                r#"{{"roots": [{{"root": "{}", "kind": "git", "scope": ["src", "app"]}}]}}"#,
                scoped.display()
            ),
        )
        .unwrap();

        env::set_var("SCOUT_REGISTRY", &reg_path);
        let result = (|| {
            assert_eq!(
                scope_for(&scoped, None).unwrap(),
                vec!["src".to_string(), "app".to_string()]
            );
            assert_eq!(scope_for(&unscoped, None).unwrap(), vec![".".to_string()]);
            let explicit = vec!["a".to_string(), "b".to_string()];
            assert_eq!(scope_for(&scoped, Some(&explicit)).unwrap(), explicit);
        })();
        env::remove_var("SCOUT_REGISTRY");
        result
    }
}
