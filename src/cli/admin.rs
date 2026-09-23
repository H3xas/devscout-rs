// `map`, `stats` and `clear`: the index build/summary/prune commands, none of
// which read the graph -- only `require_repo` and the content/read-cache
// store.

use std::path::Path;

use crate::mapcmd;
use crate::store;

use super::args::parse_int_js;
use super::root::require_repo;

// `map`. Strips the no-op alias flag `--refresh` and the compiler-fact
// enrichment rollback flag `--no-semantic` before anything else runs; the
// remaining tokens are the scope dirs, passed straight through to
// `mapcmd::map_repo`. This function's only job is the CLI glue: `require_repo`,
// flag stripping, and printing `MapReport::summary_line()`. `MapOptions::from_env()`
// decides the fragment-reuse mode (mapcmd.rs's own module header): content-hash
// reuse is the default, `SCOUT_MTIME_REUSE=1` drops back to mtime keying.
// `--no-semantic` is a flag, not an environment switch (mapcmd/options.rs's
// own doc comment), so it is read here and folded onto the env-derived
// options rather than added to `from_env` itself.
pub(crate) fn cmd_map(cwd: &Path, args: &[String]) -> (i32, String) {
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}")),
    };
    let no_semantic = args.iter().any(|a| a.as_str() == "--no-semantic");
    let dirs: Vec<String> = args
        .iter()
        .filter(|a| a.as_str() != "--refresh" && a.as_str() != "--no-semantic")
        .cloned()
        .collect();
    let opts = mapcmd::MapOptions {
        no_semantic,
        ..mapcmd::MapOptions::from_env()
    };
    match mapcmd::map_repo(&root, &dirs, opts) {
        Ok(report) => (0, report.summary_line()),
        Err(e) => (1, format!("error: {e}")),
    }
}

// `stats`. The read-cache/bash-cache/session/top-stubbed queries are NOT wrapped
// in error handling, so any error there aborts the whole command (an early
// `return` with the same `"error: {msg}"` shape every other command in this file
// uses); only the cross-repo content-store block IS fail-open, two chained `if
// let Ok(..)`s that silently produce no lines on either failure.
pub(crate) fn cmd_stats(cwd: &Path) -> (i32, String) {
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}")),
    };
    let db = match store::open_store(&root) {
        Ok(c) => c,
        Err(e) => return (1, format!("error: {e}")),
    };
    let s = match store::stats_for(&db) {
        Ok(s) => s,
        Err(e) => return (1, format!("error: {e}")),
    };

    let mut lines = vec![
        format!("devscout stats ({}):", root.display()),
        format!("  files tracked: {}", s.distinct_files),
        format!("  reads deduped (stubs): {}", s.total_stubs),
        format!("  lines saved: {}", s.lines_saved),
        format!("  bytes saved: {}", s.bytes_saved),
        format!(
            "  est tokens saved: {}",
            js_math_round(s.bytes_saved as f64 / 4.0)
        ),
    ];

    let b = match store::bash_stats_for(&db) {
        Ok(b) => b,
        Err(e) => return (1, format!("error: {e}")),
    };
    if b.commands_tracked > 0 {
        lines.push(format!("  bash commands tracked: {}", b.commands_tracked));
        lines.push(format!("  bash dedups (stubs): {}", b.total_stubs));
        lines.push(format!(
            "  bash est tokens saved: {}",
            js_math_round(b.bytes_saved as f64 / 4.0)
        ));
    }

    // Fail open on EITHER the content-store open or the stats query.
    if let Ok(cs_conn) = store::open_content_store() {
        if let Ok(cs) = store::content_stats_for(&cs_conn) {
            if cs.total_stubs > 0 {
                lines.push(format!(
                    "  cross-repo dedups (all roots, this machine): {}",
                    cs.total_stubs
                ));
                lines.push(format!(
                    "  cross-repo est tokens saved: {}",
                    js_math_round(cs.bytes_saved as f64 / 4.0)
                ));
            }
        }
    }

    let per_session = match store::session_stats(&db) {
        Ok(v) => v,
        Err(e) => return (1, format!("error: {e}")),
    };
    if !per_session.is_empty() {
        lines.push(String::new());
        lines.push("  per session:".to_string());
        for r in &per_session {
            // Session ids are ASCII (uuid/hash) in every real writer, so a
            // char-based slice of the first 8 is unambiguous.
            let sid: String = r.session_id.chars().take(8).collect();
            lines.push(format!(
                "    {sid:<8}  files {:>4}  stubs {:>4}  lines saved {:>6}  est tokens {}",
                r.files,
                r.stubs,
                r.lines_saved,
                js_math_round(r.bytes_saved as f64 / 4.0),
            ));
        }
    }

    let top = match store::top_stubbed(&db, 5) {
        Ok(v) => v,
        Err(e) => return (1, format!("error: {e}")),
    };
    if !top.is_empty() {
        lines.push(String::new());
        lines.push("  top stubbed files:".to_string());
        for r in &top {
            let sid: String = r.session_id.chars().take(8).collect();
            lines.push(format!(
                "    {}x  {} ({} lines, session {sid})",
                r.stub_count, r.rel_path, r.lines
            ));
        }
    }

    (0, lines.join("\n"))
}

