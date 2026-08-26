// sqlite: read-cache, bash-cache, content dedup. rusqlite `bundled`, WAL +
// busy_timeout on every connection, short explicit transactions, safe under a
// concurrently writing hook process.
//
// SQL text (including the three CREATE TABLE schema strings) is kept stable and
// explicit so a database's on-disk schema is fully determined by this file.
//
// Busy handling: `with_busy_retry` is a second, independent retry layer above
// the busy_timeout pragma, for the window sustained multi-writer contention can
// still miss (a hook process and this process writing the same live cache.db /
// content.db). Bounded attempts, small linear backoff, retries only
// SQLITE_BUSY/SQLITE_LOCKED, rethrows the last error unchanged on exhaustion,
// any other error rethrown immediately. The `prune()` DELETE and the one-time
// `add_agent_scope` migration are deliberately NOT wrapped.

use std::env;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, ErrorCode, OptionalExtension};

use crate::repo;

// ---------------------------------------------------------------------------
// Schema strings -- exact text (including whitespace) matters. SQLite stores
// this verbatim as `sqlite_master.sql`'s text for a `CREATE TABLE <name>
// <schema>` statement (SQLite drops `IF NOT EXISTS` from the stored text but
// keeps everything else character-for-character), so the stored schema is fully
// determined by these constants.
// ---------------------------------------------------------------------------

const READS_SCHEMA: &str = "(
      session_id   TEXT NOT NULL,
      agent_id     TEXT NOT NULL DEFAULT '',
      rel_path     TEXT NOT NULL,
      sha256       TEXT NOT NULL,
      size         INTEGER NOT NULL,
      mtime        INTEGER NOT NULL,
      first_seen_ts INTEGER NOT NULL,
      lines        INTEGER NOT NULL,
      stub_count   INTEGER NOT NULL DEFAULT 0,
      last_read_ts INTEGER,
      read_count      INTEGER NOT NULL DEFAULT 0,
      bytes_delivered INTEGER NOT NULL DEFAULT 0,
      PRIMARY KEY (session_id, agent_id, rel_path)
    )";

const BASH_SCHEMA: &str = "(
      session_id   TEXT NOT NULL,
      agent_id     TEXT NOT NULL DEFAULT '',
      cache_key    TEXT NOT NULL,
      sha256       TEXT NOT NULL,
      size         INTEGER NOT NULL,
      lines        INTEGER NOT NULL,
      stub_count   INTEGER NOT NULL DEFAULT 0,
      first_seen_ts INTEGER NOT NULL,
      last_read_ts INTEGER,
      PRIMARY KEY (session_id, agent_id, cache_key)
    )";

const CONTENT_SCHEMA: &str = "(
      session_id    TEXT NOT NULL,
      agent_id      TEXT NOT NULL DEFAULT '',
      sha256        TEXT NOT NULL,
      root          TEXT NOT NULL,
      rel_path      TEXT NOT NULL,
      size          INTEGER NOT NULL,
      lines         INTEGER NOT NULL,
      stub_count    INTEGER NOT NULL DEFAULT 0,
      first_seen_ts INTEGER NOT NULL,
      last_read_ts  INTEGER,
      PRIMARY KEY (session_id, agent_id, sha256)
    )";

const READS_COLS: [&str; 12] = [
    "session_id", "agent_id", "rel_path", "sha256", "size", "mtime",
    "first_seen_ts", "lines", "stub_count", "last_read_ts", "read_count", "bytes_delivered",
];

const BASH_COLS: [&str; 9] = [
    "session_id", "agent_id", "cache_key", "sha256", "size", "lines",
    "stub_count", "first_seen_ts", "last_read_ts",
];

const CONTENT_COLS: [&str; 10] = [
    "session_id", "agent_id", "sha256", "root", "rel_path", "size", "lines",
    "stub_count", "first_seen_ts", "last_read_ts",
];

// ---------------------------------------------------------------------------
// Busy retry
// ---------------------------------------------------------------------------

// True only for sqlite's busy/locked condition, surfaced by rusqlite as
// `ErrorCode::DatabaseBusy` / `ErrorCode::DatabaseLocked` on a `SqliteFailure`.
fn is_busy_error(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(ffi_err, _)
            if matches!(ffi_err.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

/// Runs `f`, retrying only a busy/locked error: bounded `attempts`, linear
/// backoff (`backoff_ms * (attempt + 1)`, no sleep after the last attempt). Any
/// other error propagates immediately; exhausting attempts propagates the LAST
/// busy error unchanged (not a new "gave up" error), so a caller that treats a
/// store error as "skip caching" always sees the underlying error shape.
pub fn with_busy_retry_opts<T, F>(attempts: u32, backoff_ms: u64, f: F) -> rusqlite::Result<T>
where
    F: Fn() -> rusqlite::Result<T>,
{
    let mut last_err: Option<rusqlite::Error> = None;
    for attempt in 0..attempts {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => {
                if !is_busy_error(&e) {
                    return Err(e);
                }
                if attempt < attempts - 1 {
                    thread::sleep(Duration::from_millis(backoff_ms * u64::from(attempt + 1)));
                }
                last_err = Some(e);
            }
        }
    }
    Err(last_err.expect("attempts > 0 guarantees at least one iteration"))
}

/// `with_busy_retry_opts` with the default policy (5 attempts, 10ms backoff).
/// Used by every write path below.
pub fn with_busy_retry<T, F>(f: F) -> rusqlite::Result<T>
where
    F: Fn() -> rusqlite::Result<T>,
{
    with_busy_retry_opts(5, 10, f)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

// Milliseconds since the Unix epoch.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// `PRAGMA table_info(table)`'s column names, in table-definition order. Column
// 1 of `table_info`'s row shape is `name` (0=cid, 1=name, 2=type, 3=notnull,
// 4=dflt_value, 5=pk).
fn columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    let sql = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect()
}

// One-time rebuild of a pre-agent-scope table (SQLite cannot ALTER a primary
// key). No-op once `agent_id` is already a column, which is always true for a
// table created fresh via the `*_SCHEMA` constants above -- this only fires on a
// pre-migration fixture. Existing rows predate agent scoping and therefore
// belong to the main thread (`agent_id ''`). Deliberately NOT wrapped in
// `with_busy_retry`: a one-time migration, not the hot write path.
fn add_agent_scope(conn: &Connection, table: &str, schema: &str, cols: &[&str]) -> rusqlite::Result<()> {
    if columns(conn, table)?.iter().any(|c| c == "agent_id") {
        return Ok(());
    }
    let tmp = format!("{table}_agent_scoped");
    let target = cols.join(", ");
    let source = cols
        .iter()
        .map(|c| if *c == "agent_id" { "''".to_string() } else { (*c).to_string() })
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "BEGIN;\nCREATE TABLE {tmp} {schema};\nINSERT INTO {tmp} ({target}) SELECT {source} FROM {table};\nDROP TABLE {table};\nALTER TABLE {tmp} RENAME TO {table};\nCOMMIT;"
    );
    conn.execute_batch(&sql)
}

