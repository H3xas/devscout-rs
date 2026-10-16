//! Shared harness for the fact-export acceptance tests.
//!
//! Every case copies one of the three dedicated fixture trees into a scratch
//! directory, commits it with fixed author, committer and dates, maps it with
//! the compiled binary, and drives `devscout export` there. Expectations are
//! read off the private artifacts (`graph.json`, the fragment cache, the
//! record, the digest table) rather than restated, so an expectation cannot
//! merely echo the output it judges -- except where the answer IS the contract
//! (the pinned exports and the schema), where the fixture README says so.
//!
//! The scratch root is isolated the same way `tests/support` isolates its own:
//! `HOME`, `SCOUT_REGISTRY` and `SCOUT_CONTENT_DB` all point inside the scratch
//! tree, so a run never touches the operator's real index.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use sha2::{Digest, Sha256};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// The pinned identity every fixture commit is made with, so an export's
/// `indexedHead` differs only by content.
const AUTHOR_DATE: &str = "2026-01-01T00:00:00+00:00";

/// The three fixture trees, by the name their pinned export carries.
pub const LANGUAGES: [&str; 3] = ["csharp", "typescript", "rust"];

pub struct Fixture {
    pub root: PathBuf,
    pub home: PathBuf,
    pub language: &'static str,
}

fn scratch(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-fact-export-{prefix}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create scratch dir");
    fs::canonicalize(&dir).expect("canonicalize scratch dir")
}

fn fixture_root(language: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/fact-export")
        .join(language)
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create dest dir");
    for entry in fs::read_dir(src).expect("read source dir") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name();
        let path = entry.path();
        if entry.file_type().expect("entry type").is_dir() {
            copy_tree(&path, &dst.join(&name));
        } else {
            fs::copy(&path, dst.join(&name)).expect("copy fixture file");
        }
    }
}

impl Fixture {
    /// A committed, mapped copy of one fixture tree.
    pub fn build(language: &'static str) -> Fixture {
        let base = scratch(language);
        let root = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&home).expect("create home dir");
        copy_tree(&fixture_root(language), &root);
        let fixture = Fixture {
            root,
            home,
            language,
        };
        fixture.git(&["init", "-q"]);
        fixture.git(&["config", "user.email", "fixture@example.com"]);
        fixture.git(&["config", "user.name", "fixture"]);
        fixture.git(&["add", "."]);
        let commit = Command::new("git")
            .args(["commit", "-q", "-m", "fixture"])
            .env("GIT_AUTHOR_DATE", AUTHOR_DATE)
            .env("GIT_COMMITTER_DATE", AUTHOR_DATE)
            .current_dir(&fixture.root)
            .output()
            .expect("git must run");
        assert!(commit.status.success(), "git commit failed: {commit:?}");
        let init = fixture.run(&["init", "--no-hooks", "--no-map"]);
        assert!(init.status.success(), "init failed: {init:?}");
        fixture.map();
        fixture
    }

    /// A copy of one fixture tree that was never mapped.
    pub fn unmapped(language: &'static str) -> Fixture {
        let base = scratch(&format!("{language}-unmapped"));
        let root = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&home).expect("create home dir");
        copy_tree(&fixture_root(language), &root);
        let fixture = Fixture {
            root,
            home,
            language,
        };
        fixture.git(&["init", "-q"]);
        fixture
    }

    pub fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .args(args)
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("SCOUT_REGISTRY", self.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", self.home.join("content.db"))
            .output()
            .expect("devscout must run")
    }

    pub fn git(&self, args: &[&str]) -> Output {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .output()
            .expect("git must run");
        assert!(out.status.success(), "git {args:?} failed: {out:?}");
        out
    }

    /// `map .`, asserting the run succeeded.
    pub fn map(&self) -> String {
        let out = self.run(&["map", "."]);
        assert!(out.status.success(), "map failed: {out:?}");
        String::from_utf8(out.stdout).expect("map stdout is UTF-8")
    }

    /// `devscout export` with extra flags, asserting a written stream.
    pub fn export(&self, extra: &[&str]) -> Vec<u8> {
        let mut args = vec!["export"];
        args.extend_from_slice(extra);
        let out = self.run(&args);
        assert!(
            out.status.success(),
            "export failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    }

    pub fn lines(&self) -> Vec<String> {
        let bytes = self.export(&[]);
        lines_of(&bytes)
    }

    /// The state directory's files and their content digests, for the
    /// read-only assertion. The telemetry log is left out: it is written by the
    /// harness, not by the index.
    pub fn state_snapshot(&self) -> BTreeMap<String, String> {
        let state = git_common_dir(&self.root).join("scout");
        let mut files = BTreeMap::new();
        walk(&state, &state, &mut files);
        files
    }

    pub fn graph(&self) -> Value {
        read_json(&git_common_dir(&self.root).join("scout/graph/graph.json"))
    }

    /// The fragment cache's `entries` object.
    pub fn fragments(&self) -> Value {
        let cache = read_json(&self.cache_path());
        cache["entries"].clone()
    }

    pub(crate) fn cache_path(&self) -> PathBuf {
        let mut found: Vec<PathBuf> = fs::read_dir(git_common_dir(&self.root).join("scout/graph"))
            .expect("read graph dir")
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(OsStr::to_str)
                    .is_some_and(|name| name.starts_with("fragments-v") && name.ends_with(".json"))
            })
            .collect();
        found.sort();
        found.into_iter().next().expect("a fragment cache")
    }

    pub fn record(&self) -> Value {
        read_json(&git_common_dir(&self.root).join("scout/index-state.json"))
    }

    pub fn digests(&self) -> Value {
        read_json(&git_common_dir(&self.root).join("scout/index-digests.json"))
    }

    pub fn state_path(&self) -> PathBuf {
        git_common_dir(&self.root).join("scout")
    }
}