// `clear`. Three forms, checked in this exact order so the first flag present
// wins even if both appear: `--older-than <days>` prunes rows untouched for that
// many days, `--session <prefix>` prunes one session by id prefix (refusing an
// ambiguous one), and with neither flag the whole `cache.db` file is removed. The
// whole-store form never calls `open_store` -- it only builds the path and checks
// existence, so a repo that never wrote a cache.db is never given one just to
// clear it -- and the WAL/SHM sidecar files SQLite leaves beside it are
// deliberately NOT removed here (a plain `remove_file` on the db path).
pub(crate) fn cmd_clear(cwd: &Path, args: &[String]) -> (i32, String) {
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}")),
    };

    if let Some(idx) = args.iter().position(|a| a == "--older-than") {
        let raw = args.get(idx + 1).map(String::as_str).unwrap_or("");
        let days = match parse_int_js(raw) {
            Some(d) if d >= 0 => d,
            _ => return (2, "usage: devscout clear --older-than <days>".to_string()),
        };
        let db = match store::open_store(&root) {
            Ok(c) => c,
            Err(e) => return (1, format!("error: {e}")),
        };
        let deleted = match store::prune(&db, Some(days as f64), None) {
            Ok(n) => n,
            Err(e) => return (1, format!("error: {e}")),
        };
        let suffix = if deleted == 1 { "" } else { "s" };
        return (
            0,
            format!("deleted {deleted} row{suffix} older than {days}d"),
        );
    }

    if let Some(idx) = args.iter().position(|a| a == "--session") {
        let prefix = args.get(idx + 1).map(String::as_str).unwrap_or("");
        if prefix.is_empty() {
            return (
                2,
                "usage: devscout clear --session <id-or-prefix>".to_string(),
            );
        }
        let db = match store::open_store(&root) {
            Ok(c) => c,
            Err(e) => return (1, format!("error: {e}")),
        };
        let matches = match store::session_ids_by_prefix(&db, prefix) {
            Ok(v) => v,
            Err(e) => return (1, format!("error: {e}")),
        };
        if matches.is_empty() {
            return (0, format!("no sessions match \"{prefix}\""));
        }
        if matches.len() > 1 {
            return (
                2,
                format!(
                    "ambiguous session prefix \"{prefix}\": {}",
                    matches.join(", ")
                ),
            );
        }
        let deleted = match store::prune(&db, None, Some(matches[0].as_str())) {
            Ok(n) => n,
            Err(e) => return (1, format!("error: {e}")),
        };
        let suffix = if deleted == 1 { "" } else { "s" };
        let session = &matches[0];
        return (
            0,
            format!("deleted {deleted} row{suffix} for session {session}"),
        );
    }

    let db_path = crate::repo::scout_dir(&root).join("cache.db");
    if db_path.exists() {
        if let Err(e) = std::fs::remove_file(&db_path) {
            return (1, format!("error: {e}"));
        }
    }
    (0, "cache cleared".to_string())
}

// `Math.round`-style rounding: `floor(x + 0.5)`, NOT Rust's `f64::round` (which
// rounds ties away from zero -- the two agree for every non-negative input,
// which is all `cmd_stats` ever feeds this, but this spells out the exact rule
// rather than relying on that coincidence).
pub(crate) fn js_math_round(x: f64) -> i64 {
    (x + 0.5).floor() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    // A bare `.git` marker directory is all `require_repo` needs to resolve a
    // root -- no real git repository has to exist for `cmd_map` itself.
    fn fake_repo(label: &str) -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "scout-admin-test-{label}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(dir.join(".git")).expect("create fake repo root");
        dir
    }

    // `--no-semantic` (like `--refresh`) must never survive into `dirs` and be
    // mistaken for a scope directory -- both flags are stripped before
    // `map_repo` sees the argument list.
    #[test]
    fn no_semantic_flag_is_stripped_and_never_becomes_a_scope_dir() {
        let root = fake_repo("no-semantic");
        let (code, out) = cmd_map(
            &root,
            &["--no-semantic".to_string(), "--refresh".to_string()],
        );
        assert_eq!(code, 0, "unexpected failure: {out}");
        assert!(
            out.contains("mapped 0 files under ."),
            "with no positional scope dirs left, the run covers the whole repo: {out}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn no_semantic_flag_coexists_with_a_real_scope_dir() {
        let root = fake_repo("no-semantic-scope");
        let (code, out) = cmd_map(&root, &["--no-semantic".to_string(), "src".to_string()]);
        assert_eq!(code, 0, "unexpected failure: {out}");
        assert!(
            out.contains("under src"),
            "the real scope dir must survive flag stripping: {out}"
        );
        std::fs::remove_dir_all(&root).ok();
    }
}