// ---------------------------------------------------------------------------
// cache.db -- reads + bash_reads
// ---------------------------------------------------------------------------

/// Opens (creating on first use) `<root>/.scout/cache.db`, applies the WAL +
/// busy_timeout=5000 pragmas (load-bearing for concurrent writers), creates
/// `reads`/`bash_reads` if absent, and runs every standing migration
/// (last_read_ts backfill, read_count/bytes_delivered backfill, agent-scope
/// rebuild) idempotently. A freshly created database never takes any migration
/// branch -- the `CREATE TABLE` already has every column the migrations would
/// otherwise add.
pub fn open_store(root: &Path) -> rusqlite::Result<Connection> {
    let db_path = repo::scout_dir(root).join("cache.db");
    let conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA journal_mode = WAL")?;
    conn.execute_batch("PRAGMA busy_timeout = 5000")?;
    conn.execute_batch(&format!("CREATE TABLE IF NOT EXISTS reads {READS_SCHEMA};"))?;
    conn.execute_batch(&format!("CREATE TABLE IF NOT EXISTS bash_reads {BASH_SCHEMA};"))?;
    for table in ["reads", "bash_reads"] {
        if !columns(&conn, table)?.iter().any(|c| c == "last_read_ts") {
            conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN last_read_ts INTEGER"))?;
        }
    }
    if !columns(&conn, "reads")?.iter().any(|c| c == "read_count") {
        conn.execute_batch("ALTER TABLE reads ADD COLUMN read_count INTEGER NOT NULL DEFAULT 0")?;
        conn.execute_batch("ALTER TABLE reads ADD COLUMN bytes_delivered INTEGER NOT NULL DEFAULT 0")?;
        conn.execute_batch("UPDATE reads SET read_count = 1, bytes_delivered = size WHERE sha256 <> ''")?;
    }
    add_agent_scope(&conn, "reads", READS_SCHEMA, &READS_COLS)?;
    add_agent_scope(&conn, "bash_reads", BASH_SCHEMA, &BASH_COLS)?;
    Ok(conn)
}

/// Row shape shared by `lookup_read` and `lookup_bash`: the
/// `SELECT sha256, lines, stub_count FROM ... WHERE ...` result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StubRow {
    pub sha256: String,
    pub lines: i64,
    pub stub_count: i64,
}

/// The cached stub row for a bash command, or `None` if uncached.
pub fn lookup_bash(conn: &Connection, session_id: &str, cache_key: &str, agent_id: &str) -> rusqlite::Result<Option<StubRow>> {
    conn.query_row(
        "SELECT sha256, lines, stub_count FROM bash_reads WHERE session_id = ? AND agent_id = ? AND cache_key = ?",
        params![session_id, agent_id, cache_key],
        |row| Ok(StubRow { sha256: row.get(0)?, lines: row.get(1)?, stub_count: row.get(2)? }),
    )
    .optional()
}

/// Arguments for `record_bash_fresh`: a freshly cached bash command result.
pub struct RecordBashFresh<'a> {
    pub session_id: &'a str,
    pub agent_id: &'a str,
    pub cache_key: &'a str,
    pub sha256: &'a str,
    pub size: i64,
    pub lines: i64,
}

pub fn record_bash_fresh(conn: &Connection, p: &RecordBashFresh) -> rusqlite::Result<()> {
    let now = now_ms();
    with_busy_retry(|| {
        conn.execute(
            "INSERT INTO bash_reads (session_id, agent_id, cache_key, sha256, size, lines, stub_count, first_seen_ts, last_read_ts)
             VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)
             ON CONFLICT(session_id, agent_id, cache_key) DO UPDATE SET
               sha256 = excluded.sha256, size = excluded.size, lines = excluded.lines,
               stub_count = 0, last_read_ts = excluded.last_read_ts",
            params![p.session_id, p.agent_id, p.cache_key, p.sha256, p.size, p.lines, now, now],
        )?;
        Ok(())
    })
}

/// Records another stubbed (cache-hit) read of a bash command.
pub fn bump_bash_stub(conn: &Connection, session_id: &str, cache_key: &str, agent_id: &str) -> rusqlite::Result<()> {
    let now = now_ms();
    with_busy_retry(|| {
        conn.execute(
            "UPDATE bash_reads SET stub_count = stub_count + 1, last_read_ts = ? WHERE session_id = ? AND agent_id = ? AND cache_key = ?",
            params![now, session_id, agent_id, cache_key],
        )?;
        Ok(())
    })
}

