// `devscout hook read` and `devscout hook bash`: stdin JSON -> decision -> stdout,
// fail-open, < 10 ms end-to-end.
//
// Components:
//   - the read hook -- `handle_read` below
//   - the bash hook -- `handle_bash` below
//   - hashing helpers (sha256, count_lines, short_sha)
//   - debug helpers (debug_enabled, debug_log)
//
// Ordering-sensitive JSON: both hooks echo back fragments of the ORIGINAL stdin
// payload (the bash hook mirrors the response with `stdout` overridden; the read
// hook mirrors the original `file` object's fields) and JSON object key order is
// text insertion order. `serde_json::Value` is BTreeMap-backed (no
// `preserve_order` feature, the same constraint manifest.rs's header documents)
// and would silently re-sort keys, changing the output bytes. So this module
// parses and re-serializes through `crate::manifest::Value` instead -- a public,
// order-preserving JSON tree with a correct-escaping Serialize impl (it delegates
// to serde_json's string serializer, just keeps a `Vec<(String, Value)>` instead
// of a map).
//
// sha256: the `sha2` crate. The unit tests keep the standard NIST vectors as a
// regression guard on the wrapper.
//
// Fail-open: `run_read`/`run_bash` are the ONLY boundary that matters for "any
// error, timeout, or malformed stdin -> exit 0, tool result untouched" -- they
// wrap the entire decision path in `catch_unwind` and collapse every
// `Result::Err` to empty output. The one deliberately NARROWER catch is
// `content_dedup`, which has its own local try/catch around cross-repo
// content-store access (a failure there falls through to the fresh-record path,
// it does not abort the whole hook).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::manifest::{self, Value as JVal};
use crate::repo;
use crate::store;

type HookResult<T> = Result<T, Box<dyn std::error::Error>>;

const MIN_STDOUT_BYTES: usize = 256;

// ---------------------------------------------------------------------------
// CLI entry points (called from cli.rs's `hook read` / `hook bash` arms).
// This IS the fail-open boundary: `catch_unwind` catches a genuine panic,
// `.unwrap_or_default()` catches every `Result::Err` (malformed stdin,
// non-UTF8 stdin, missing store, sqlite failure, ...) -- both collapse to
// empty output -- the exit-0, tool-result-untouched contract.
// ---------------------------------------------------------------------------

/// Processes a read-hook JSON payload and returns the replacement payload.
pub fn run_read(raw: &[u8]) -> Vec<u8> {
    std::panic::catch_unwind(|| decode_and_handle(raw, handle_read))
        .unwrap_or_default()
        .into_bytes()
}

/// Processes a shell-hook JSON payload and returns the replacement payload.
pub fn run_bash(raw: &[u8]) -> Vec<u8> {
    std::panic::catch_unwind(|| decode_and_handle(raw, handle_bash))
        .unwrap_or_default()
        .into_bytes()
}