pub fn lines_of(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8(bytes.to_vec()).expect("the stream is UTF-8");
    assert!(
        text.ends_with('\n') && !text.contains('\r'),
        "the stream is LF-terminated and carries no carriage return"
    );
    text.lines().map(str::to_string).collect()
}

pub fn header_of(lines: &[String]) -> Value {
    serde_json::from_str(lines.first().expect("a header line")).expect("the header parses")
}

pub fn trailer_of(lines: &[String]) -> Value {
    serde_json::from_str(lines.last().expect("a trailer line")).expect("the trailer parses")
}

pub fn facts_of(lines: &[String]) -> Vec<Value> {
    if lines.len() <= 2 {
        return Vec::new();
    }
    lines[1..lines.len() - 1]
        .iter()
        .map(|line| serde_json::from_str(line).expect("a fact line parses"))
        .collect()
}

pub fn families_of(facts: &[Value]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for fact in facts {
        let family = fact["family"]
            .as_str()
            .expect("every fact names its family");
        *counts.entry(family.to_string()).or_insert(0) += 1;
    }
    counts
}

pub fn family_lines(lines: &[String], family: &str) -> Vec<Value> {
    facts_of(lines)
        .into_iter()
        .filter(|fact| fact["family"] == family)
        .collect()
}

/// The counts `graph.json` and the fragment cache record, read as data rather
/// than restated: the export's own trailer must agree with the index, not with
/// a second copy of the expectation.
pub fn oracle_counts(fixture: &Fixture) -> BTreeMap<String, usize> {
    let graph = fixture.graph();
    let mut counts = BTreeMap::new();
    counts.insert("definition".to_string(), array_len(&graph, "defs"));
    counts.insert("edge".to_string(), array_len(&graph, "edges"));
    counts.insert(
        "unresolved-import".to_string(),
        array_len(&graph, "unresolved_imports"),
    );
    let (publishes, handlers, registrations) = fragment_messaging(fixture);
    counts.insert("publish-site".to_string(), publishes);
    counts.insert("handler-registration".to_string(), handlers);
    counts.insert("di-registration".to_string(), registrations);
    counts
}

/// The three messaging arrays the fragment cache carries, summed over every
/// entry.
pub fn fragment_messaging(fixture: &Fixture) -> (usize, usize, usize) {
    let fragments = fixture.fragments();
    let mut publishes = 0;
    let mut handlers = 0;
    let mut registrations = 0;
    for (_, entry) in fragments.as_object().expect("entries is an object") {
        let fragment = &entry["fragment"];
        publishes += array_len(fragment, "publishes");
        handlers += array_len(fragment, "handlerRegistrations");
        registrations += array_len(fragment, "registrations");
    }
    (publishes, handlers, registrations)
}

pub fn array_len(value: &Value, key: &str) -> usize {
    value.get(key).and_then(Value::as_array).map_or(0, Vec::len)
}

fn git_common_dir(root: &Path) -> PathBuf {
    let dot_git = root.join(".git");
    let meta = fs::metadata(&dot_git).expect("the fixture is a git repository");
    if meta.is_dir() {
        return dot_git;
    }
    let contents = fs::read_to_string(&dot_git).expect("read .git file");
    let target = contents
        .lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:"))
        .expect("a gitdir line")
        .trim();
    let dir = root.join(target);
    let commondir = dir.join("commondir");
    if !commondir.exists() {
        return dir;
    }
    let rel = fs::read_to_string(&commondir).expect("read commondir");
    dir.join(rel.trim())
}

fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<String, String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy().to_string();
        if name == "log" {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, files);
        } else if let Ok(bytes) = fs::read(&path) {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            files.insert(rel, sha256(&bytes));
        }
    }
}

pub fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// The committed pin for one language: the raw export with the five per-run or
/// per-release header values replaced by fixed placeholders, so a new `map`
/// generation or a version bump does not invalidate it. The placeholders are
/// listed in `fixtures/fact-export/README.md`.
pub fn normalize(export: &[u8]) -> String {
    let mut lines = lines_of(export);
    assert!(lines.len() >= 2, "a stream has a header and a trailer");
    let header = lines.remove(0);
    let normalized = [
        ("generation", "\"generation\""),
        ("indexedAt", "\"indexedAt\""),
        ("indexedHead", "\"indexedHead\""),
        ("devscoutVersion", "\"devscoutVersion\""),
        ("fragmentGeneration", "0"),
    ]
    .iter()
    .fold(header, |text, (key, placeholder)| {
        replace_scalar(&text, key, placeholder)
    });
    lines.insert(0, normalized);
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Replaces the JSON scalar that follows `"<key>":` with `placeholder`,
/// leaving every other byte of the line alone -- the point is that a pin keeps
/// the real key order and the real surrounding bytes.
fn replace_scalar(line: &str, key: &str, placeholder: &str) -> String {
    let marker = format!("\"{key}\":");
    let Some(start) = line.find(&marker) else {
        return line.to_string();
    };
    let value_start = start + marker.len();
    let rest = &line[value_start..];
    let end = if rest.starts_with('"') {
        rest[1..].find('"').map(|i| i + 2).unwrap_or(rest.len())
    } else {
        rest.find([',', '}']).unwrap_or(rest.len())
    };
    format!(
        "{}{}{}",
        &line[..value_start],
        placeholder,
        &line[value_start + end..]
    )
}

pub fn pin_path(language: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/fact-export")
        .join(format!("{language}.ndjson"))
}

pub fn schema_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/reference/fact-export.schema.json")
}

pub fn schema() -> Value {
    read_json(&schema_path())
}

// ---------------------------------------------------------------------------
// The JSON Schema subset.
//
// The schema, the Rust-side validator and the dependency-free Node check all
// speak the same closed subset: `$schema`, `$id`, `$defs`, local `$ref`,
// `title`, `description`, `type`, `const`, `enum`, `required`, `properties`,
// `additionalProperties: false`, `items`, `oneOf`, `pattern`, `minimum`. A
// keyword outside it is a REFUSAL, not something to ignore: a check that
// silently skipped a constraint would pass everything.
// ---------------------------------------------------------------------------

pub const SCHEMA_KEYWORDS: [&str; 14] = [
    "$schema",
    "$id",
    "$defs",
    "$ref",
    "title",
    "description",
    "type",
    "const",
    "enum",
    "required",
    "properties",
    "additionalProperties",
    "items",
    "oneOf",
];

pub const SCHEMA_VALUE_KEYWORDS: [&str; 3] = ["pattern", "minimum", "required"];