/// Aggregate bash-cache statistics (`bash_stats_for`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BashStats {
    pub commands_tracked: i64,
    pub total_stubs: i64,
    pub bytes_saved: i64,
}

pub fn bash_stats_for(conn: &Connection) -> rusqlite::Result<BashStats> {
    conn.query_row(
        "SELECT COUNT(*) AS n, COALESCE(SUM(stub_count),0) AS stubs, COALESCE(SUM(size*stub_count),0) AS bytes_saved FROM bash_reads",
        [],
        |row| {
            Ok(BashStats {
                commands_tracked: row.get(0)?,
                total_stubs: row.get(1)?,
                bytes_saved: row.get(2)?,
            })
        },
    )
}

/// The cached stub row for a file read, or `None` if uncached.
pub fn lookup_read(conn: &Connection, session_id: &str, rel_path: &str, agent_id: &str) -> rusqlite::Result<Option<StubRow>> {
    conn.query_row(
        "SELECT sha256, lines, stub_count FROM reads WHERE session_id = ? AND agent_id = ? AND rel_path = ?",
        params![session_id, agent_id, rel_path],
        |row| Ok(StubRow { sha256: row.get(0)?, lines: row.get(1)?, stub_count: row.get(2)? }),
    )
    .optional()
}

/// Arguments for `record_fresh`: a freshly cached file read. `delivered = false`
/// is the cross-repo content-stub caller: bytes never reached the model, so
/// `bytes_delivered` must not move even though the row is written.
pub struct RecordFresh<'a> {
    pub session_id: &'a str,
    pub agent_id: &'a str,
    pub rel_path: &'a str,
    pub sha256: &'a str,
    pub size: i64,
    pub mtime: i64,
    pub lines: i64,
    pub delivered: bool,
}

pub fn record_fresh(conn: &Connection, p: &RecordFresh) -> rusqlite::Result<()> {
    let now = now_ms();
    let bytes_delivered = if p.delivered { p.size } else { 0 };
    with_busy_retry(|| {
        conn.execute(
            "INSERT INTO reads (session_id, agent_id, rel_path, sha256, size, mtime, first_seen_ts, lines, stub_count, last_read_ts, read_count, bytes_delivered)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?, 1, ?)
             ON CONFLICT(session_id, agent_id, rel_path) DO UPDATE SET
               sha256 = excluded.sha256, size = excluded.size, mtime = excluded.mtime,
               lines = excluded.lines, stub_count = 0, last_read_ts = excluded.last_read_ts,
               read_count = reads.read_count + 1,
               bytes_delivered = reads.bytes_delivered + excluded.bytes_delivered",
            params![p.session_id, p.agent_id, p.rel_path, p.sha256, p.size, p.mtime, now, p.lines, now, bytes_delivered],
        )?;
        Ok(())
    })
}

/// Arguments for `record_spend`: a partial read where the model paid for the
/// slice but nothing about it may be cached, so the row (on first insert)
/// carries `sha256 ''` -- a value no real digest can equal, so `lookup_read`'s
/// equality check can never treat it as a stub candidate. On conflict, only the
/// spend columns move.
pub struct RecordSpend<'a> {
    pub session_id: &'a str,
    pub agent_id: &'a str,
    pub rel_path: &'a str,
    pub size: i64,
}

pub fn record_spend(conn: &Connection, p: &RecordSpend) -> rusqlite::Result<()> {
    let now = now_ms();
    with_busy_retry(|| {
        conn.execute(
            "INSERT INTO reads (session_id, agent_id, rel_path, sha256, size, mtime, first_seen_ts, lines, stub_count, last_read_ts, read_count, bytes_delivered)
             VALUES (?, ?, ?, '', 0, 0, ?, 0, 0, ?, 1, ?)
             ON CONFLICT(session_id, agent_id, rel_path) DO UPDATE SET
               last_read_ts = excluded.last_read_ts,
               read_count = reads.read_count + 1,
               bytes_delivered = reads.bytes_delivered + excluded.bytes_delivered",
            params![p.session_id, p.agent_id, p.rel_path, now, now, p.size],
        )?;
        Ok(())
    })
}

/// Records another stubbed (cache-hit) read of a file.
pub fn bump_stub(conn: &Connection, session_id: &str, rel_path: &str, agent_id: &str) -> rusqlite::Result<()> {
    let now = now_ms();
    with_busy_retry(|| {
        conn.execute(
            "UPDATE reads SET stub_count = stub_count + 1, read_count = read_count + 1, last_read_ts = ? WHERE session_id = ? AND agent_id = ? AND rel_path = ?",
            params![now, session_id, agent_id, rel_path],
        )?;
        Ok(())
    })
}

/// One row of the per-(session, agent, path) breakdown `scout report` shapes.
/// Rows with no reads at all (`read_count = 0` AND `stub_count = 0`) are
/// excluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathBreakdownRow {
    pub session_id: String,
    pub agent_id: String,
    pub rel_path: String,
    pub read_count: i64,
    pub bytes_delivered: i64,
    pub size: i64,
    pub lines: i64,
    pub stub_count: i64,
    pub first_seen_ts: i64,
    pub last_read_ts: Option<i64>,
}

pub fn path_breakdown(conn: &Connection) -> rusqlite::Result<Vec<PathBreakdownRow>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, agent_id, rel_path, read_count, bytes_delivered,
                size, lines, stub_count, first_seen_ts, last_read_ts
         FROM reads
         WHERE read_count > 0 OR stub_count > 0
         ORDER BY session_id, agent_id, rel_path",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(PathBreakdownRow {
            session_id: row.get(0)?,
            agent_id: row.get(1)?,
            rel_path: row.get(2)?,
            read_count: row.get(3)?,
            bytes_delivered: row.get(4)?,
            size: row.get(5)?,
            lines: row.get(6)?,
            stub_count: row.get(7)?,
            first_seen_ts: row.get(8)?,
            last_read_ts: row.get(9)?,
        })
    })?;
    rows.collect()
}

