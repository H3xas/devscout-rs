// One JSON line per query-verb invocation, appended to
// `<git-common-dir>/scout/log/queries.jsonl` (or `<root>/.scout/log/queries.jsonl`
// outside a git repository) when `SCOUT_TELEMETRY=1` is set. `cli.rs`'s five
// query-verb command functions each call `record` once, at the point they
// already know their own seed, outcome and row count -- never by re-parsing
// the answer they just rendered, so a verb's exit code and output are the
// same with or without telemetry. A write that fails for any reason (the
// switch simply being off, a directory that cannot be created, a file that
// cannot be opened for append) is silent.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::hookio::iso8601_now;
use crate::query::Outcome;
use crate::repo::{git_common_dir, scout_dir};

/// This record's own schema version -- independent of `graph::GRAPH_SCHEMA_VERSION`
/// and of any `--json` answer schema, bumped only when this line's own shape
/// changes.
const SCHEMA_VERSION: u64 = 1;

/// What one query-verb invocation reports about itself: the seed the caller
/// asked for, the closed-vocabulary outcome the verb already resolved, and how
/// many rows the answer carried (`0` when it carried none).
pub(crate) struct QueryEvent<'a> {
    /// The verb value.
    pub verb: &'static str,
    /// The seed value.
    pub seed: &'a str,
    /// The outcome value.
    pub outcome: Outcome,
    /// The candidate count value.
    pub candidate_count: usize,
}

// Mirrors `graph.rs`'s private `graph_dir`/`manifest.rs`'s private
// `manifest_path`: shared artifacts key off the git COMMON dir so every linked
// worktree of one repo sees the same log, falling back to `<root>/.scout/log/`
// outside a git repo.
fn log_path(root: &Path) -> PathBuf {
    let dir = match git_common_dir(root) {
        Some(common) => common.join("scout").join("log"),
        None => scout_dir(root).join("log"),
    };
    dir.join("queries.jsonl")
}

/// Whether telemetry is switched on: exactly `SCOUT_TELEMETRY=1`. Unset,
/// empty, or any other value is off -- only the literal string `"1"` turns it
/// on.
fn enabled() -> bool {
    std::env::var("SCOUT_TELEMETRY").as_deref() == Ok("1")
}

/// Appends one telemetry line for a query-verb invocation. A no-op unless
/// `SCOUT_TELEMETRY=1`; a directory that cannot be created or a file that
/// cannot be opened for append is swallowed rather than propagated, so a
/// broken log can never change the caller's exit code or output.
pub(crate) fn record(root: &Path, event: &QueryEvent, elapsed: Duration, result_bytes: usize) {
    if !enabled() {
        return;
    }
    let path = log_path(root);
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    let line = format_line(event, elapsed, result_bytes);
    let _ = file
        .write_all(line.as_bytes())
        .and_then(|()| file.write_all(b"\n"));
}

// Key order is the contract every reader of this file relies on: `ts`,
// `schema_version`, `verb`, `seed`, `outcome`, `elapsed_ms`, `result_bytes`,
// `candidate_count`.
fn format_line(event: &QueryEvent, elapsed: Duration, result_bytes: usize) -> String {
    format!(
        "{{\"ts\":{ts},\"schema_version\":{sv},\"verb\":{verb},\"seed\":{seed},\"outcome\":{outcome},\"elapsed_ms\":{ms},\"result_bytes\":{bytes},\"candidate_count\":{cc}}}",
        ts = json_str(&iso8601_now()),
        sv = SCHEMA_VERSION,
        verb = json_str(event.verb),
        seed = json_str(event.seed),
        outcome = json_str(event.outcome.as_str()),
        ms = elapsed.as_millis(),
        bytes = result_bytes,
        cc = event.candidate_count,
    )
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).expect("string JSON encoding cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    // `SCOUT_TELEMETRY` is process-wide state; every test that sets it holds
    // this mutex so parallel test threads never see each other's value.
    static ENV_LOCK: Mutex<()> = Mutex::new(());
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "scout-telemetry-rs-{prefix}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn sample_event(seed: &str) -> QueryEvent<'_> {
        QueryEvent {
            verb: "refs",
            seed,
            outcome: Outcome::Hit,
            candidate_count: 3,
        }
    }

    #[test]
    fn format_line_keeps_the_fixed_key_order() {
        let event = sample_event("Widget");
        let line = format_line(&event, Duration::from_millis(7), 42);
        let keys = [
            "\"ts\":",
            "\"schema_version\":",
            "\"verb\":",
            "\"seed\":",
            "\"outcome\":",
            "\"elapsed_ms\":",
            "\"result_bytes\":",
            "\"candidate_count\":",
        ];
        let mut last = 0;
        for key in keys {
            let pos = line.find(key).unwrap_or_else(|| panic!("missing {key}"));
            assert!(pos >= last, "{key} out of order in {line}");
            last = pos;
        }
        assert!(line.contains("\"schema_version\":1"));
        assert!(line.contains("\"verb\":\"refs\""));
        assert!(line.contains("\"seed\":\"Widget\""));
        assert!(line.contains("\"outcome\":\"hit\""));
        assert!(line.contains("\"elapsed_ms\":7"));
        assert!(line.contains("\"result_bytes\":42"));
        assert!(line.contains("\"candidate_count\":3"));
    }

    #[test]
    fn disabled_without_the_env_var_creates_no_file_or_directory() {
        let _guard = ENV_LOCK.lock().unwrap();
        let root = unique_temp_dir("off");
        std::env::remove_var("SCOUT_TELEMETRY");
        record(&root, &sample_event("Widget"), Duration::default(), 1);
        assert!(!log_path(&root).exists());
        assert!(!log_path(&root).parent().unwrap().exists());
    }

    #[test]
    fn any_value_other_than_the_literal_one_is_off() {
        let _guard = ENV_LOCK.lock().unwrap();
        let root = unique_temp_dir("off-other");
        std::env::set_var("SCOUT_TELEMETRY", "true");
        record(&root, &sample_event("Widget"), Duration::default(), 1);
        std::env::remove_var("SCOUT_TELEMETRY");
        assert!(!log_path(&root).exists());
    }

    #[test]
    fn enabled_appends_one_line_per_call() {
        let _guard = ENV_LOCK.lock().unwrap();
        let root = unique_temp_dir("on");
        std::env::set_var("SCOUT_TELEMETRY", "1");
        record(&root, &sample_event("Widget"), Duration::from_millis(1), 10);
        record(&root, &sample_event("Gadget"), Duration::from_millis(2), 20);
        std::env::remove_var("SCOUT_TELEMETRY");
        let text = std::fs::read_to_string(log_path(&root)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "one line per call, appended not truncated");
        assert!(lines[0].contains("\"seed\":\"Widget\""));
        assert!(lines[1].contains("\"seed\":\"Gadget\""));
    }

    #[test]
    fn an_unwritable_log_path_is_silent() {
        let _guard = ENV_LOCK.lock().unwrap();
        let root = unique_temp_dir("unwritable");
        std::env::set_var("SCOUT_TELEMETRY", "1");
        // Pre-create the log FILE'S OWN path as a directory: opening it for
        // append then fails with a filesystem error this call must swallow.
        std::fs::create_dir_all(log_path(&root)).unwrap();
        record(&root, &sample_event("Widget"), Duration::default(), 1);
        std::env::remove_var("SCOUT_TELEMETRY");
        // No panic, and the path is still the directory it was.
        assert!(log_path(&root).is_dir());
    }
}