/// Whether a schema document uses no keyword outside the subset.
pub fn schema_subset_violation(schema: &Value) -> Option<String> {
    let Some(object) = schema.as_object() else {
        return Some("a schema must be an object".to_string());
    };
    for (key, value) in object {
        let known = SCHEMA_KEYWORDS.contains(&key.as_str())
            || SCHEMA_VALUE_KEYWORDS.contains(&key.as_str());
        if !known {
            return Some(format!("`{key}` is outside the supported keyword subset"));
        }
        match key.as_str() {
            "$defs" | "properties" => {
                for (name, sub) in value.as_object().into_iter().flatten() {
                    if let Some(problem) = schema_subset_violation(sub) {
                        return Some(format!("{name}: {problem}"));
                    }
                }
            }
            "items" | "oneOf" | "additionalProperties" => {
                let subs: Vec<&Value> = match value {
                    Value::Array(items) => items.iter().collect(),
                    other => vec![other],
                };
                for sub in subs {
                    // `additionalProperties: false` is a directive, not a schema.
                    if !sub.is_object() && !sub.is_array() {
                        continue;
                    }
                    if let Some(problem) = schema_subset_violation(sub) {
                        return Some(problem);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Validates `value` against `schema`, resolving `$ref` against `root`.
pub fn validate(value: &Value, schema: &Value, root: &Value) -> Result<(), String> {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let name = reference
            .strip_prefix("#/$defs/")
            .ok_or_else(|| format!("`{reference}` is not a local $defs reference"))?;
        let target = root
            .get("$defs")
            .and_then(|defs| defs.get(name))
            .ok_or_else(|| format!("`{reference}` names no definition"))?;
        return validate(value, target, root);
    }
    if let Some(expected) = schema.get("const") {
        if value != expected {
            return Err(format!("expected the constant {expected}, got {value}"));
        }
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        if !allowed.contains(value) {
            return Err(format!("{value} is not one of the enumerated values"));
        }
    }
    if let Some(kind) = schema.get("type").and_then(Value::as_str) {
        if !type_matches(value, kind) {
            return Err(format!("expected a {kind}, got {value}"));
        }
    }
    if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64) {
        let actual = value
            .as_f64()
            .ok_or_else(|| format!("a minimum applies to a number, got {value}"))?;
        if actual < minimum {
            return Err(format!("{actual} is below the minimum {minimum}"));
        }
    }
    if let Some(pattern) = schema.get("pattern").and_then(Value::as_str) {
        let text = value
            .as_str()
            .ok_or_else(|| format!("a pattern applies to a string, got {value}"))?;
        if !pattern_matches(text, pattern) {
            return Err(format!("{text} does not match {pattern}"));
        }
    }
    if schema.get("additionalProperties").and_then(Value::as_bool) == Some(true) {
        return Err("the subset admits only `additionalProperties: false`".to_string());
    }
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        let object = value
            .as_object()
            .ok_or_else(|| format!("required applies to an object, got {value}"))?;
        for name in required.iter().filter_map(Value::as_str) {
            if !object.contains_key(name) {
                return Err(format!("the required key `{name}` is missing"));
            }
        }
    }
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        let object = value
            .as_object()
            .ok_or_else(|| format!("properties applies to an object, got {value}"))?;
        for (name, sub) in object {
            if let Some(property) = properties.get(name) {
                validate(sub, property, root).map_err(|e| format!("{name}: {e}"))?;
            } else if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                return Err(format!("`{name}` is not a declared key"));
            }
        }
    }
    if let Some(items) = schema.get("items") {
        let array = value
            .as_array()
            .ok_or_else(|| format!("items applies to an array, got {value}"))?;
        for (index, item) in array.iter().enumerate() {
            validate(item, items, root).map_err(|e| format!("[{index}]: {e}"))?;
        }
    }
    if let Some(options) = schema.get("oneOf").and_then(Value::as_array) {
        let failures: Vec<String> = options
            .iter()
            .filter_map(|option| validate(value, option, root).err())
            .collect();
        if failures.len() != options.len() - 1 {
            let matched = options.len() - failures.len();
            let detail = failures.join(" | ");
            return Err(format!(
                "matches {matched} of the oneOf branches (want exactly one); failures: {detail}"
            ));
        }
    }
    Ok(())
}

fn type_matches(value: &Value, kind: &str) -> bool {
    match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        _ => false,
    }
}

// The subset's `pattern` uses only `^`, `$`, `[0-9]`, `[a-z]`, character
// classes and quantifiers, so a hand-rolled matcher is enough here; the Node
// check uses the platform's own regexp for the same patterns.
fn pattern_matches(text: &str, pattern: &str) -> bool {
    let mut body = pattern;
    let anchored_start = body.starts_with('^');
    let anchored_end = body.ends_with('$');
    body = body.trim_start_matches('^').trim_end_matches('$');
    if anchored_start && anchored_end && !body.contains(['[', '+', '*', '?']) {
        return text == body;
    }
    // `^sha256:[0-9a-f]{64}$` and friends: an optional literal prefix, one
    // character class, and a fixed count.
    if let Some((prefix, class, count)) = literal_class_count(body) {
        if anchored_start && anchored_end {
            let Some(rest) = text.strip_prefix(prefix.as_str()) else {
                return false;
            };
            return rest.chars().count() == count
                && rest.chars().all(|c| class_matches(class.as_str(), c));
        }
    }
    text.contains(body)
}

/// Whether `c` is in a character class body: single characters and `a-z`
/// ranges, which is every class the subset uses.
fn class_matches(class: &str, c: char) -> bool {
    let chars: Vec<char> = class.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if index + 2 < chars.len() && chars[index + 1] == '-' {
            if c >= chars[index] && c <= chars[index + 2] {
                return true;
            }
            index += 3;
        } else {
            if c == chars[index] {
                return true;
            }
            index += 1;
        }
    }
    false
}

/// Splits `prefix[class]{n}` into its three parts.
fn literal_class_count(body: &str) -> Option<(String, String, usize)> {
    let open = body.find('[')?;
    let close = body.find(']')?;
    let prefix = body[..open].to_string();
    let class = body[open + 1..close].to_string();
    let count = body[close + 1..]
        .strip_prefix('{')?
        .strip_suffix('}')?
        .parse::<usize>()
        .ok()?;
    Some((prefix, class, count))
}