/// Aggregate read-cache statistics (`stats_for`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatsFor {
    pub distinct_files: i64,
    pub total_stubs: i64,
    pub lines_saved: i64,
    pub bytes_saved: i64,
}

pub fn stats_for(conn: &Connection) -> rusqlite::Result<StatsFor> {
    let distinct_files: i64 = conn.query_row("SELECT COUNT(DISTINCT rel_path) AS n FROM reads", [], |row| row.get(0))?;
    let total_stubs: i64 = conn.query_row("SELECT COALESCE(SUM(stub_count),0) AS n FROM reads", [], |row| row.get(0))?;
    let (lines_saved, bytes_saved): (i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(lines*stub_count),0) AS lines_saved, COALESCE(SUM(size*stub_count),0) AS bytes_saved FROM reads",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(StatsFor { distinct_files, total_stubs, lines_saved, bytes_saved })
}

/// Per-session read-cache statistics (`session_stats`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStatsRow {
    pub session_id: String,
    pub files: i64,
    pub stubs: i64,
    pub lines_saved: i64,
    pub bytes_saved: i64,
}

pub fn session_stats(conn: &Connection) -> rusqlite::Result<Vec<SessionStatsRow>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, COUNT(*) AS files, COALESCE(SUM(stub_count),0) AS stubs,
                COALESCE(SUM(lines*stub_count),0) AS lines_saved,
                COALESCE(SUM(size*stub_count),0) AS bytes_saved
         FROM reads GROUP BY session_id ORDER BY stubs DESC, session_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(SessionStatsRow {
            session_id: row.get(0)?,
            files: row.get(1)?,
            stubs: row.get(2)?,
            lines_saved: row.get(3)?,
            bytes_saved: row.get(4)?,
        })
    })?;
    rows.collect()
}

/// One of the most-stubbed files (`top_stubbed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopStubbedRow {
    pub rel_path: String,
    pub session_id: String,
    pub stub_count: i64,
    pub lines: i64,
}

pub fn top_stubbed(conn: &Connection, n: i64) -> rusqlite::Result<Vec<TopStubbedRow>> {
    let mut stmt = conn.prepare(
        "SELECT rel_path, session_id, stub_count, lines FROM reads WHERE stub_count > 0 ORDER BY stub_count DESC, rel_path LIMIT ?",
    )?;
    let rows = stmt.query_map(params![n], |row| {
        Ok(TopStubbedRow {
            rel_path: row.get(0)?,
            session_id: row.get(1)?,
            stub_count: row.get(2)?,
            lines: row.get(3)?,
        })
    })?;
    rows.collect()
}

/// Deletes cached read rows. `session_id` takes priority over `older_than_days`
/// when both are given (short-circuits before the age check). Age is
/// `COALESCE(last_read_ts, first_seen_ts)`: legacy rows migrated without
/// `last_read_ts` fall back to their insert time. Deliberately NOT wrapped in
/// `with_busy_retry`.
pub fn prune(conn: &Connection, older_than_days: Option<f64>, session_id: Option<&str>) -> rusqlite::Result<usize> {
    if let Some(sid) = session_id {
        return conn.execute("DELETE FROM reads WHERE session_id = ?", params![sid]);
    }
    if let Some(days) = older_than_days {
        let cutoff = now_ms() - (days * 86400.0 * 1000.0) as i64;
        return conn.execute("DELETE FROM reads WHERE COALESCE(last_read_ts, first_seen_ts) < ?", params![cutoff]);
    }
    Ok(0)
}

/// Distinct session ids whose id starts with `prefix`.
pub fn session_ids_by_prefix(conn: &Connection, prefix: &str) -> rusqlite::Result<Vec<String>> {
    let like = format!("{prefix}%");
    let mut stmt = conn.prepare("SELECT DISTINCT session_id FROM reads WHERE session_id LIKE ?")?;
    let rows = stmt.query_map(params![like], |row| row.get::<_, String>(0))?;
    rows.collect()
}

// ---------------------------------------------------------------------------
// content.db -- cross-repo content-addressed dedup
// ---------------------------------------------------------------------------

/// The content database path: the `SCOUT_CONTENT_DB` env var when set, otherwise
/// `default_content_db_path()`. Only an entirely *unset* var falls back; an
/// explicitly empty value is used verbatim. The fallback is derived at runtime
/// from `$HOME` as `$HOME/.claude/scout/content.db`; when `HOME` is unset it
/// degrades to a bare cwd-relative `content.db`. Set `SCOUT_CONTENT_DB` to pick
/// the path explicitly.
pub fn content_db_path() -> PathBuf {
    match env::var("SCOUT_CONTENT_DB") {
        Ok(v) => PathBuf::from(v),
        Err(_) => default_content_db_path(),
    }
}

fn default_content_db_path() -> PathBuf {
    match env::var("HOME") {
        Ok(home) => PathBuf::from(home).join(".claude").join("scout").join("content.db"),
        Err(_) => PathBuf::from("content.db"),
    }
}

/// Opens the content database with the same WAL + busy_timeout=5000 treatment
/// and the same one-time agent-scope migration as `open_store`, applied to the
/// single `content` table.
pub fn open_content_store() -> rusqlite::Result<Connection> {
    let conn = Connection::open(content_db_path())?;
    conn.execute_batch("PRAGMA journal_mode = WAL")?;
    conn.execute_batch("PRAGMA busy_timeout = 5000")?;
    conn.execute_batch(&format!("CREATE TABLE IF NOT EXISTS content {CONTENT_SCHEMA};"))?;
    add_agent_scope(&conn, "content", CONTENT_SCHEMA, &CONTENT_COLS)?;
    Ok(conn)
}