fn decode_and_handle(raw: &[u8], handler: fn(&JVal) -> HookResult<String>) -> String {
    let text = match std::str::from_utf8(raw) {
        Ok(t) => t,
        Err(_) => return String::new(),
    };
    let input: JVal = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    handler(&input).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Shared stdin-shape helpers (both hooks read session_id/agent_id the same way).
// ---------------------------------------------------------------------------

// `input.session_id`, defaulting to `"unknown"` when missing or null. A
// present-but-non-string session_id is also treated as missing; not reachable
// with a real harness payload (session_id is always a string), so not chased.
fn session_id_of(input: &JVal) -> &str {
    input
        .get("session_id")
        .and_then(JVal::as_str)
        .unwrap_or("unknown")
}

// `input.agent_id` when it is a string, else `""`: a non-string agent_id
// (including explicit null) defaults to `""` the same as absent.
fn agent_scope(input: &JVal) -> &str {
    input.get("agent_id").and_then(JVal::as_str).unwrap_or("")
}

fn jval_i64(v: &JVal) -> Option<i64> {
    match v {
        JVal::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        _ => None,
    }
}

fn jval_f64(v: &JVal) -> Option<f64> {
    match v {
        JVal::Number(n) => n.as_f64(),
        _ => None,
    }
}

// JavaScript-style truthiness for a `Value`: `false`, `""`, numeric `0`,
// `null`, and "missing" (`None`) are falsy; everything else (including an empty
// array/object) is truthy.
fn jval_truthy(v: Option<&JVal>) -> bool {
    match v {
        None | Some(JVal::Null) => false,
        Some(JVal::Bool(b)) => *b,
        Some(JVal::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(JVal::String(s)) => !s.is_empty(),
        Some(JVal::Array(_)) | Some(JVal::Object(_)) => true,
    }
}

// Nullish-coalescing for one object field: `obj[key]` unless that read is
// missing or null, in which case `default`. Missing key and explicit JSON `null`
// both fall back; any other value (including falsy-but-not-nullish values like
// `0`/`""`) passes through unchanged.
fn nullish_or(obj: &JVal, key: &str, default: JVal) -> JVal {
    match obj.get(key) {
        Some(v) if !matches!(v, JVal::Null) => v.clone(),
        _ => default,
    }
}

// ---------------------------------------------------------------------------
// JSON output construction -- order-preserving via `manifest::Value`.
// ---------------------------------------------------------------------------

// Wraps `value` in the hook output envelope:
// `{ hookSpecificOutput: { hookEventName: "PostToolUse", <field>: value } }`.
fn envelope(field: &str, value: JVal) -> String {
    let inner = JVal::object(vec![
        ("hookEventName", JVal::string("PostToolUse")),
        (field, value),
    ]);
    let outer = JVal::object(vec![("hookSpecificOutput", inner)]);
    serde_json::to_string(&outer).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Hashing helpers.
// ---------------------------------------------------------------------------

// Lowercase hex SHA-256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    sha256::digest_hex(bytes)
}

// Number of lines in `text`. `\n` is a single ASCII byte, so splitting on it is
// encoding-independent -- no offset translation needed here.
fn count_lines(text: &str) -> i64 {
    if text.is_empty() {
        return 0;
    }
    let trimmed = text.strip_suffix('\n').unwrap_or(text);
    trimmed.split('\n').count() as i64
}

// First 8 hex chars of `hex`.
fn short_sha(hex: &str) -> &str {
    &hex[..hex.len().min(8)]
}

mod sha256 {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;

    pub fn digest_hex(input: &[u8]) -> String {
        let digest = Sha256::digest(input);
        let mut out = String::with_capacity(64);
        for byte in digest {
            let _ = write!(out, "{byte:02x}");
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn empty_string_vector() {
            assert_eq!(
                digest_hex(b""),
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
            );
        }

        #[test]
        fn abc_vector() {
            assert_eq!(
                digest_hex(b"abc"),
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            );
        }

        #[test]
        fn two_block_vector() {
            // NIST's standard 448-bit multi-block vector.
            let input = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
            assert_eq!(
                digest_hex(input),
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Debug helpers.
// ---------------------------------------------------------------------------

// Whether debug logging is on: env `SCOUT_DEBUG=1`, or a `<root>/.scout/debug` file.
fn debug_enabled(root: &Path) -> bool {
    if env::var("SCOUT_DEBUG").ok().as_deref() == Some("1") {
        return true;
    }
    repo::scout_dir(root).join("debug").exists()
}

// Appends one JSON line to the debug log. Must never throw or otherwise disturb
// the caller; every failure (disabled, io error, json error) is a silent no-op.
// Field order is `ts`, `event`, then caller-supplied fields in call order.
fn debug_log(root: &Path, event: &str, fields: Vec<(&str, JVal)>) {
    if !debug_enabled(root) {
        return;
    }
    let mut entries: Vec<(String, JVal)> = vec![
        ("ts".to_string(), JVal::string(iso8601_now())),
        ("event".to_string(), JVal::string(event)),
    ];
    entries.extend(fields.into_iter().map(|(k, v)| (k.to_string(), v)));
    let line = match serde_json::to_string(&JVal::Object(entries)) {
        Ok(s) => s,
        Err(_) => return,
    };
    let log_path = repo::scout_dir(root).join("debug.log");
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        use std::io::Write;
        let _ = writeln!(f, "{line}");
    }
}

// An ISO-8601 UTC timestamp (`YYYY-MM-DDThh:mm:ss.sssZ`), hand-rolled with no
// date/time dependency. Civil-from-days is Howard Hinnant's well-known
// constant-time algorithm. `pub(crate)`: manifest.rs's `write_index_state`
// reuses it rather than hand-rolling a second copy.
pub(crate) fn iso8601_now() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;
    let millis = now.subsec_millis();
    let days = secs.div_euclid(86400);
    let secs_of_day = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let hh = secs_of_day / 3600;
    let mm = (secs_of_day % 3600) / 60;
    let ss = secs_of_day % 60;
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}.{millis:03}Z")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod civil_tests {
    use super::civil_from_days;

    #[test]
    fn epoch_day_zero_is_1970_01_01() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn known_date_2026_08_15() {
        // Days from 1970-01-01 to 2026-08-15 (verified via
        // `datetime.date(2026,8,15) - datetime.date(1970,1,1)`).
        assert_eq!(civil_from_days(20680), (2026, 8, 15));
    }
}

// ===========================================================================
// Read-hook implementation
// ===========================================================================

// The file/text content out of a tool response, if any.
fn extract_text<'a>(tool_response: Option<&'a JVal>) -> Option<&'a str> {
    let tr = tool_response?;
    if let Some(s) = tr
        .get("file")
        .and_then(|f| f.get("content"))
        .and_then(JVal::as_str)
    {
        return Some(s);
    }
    tr.get("text").and_then(JVal::as_str)
}

// Whether the read covered only part of the file (offset > 1, or numLines < totalLines).
fn is_partial_read(input: &JVal) -> bool {
    if let Some(offset) = input
        .get("tool_input")
        .and_then(|t| t.get("offset"))
        .and_then(jval_f64)
    {
        if offset > 1.0 {
            return true;
        }
    }
    if let Some(file) = input.get("tool_response").and_then(|r| r.get("file")) {
        if let (Some(num_lines), Some(total_lines)) = (
            file.get("numLines").and_then(jval_f64),
            file.get("totalLines").and_then(jval_f64),
        ) {
            return num_lines < total_lines;
        }
    }
    false
}

// The read-hook envelope carrying the stub text, mirroring the original file object's shape when present.
fn stub_envelope(stub_text: &str, orig_file: Option<&JVal>, file_path: &str, lines: i64) -> String {
    let has_orig = matches!(orig_file, Some(v) if !matches!(v, JVal::Null));
    if has_orig {
        let of = orig_file.expect("has_orig guarantees Some");
        let file_obj = JVal::object(vec![
            (
                "filePath",
                nullish_or(of, "filePath", JVal::string(file_path)),
            ),
            ("content", JVal::string(stub_text)),
            ("numLines", JVal::number(1i64)),
            ("startLine", nullish_or(of, "startLine", JVal::number(1i64))),
            (
                "totalLines",
                nullish_or(of, "totalLines", JVal::number(lines)),
            ),
        ]);
        let inner = JVal::object(vec![("type", JVal::string("text")), ("file", file_obj)]);
        envelope("updatedToolOutput", inner)
    } else {
        let inner = JVal::object(vec![
            ("type", JVal::string("text")),
            ("text", JVal::string(stub_text)),
        ]);
        envelope("updatedToolOutput", inner)
    }
}

// Cross-repo content dedup. Deliberately swallows every internal error locally
// (its own try/catch) instead of using `?` -- a content-store failure must fall
// through to the fresh-record path, not abort the whole hook.
#[allow(clippy::too_many_arguments)]
fn content_dedup(
    session_id: &str,
    agent_id: &str,
    hash: &str,
    root: &str,
    rel: &str,
    size: i64,
    lines: i64,
) -> Option<String> {
    let attempt = || -> HookResult<Option<String>> {
        let cdb = store::open_content_store()?;
        if let Some(hit) = store::lookup_content(&cdb, session_id, hash, agent_id)? {
            if hit.root == root && hit.rel_path == rel {
                return Ok(None);
            }
            store::bump_content_stub(&cdb, session_id, hash, agent_id)?;
            return Ok(Some(format!(
                "[devscout: identical content already read this session as {} — {lines} lines, sha {}. Full content already in context.]",
                hit.rel_path,
                short_sha(hash)
            )));
        }
        store::record_content(
            &cdb,
            &store::RecordContent {
                session_id,
                agent_id,
                sha256: hash,
                root,
                rel_path: rel,
                size,
                lines,
            },
        )?;
        Ok(None)
    };
    attempt().unwrap_or(None)
}

// The read hook's decision: stub, cross-repo stub, fresh record, or stale-manifest note.
fn handle_read(input: &JVal) -> HookResult<String> {
    let file_path = input
        .get("tool_input")
        .and_then(|t| t.get("file_path"))
        .and_then(JVal::as_str);
    let text = extract_text(input.get("tool_response"));
    let (file_path, text) = match (file_path, text) {
        (Some(fp), Some(t)) => (fp, t),
        _ => return Ok(String::new()),
    };

    let root = match repo::find_scout_root(Path::new(file_path)) {
        Some(r) => r,
        None => return Ok(String::new()),
    };

    let rel = repo::rel_path(&root, Path::new(file_path));
    let session_id = session_id_of(input);
    let agent_id = agent_scope(input);
    let size = text.len() as i64;

    if is_partial_read(input) {
        let db = store::open_store(&root)?;
        store::record_spend(
            &db,
            &store::RecordSpend {
                session_id,
                agent_id,
                rel_path: &rel,
                size,
            },
        )?;
        debug_log(
            &root,
            "skip-partial",
            vec![
                ("rel", JVal::string(rel.clone())),
                ("session", JVal::string(session_id)),
                ("agent", JVal::string(agent_id)),
                ("bytes", JVal::number(size)),
            ],
        );
        if let Some(m) = manifest::read_manifest(&root)? {
            let built_at_head = m.get("built_at_head");
            if jval_truthy(built_at_head) {
                let head = manifest::git_head(&root);
                let equal = matches!((head.as_deref(), built_at_head), (Some(h), Some(JVal::String(s))) if h == s);
                if !equal {
                    return Ok(String::new());
                }
            }
        }
        if !repo::scout_dir(&root).join("refresh-needed").exists() {
            if let Some(offer) = first_read_offer(&root, &rel, input) {
                return Ok(envelope("additionalContext", JVal::string(offer)));
            }
        }
        return Ok(String::new());
    }

    let hash = sha256_hex(text.as_bytes());
    let orig_file = input.get("tool_response").and_then(|r| r.get("file"));
    let lines = orig_file
        .and_then(|f| f.get("numLines"))
        .and_then(jval_i64)
        .unwrap_or_else(|| count_lines(text));
    let mtime = fs::metadata(file_path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let db = store::open_store(&root)?;
    let prior = store::lookup_read(&db, session_id, &rel, agent_id)?;

    // 1. Cheapest layer: per-root (session, agent, rel_path) path cache.
    if let Some(p) = &prior {
        if p.sha256 == hash {
            store::bump_stub(&db, session_id, &rel, agent_id)?;
            debug_log(
                &root,
                "stub",
                vec![
                    ("rel", JVal::string(rel.clone())),
                    ("sha", JVal::string(short_sha(&hash))),
                    ("session", JVal::string(session_id)),
                    ("agent", JVal::string(agent_id)),
                ],
            );
            let stub_text = format!(
                "[devscout: unchanged since first read this session — {lines} lines, sha {}. Full content already in context.]",
                short_sha(&hash)
            );
            return Ok(stub_envelope(&stub_text, orig_file, file_path, lines));
        }
    }

    // 2. Second chance: cross-repo content cache, path miss only.
    let root_str = root.to_string_lossy().into_owned();
    let cross_stub = content_dedup(session_id, agent_id, &hash, &root_str, &rel, size, lines);
    if let Some(stub_text) = cross_stub {
        store::record_fresh(
            &db,
            &store::RecordFresh {
                session_id,
                agent_id,
                rel_path: &rel,
                sha256: &hash,
                size,
                mtime,
                lines,
                delivered: false,
            },
        )?;
        debug_log(
            &root,
            "content-stub",
            vec![
                ("rel", JVal::string(rel.clone())),
                ("sha", JVal::string(short_sha(&hash))),
                ("session", JVal::string(session_id)),
                ("agent", JVal::string(agent_id)),
            ],
        );
        return Ok(stub_envelope(&stub_text, orig_file, file_path, lines));
    }

    store::record_fresh(
        &db,
        &store::RecordFresh {
            session_id,
            agent_id,
            rel_path: &rel,
            sha256: &hash,
            size,
            mtime,
            lines,
            delivered: true,
        },
    )?;
    debug_log(
        &root,
        "fresh",
        vec![
            ("rel", JVal::string(rel.clone())),
            ("sha", JVal::string(short_sha(&hash))),
            ("session", JVal::string(session_id)),
            ("agent", JVal::string(agent_id)),
        ],
    );

    if let Some(m) = manifest::read_manifest(&root)? {
        let built_at_head = m.get("built_at_head");
        if jval_truthy(built_at_head) {
            let head = manifest::git_head(&root);
            let equal = matches!((head.as_deref(), built_at_head), (Some(h), Some(JVal::String(s))) if h == s);
            if !equal {
                let flag = repo::scout_dir(&root).join("refresh-needed");
                if !flag.exists() {
                    // Formats `<built_at_head> -> <head or "unknown">`. A real
                    // manifest writer only ever puts a string in `built_at_head`
                    // (see manifest.rs's header comment); a truthy non-string is
                    // not produced in practice.
                    let built_display = match built_at_head {
                        Some(JVal::String(s)) => s.clone(),
                        _ => String::new(),
                    };
                    let head_display = head.unwrap_or_else(|| "unknown".to_string());
                    fs::write(&flag, format!("{built_display} -> {head_display}\n"))?;
                    return Ok(envelope(
                        "additionalContext",
                        JVal::string("[devscout: manifest stale — HEAD moved since last map. Run 'devscout map --refresh'.]"),
                    ));
                }
            }
        }
    }

    if !repo::scout_dir(&root).join("refresh-needed").exists() {
        if let Some(offer) = first_read_offer(&root, &rel, input) {
            return Ok(envelope("additionalContext", JVal::string(offer)));
        }
    }

    Ok(String::new())
}

// Builds the one-line `devscout read` offer for a freshly-delivered file:
// `None` whenever there is nothing honest to offer -- no graph artifact for
// this root, no defs mapped in this file -- so those reads keep today's
// empty output exactly. The offered symbol is the def whose declaration span
// sits nearest to the range this read returned; when the payload does not
// say where the read sat (the legacy text-only response shape carries no
// line bounds at all), the fallback is the file's FIRST mapped symbol --
// noted here because it is a limitation of the payload shape, not a choice
// about the code: a whole-file read would tie-break to that same first row
// anyway.
fn first_read_offer(root: &Path, rel: &str, input: &JVal) -> Option<String> {
    let g = crate::graph::read_graph(root)?;
    let defs: Vec<&crate::graph::Def> = g.defs.iter().filter(|d| d.file == rel).collect();
    if defs.is_empty() {
        return None;
    }

    // Requested range: what the tool_response reported about itself
    // (`file.startLine`, `file.numLines`), falling back to tool_input's
    // `offset` when the response carries no start of its own. A start with
    // no length reads as "everything from here down" (`usize::MAX` end),
    // which makes every span at-or-below the start overlap it; no bounds in
    // the payload at all reads as a whole-file read, `(1, MAX)` -- whose
    // tie-break is exactly the first-mapped-symbol fallback documented on
    // this function.
    let orig_file = input.get("tool_response").and_then(|r| r.get("file"));
    let req_start = orig_file
        .and_then(|f| f.get("startLine"))
        .and_then(jval_i64)
        .or_else(|| {
            input
                .get("tool_input")
                .and_then(|t| t.get("offset"))
                .and_then(jval_i64)
        })
        .map(|v| v.max(1) as usize);
    let num_lines = orig_file.and_then(|f| f.get("numLines")).and_then(jval_i64);
    let (rs, re) = match (req_start, num_lines) {
        (Some(s), Some(n)) => (s, s.saturating_sub(1).saturating_add(n.max(0) as usize)),
        (Some(s), None) => (s, usize::MAX),
        (None, _) => (1, usize::MAX),
    };
    nearest_def(&defs, rs, re).map(|d| offer_text(&g, d))
}

// Gap between a def's declaration span and the requested range: zero on any
// overlap, otherwise how many lines apart they sit. Ties break by earlier
// start line, then by id -- deterministic, independent of map order.
fn nearest_def<'a>(
    defs: &[&'a crate::graph::Def],
    req_start: usize,
    req_end: usize,
) -> Option<&'a crate::graph::Def> {
    defs.iter().copied().min_by(|a, b| {
        let key = |d: &crate::graph::Def| {
            let ds = d.line.max(1);
            // `0` is the "no end recorded" sentinel (TS defs); such a
            // span measures as its start line alone rather than as a
            // guessed range.
            let de = if d.end_line >= ds { d.end_line } else { ds };
            // Below the range: how far short it falls. Otherwise 0 on a true
            // overlap, and for a span past the range's end saturating_sub
            // yields the same positive gap the explicit branch did.
            let gap = if de < req_start {
                req_start - de
            } else {
                ds.saturating_sub(req_end)
            };
            (gap, ds)
        };
        key(a).cmp(&key(b)).then_with(|| a.id.cmp(&b.id))
    })
}

// The offer sentence itself. The symbol is named the way the verb can take
// it: the simple name when no other def shares it (the resolver answers a
// unique name directly), the full def id when the bare name would print an
// ambiguity -- never a guess between the two.
fn offer_text(g: &crate::graph::Graph, chosen: &crate::graph::Def) -> String {
    let ambiguous = g
        .defs
        .iter()
        .any(|d| d.id != chosen.id && d.name == chosen.name);
    let subject = if ambiguous {
        chosen.id.as_str()
    } else {
        chosen.name.as_str()
    };
    format!(
        "[devscout: this file is indexed — 'devscout read {subject}' shows its declaration span and inbound references.]"
    )
}

// ===========================================================================
// Bash-hook implementation
// ===========================================================================

struct BashTarget {
    anchor_path: PathBuf,
}

// Trims the command and strips a leading `rtk proxy ` or `rtk ` prefix.
fn normalize_command(command: &str) -> String {
    let trimmed = command.trim();
    if let Some(after_rtk) = trimmed.strip_prefix("rtk").and_then(skip_ws1) {
        if let Some(after_proxy) = after_rtk.strip_prefix("proxy").and_then(skip_ws1) {
            return after_proxy.to_string();
        }
    }
    if let Some(after_rtk) = trimmed.strip_prefix("rtk").and_then(skip_ws1) {
        return after_rtk.to_string();
    }
    trimmed.to_string()
}

// Consumes one or more whitespace chars from the front of `s`; `None` if `s`
// has no leading whitespace at all.
fn skip_ws1(s: &str) -> Option<&str> {
    let trimmed = s.trim_start();
    if trimmed.len() == s.len() {
        None
    } else {
        Some(trimmed)
    }
}

// Matches `git [-C <dir>] show <ref>:<path>`. Returns
// `(optional -C arg, ref, path)`.
fn match_git_show(command: &str) -> Option<(Option<&str>, &str, &str)> {
    let rest = command.strip_prefix("git ")?;
    if let Some(after_c) = rest.strip_prefix("-C ") {
        let sp = after_c.find(' ')?;
        if sp == 0 {
            return None;
        }
        let dir = &after_c[..sp];
        let after_show = after_c[sp + 1..].strip_prefix("show ")?;
        let (r, p) = split_ref_path(after_show)?;
        return Some((Some(dir), r, p));
    }
    let after_show = rest.strip_prefix("show ")?;
    let (r, p) = split_ref_path(after_show)?;
    Some((None, r, p))
}

// Splits the tail after "show " into `<ref>:<path>`: neither side may contain
// whitespace, `ref` may not contain `:` (so the FIRST `:` is unambiguously the
// separator), both sides non-empty.
fn split_ref_path(s: &str) -> Option<(&str, &str)> {
    if s.is_empty() || s.chars().any(char::is_whitespace) {
        return None;
    }
    let colon = s.find(':')?;
    let (r, rest) = s.split_at(colon);
    let p = &rest[1..];
    if r.is_empty() || p.is_empty() {
        return None;
    }
    Some((r, p))
}

// Matches `cat <path>` with a single whitespace-free path argument.
fn match_cat(command: &str) -> Option<&str> {
    let rest = command.strip_prefix("cat ")?;
    if rest.is_empty() || rest.chars().any(char::is_whitespace) {
        return None;
    }
    Some(rest)
}

// Whether `s` contains any shell metacharacter (`* ? [ ] | & ; < >`).
fn has_forbidden_chars(s: &str) -> bool {
    s.chars()
        .any(|c| matches!(c, '*' | '?' | '[' | ']' | '|' | '&' | ';' | '<' | '>'))
}

// Matches `read <token>...`. Unlike `match_cat` this is NOT end-anchored: it
// captures the first whitespace-delimited token after `read ` and tolerates
// trailing flags (e.g. `--max-lines 5`), which are simply not part of the
// capture.
fn match_rtk_read(command: &str) -> Option<&str> {
    let rest = command.strip_prefix("read ")?;
    let first = rest.chars().next()?; // \S+ needs >=1 char; "read " (nothing after) -> None
    if first.is_whitespace() {
        return None; // \S+ cannot start on a whitespace char ("read  x" double space)
    }
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    Some(&rest[..end])
}

// Resolves `target` against `base` for the two call sites inside `classify`
// where the payload's `cwd` (NOT this process's own cwd) is the base. Falls back
// to this process's actual cwd only in the unreachable-in-practice case where
// `cwd` itself is relative or absent. No `.`/`..` collapsing here --
// `repo::find_scout_root` re-normalizes through its own `resolve_path`/
// `normalize` before this path is used for anything, so a second normalization
// pass here would be redundant, not incorrect either way.
fn resolve_against(base: &str, target: &str) -> PathBuf {
    let target_path = Path::new(target);
    if target_path.is_absolute() {
        return target_path.to_path_buf();
    }
    let base_path = Path::new(base);
    if base_path.is_absolute() {
        base_path.join(target_path)
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(base_path)
            .join(target_path)
    }
}

// Classifies a bash command as a file-read target (git show / cat / rtk read), or `None`.
fn classify_bash(command: &str, cwd: Option<&str>) -> Option<BashTarget> {
    if let Some((c_arg, _git_ref, git_path)) = match_git_show(command) {
        let base: PathBuf = match c_arg {
            Some(dir) if Path::new(dir).is_absolute() => PathBuf::from(dir),
            Some(dir) => resolve_against(cwd.unwrap_or("."), dir),
            None => PathBuf::from(cwd.unwrap_or(".")),
        };
        return Some(BashTarget {
            anchor_path: base.join(git_path),
        });
    }
    if let Some(path) = match_cat(command) {
        if !has_forbidden_chars(path) {
            let p = if Path::new(path).is_absolute() {
                PathBuf::from(path)
            } else {
                resolve_against(cwd.unwrap_or("."), path)
            };
            return Some(BashTarget { anchor_path: p });
        }
    }
    // rtk's PreToolUse hook renames the verb: `cat FILE` -> `rtk read FILE` and
    // `head -n N FILE` -> `rtk read FILE --max-lines N`. normalize_command()
    // strips only the `rtk ` prefix, so without this branch every
    // rtk-rewritten read classifies as None and never dedups. Trailing flags
    // are tolerated here but remain part of the cache key (the caller keys on
    // the full normalized string), so a truncated read can never collide with
    // a full one.
    if let Some(path) = match_rtk_read(command) {
        if !has_forbidden_chars(path) {
            let p = if Path::new(path).is_absolute() {
                PathBuf::from(path)
            } else {
                resolve_against(cwd.unwrap_or("."), path)
            };
            return Some(BashTarget { anchor_path: p });
        }
    }
    None
}

// Copies the ORIGINAL tool_response, overriding only `stdout`'s value.
// Overwriting an existing key in place preserves its position, so the mirrored
// object keeps `resp`'s exact original key order.
fn mirror_with_stdout(resp: &JVal, stub_text: &str) -> JVal {
    match resp {
        JVal::Object(entries) => JVal::Object(
            entries
                .iter()
                .map(|(k, v)| {
                    if k == "stdout" {
                        (k.clone(), JVal::string(stub_text))
                    } else {
                        (k.clone(), v.clone())
                    }
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

// The bash hook's decision: stub the output on a repeat, else record it fresh.
fn handle_bash(input: &JVal) -> HookResult<String> {
    let command = input
        .get("tool_input")
        .and_then(|t| t.get("command"))
        .and_then(JVal::as_str);
    let resp = input.get("tool_response");
    let stdout = resp.and_then(|r| r.get("stdout")).and_then(JVal::as_str);
    let (command, resp, stdout) = match (command, resp, stdout) {
        (Some(c), Some(r), Some(s)) => (c, r, s),
        _ => return Ok(String::new()),
    };

    if matches!(resp.get("interrupted"), Some(JVal::Bool(true))) {
        return Ok(String::new());
    }
    if matches!(resp.get("isImage"), Some(JVal::Bool(true))) {
        return Ok(String::new());
    }
    if let Some(stderr) = resp.get("stderr").and_then(JVal::as_str) {
        if !stderr.trim().is_empty() {
            return Ok(String::new());
        }
    }
    if stdout.len() < MIN_STDOUT_BYTES {
        return Ok(String::new());
    }

    let normalized = normalize_command(command);
    let cwd = input.get("cwd").and_then(JVal::as_str);
    let target = match classify_bash(&normalized, cwd) {
        Some(t) => t,
        None => return Ok(String::new()),
    };

    let root = match repo::find_scout_root(&target.anchor_path) {
        Some(r) => r,
        None => return Ok(String::new()),
    };

    let session_id = session_id_of(input);
    let agent_id = agent_scope(input);
    let hash = sha256_hex(stdout.as_bytes());
    let lines = count_lines(stdout);

    let db = store::open_store(&root)?;
    let prior = store::lookup_bash(&db, session_id, &normalized, agent_id)?;

    if let Some(p) = &prior {
        if p.sha256 == hash {
            store::bump_bash_stub(&db, session_id, &normalized, agent_id)?;
            debug_log(
                &root,
                "bash-stub",
                vec![
                    ("cmd", JVal::string(normalized.clone())),
                    ("sha", JVal::string(short_sha(&hash))),
                    ("session", JVal::string(session_id)),
                    ("agent", JVal::string(agent_id)),
                ],
            );
            let stub_text =
                format!("[devscout: identical output already in context this session — {lines} lines, sha {}.]", short_sha(&hash));
            let mirrored = mirror_with_stdout(resp, &stub_text);
            return Ok(envelope("updatedToolOutput", mirrored));
        }
    }

    store::record_bash_fresh(
        &db,
        &store::RecordBashFresh {
            session_id,
            agent_id,
            cache_key: &normalized,
            sha256: &hash,
            size: stdout.len() as i64,
            lines,
        },
    )?;
    debug_log(
        &root,
        "bash-fresh",
        vec![
            ("cmd", JVal::string(normalized.clone())),
            ("sha", JVal::string(short_sha(&hash))),
            ("session", JVal::string(session_id)),
            ("agent", JVal::string(agent_id)),
        ],
    );
    Ok(String::new())
}

// ===========================================================================
// Unit tests -- pure-function coverage (classify/normalize/count_lines/envelope
// shape). Cross-process interop and the latency measurement run out-of-crate:
// both need a throwaway `.scout` fixture tree and a separate process, which does
// not belong in a `cargo test` unit block any more than store.rs's own interop
// coverage does (see that module's header comment on the same split).
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    // Same convention as store.rs's own test module (unique_temp_dir/repo):
    // a per-process, monotonically-increasing suffix keeps parallel `cargo
    // test` threads from colliding on the same directory name.
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        env::temp_dir().join(format!(
            "scout-hookio-rs-{prefix}-{}-{n}",
            std::process::id()
        ))
    }

    fn bash_repo() -> PathBuf {
        let root = unique_temp_dir("bash-repo");
        fs::create_dir_all(root.join(".scout")).unwrap();
        root
    }

    // `'line of output\n'.repeat(40)` -- > MIN_STDOUT_BYTES (256) so it clears
    // the size gate.
    fn big_stdout() -> String {
        "line of output\n".repeat(40)
    }

    // Builds a harness-shaped bash payload (defaults `stderr: ""`,
    // `interrupted: false`, `isImage: false`, `noOutputExpected: false`).
    fn bash_payload(root: &Path, command: &str, stdout: &str, session: &str) -> String {
        format!(
            r#"{{"session_id":"{session}","cwd":{cwd},"tool_name":"Bash","tool_input":{{"command":{command},"description":"test"}},"tool_response":{{"stdout":{stdout},"stderr":"","interrupted":false,"isImage":false,"noOutputExpected":false}}}}"#,
            cwd = serde_json::to_string(&root.to_string_lossy()).unwrap(),
            command = serde_json::to_string(command).unwrap(),
            stdout = serde_json::to_string(stdout).unwrap(),
        )
    }

    fn run_bash_str(payload: &str) -> String {
        String::from_utf8(run_bash(payload.as_bytes())).unwrap()
    }

    // rtk read dedupes; glob or pipe forms do not.
    #[test]
    fn rtk_read_dedupes_glob_or_pipe_forms_do_not() {
        let root = bash_repo();
        let big = big_stdout();
        assert_eq!(
            run_bash_str(&bash_payload(&root, "rtk read src/a.ts", &big, "b1")),
            ""
        );
        assert!(
            run_bash_str(&bash_payload(&root, "rtk read src/a.ts", &big, "b1"))
                .contains("devscout: identical output")
        );
        assert_eq!(
            run_bash_str(&bash_payload(&root, "rtk read src/*.ts", &big, "b1")),
            ""
        );
        assert_eq!(
            run_bash_str(&bash_payload(&root, "rtk read src/*.ts", &big, "b1")),
            ""
        );
        assert_eq!(
            run_bash_str(&bash_payload(&root, "rtk read a.ts | head", &big, "b1")),
            ""
        );
    }

    // rtk read --max-lines is a distinct cache key from the full read.
    #[test]
    fn rtk_read_max_lines_is_distinct_cache_key_from_full_read() {
        let root = bash_repo();
        let big = big_stdout();
        assert_eq!(
            run_bash_str(&bash_payload(
                &root,
                "rtk read src/a.ts --max-lines 50",
                &big,
                "b1"
            )),
            ""
        );
        assert_eq!(
            run_bash_str(&bash_payload(&root, "rtk read src/a.ts", &big, "b1")),
            ""
        );
        assert!(run_bash_str(&bash_payload(
            &root,
            "rtk read src/a.ts --max-lines 50",
            &big,
            "b1"
        ))
        .contains("devscout: identical output"));
    }

    // -- count_lines ------------------------------------------------------

    #[test]
    fn count_lines_matches_node_semantics() {
        assert_eq!(count_lines(""), 0);
        assert_eq!(count_lines("a"), 1);
        assert_eq!(count_lines("a\n"), 1);
        assert_eq!(count_lines("a\nb"), 2);
        assert_eq!(count_lines("a\nb\n"), 2);
        assert_eq!(count_lines("\n"), 1);
    }

    // -- short_sha ----------------------------------------------------------

    #[test]
    fn short_sha_takes_first_eight_hex_chars() {
        let hex = sha256_hex(b"abc");
        assert_eq!(short_sha(&hex), &hex[..8]);
        assert_eq!(short_sha(&hex).len(), 8);
    }

    // -- normalize_command --------------------------------------------------

    #[test]
    fn normalize_command_strips_rtk_and_rtk_proxy_prefixes() {
        assert_eq!(normalize_command("git status"), "git status");
        assert_eq!(normalize_command("  git status  "), "git status");
        assert_eq!(normalize_command("rtk git status"), "git status");
        assert_eq!(normalize_command("rtk proxy git status"), "git status");
        assert_eq!(normalize_command("rtkgit status"), "rtkgit status");
    }

    // -- match_git_show / classify_bash -------------------------------------

    #[test]
    fn git_show_matches_bare_form() {
        let (c, r, p) = match_git_show("git show origin/main:src/a.ts").unwrap();
        assert_eq!(c, None);
        assert_eq!(r, "origin/main");
        assert_eq!(p, "src/a.ts");
    }

    #[test]
    fn git_show_matches_dash_c_form() {
        let (c, r, p) = match_git_show("git -C /tmp/repo show HEAD:src/a.ts").unwrap();
        assert_eq!(c, Some("/tmp/repo"));
        assert_eq!(r, "HEAD");
        assert_eq!(p, "src/a.ts");
    }

    #[test]
    fn git_show_rejects_non_matching_shapes() {
        assert!(match_git_show("git status").is_none());
        assert!(match_git_show("git show x").is_none()); // no colon
        assert!(match_git_show("git show :path").is_none()); // empty ref
        assert!(match_git_show("git show ref:").is_none()); // empty path
    }

    #[test]
    fn cat_matches_single_token_only() {
        assert_eq!(match_cat("cat src/a.ts"), Some("src/a.ts"));
        assert_eq!(match_cat("cat src/a.ts extra"), None);
        assert_eq!(match_cat("cat"), None);
    }

    #[test]
    fn cat_glob_and_pipe_are_excluded_by_classify() {
        assert!(has_forbidden_chars("src/*.ts"));
        assert!(classify_bash("cat src/*.ts", Some("/tmp")).is_none());
        assert!(classify_bash("cat a.ts | head", Some("/tmp")).is_none()); // doesn't even match `cat` shape
    }

    #[test]
    fn classify_git_c_anchors_to_dash_c_dir_ignoring_cwd() {
        let target = classify_bash(
            "git -C /repo-root show HEAD:src/a.ts",
            Some("/somewhere/else"),
        )
        .unwrap();
        assert_eq!(target.anchor_path, Path::new("/repo-root/src/a.ts"));
    }

    #[test]
    fn classify_cat_resolves_relative_against_cwd() {
        let target = classify_bash("cat src/a.ts", Some("/repo-root")).unwrap();
        assert_eq!(target.anchor_path, Path::new("/repo-root/src/a.ts"));
    }

    // -- match_rtk_read / classify_bash rtk-read branch ----------------------

    #[test]
    fn rtk_read_matches_bare_form() {
        assert_eq!(match_rtk_read("read src/a.ts"), Some("src/a.ts"));
    }

    #[test]
    fn rtk_read_matches_and_excludes_trailing_flags_from_capture() {
        // The read pattern is not end-anchored -- flags after the first token
        // are tolerated but not part of the captured path.
        assert_eq!(
            match_rtk_read("read src/a.ts --max-lines 50"),
            Some("src/a.ts")
        );
    }

    #[test]
    fn rtk_read_rejects_non_matching_shapes() {
        assert_eq!(match_rtk_read("cat src/a.ts"), None); // wrong verb
        assert_eq!(match_rtk_read("read"), None); // no trailing space, no path
        assert_eq!(match_rtk_read("read "), None); // trailing space, empty path (\S+ needs >=1 char)
        assert_eq!(match_rtk_read("read  src/a.ts"), None); // double space -- \S+ can't start on whitespace
        assert_eq!(match_rtk_read("aread src/a.ts"), None); // not a prefix match
    }

    #[test]
    fn rtk_read_glob_is_excluded_by_classify() {
        assert!(classify_bash("read src/*.ts", Some("/tmp")).is_none());
    }

    // Behavioral subtlety: unlike `match_cat` (end-anchored, so it rejects
    // `cat a.ts | head` outright because the whole remainder must be the path),
    // `match_rtk_read` is NOT end-anchored. It only ever captures the FIRST
    // whitespace-delimited token, so "read a.ts | head" DOES classify -- anchor
    // path "a.ts" -- and the pipe/trailing text never reaches the metacharacter
    // guard (it isn't part of the captured token). The command still dedupes
    // correctly (keyed on the FULL normalized string, pipe included, per
    // `handle_bash`'s cache-key contract), so this is not a correctness bug --
    // just a sharper trigger condition than `cat`'s for what counts as a
    // "read-shaped" command.
    #[test]
    fn rtk_read_pipe_form_still_classifies_first_token_only() {
        let target = classify_bash("read a.ts | head", Some("/tmp")).unwrap();
        assert_eq!(target.anchor_path, Path::new("/tmp/a.ts"));
    }

    #[test]
    fn classify_rtk_read_resolves_relative_against_cwd() {
        let target = classify_bash("read src/a.ts", Some("/repo-root")).unwrap();
        assert_eq!(target.anchor_path, Path::new("/repo-root/src/a.ts"));
    }

    #[test]
    fn classify_rtk_read_absolute_path_passes_through() {
        let target = classify_bash("read /abs/src/a.ts", Some("/repo-root")).unwrap();
        assert_eq!(target.anchor_path, Path::new("/abs/src/a.ts"));
    }

    #[test]
    fn classify_rtk_read_flags_stay_out_of_anchor_path_but_flagged_and_bare_forms_are_still_distinct_commands(
    ) {
        // classify() only ever sees the anchor path; the cache key distinction
        // between `read FILE` and `read FILE --max-lines 50` lives one layer up
        // (handle_bash keys on the full normalized string) -- covered by the
        // full-pipeline test rtk_read_max_lines_is_distinct_cache_key below.
        let bare = classify_bash("read src/a.ts", Some("/repo-root")).unwrap();
        let flagged = classify_bash("read src/a.ts --max-lines 50", Some("/repo-root")).unwrap();
        assert_eq!(bare.anchor_path, flagged.anchor_path);
    }

    // -- envelope / stub_envelope shape --------------------------------------

    #[test]
    fn envelope_field_order_matches_node() {
        let out = envelope("additionalContext", JVal::string("hi"));
        assert_eq!(
            out,
            r#"{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"hi"}}"#
        );
    }

    #[test]
    fn stub_envelope_legacy_text_shape_when_no_orig_file() {
        let out = stub_envelope("stub", None, "/a/b.ts", 3);
        assert_eq!(
            out,
            r#"{"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedToolOutput":{"type":"text","text":"stub"}}}"#
        );
    }

    #[test]
    fn stub_envelope_file_shape_defaults_missing_fields() {
        let orig = JVal::object(vec![("filePath", JVal::string("/a/b.ts"))]);
        let out = stub_envelope("stub", Some(&orig), "/a/b.ts", 7);
        assert_eq!(
            out,
            r#"{"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedToolOutput":{"type":"text","file":{"filePath":"/a/b.ts","content":"stub","numLines":1,"startLine":1,"totalLines":7}}}}"#
        );
    }

    #[test]
    fn stub_envelope_null_orig_file_falls_back_to_legacy_shape() {
        let out = stub_envelope("stub", Some(&JVal::Null), "/a/b.ts", 3);
        assert_eq!(
            out,
            r#"{"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedToolOutput":{"type":"text","text":"stub"}}}"#
        );
    }

    // -- mirror_with_stdout ---------------------------------------------------

    #[test]
    fn mirror_with_stdout_preserves_original_key_order() {
        let resp = JVal::object(vec![
            ("stdout", JVal::string("orig")),
            ("stderr", JVal::string("")),
            ("interrupted", JVal::Bool(false)),
        ]);
        let mirrored = mirror_with_stdout(&resp, "stub");
        let out = serde_json::to_string(&mirrored).unwrap();
        assert_eq!(out, r#"{"stdout":"stub","stderr":"","interrupted":false}"#);
    }

    // -- run_read / run_bash fail-open boundary ------------------------------

    #[test]
    fn run_read_malformed_json_fails_open_empty_output() {
        assert_eq!(run_read(b"{ not json"), Vec::<u8>::new());
    }

    #[test]
    fn run_read_non_utf8_fails_open_empty_output() {
        assert_eq!(run_read(&[0xff, 0xfe, 0x00]), Vec::<u8>::new());
    }

    #[test]
    fn run_read_missing_fields_fails_open_empty_output() {
        assert_eq!(run_read(b"{}"), Vec::<u8>::new());
    }

    #[test]
    fn run_bash_malformed_json_fails_open_empty_output() {
        assert_eq!(run_bash(b"not json at all"), Vec::<u8>::new());
    }

    #[test]
    fn run_bash_missing_fields_fails_open_empty_output() {
        assert_eq!(run_bash(b"{}"), Vec::<u8>::new());
    }

    #[test]
    fn run_read_no_scout_root_fails_open_empty_output() {
        // A real file_path/tool_response but no `.scout` anywhere above it
        // (temp dir root) -- must produce empty output, not an error exit.
        let dir = std::env::temp_dir().join(format!("scout-hookio-noroot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.ts");
        std::fs::write(&file, "hello\n").unwrap();
        let payload = format!(
            r#"{{"session_id":"s1","tool_input":{{"file_path":"{}"}},"tool_response":{{"text":"hello\n"}}}}"#,
            file.to_string_lossy().replace('\\', "\\\\")
        );
        assert_eq!(run_read(payload.as_bytes()), Vec::<u8>::new());
    }
}