/// A content-dedup row (`lookup_content`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentRow {
    pub session_id: String,
    pub agent_id: String,
    pub sha256: String,
    pub root: String,
    pub rel_path: String,
    pub size: i64,
    pub lines: i64,
    pub stub_count: i64,
}

pub fn lookup_content(conn: &Connection, session_id: &str, sha256: &str, agent_id: &str) -> rusqlite::Result<Option<ContentRow>> {
    conn.query_row(
        "SELECT session_id, agent_id, sha256, root, rel_path, size, lines, stub_count FROM content WHERE session_id = ? AND agent_id = ? AND sha256 = ?",
        params![session_id, agent_id, sha256],
        |row| {
            Ok(ContentRow {
                session_id: row.get(0)?,
                agent_id: row.get(1)?,
                sha256: row.get(2)?,
                root: row.get(3)?,
                rel_path: row.get(4)?,
                size: row.get(5)?,
                lines: row.get(6)?,
                stub_count: row.get(7)?,
            })
        },
    )
    .optional()
}

/// Arguments for `record_content`. The first path to carry this content keeps
/// the naming rights: `ON CONFLICT DO NOTHING`.
pub struct RecordContent<'a> {
    pub session_id: &'a str,
    pub agent_id: &'a str,
    pub sha256: &'a str,
    pub root: &'a str,
    pub rel_path: &'a str,
    pub size: i64,
    pub lines: i64,
}

pub fn record_content(conn: &Connection, p: &RecordContent) -> rusqlite::Result<()> {
    let now = now_ms();
    with_busy_retry(|| {
        conn.execute(
            "INSERT INTO content (session_id, agent_id, sha256, root, rel_path, size, lines, stub_count, first_seen_ts, last_read_ts)
             VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?)
             ON CONFLICT(session_id, agent_id, sha256) DO NOTHING",
            params![p.session_id, p.agent_id, p.sha256, p.root, p.rel_path, p.size, p.lines, now, now],
        )?;
        Ok(())
    })
}

/// Records another stubbed (cache-hit) read of content by digest.
pub fn bump_content_stub(conn: &Connection, session_id: &str, sha256: &str, agent_id: &str) -> rusqlite::Result<()> {
    let now = now_ms();
    with_busy_retry(|| {
        conn.execute(
            "UPDATE content SET stub_count = stub_count + 1, last_read_ts = ? WHERE session_id = ? AND agent_id = ? AND sha256 = ?",
            params![now, session_id, agent_id, sha256],
        )?;
        Ok(())
    })
}

/// Aggregate content-dedup statistics (`content_stats_for`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentStats {
    pub distinct_contents: i64,
    pub total_stubs: i64,
    pub bytes_saved: i64,
}

pub fn content_stats_for(conn: &Connection) -> rusqlite::Result<ContentStats> {
    conn.query_row(
        "SELECT COUNT(*) AS n, COALESCE(SUM(stub_count),0) AS stubs, COALESCE(SUM(size*stub_count),0) AS bytes_saved FROM content",
        [],
        |row| {
            Ok(ContentStats {
                distinct_contents: row.get(0)?,
                total_stubs: row.get(1)?,
                bytes_saved: row.get(2)?,
            })
        },
    )
}

// ---------------------------------------------------------------------------
// Unit tests -- retry helper, pragma read-backs, migration on a pre-migration
// fixture, and a same-process round-trip smoke test per table. Cross-process
// interop, concurrency, and schema comparison live in the integration suite,
// which needs to shell out to a separate process and does not belong in a
// `#[cfg(test)]` unit block.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    static CONTENT_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = env::temp_dir().join(format!("scout-store-rs-{prefix}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn repo() -> PathBuf {
        let root = unique_temp_dir("repo");
        std::fs::create_dir_all(root.join(".scout")).unwrap();
        root
    }

    // -- with_busy_retry ----------------------------------------------------

    fn busy_err() -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error { code: ErrorCode::DatabaseBusy, extended_code: 5 },
            Some("database is locked".to_string()),
        )
    }

    fn locked_err() -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error { code: ErrorCode::DatabaseLocked, extended_code: 6 },
            Some("database is locked".to_string()),
        )
    }

    fn other_err() -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error { code: ErrorCode::Unknown, extended_code: 1 },
            Some("near \"SELCT\": syntax error".to_string()),
        )
    }

    #[test]
    fn retries_on_busy_then_succeeds() {
        let calls = std::cell::Cell::new(0);
        let result = with_busy_retry_opts(5, 0, || {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                Err(busy_err())
            } else {
                Ok("ok")
            }
        });
        assert_eq!(result.unwrap(), "ok");
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn retries_on_locked_then_succeeds() {
        let calls = std::cell::Cell::new(0);
        let result = with_busy_retry_opts(5, 0, || {
            calls.set(calls.get() + 1);
            if calls.get() < 2 {
                Err(locked_err())
            } else {
                Ok("ok")
            }
        });
        assert_eq!(result.unwrap(), "ok");
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn other_error_rethrows_immediately_no_retry() {
        let calls = std::cell::Cell::new(0);
        let result: rusqlite::Result<()> = with_busy_retry_opts(5, 0, || {
            calls.set(calls.get() + 1);
            Err(other_err())
        });
        assert!(result.is_err());
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn exhaustion_rethrows_the_last_busy_error() {
        let calls = std::cell::Cell::new(0);
        let result: rusqlite::Result<()> = with_busy_retry_opts(3, 0, || {
            calls.set(calls.get() + 1);
            Err(busy_err())
        });
        assert_eq!(calls.get(), 3);
        match result {
            Err(rusqlite::Error::SqliteFailure(e, _)) => assert_eq!(e.code, ErrorCode::DatabaseBusy),
            other => panic!("expected a busy SqliteFailure, got {other:?}"),
        }
    }

    // -- pragma read-backs ----------------------------------------------

    #[test]
    fn open_store_enables_wal_and_5s_busy_timeout() {
        let conn = open_store(&repo()).unwrap();
        let journal_mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(journal_mode, "wal");
        let timeout: i64 = conn.query_row("PRAGMA busy_timeout", [], |r| r.get(0)).unwrap();
        assert_eq!(timeout, 5000);
    }

    #[test]
    fn open_content_store_enables_wal_and_5s_busy_timeout() {
        let _guard = CONTENT_ENV_LOCK.lock().unwrap();
        let dir = unique_temp_dir("content-pragma");
        env::set_var("SCOUT_CONTENT_DB", dir.join("content.db"));
        let conn = open_content_store().unwrap();
        let journal_mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(journal_mode, "wal");
        let timeout: i64 = conn.query_row("PRAGMA busy_timeout", [], |r| r.get(0)).unwrap();
        assert_eq!(timeout, 5000);
        env::remove_var("SCOUT_CONTENT_DB");
    }

    // -- migration on a pre-migration fixture ----------------------------

    // Builds a `reads` table shaped like the database BEFORE agent scoping
    // and the read_count/bytes_delivered spend columns existed (the historical
    // starting point `add_agent_scope`'s doc comment describes), then opens it
    // through `open_store` and asserts every
    // migration branch fired: agent_id present (defaulted to '' on old
    // rows), last_read_ts present, read_count/bytes_delivered backfilled to
    // the provable floor (1 / size) for a row that carries a real sha256.
    #[test]
    fn open_store_migrates_a_pre_agent_scope_pre_spend_fixture() {
        let root = repo();
        let db_path = repo::scout_dir(&root).join("cache.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE reads (
                    session_id TEXT NOT NULL,
                    rel_path TEXT NOT NULL,
                    sha256 TEXT NOT NULL,
                    size INTEGER NOT NULL,
                    mtime INTEGER NOT NULL,
                    first_seen_ts INTEGER NOT NULL,
                    lines INTEGER NOT NULL,
                    stub_count INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (session_id, rel_path)
                )",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO reads (session_id, rel_path, sha256, size, mtime, first_seen_ts, lines, stub_count)
                 VALUES ('legacy-sess', 'old.ts', 'deadbeef', 42, 100, 900, 7, 3)",
                [],
            )
            .unwrap();
        }

        let conn = open_store(&root).unwrap();
        let cols = columns(&conn, "reads").unwrap();
        assert!(cols.iter().any(|c| c == "agent_id"));
        assert!(cols.iter().any(|c| c == "last_read_ts"));
        assert!(cols.iter().any(|c| c == "read_count"));
        assert!(cols.iter().any(|c| c == "bytes_delivered"));

        let row = lookup_read(&conn, "legacy-sess", "old.ts", "").unwrap().expect("migrated row present under agent_id ''");
        assert_eq!(row.sha256, "deadbeef");
        assert_eq!(row.stub_count, 3);

        let (read_count, bytes_delivered): (i64, i64) = conn
            .query_row(
                "SELECT read_count, bytes_delivered FROM reads WHERE session_id = 'legacy-sess' AND rel_path = 'old.ts'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(read_count, 1, "backfilled to the provable floor, not left at 0");
        assert_eq!(bytes_delivered, 42, "backfilled to size for a row with a real sha256");
    }

    #[test]
    fn open_content_store_migrates_a_pre_agent_scope_fixture() {
        let _guard = CONTENT_ENV_LOCK.lock().unwrap();
        let dir = unique_temp_dir("content-migrate");
        let db_path = dir.join("content.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE content (
                    session_id TEXT NOT NULL,
                    sha256 TEXT NOT NULL,
                    root TEXT NOT NULL,
                    rel_path TEXT NOT NULL,
                    size INTEGER NOT NULL,
                    lines INTEGER NOT NULL,
                    stub_count INTEGER NOT NULL DEFAULT 0,
                    first_seen_ts INTEGER NOT NULL,
                    last_read_ts INTEGER,
                    PRIMARY KEY (session_id, sha256)
                )",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO content (session_id, sha256, root, rel_path, size, lines, stub_count, first_seen_ts, last_read_ts)
                 VALUES ('legacy-sess', 'cafef00d', '/r1', 'old.ts', 10, 2, 1, 900, 900)",
                [],
            )
            .unwrap();
        }
        env::set_var("SCOUT_CONTENT_DB", &db_path);
        let conn = open_content_store().unwrap();
        let cols = columns(&conn, "content").unwrap();
        assert!(cols.iter().any(|c| c == "agent_id"));
        let row = lookup_content(&conn, "legacy-sess", "cafef00d", "").unwrap().expect("migrated row present under agent_id ''");
        assert_eq!(row.root, "/r1");
        assert_eq!(row.rel_path, "old.ts");
        env::remove_var("SCOUT_CONTENT_DB");
    }

    // -- round-trip smoke tests, one per table --------------------------

    #[test]
    fn reads_round_trip() {
        let conn = open_store(&repo()).unwrap();
        assert!(lookup_read(&conn, "s1", "a.ts", "").unwrap().is_none());
        record_fresh(
            &conn,
            &RecordFresh { session_id: "s1", agent_id: "", rel_path: "a.ts", sha256: "abc", size: 3, mtime: 100, lines: 1, delivered: true },
        )
        .unwrap();
        let row = lookup_read(&conn, "s1", "a.ts", "").unwrap().unwrap();
        assert_eq!(row.sha256, "abc");
        assert_eq!(row.stub_count, 0);
        bump_stub(&conn, "s1", "a.ts", "").unwrap();
        bump_stub(&conn, "s1", "a.ts", "").unwrap();
        assert_eq!(lookup_read(&conn, "s1", "a.ts", "").unwrap().unwrap().stub_count, 2);
        record_fresh(
            &conn,
            &RecordFresh { session_id: "s1", agent_id: "", rel_path: "a.ts", sha256: "def", size: 4, mtime: 200, lines: 1, delivered: true },
        )
        .unwrap();
        let row = lookup_read(&conn, "s1", "a.ts", "").unwrap().unwrap();
        assert_eq!(row.stub_count, 0);
        assert_eq!(row.sha256, "def");
    }

    #[test]
    fn sessions_are_isolated() {
        let conn = open_store(&repo()).unwrap();
        record_fresh(
            &conn,
            &RecordFresh { session_id: "s1", agent_id: "", rel_path: "a.ts", sha256: "abc", size: 3, mtime: 100, lines: 1, delivered: true },
        )
        .unwrap();
        assert!(lookup_read(&conn, "s2", "a.ts", "").unwrap().is_none());
    }

    #[test]
    fn stats_for_counts_distinct_files_and_total_stubs() {
        let conn = open_store(&repo()).unwrap();
        record_fresh(&conn, &RecordFresh { session_id: "s1", agent_id: "", rel_path: "a.ts", sha256: "abc", size: 3, mtime: 100, lines: 1, delivered: true }).unwrap();
        record_fresh(&conn, &RecordFresh { session_id: "s1", agent_id: "", rel_path: "b.ts", sha256: "xyz", size: 3, mtime: 100, lines: 1, delivered: true }).unwrap();
        bump_stub(&conn, "s1", "a.ts", "").unwrap();
        let s = stats_for(&conn).unwrap();
        assert_eq!(s.distinct_files, 2);
        assert_eq!(s.total_stubs, 1);
    }

    #[test]
    fn record_spend_never_produces_a_matchable_sha_and_tracks_only_spend() {
        let conn = open_store(&repo()).unwrap();
        record_spend(&conn, &RecordSpend { session_id: "s1", agent_id: "", rel_path: "big.ts", size: 500 }).unwrap();
        let row = lookup_read(&conn, "s1", "big.ts", "").unwrap().unwrap();
        assert_eq!(row.sha256, "");
        assert_eq!(row.stub_count, 0);
    }

    // `delivered: false` is the cross-repo content-stub caller: the row is
    // written (so a later repeat takes the cheap path cache) but the bytes
    // never reached the model, so bytes_delivered must not move even though
    // read_count still does.
    #[test]
    fn record_fresh_not_delivered_writes_the_row_without_moving_bytes_delivered() {
        let conn = open_store(&repo()).unwrap();
        record_fresh(
            &conn,
            &RecordFresh { session_id: "s1", agent_id: "", rel_path: "cross.ts", sha256: "abc", size: 999, mtime: 1, lines: 3, delivered: false },
        )
        .unwrap();
        let (read_count, bytes_delivered): (i64, i64) = conn
            .query_row(
                "SELECT read_count, bytes_delivered FROM reads WHERE session_id = 's1' AND rel_path = 'cross.ts'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(read_count, 1);
        assert_eq!(bytes_delivered, 0, "delivered=false must never move bytes_delivered");

        // A subsequent delivered=true call accumulates on top, matching the
        // ON CONFLICT arithmetic used by every caller regardless of delivered.
        record_fresh(
            &conn,
            &RecordFresh { session_id: "s1", agent_id: "", rel_path: "cross.ts", sha256: "abc", size: 999, mtime: 2, lines: 3, delivered: true },
        )
        .unwrap();
        let (read_count, bytes_delivered): (i64, i64) = conn
            .query_row(
                "SELECT read_count, bytes_delivered FROM reads WHERE session_id = 's1' AND rel_path = 'cross.ts'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(read_count, 2);
        assert_eq!(bytes_delivered, 999);
    }

    #[test]
    fn bash_round_trip() {
        let conn = open_store(&repo()).unwrap();
        assert!(lookup_bash(&conn, "s1", "cat f.ts", "").unwrap().is_none());
        record_bash_fresh(&conn, &RecordBashFresh { session_id: "s1", agent_id: "", cache_key: "cat f.ts", sha256: "abc", size: 3, lines: 1 }).unwrap();
        let row = lookup_bash(&conn, "s1", "cat f.ts", "").unwrap().unwrap();
        assert_eq!(row.sha256, "abc");
        bump_bash_stub(&conn, "s1", "cat f.ts", "").unwrap();
        assert_eq!(lookup_bash(&conn, "s1", "cat f.ts", "").unwrap().unwrap().stub_count, 1);
        let stats = bash_stats_for(&conn).unwrap();
        assert_eq!(stats.commands_tracked, 1);
        assert_eq!(stats.total_stubs, 1);
        assert_eq!(stats.bytes_saved, 3);
    }

    #[test]
    fn content_round_trip_and_conflict_keeps_first_path() {
        let _guard = CONTENT_ENV_LOCK.lock().unwrap();
        let dir = unique_temp_dir("content-roundtrip");
        env::set_var("SCOUT_CONTENT_DB", dir.join("content.db"));
        let conn = open_content_store().unwrap();
        assert!(lookup_content(&conn, "s", "dup", "").unwrap().is_none());
        record_content(&conn, &RecordContent { session_id: "s", agent_id: "", sha256: "dup", root: "/r1", rel_path: "first.ts", size: 10, lines: 1 }).unwrap();
        record_content(&conn, &RecordContent { session_id: "s", agent_id: "", sha256: "dup", root: "/r2", rel_path: "second.ts", size: 10, lines: 1 }).unwrap();
        let hit = lookup_content(&conn, "s", "dup", "").unwrap().unwrap();
        assert_eq!(hit.rel_path, "first.ts");
        assert_eq!(hit.root, "/r1");
        bump_content_stub(&conn, "s", "dup", "").unwrap();
        let stats = content_stats_for(&conn).unwrap();
        assert_eq!(stats.distinct_contents, 1);
        assert_eq!(stats.total_stubs, 1);
        env::remove_var("SCOUT_CONTENT_DB");
    }

    #[test]
    fn content_db_path_uses_runtime_home_and_honours_override() {
        let _guard = CONTENT_ENV_LOCK.lock().unwrap();
        let old_home = env::var_os("HOME");
        let old_override = env::var_os("SCOUT_CONTENT_DB");
        let dir = unique_temp_dir("content-path");
        let target = dir.join("content.db");

        env::remove_var("SCOUT_CONTENT_DB");
        env::set_var("HOME", &dir);
        assert_eq!(content_db_path(), dir.join(".claude/scout/content.db"));

        env::set_var("SCOUT_CONTENT_DB", &target);
        assert_eq!(content_db_path(), target);

        match old_override {
            Some(value) => env::set_var("SCOUT_CONTENT_DB", value),
            None => env::remove_var("SCOUT_CONTENT_DB"),
        }
        match old_home {
            Some(value) => env::set_var("HOME", value),
            None => env::remove_var("HOME"),
        }
    }

    #[test]
    fn prune_by_session_and_by_age() {
        let conn = open_store(&repo()).unwrap();
        record_fresh(&conn, &RecordFresh { session_id: "s1", agent_id: "", rel_path: "a.ts", sha256: "abc", size: 3, mtime: 100, lines: 1, delivered: true }).unwrap();
        record_fresh(&conn, &RecordFresh { session_id: "s2", agent_id: "", rel_path: "b.ts", sha256: "def", size: 3, mtime: 100, lines: 1, delivered: true }).unwrap();
        let deleted = prune(&conn, None, Some("s1")).unwrap();
        assert_eq!(deleted, 1);
        assert!(lookup_read(&conn, "s1", "a.ts", "").unwrap().is_none());
        assert!(lookup_read(&conn, "s2", "b.ts", "").unwrap().is_some());

        // olderThanDays with a cutoff before any row's timestamp deletes nothing.
        let deleted = prune(&conn, Some(9999.0), None).unwrap();
        assert_eq!(deleted, 0);
        // A negative window (cutoff in the future) deletes everything remaining.
        let deleted = prune(&conn, Some(-1.0), None).unwrap();
        assert_eq!(deleted, 1);
    }

    #[test]
    fn session_ids_by_prefix_matches() {
        let conn = open_store(&repo()).unwrap();
        record_fresh(&conn, &RecordFresh { session_id: "abc123", agent_id: "", rel_path: "a.ts", sha256: "x", size: 1, mtime: 1, lines: 1, delivered: true }).unwrap();
        record_fresh(&conn, &RecordFresh { session_id: "abcxyz", agent_id: "", rel_path: "b.ts", sha256: "y", size: 1, mtime: 1, lines: 1, delivered: true }).unwrap();
        record_fresh(&conn, &RecordFresh { session_id: "zzz999", agent_id: "", rel_path: "c.ts", sha256: "z", size: 1, mtime: 1, lines: 1, delivered: true }).unwrap();
        let mut matches = session_ids_by_prefix(&conn, "abc").unwrap();
        matches.sort();
        assert_eq!(matches, vec!["abc123".to_string(), "abcxyz".to_string()]);
    }

    #[test]
    fn top_stubbed_orders_by_stub_count_desc_then_path() {
        let conn = open_store(&repo()).unwrap();
        record_fresh(&conn, &RecordFresh { session_id: "s1", agent_id: "", rel_path: "a.ts", sha256: "x", size: 1, mtime: 1, lines: 5, delivered: true }).unwrap();
        record_fresh(&conn, &RecordFresh { session_id: "s1", agent_id: "", rel_path: "b.ts", sha256: "y", size: 1, mtime: 1, lines: 5, delivered: true }).unwrap();
        bump_stub(&conn, "s1", "a.ts", "").unwrap();
        bump_stub(&conn, "s1", "b.ts", "").unwrap();
        bump_stub(&conn, "s1", "b.ts", "").unwrap();
        let top = top_stubbed(&conn, 10).unwrap();
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].rel_path, "b.ts");
        assert_eq!(top[0].stub_count, 2);
        assert_eq!(top[1].rel_path, "a.ts");
    }

    #[test]
    fn path_breakdown_excludes_rows_with_no_reads() {
        let conn = open_store(&repo()).unwrap();
        // record_spend gives read_count=1 immediately, so no zero-read row
        // is producible through the public write API alone -- insert a raw
        // shell row directly to prove the WHERE clause excludes it.
        conn.execute(
            "INSERT INTO reads (session_id, agent_id, rel_path, sha256, size, mtime, first_seen_ts, lines, stub_count, read_count, bytes_delivered)
             VALUES ('s1', '', 'inert.ts', '', 0, 0, 1, 0, 0, 0, 0)",
            [],
        )
        .unwrap();
        record_fresh(&conn, &RecordFresh { session_id: "s1", agent_id: "", rel_path: "active.ts", sha256: "abc", size: 1, mtime: 1, lines: 1, delivered: true }).unwrap();
        let rows = path_breakdown(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].rel_path, "active.ts");
    }
}
