// Arg parsing + dispatch for the subcommand set. Dispatch is hand-rolled; no
// flag-parsing dependency is pulled in until a subcommand needs real flag
// parsing.
//
// Alongside the user-facing verbs (README.md), four dev/diagnostic subcommands
// are wired here and nowhere else: `noop` (cold-start floor), `parse` and
// `spans` (seed AST dumps), and `extract-dump` (full-extraction JSON).
//
// `init` dispatches through `initcmd::cmd_init_full`, NOT `cmd_init` directly --
// see initcmd.rs for why that split is load-bearing rather than stylistic.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process;

use crate::audit;
use crate::extract;
use crate::graph;
use crate::hookio;
use crate::initcmd;
use crate::manifest;
use crate::mapcmd;
use crate::parse;
use crate::query;
use crate::query::json::J;
use crate::render;
use crate::store;
use crate::suggest;

// Command output writer. `println!` PANICS on a broken pipe ("failed printing to
// stdout"), which would kill `devscout find | head` noisily the moment `head`
// closed its end. Exit silently there instead: write through `write_all` and
// swallow the error -- on stdout that error is effectively only EPIPE, and the
// exit code the command already computed still gets delivered by the caller's
// `process::exit`.
fn print_out(out: &str) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    let _ = lock
        .write_all(out.as_bytes())
        .and_then(|()| lock.write_all(b"\n"));
}

// A zero-hit answer is neither success nor failure: the index was read and the
// answer is empty. Distinct from 1 (environment, refusal) and 2 (usage) so a
// caller can branch on it instead of rephrasing the query.
const EXIT_NO_RESULT: i32 = 3;

// These four notes go to STDERR, never stdout, so a zero hit leaves stdout
// exactly as empty as it was -- a caller parsing stdout sees no difference, and
// the note is advice for a human reader.
const ZERO_HIT_FIND: &str = "devscout find: zero hits — the manifest was searched and nothing matched. Not an error; fall back to text search (rg/grep) rather than rephrasing.";
const ZERO_HIT_REFS: &str = "devscout refs: zero hits — the graph was searched and no symbol matched. Not an error; fall back to text search (rg/grep) rather than rephrasing.";
const ZERO_HIT_READ: &str = "devscout read: zero hits — the graph was searched and no symbol matched. Not an error; fall back to text search (rg/grep) rather than rephrasing.";
const ZERO_HIT_IMPACT: &str = "devscout impact: zero hits — the graph was searched and no affected file came back. Not an error; fall back to text search (rg/grep) rather than rephrasing.";
const ZERO_HIT_TESTS: &str = "devscout tests: zero hits — the graph was searched and no symbol matched. Not an error; fall back to text search (rg/grep) rather than rephrasing.";

// The one place a zero-hit line is emitted. The "did you mean" candidates extend
// that same note rather than adding a second one; `query` carries the name the
// caller asked for on the two verbs that offer them and is `None` on the two that
// do not. Broken-pipe errors are swallowed for the same reason `print_out`
// swallows them.
fn emit_zero_hit_note(code: i32, note: &str, cwd: &Path, query: Option<&str>) {
    if code != EXIT_NO_RESULT {
        return;
    }
    let mut text = String::from(note);
    let rows = query.map(|q| nearest_names(cwd, q)).unwrap_or_default();
    if !rows.is_empty() {
        text.push_str("\ndid you mean:");
        for row in rows {
            text.push('\n');
            text.push_str(&row);
        }
    }
    let stderr = std::io::stderr();
    let mut lock = stderr.lock();
    let _ = lock
        .write_all(text.as_bytes())
        .and_then(|()| lock.write_all(b"\n"));
}

// Query-time index freshness, `find`/`refs`/`impact` only: `map` just rebuilt
// the index and has nothing to say about it being stale relative to itself. Root
// resolution mirrors `require_repo`'s plain climb, never
// `require_repo_for_path`'s argument-named-file fallback -- a query that only
// resolved its root through an argument gets no freshness check, silently, the
// safe default. Called BEFORE `emit_zero_hit_note` so the two lines land on
// stderr in a fixed order when both fire for the same query. Never touches
// stdout, never changes the exit code.
fn emit_freshness_warning(cwd: &Path) {
    let Some(root) = crate::repo::find_scout_root(cwd).or_else(|| crate::repo::find_repo_root(cwd))
    else {
        return;
    };
    let Some(warning) = manifest::freshness_warning(&root) else {
        return;
    };
    let stderr = std::io::stderr();
    let mut lock = stderr.lock();
    let _ = lock
        .write_all(warning.as_bytes())
        .and_then(|()| lock.write_all(b"\n"));
}

// The nearest names a zero-hit `find`/`refs` offers, never substituted for the
// query and never run. The graph is read here rather than carried out of the
// command that just ran: this path is reached only once that command has decided
// it has nothing to print.
fn nearest_names(cwd: &Path, query: &str) -> Vec<String> {
    let Ok(root) = require_repo(cwd) else {
        return Vec::new();
    };
    let Some(g) = graph::read_graph(&root) else {
        return Vec::new();
    };
    suggest::suggestion_lines(&g.names, query)
}

// The first non-flag argument -- the symbol `refs`/`read`/`tests` take.
// Shared with `dispatch`, which needs the same string to build the suggestion
// block for a query `cmd_refs`/`cmd_read` has already reported as a zero hit.
// Skips `--pick`'s OWN value: unlike every other flag these three verbs
// accept, `--pick N` carries a value that does not start with `--` and so
// would otherwise be mistaken for the query itself.
fn first_positional(args: &[String]) -> Option<&str> {
    let mut skip_next = false;
    for a in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if a == "--pick" {
            skip_next = true;
            continue;
        }
        if !a.starts_with("--") {
            return Some(a.as_str());
        }
    }
    None
}

// `devscout help` / `--help`. Written to stdout with exit 0 so a shell
// `devscout --help` is a success, unlike the usage line the unknown-command
// arm sends to stderr.
const HELP: &str = "\
devscout -- fast code index for C# and TypeScript codebases.

usage: devscout <command> [args]

index
  init [scope ...]           register this repo, install hooks, first map
                             [--label L] [--no-hooks] [--no-map]
  map [scope ...] [--refresh]  build the index for the given scopes
  clear --older-than <days>  drop freshness rows older than N days
  clear --session <id>       drop one session's freshness rows

query
  find <query> [--resources] search the manifest by name or purpose
  refs <symbol>              references to a symbol   [--out --all --no-guess --pick N --json|--compact]
  read <symbol>              decl span + inbound refs [--no-guess --pick N --json|--compact]
  impact <file|symbol>       blast radius             [--hops N --no-guess --pick N --json|--compact]
  tests <symbol>             tests reaching a symbol  [--no-guess --pick N --json|--compact]
  stats                      index + cache summary for this repo

plumbing
  parse <file.cs>            dump the parse tree
  spans <file.cs>            dump declaration spans
  extract-dump <file.cs>     dump extraction records
  audit --semantic <refs.jsonl>  score uses-member edges against a semantic oracle [--units F] [--defs F] [--json] [--assert F]
  hook <read|bash>           agent hook filters, stdin -> stdout
  noop                       exit 0 (harness probe)

  -C <dir>                   run as if devscout started in <dir>
  -h, --help                 show this help
  -V, --version              show the version
";

/// Parses and executes a `devscout` command from its argument vector.
#[allow(
    clippy::too_many_lines,
    reason = "one flat match over every subcommand; splitting it would scatter the dispatch table across files"
)]
pub fn dispatch(args: Vec<String>) {
    let (cwd, args) = match apply_global_options(&current_dir(), &args) {
        Ok(v) => v,
        Err(e) => {
            print_out(&format!("error: {e}"));
            process::exit(1);
        }
    };
    match args.get(1).map(String::as_str) {
        Some("noop") => process::exit(0),
        Some("help") | Some("--help") | Some("-h") => {
            print!("{HELP}");
            process::exit(0);
        }
        Some("--version") | Some("-V") => {
            println!("devscout {}", env!("CARGO_PKG_VERSION"));
            process::exit(0);
        }
        Some("parse") => {
            let Some(path) = args.get(2) else {
                eprintln!("usage: devscout parse <file.cs>");
                process::exit(1);
            };
            parse::run_parse(path);
        }
        Some("spans") => {
            let Some(path) = args.get(2) else {
                eprintln!("usage: devscout spans <file.cs>");
                process::exit(1);
            };
            parse::run_spans(path);
        }
        Some("extract-dump") => {
            let Some(path) = args.get(2) else {
                eprintln!("usage: devscout extract-dump <file.cs>");
                process::exit(1);
            };
            extract::run_extract_dump(path);
        }
        Some("audit") => {
            let (code, out) = audit::cmd_audit(&cwd, &args[2..]);
            print_out(&out);
            process::exit(code);
        }
        Some("hook") => match args.get(2).map(String::as_str) {
            Some("read") => run_hook(hookio::run_read),
            Some("bash") => run_hook(hookio::run_bash),
            _ => {
                eprintln!("usage: devscout hook <read|bash>");
                process::exit(1);
            }
        },
        Some("refs") => {
            let (code, out) = cmd_refs(&cwd, &args[2..]);
            print_out(&out);
            emit_freshness_warning(&cwd);
            emit_zero_hit_note(code, ZERO_HIT_REFS, &cwd, first_positional(&args[2..]));
            process::exit(code);
        }
        Some("read") => {
            let (code, out) = cmd_read(&cwd, &args[2..]);
            print_out(&out);
            emit_freshness_warning(&cwd);
            emit_zero_hit_note(code, ZERO_HIT_READ, &cwd, first_positional(&args[2..]));
            process::exit(code);
        }
        Some("impact") => {
            let (code, out) = cmd_impact(&cwd, &args[2..]);
            print_out(&out);
            emit_freshness_warning(&cwd);
            emit_zero_hit_note(code, ZERO_HIT_IMPACT, &cwd, None);
            process::exit(code);
        }
        Some("tests") => {
            let (code, out) = cmd_tests(&cwd, &args[2..]);
            print_out(&out);
            emit_zero_hit_note(code, ZERO_HIT_TESTS, &cwd, None);
            process::exit(code);
        }
        Some("find") => {
            // `--resources` is parsed out first, then every remaining arg after
            // the subcommand becomes one space-joined query, same as before the
            // flag existed.
            let resources = args[2..].iter().any(|a| a == "--resources");
            let query_str = args[2..]
                .iter()
                .filter(|a| a.as_str() != "--resources")
                .cloned()
                .collect::<Vec<_>>()
                .join(" ");
            let (code, out) = cmd_find(&cwd, &query_str, resources);
            print_out(&out);
            emit_freshness_warning(&cwd);
            emit_zero_hit_note(code, ZERO_HIT_FIND, &cwd, Some(query_str.as_str()));
            process::exit(code);
        }
        Some("map") => {
            let (code, out) = cmd_map(&cwd, &args[2..]);
            print_out(&out);
            process::exit(code);
        }
        Some("stats") => {
            let (code, out) = cmd_stats(&cwd);
            print_out(&out);
            process::exit(code);
        }
        Some("clear") => {
            let (code, out) = cmd_clear(&cwd, &args[2..]);
            print_out(&out);
            process::exit(code);
        }
        Some("init") => {
            let (code, out) = initcmd::cmd_init_full(&cwd, &args[2..]);
            print_out(&out);
            process::exit(code);
        }
        _ => {
            eprintln!("usage: devscout <noop|parse|spans|extract-dump|hook|refs|read|impact|tests|find|map|stats|clear|init> [args]");
            process::exit(1);
        }
    }
}

// Shared `hook read`/`hook bash` plumbing: read all of stdin (best-effort -- a
// stdin read failure just means an empty/partial buffer goes into the fail-open
// decision path, same outcome), run the hook, write stdout only if non-empty,
// exit 0 unconditionally. Fail-open: a broken scout must never break the tool
// call whose result it is post-processing, so this function itself must never
// propagate a nonzero exit or a panic.
fn run_hook(handler: fn(&[u8]) -> Vec<u8>) {
    let mut buf = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut buf);
    let out = handler(&buf);
    if !out.is_empty() {
        let _ = std::io::stdout().write_all(&out);
    }
    process::exit(0);
}

// ---------------------------------------------------------------------------
// `refs`/`impact`/`find`. Each command function returns `(code, out)` directly;
// the few points that would otherwise be a thrown error (`require_repo`'s
// missing-root error, `find`'s corrupt-manifest error) build the same `"error:
// {msg}"` string by hand. All command output goes to stdout -- these commands
// never write to stderr; the dispatcher writes `out` and exits with `code`.
// ---------------------------------------------------------------------------

fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

// The repo root for `cwd`: an initialized `.scout` ancestor wins; otherwise fall
// back to the nearest `.git` ancestor. `Err` carries the message callers wrap as
// `"error: {message}"`.
//
// `pub(crate)`: `audit.rs`'s `cmd_audit` resolves its repo root the same way
// every other command here does.
pub(crate) fn require_repo(cwd: &Path) -> Result<PathBuf, String> {
    require_repo_for_path(cwd, None)
}

// Root resolution with a fallback to the verb's own path argument. `arg_path` is
// consulted only after the caller's directory has come up empty, so a caller
// sitting in a repo never has its root decided by an argument pointing outside
// it.
fn require_repo_for_path(cwd: &Path, arg_path: Option<&str>) -> Result<PathBuf, String> {
    crate::repo::find_scout_root(cwd)
        .or_else(|| crate::repo::find_repo_root(cwd))
        .or_else(|| root_from_arg(cwd, arg_path))
        .ok_or_else(|| {
            "no .scout or .git ancestor; run 'devscout init' from the repo or directory root"
                .to_string()
        })
}

// Only a path-shaped argument naming something that is actually on disk is
// allowed to decide a root; a symbol, or a repo-relative path that means nothing
// from where the caller stands, resolves to a directory that has no bearing on
// the query and is refused.
fn root_from_arg(cwd: &Path, arg_path: Option<&str>) -> Option<PathBuf> {
    let arg_path = arg_path?;
    if !query::looks_like_file_path(arg_path) {
        return None;
    }
    let abs = crate::repo::resolve_from(cwd, Path::new(arg_path));
    if !abs.exists() {
        return None;
    }
    crate::repo::find_scout_root(&abs).or_else(|| crate::repo::find_repo_root(&abs))
}

// The manifest and the graph key files by their exact repo-relative path, so an
// absolute or subdirectory-relative argument is a guaranteed miss; rewriting one
// that names a real path inside the root can only turn that miss into an answer.
// Everything else -- a symbol, a path that is not on disk, a path outside the
// root -- is handed back untouched, so output matches the same query run from the
// root itself.
fn repo_relative_arg(cwd: &Path, root: &Path, arg: &str) -> String {
    if !query::looks_like_file_path(arg) {
        return arg.to_string();
    }
    let abs = crate::repo::resolve_from(cwd, Path::new(arg));
    if !abs.exists() {
        return arg.to_string();
    }
    match abs.strip_prefix(root) {
        Ok(rest) if !rest.as_os_str().is_empty() => crate::repo::rel_path(root, &abs),
        _ => arg.to_string(),
    }
}

// `-C <dir>`, git's own semantics: every root resolution and every relative path
// argument below reads as if the process had started in `<dir>`. Consumed before
// the subcommand, so it composes on repeat exactly as git's does. `args[0]` (the
// program name) is carried through untouched, keeping every caller's argument
// indices as they were.
fn apply_global_options(cwd: &Path, args: &[String]) -> Result<(PathBuf, Vec<String>), String> {
    let mut cwd = cwd.to_path_buf();
    let mut idx = 1;
    while args.get(idx).map(String::as_str) == Some("-C") {
        let Some(dir) = args.get(idx + 1) else {
            return Err("no directory given for '-C' option".to_string());
        };
        let next = crate::repo::resolve_from(&cwd, Path::new(dir));
        if !next.is_dir() {
            return Err(format!("cannot change to '{dir}': no such directory"));
        }
        cwd = next;
        idx += 2;
    }
    if idx == 1 {
        return Ok((cwd, args.to_vec()));
    }
    let mut rest = vec![args[0].clone()];
    rest.extend_from_slice(&args[idx..]);
    Ok((cwd, rest))
}

// Shared by `cmd_refs`/`cmd_impact` -- the "never guess" house rule: print every
// candidate's `{id, def site, kind}` and exit 1, regardless of
// `--compact`/`--json` (see `cmd_refs`/`cmd_impact`: this is reached BEFORE
// either flag is consulted).
fn ambiguous_candidates_out(index: &query::GraphIndex, q: &str, ids: &[String]) -> (i32, String) {
    let mut rows: Vec<String> = ids
        .iter()
        .map(|id| {
            // Every id here was sourced from `index.by_simple_name`/
            // `by_lower_name`, both built from `index.by_id`'s own keys during
            // construction -- `index.def(id)` cannot miss. `.expect` fails loud
            // if that invariant ever broke.
            let d = index
                .def(id)
                .expect("ambiguous candidate id must resolve to a graph def");
            format!("{id}  {}:{}  {}", d.file, d.line, d.kind)
        })
        .collect();
    // `Vec<String>::sort` compares by UTF-8 byte order, which for the ASCII
    // identifier/path/kind text these rows are built from is a stable, total
    // order (the same seam resolve.rs's candidate sort documents).
    rows.sort();
    let mut out = vec![format!(
        "ambiguous symbol \"{q}\" — {} candidates:",
        rows.len()
    )];
    out.append(&mut rows);
    (1, out.join("\n"))
}

// Renders a member-ambiguous outcome (`RefsResult::MemberAmbiguous` and its
// mirrors on `read`/`impact`/`tests`): one row per candidate, its declaring
// type, file and line -- NEVER the bare `{id, def site, kind}` list
// `ambiguous_candidates_out` renders for an ambiguous TYPE name. Unlike that
// list, this one DOES respect `--json`: there was no JSON shape for a member
// answer to keep unchanged, so carrying `outcome` here is purely additive.
fn member_ambiguous_out(
    q: &str,
    candidates: &[query::MemberCandidate],
    json: bool,
) -> (i32, String) {
    if json {
        let out = J::Obj(vec![
            ("schema_version", J::UInt(query::SCHEMA_VERSION)),
            (
                "outcome",
                J::Str(query::Outcome::Ambiguous.as_str().to_string()),
            ),
            ("query", J::Str(q.to_string())),
            (
                "candidates",
                J::Arr(
                    candidates
                        .iter()
                        .map(|c| {
                            J::Obj(vec![
                                ("owner", J::Str(c.owner.clone())),
                                ("name", J::Str(c.name.clone())),
                                ("file", J::Str(c.file.clone())),
                                ("line", J::UInt(c.line as u64)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
        .to_json_string();
        return (1, out);
    }
    let mut out = vec![format!(
        "ambiguous member \"{q}\" — {} candidates:",
        candidates.len()
    )];
    for c in candidates {
        out.push(format!("{}.{}  {}:{}", c.owner, c.name, c.file, c.line));
    }
    (1, out.join("\n"))
}

// Renders a fully-unresolved query (`NotFound` on every one of `refs`/`read`/
// `impact`/`tests`): `plain` unchanged under text/`--compact` -- the exact
// bytes these verbs always printed here -- and, only under `--json`, a small
// object naming the outcome. No JSON shape existed for this case before, so
// this is additive the same way `member_ambiguous_out`'s JSON arm is.
fn fallback_advised_out(q: &str, json: bool, plain: String) -> (i32, String) {
    if json {
        let out = J::Obj(vec![
            ("schema_version", J::UInt(query::SCHEMA_VERSION)),
            (
                "outcome",
                J::Str(query::Outcome::FallbackAdvised.as_str().to_string()),
            ),
            ("query", J::Str(q.to_string())),
        ])
        .to_json_string();
        (EXIT_NO_RESULT, out)
    } else {
        (EXIT_NO_RESULT, plain)
    }
}

// `--pick <n>`, shared by `refs`/`read`/`impact`/`tests`: a strict positive
// integer (unlike `--hops`'s lenient `parse_int_js` -- a candidate index is
// never "close enough"). `Ok(None)` when the flag is absent, `Err(())` on a
// missing, non-numeric, or zero value, which every call site turns into its
// own usage error. Range-checking against the actual candidate count is the
// caller's job: that count is only known once resolution has already
// answered `MemberAmbiguous`.
fn parse_pick(args: &[String]) -> Result<Option<usize>, ()> {
    let Some(idx) = args.iter().position(|a| a == "--pick") else {
        return Ok(None);
    };
    let raw = args.get(idx + 1).map(String::as_str).unwrap_or("");
    match raw.parse::<usize>() {
        Ok(n) if n >= 1 => Ok(Some(n)),
        _ => Err(()),
    }
}

// Lenient integer parse: skip nothing but a leading sign, take the longest
// leading run of ASCII digits, ignore trailing garbage; no digits at all
// (including an absent/empty string) is `None`. Used by the `--hops`,
// `--iface-max-fanin` and `--hub-max-indegree` parsing, so a value like `"3abc"`
// parses to `3` rather than being a usage error.
fn parse_int_js(s: &str) -> Option<i64> {
    let s = s.trim_start();
    let bytes = s.as_bytes();
    let mut idx = 0;
    if idx < bytes.len() && (bytes[idx] == b'+' || bytes[idx] == b'-') {
        idx += 1;
    }
    let digits_start = idx;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == digits_start {
        return None;
    }
    s[..idx].parse::<i64>().ok()
}

// `--no-guess`, shared by the four verbs that read the graph index. Spelled
// once and read at INDEX BUILD rather than at each render site: a guess that
// never entered the adjacency cannot leak back out through a consumer that
// forgot to filter, which is the same reasoning that gives heuristic edges
// their own buckets in the first place.
//
// The flag is a plain presence test like `--json`/`--out`, so it takes no
// value and can never be mistaken for the query -- no usage-error branch of
// its own, which is why the four call sites below just call this.
fn index_options(args: &[String]) -> query::IndexOptions {
    query::IndexOptions {
        include_guesses: !args.iter().any(|a| a == "--no-guess"),
    }
}

// `refs`. Check order: `--compact`+`--json` conflict, then missing query, THEN
// `require_repo`, THEN the graph-present check -- a query run with no repo
// present reports the missing-repo error even if the query itself is also
// absent-adjacent.
const REFS_USAGE: &str =
    "usage: devscout refs <symbol> [--out] [--all] [--no-guess] [--pick N] [--json|--compact]";

fn cmd_refs(cwd: &Path, args: &[String]) -> (i32, String) {
    let json = args.iter().any(|a| a == "--json");
    let compact = args.iter().any(|a| a == "--compact");
    if json && compact {
        return (
            1,
            "devscout refs: --compact and --json are mutually exclusive".to_string(),
        );
    }
    let Ok(pick) = parse_pick(args) else {
        return (2, REFS_USAGE.to_string());
    };
    let Some(q) = first_positional(args) else {
        return (2, REFS_USAGE.to_string());
    };
    let out = args.iter().any(|a| a == "--out");
    // `--all` lifts only `query::OUTBOUND_CAP`; it is otherwise inert without
    // `--out`, same as `--out` is inert on the bare-member fallback path.
    let all_out = args.iter().any(|a| a == "--all");
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}")),
    };
    let Some(g) = graph::read_graph(&root) else {
        return (
            1,
            "no graph.json for this repo — run `devscout map` on a C# scope first".to_string(),
        );
    };
    let index = query::load_graph_index_with(&g, &root, index_options(args));

    let build = |seed: &str| {
        query::build_refs_model(
            &index,
            seed,
            out,
            query::DEFAULT_CAP,
            query::INBOUND_CAP,
            query::OUTBOUND_CAP,
            all_out,
        )
    };
    // `--pick n` only ever narrows a `MemberAmbiguous` answer: it re-resolves
    // against the SAME synthetic `Owner.Member` seed a caller could have typed
    // by hand, rather than adding a second resolution path only `--pick`
    // takes. An out-of-range `n` is a usage error; `--pick` is silently
    // inert on every other outcome.
    let result = match build(q) {
        query::RefsResult::MemberAmbiguous(candidates) => match pick {
            Some(n) if n <= candidates.len() => build(&query::qualified_seed(&candidates[n - 1])),
            Some(_) => return (2, REFS_USAGE.to_string()),
            None => query::RefsResult::MemberAmbiguous(candidates),
        },
        other => other,
    };

    match result {
        query::RefsResult::NotFound => {
            fallback_advised_out(q, json, format!("no symbol matches \"{q}\""))
        }
        query::RefsResult::Ambiguous(ids) => ambiguous_candidates_out(&index, q, &ids),
        query::RefsResult::MemberAmbiguous(candidates) => {
            member_ambiguous_out(q, &candidates, json)
        }
        // A bare member answers with one ordinary refs model per declaring type,
        // so each block renders through the very renderer a type uses and
        // `--json` wraps those same objects in an array rather than reshaping
        // them.
        query::RefsResult::Members(models) => {
            if json {
                (0, query::json::member_refs_to_json(q, &models))
            } else if compact {
                (
                    0,
                    models
                        .iter()
                        .map(render::render_refs_compact)
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            } else {
                (
                    0,
                    models
                        .iter()
                        .map(render::render_refs_text)
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            }
        }
        query::RefsResult::Resolved(model) => {
            if json {
                (0, query::json::refs_model_to_json(&model))
            } else if compact {
                (0, render::render_refs_compact(&model))
            } else {
                (0, render::render_refs_text(&model))
            }
        }
    }
}

// `read`. Check order is `refs`' own: flag conflict, missing query,
// `require_repo`, graph-present. The resolution IS refs' --
// `build_read_model` wraps `build_refs_model` and changes nothing about how
// a name becomes an answer -- so the ambiguity and zero-hit discipline
// cannot drift between the two verbs; only the resolved arm grows the
// declaration span.
const READ_USAGE: &str = "usage: devscout read <symbol> [--no-guess] [--pick N] [--json|--compact]";

fn cmd_read(cwd: &Path, args: &[String]) -> (i32, String) {
    let json = args.iter().any(|a| a == "--json");
    let compact = args.iter().any(|a| a == "--compact");
    if json && compact {
        return (
            1,
            "devscout read: --compact and --json are mutually exclusive".to_string(),
        );
    }
    let Ok(pick) = parse_pick(args) else {
        return (2, READ_USAGE.to_string());
    };
    let Some(q) = first_positional(args) else {
        return (2, READ_USAGE.to_string());
    };
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}")),
    };
    let Some(g) = graph::read_graph(&root) else {
        return (
            1,
            "no graph.json for this repo — run `devscout map` on a C# scope first".to_string(),
        );
    };
    let index = query::load_graph_index_with(&g, &root, index_options(args));

    // Same `--pick` narrowing rule `cmd_refs` applies: only a `MemberAmbiguous`
    // answer is ever re-resolved, against the synthetic `Owner.Member` seed the
    // picked candidate names.
    let result = match query::build_read_model(&index, q) {
        query::ReadResult::MemberAmbiguous(candidates) => match pick {
            Some(n) if n <= candidates.len() => {
                query::build_read_model(&index, &query::qualified_seed(&candidates[n - 1]))
            }
            Some(_) => return (2, READ_USAGE.to_string()),
            None => query::ReadResult::MemberAmbiguous(candidates),
        },
        other => other,
    };

    match result {
        query::ReadResult::NotFound => {
            fallback_advised_out(q, json, format!("no symbol matches \"{q}\""))
        }
        query::ReadResult::Ambiguous(ids) => ambiguous_candidates_out(&index, q, &ids),
        query::ReadResult::MemberAmbiguous(candidates) => {
            member_ambiguous_out(q, &candidates, json)
        }
        // A bare member answers through refs' own member rendering in all
        // three forms: the member answer carries no declaration-span fact to
        // add, so reshaping it here would only create a second shape to keep
        // in step.
        query::ReadResult::Members(models) => {
            if json {
                (0, query::json::member_refs_to_json(q, &models))
            } else if compact {
                (
                    0,
                    models
                        .iter()
                        .map(render::render_refs_compact)
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            } else {
                (
                    0,
                    models
                        .iter()
                        .map(render::render_refs_text)
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            }
        }
        query::ReadResult::Resolved(model) => {
            if json {
                (0, query::json::read_model_to_json(&model))
            } else if compact {
                (0, render::render_read_compact(&model))
            } else {
                (0, render::render_read_text(&model))
            }
        }
    }
}

const IMPACT_USAGE: &str = "usage: devscout impact <file|symbol> [--hops N] [--no-iface] [--no-guess] [--iface-max-fanin N] [--hub-max-indegree N] [--pick N] [--json|--compact]";

/// The numeric/positional argument parse `cmd_impact` needs before it can
/// touch the repo or the graph: `--hops`/`--iface-max-fanin`/
/// `--hub-max-indegree`/`--pick`, and the query -- the first non-flag
/// argument that is not one of those four flags' own values.
struct ImpactArgs<'a> {
    hops: u32,
    iface_max_fanin: usize,
    hub_max_indegree: usize,
    pick: Option<usize>,
    query: &'a str,
}

fn parse_impact_args(args: &[String]) -> Result<ImpactArgs<'_>, (i32, String)> {
    let usage_err = || (2, IMPACT_USAGE.to_string());

    let mut hops: u32 = query::DEFAULT_HOPS;
    if let Some(idx) = args.iter().position(|a| a == "--hops") {
        let raw = args.get(idx + 1).map(String::as_str).unwrap_or("");
        match parse_int_js(raw) {
            Some(h) if h >= 1 => hops = h as u32,
            _ => return Err(usage_err()),
        }
    }

    // `--iface-max-fanin`'s own value is never the query. `0` is a legal value
    // and does not start with `--`, so without the guard below it would be picked
    // up as the seed. Parsed AFTER `--hops` and BEFORE the missing-query check.
    let mut iface_max_fanin: usize = query::DEFAULT_IFACE_MAX_FANIN;
    if let Some(idx) = args.iter().position(|a| a == "--iface-max-fanin") {
        let raw = args.get(idx + 1).map(String::as_str).unwrap_or("");
        match parse_int_js(raw) {
            Some(n) if n >= 0 => iface_max_fanin = n as usize,
            _ => return Err(usage_err()),
        }
    }

    // The same rule as `--iface-max-fanin`: its own value is never the query, and
    // `0` is a legal value that does not start with `--`.
    let mut hub_max_indegree: usize = query::DEFAULT_HUB_MAX_INDEGREE;
    if let Some(idx) = args.iter().position(|a| a == "--hub-max-indegree") {
        let raw = args.get(idx + 1).map(String::as_str).unwrap_or("");
        match parse_int_js(raw) {
            Some(n) if n >= 0 => hub_max_indegree = n as usize,
            _ => return Err(usage_err()),
        }
    }

    let Ok(pick) = parse_pick(args) else {
        return Err(usage_err());
    };

    // The first non-flag argument that is not the value of `--hops`,
    // `--iface-max-fanin`, `--hub-max-indegree`, or `--pick`.
    let mut query: Option<&str> = None;
    for (i, a) in args.iter().enumerate() {
        if a.starts_with("--") {
            continue;
        }
        if i > 0
            && (args[i - 1] == "--hops"
                || args[i - 1] == "--iface-max-fanin"
                || args[i - 1] == "--hub-max-indegree"
                || args[i - 1] == "--pick")
        {
            continue;
        }
        query = Some(a.as_str());
        break;
    }
    let Some(query) = query else {
        return Err(usage_err());
    };

    Ok(ImpactArgs {
        hops,
        iface_max_fanin,
        hub_max_indegree,
        pick,
        query,
    })
}

// `impact`. Check order: `--compact`+`--json` conflict, `--hops` parse (usage
// error on a bad/missing value), missing query, THEN `require_repo`, THEN the
// graph-present check.
fn cmd_impact(cwd: &Path, args: &[String]) -> (i32, String) {
    let json = args.iter().any(|a| a == "--json");
    let compact = args.iter().any(|a| a == "--compact");
    if json && compact {
        return (
            1,
            "devscout impact: --compact and --json are mutually exclusive".to_string(),
        );
    }

    let ImpactArgs {
        hops,
        iface_max_fanin,
        hub_max_indegree,
        pick,
        query: q,
    } = match parse_impact_args(args) {
        Ok(a) => a,
        Err(e) => return e,
    };

    let root = match require_repo_for_path(cwd, Some(q)) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}")),
    };
    let q = repo_relative_arg(cwd, &root, q);
    let q = q.as_str();
    let Some(g) = graph::read_graph(&root) else {
        return (
            1,
            "no graph.json for this repo — run `devscout map` on a C# scope first".to_string(),
        );
    };
    let index = query::load_graph_index_with(&g, &root, index_options(args));

    let build = |seed: &str| {
        query::build_impact_model(
            &index,
            seed,
            hops,
            query::DEFAULT_CAP,
            !args.iter().any(|a| a == "--no-iface"),
            iface_max_fanin,
            hub_max_indegree,
        )
    };
    // Same `--pick` narrowing rule `cmd_refs` applies: only a `MemberAmbiguous`
    // answer is ever re-resolved.
    let result = match build(q) {
        query::ImpactResult::MemberAmbiguous(candidates) => match pick {
            Some(n) if n <= candidates.len() => build(&query::qualified_seed(&candidates[n - 1])),
            Some(_) => return (2, IMPACT_USAGE.to_string()),
            None => query::ImpactResult::MemberAmbiguous(candidates),
        },
        other => other,
    };

    match result {
        query::ImpactResult::NotFound { kind } => fallback_advised_out(
            q,
            json,
            format!("no {} match for \"{q}\"", render::seed_kind_str(kind)),
        ),
        query::ImpactResult::Ambiguous { ids, .. } => ambiguous_candidates_out(&index, q, &ids),
        query::ImpactResult::MemberAmbiguous(candidates) => {
            member_ambiguous_out(q, &candidates, json)
        }
        query::ImpactResult::Resolved(model) => {
            let out = if json {
                query::json::impact_model_to_json(q, &model)
            } else if compact {
                render::render_impact_compact(q, &model)
            } else {
                render::render_impact_text(q, &model)
            };
            // A resolved seed that reaches nothing beyond its own files is the
            // same answer as an unresolved one -- empty -- and gets the same
            // signal.
            if model.rows.is_empty() {
                (EXIT_NO_RESULT, out)
            } else {
                (0, out)
            }
        }
    }
}

const TESTS_USAGE: &str =
    "usage: devscout tests <symbol> [--no-guess] [--pick N] [--json|--compact]";

// `tests`. Mirrors `cmd_refs` -- same flag conflict, same missing-query usage
// error, same `require_repo`/graph-present order, same notfound/ambiguous exits.
fn cmd_tests(cwd: &Path, args: &[String]) -> (i32, String) {
    let json = args.iter().any(|a| a == "--json");
    let compact = args.iter().any(|a| a == "--compact");
    if json && compact {
        return (
            1,
            "devscout tests: --compact and --json are mutually exclusive".to_string(),
        );
    }
    let Ok(pick) = parse_pick(args) else {
        return (2, TESTS_USAGE.to_string());
    };
    let Some(q) = first_positional(args) else {
        return (2, TESTS_USAGE.to_string());
    };
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}")),
    };
    let Some(g) = graph::read_graph(&root) else {
        return (
            1,
            "no graph.json for this repo — run `devscout map` on a C# scope first".to_string(),
        );
    };
    let index = query::load_graph_index_with(&g, &root, index_options(args));

    // Same `--pick` narrowing rule `cmd_refs` applies: only a `MemberAmbiguous`
    // answer is ever re-resolved.
    let result = match query::build_tests_model(&index, q) {
        query::TestsResult::MemberAmbiguous(candidates) => match pick {
            Some(n) if n <= candidates.len() => {
                query::build_tests_model(&index, &query::qualified_seed(&candidates[n - 1]))
            }
            Some(_) => return (2, TESTS_USAGE.to_string()),
            None => query::TestsResult::MemberAmbiguous(candidates),
        },
        other => other,
    };

    match result {
        query::TestsResult::NotFound => {
            fallback_advised_out(q, json, format!("no symbol matches \"{q}\""))
        }
        query::TestsResult::Ambiguous(ids) => ambiguous_candidates_out(&index, q, &ids),
        query::TestsResult::MemberAmbiguous(candidates) => {
            member_ambiguous_out(q, &candidates, json)
        }
        query::TestsResult::Resolved(model) => {
            if json {
                (0, query::json::tests_model_to_json(&model))
            } else if compact {
                (0, render::render_tests_compact(&model))
            } else {
                (0, render::render_tests_text(&model))
            }
        }
    }
}

// Output caps: an uncapped find can dump the whole near-match pool (measured
// 270-450KB on broad multi-token queries against a 5k-file manifest). The
// full-match pool is what the user asked for, so it gets the wider cap; the
// OR-fallback pool is near-matches only, so it gets the tighter one. Both
// manifest pools are RANKED before their cap bites -- tokens matched, then the
// file's inbound-edge count (see `manifest::find_in_manifest_detailed`) -- so
// the rows a cap drops are the weakest, not just the last.
const FIND_FULL_CAP: usize = 25;
const FIND_FALLBACK_CAP: usize = 10;
// The declaration block gets its own cap: it is a different pool from the
// manifest's, and sharing one would let a name carried by 200 members swallow
// the file rows the same query earned. It stays in build order on purpose --
// ranking is the manifest pools' job, not this one's.
const FIND_NAMES_CAP: usize = 25;

// `find`. Check order: `require_repo` FIRST, THEN the missing-query check -- the
// one query command that does NOT check its own usage before root resolution
// (deliberate, not a slip). Caps per pool kind; the `… +K more (refine query)`
// tail line keeps the true pool size honest.
fn cmd_find(cwd: &Path, query_str: &str, resources: bool) -> (i32, String) {
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}")),
    };
    if query_str.is_empty() {
        return (2, "usage: devscout find <query> [--resources]".to_string());
    }
    // The declaration block leads: a caller who named a member wants the site,
    // and the manifest block keeps its own tail line at the bottom where it
    // already sat. `file:line  <source line>` -- two spaces -- degrading to bare
    // `file:line` when the line cannot be read. A repo with no graph.json (never
    // mapped, or no graph source in scope) contributes no block at all.
    //
    // Tier 1 (a code symbol) and tier 2 (a markup/binding name) are the default
    // pool; tier 3 (a resource key) is demoted to a one-line trailer unless
    // `--resources` asks for it inline. The zero-hit brake below reads
    // `decl_lines`, so it considers tiers 1-2 only: a query that matches nothing
    // but resource keys is a miss for it, correctly, even though `named` itself is
    // non-empty.
    let graph = graph::read_graph(&root);
    // One graph read per query: the ranking map folds off the SAME read the
    // declaration block uses. No graph file (never mapped) reads as an empty
    // map -- every entry ranks at 0 inbound and the manifest answers in its
    // on-disk order, exactly as it did before this existed.
    let inbound_counts = graph
        .as_ref()
        .map(query::file_inbound_counts)
        .unwrap_or_default();
    let (decl_lines, resource_count): (Vec<String>, usize) = match graph.as_ref() {
        None => (Vec::new(), 0),
        Some(g) => {
            let named = query::find_names(g, query_str);
            let mut primary: Vec<&graph::GraphName> = Vec::new();
            let mut resource_count = 0usize;
            for n in named.iter() {
                if query::name_tier(&n.kind) <= 2 {
                    primary.push(n);
                } else {
                    resource_count += 1;
                }
            }
            let pool: Vec<&graph::GraphName> = if resources { named } else { primary };
            let mut out: Vec<String> = pool
                .iter()
                .take(FIND_NAMES_CAP)
                .map(|n| {
                    let text = query::source_line(&root, &n.file, n.line);
                    if text.is_empty() {
                        format!("{}:{}", n.file, n.line)
                    } else {
                        format!("{}:{}  {text}", n.file, n.line)
                    }
                })
                .collect();
            if pool.len() > FIND_NAMES_CAP {
                out.push(format!(
                    "… +{} more declarations (refine query)",
                    pool.len() - FIND_NAMES_CAP
                ));
            }
            (out, resource_count)
        }
    };
    match manifest::find_in_manifest_detailed(&root, query_str, &inbound_counts) {
        Ok(r) if r.hits.is_empty() && decl_lines.is_empty() => (
            EXIT_NO_RESULT,
            format!("no matches for \"{query_str}\" (run 'devscout map' if manifest is missing)"),
        ),
        Ok(r) => {
            let cap = if r.fallback {
                FIND_FALLBACK_CAP
            } else {
                FIND_FULL_CAP
            };
            let mut lines: Vec<String> = decl_lines;
            if !resources && resource_count > 0 {
                lines.push(format!(
                    "+{resource_count} resource-key hits, use --resources"
                ));
            }
            // Every manifest-pool row carries a line too, same as the declaration
            // block above it: the file's own first declaration where the name
            // index has one, line 1 (an always-valid "open the file" anchor) for a
            // file the index carries no declared symbol for at all.
            let decl_line_by_file = graph
                .as_ref()
                .map(query::first_decl_line_by_file)
                .unwrap_or_default();
            lines.extend(r.hits.iter().take(cap).map(|h| {
                // An absent purpose renders as the literal text "undefined"
                // -- not an empty string. See manifest.rs's `FindHit::purpose`
                // doc comment.
                let purpose = h.purpose.as_deref().unwrap_or("undefined");
                let agent = if h.source == "agent" { " [agent]" } else { "" };
                let line = decl_line_by_file.get(&h.path).copied().unwrap_or(1);
                format!("{}:{line}: {purpose}{agent}", h.path)
            }));
            if r.hits.len() > cap {
                lines.push(format!("… +{} more (refine query)", r.hits.len() - cap));
            }
            (0, lines.join("\n"))
        }
        Err(e) => (1, format!("error: {e}")),
    }
}

// `map`. Strips the no-op alias flag `--refresh` before anything else runs; the
// remaining tokens are the scope dirs, passed straight through to
// `mapcmd::map_repo`. This function's only job is the CLI glue: `require_repo`,
// flag stripping, and printing `MapReport::summary_line()`. `MapOptions::from_env()`
// decides the fragment-reuse mode (mapcmd.rs's own module header): content-hash
// reuse is the default, `SCOUT_MTIME_REUSE=1` drops back to mtime keying.
fn cmd_map(cwd: &Path, args: &[String]) -> (i32, String) {
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}")),
    };
    let dirs: Vec<String> = args
        .iter()
        .filter(|a| a.as_str() != "--refresh")
        .cloned()
        .collect();
    match mapcmd::map_repo(&root, &dirs, mapcmd::MapOptions::from_env()) {
        Ok(report) => (0, report.summary_line()),
        Err(e) => (1, format!("error: {e}")),
    }
}

// `stats`. The read-cache/bash-cache/session/top-stubbed queries are NOT wrapped
// in error handling, so any error there aborts the whole command (an early
// `return` with the same `"error: {msg}"` shape every other command in this file
// uses); only the cross-repo content-store block IS fail-open, two chained `if
// let Ok(..)`s that silently produce no lines on either failure.
fn cmd_stats(cwd: &Path) -> (i32, String) {
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
fn cmd_clear(cwd: &Path, args: &[String]) -> (i32, String) {
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
fn js_math_round(x: f64) -> i64 {
    (x + 0.5).floor() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    // The `--json` builders (`J`, `refs_model_to_json`, `impact_model_to_json`,
    // ...) moved to `query::json` to keep this file under its size ratchet;
    // the tests pinning their byte shape stayed here rather than following
    // them, so this brings the moved names back into scope unqualified.
    use crate::query::json::*;

    #[test]
    fn js_float_string_matches_node_json_stringify_on_checked_values() {
        // Expected values for JSON number formatting of each of these:
        assert_eq!(js_float_string(1.0), "1");
        assert_eq!(js_float_string(100.0), "100");
        assert_eq!(js_float_string(0.0), "0");
        assert_eq!(js_float_string(-0.0), "0");
        assert_eq!(js_float_string(0.15), "0.15");
        assert_eq!(js_float_string(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(js_float_string(1.0 / 3.0), "0.3333333333333333");
        assert_eq!(js_float_string(1e21), "1e+21");
        assert_eq!(js_float_string(0.0000001), "1e-7");
        assert_eq!(js_float_string(f64::NAN), "null");
        assert_eq!(js_float_string(f64::INFINITY), "null");
    }

    #[test]
    fn js_math_round_matches_node_math_round_on_checked_values() {
        // Expected `Math.round`-style rounding for each of these:
        assert_eq!(js_math_round(0.0), 0);
        assert_eq!(js_math_round(1.4), 1);
        assert_eq!(js_math_round(1.5), 2);
        assert_eq!(js_math_round(1.49999), 1);
        assert_eq!(js_math_round(2.5), 3);
        assert_eq!(js_math_round(100.0 / 4.0), 25);
        assert_eq!(js_math_round(101.0 / 4.0), 25);
        assert_eq!(js_math_round(103.0 / 4.0), 26);
    }

    #[test]
    fn tests_json_appends_via_project_last_and_omits_it_for_an_attribute_row() {
        let attribute_row = query::TestRow {
            file: "tests/OrderServiceTests.cs".to_string(),
            test_defs: vec!["App.Orders.Tests.OrderServiceTests".to_string()],
            lines: vec![10],
            ref_count: 1,
            heuristic: false,
            tier: None,
            via: query::TestVia::Attribute,
        };
        let project_row = query::TestRow {
            file: "tests/App.Tests/FakeServer.cs".to_string(),
            test_defs: vec![],
            lines: vec![12, 34],
            ref_count: 2,
            heuristic: false,
            tier: None,
            via: query::TestVia::Project,
        };
        let model = query::TestsModel {
            query: "Order".to_string(),
            symbol: "App.Orders.Order".to_string(),
            def_files: vec!["src/Order.cs".to_string()],
            rows: vec![attribute_row, project_row],
            test_file_count: 2,
            ref_count: 3,
            heuristic_file_count: 0,
            heuristic_ref_count: 0,
        };
        let json = tests_model_to_json(&model);
        assert!(
            json.contains(
                r#"{"file":"tests/OrderServiceTests.cs","testDefs":["App.Orders.Tests.OrderServiceTests"],"lines":[10],"refCount":1,"why":"test-attribute"}"#
            ),
            "an attribute row carries no via key at all: {json}"
        );
        assert!(
            json.contains(
                r#"{"file":"tests/App.Tests/FakeServer.cs","testDefs":[],"lines":[12,34],"refCount":2,"via":"project","why":"test-project"}"#
            ),
            "a project row appends via LAST (no heuristic/tier on this row): {json}"
        );
    }

    #[test]
    fn parse_int_js_matches_node_parseint_on_checked_values() {
        assert_eq!(parse_int_js("3"), Some(3));
        assert_eq!(parse_int_js("3abc"), Some(3));
        assert_eq!(parse_int_js("-5"), Some(-5));
        assert_eq!(parse_int_js("  7"), Some(7));
        assert_eq!(parse_int_js(""), None);
        assert_eq!(parse_int_js("abc"), None);
        assert_eq!(parse_int_js("-"), None);
    }

    #[test]
    fn j_object_and_array_serialize_with_no_extra_whitespace_like_json_stringify() {
        let j = J::Obj(vec![
            ("a", J::UInt(1)),
            ("b", J::Arr(vec![J::Str("x".to_string()), J::UInt(2)])),
        ]);
        assert_eq!(j.to_json_string(), r#"{"a":1,"b":["x",2]}"#);
    }

    #[test]
    fn j_string_escapes_control_chars_and_quotes_like_json_stringify() {
        let j = J::Str("a\"b\\c\nd".to_string());
        assert_eq!(j.to_json_string(), r#""a\"b\\c\nd""#);
    }

    // `--json` key ORDER, which a parsed-object comparison cannot see (object key
    // order is not observable there). These pin three placements: a refs row's
    // `heuristic` appended LAST and absent when precise, an impact row's
    // `heuristicCount` then `heuristic` appended after `score` and both absent
    // when precise, and `heuristicAffected` appended after `manifestGap` on the
    // model itself.

    fn json_refs_model(rows: Vec<query::InboundRow>, out: bool) -> query::RefsModel {
        let empty_in = || query::Table {
            total: 0,
            dropped: 0,
            rows: Vec::<query::InboundRow>::new(),
        };
        let empty_out = || query::Table {
            total: 0,
            dropped: 0,
            rows: Vec::<query::OutboundRow>::new(),
        };
        query::RefsModel {
            query: "Widget".to_string(),
            id: "App.Widget".to_string(),
            kind: "class".to_string(),
            sites: vec![query::DefSite {
                file: "src/Widget.cs".to_string(),
                line: 3,
            }],
            inbound: query::InboundTables {
                inherits: empty_in(),
                uses_type: empty_in(),
                uses_member: query::Table {
                    total: rows.len(),
                    dropped: 0,
                    rows,
                },
            },
            outbound: out.then(|| query::OutboundTables {
                inherits: empty_out(),
                uses_type: empty_out(),
                uses_member: empty_out(),
                imports: query::Table {
                    total: 0,
                    dropped: 0,
                    rows: Vec::new(),
                },
            }),
            ambiguous: query::AmbiguousTables {
                inbound: query::Table {
                    total: 0,
                    dropped: 0,
                    rows: Vec::new(),
                },
                outbound: query::Table {
                    total: 0,
                    dropped: 0,
                    rows: Vec::new(),
                },
            },
            manifest_gap: 0,
            member_refs: None,
        }
    }

    #[test]
    fn refs_json_leads_with_schema_version_ahead_of_every_existing_key() {
        let model = json_refs_model(Vec::new(), false);
        let json = refs_model_to_json(&model);
        assert!(
            json.starts_with(
                r#"{"schema_version":1,"status":"resolved","query":"Widget","id":"App.Widget","kind":"class""#
            ),
            "schema_version is the first key and every existing key keeps its slot after it: {json}"
        );
    }

    #[test]
    fn read_json_leads_with_schema_version_and_keeps_span_right_after_kind() {
        let model = query::ReadModel {
            refs: json_refs_model(Vec::new(), false),
            span: Some(query::ReadSpan {
                file: "src/Widget.cs".to_string(),
                start_line: 3,
                end_line: 5,
                source: "class Widget {}".to_string(),
            }),
        };
        let json = read_model_to_json(&model);
        assert!(
            json.starts_with(
                r#"{"schema_version":1,"status":"resolved","query":"Widget","id":"App.Widget","kind":"class","span":{"#
            ),
            "schema_version leads, and span still sits right after kind, unmoved: {json}"
        );
    }

    #[test]
    fn refs_json_appends_heuristic_then_source_last_and_omits_each_when_it_has_no_value() {
        let model = json_refs_model(
            vec![
                query::InboundRow {
                    file: "src/Fact.cs".into(),
                    line: 4,
                    heuristic: false,
                    tier: None,
                    source: String::new(),
                },
                query::InboundRow {
                    file: "src/Guess.cs".into(),
                    line: 9,
                    heuristic: true,
                    tier: None,
                    source: "var w = new Widget();".into(),
                },
            ],
            true,
        );
        let json = refs_model_to_json(&model);
        assert!(
            json.contains(
                r#""rows":[{"file":"src/Fact.cs","line":4,"why":"uses-member-precise"},{"file":"src/Guess.cs","line":9,"heuristic":true,"source":"var w = new Widget();","why":"uses-member-guess"}]"#
            ),
            "{json}"
        );
    }

    #[test]
    fn refs_json_appends_tier_after_heuristic_and_omits_it_on_a_precise_row() {
        let row = |file: &str, line: usize, tier: Option<graph::HeuristicTier>| query::InboundRow {
            file: file.into(),
            line,
            heuristic: tier.is_some(),
            tier,
            source: String::new(),
        };
        let model = json_refs_model(
            vec![
                row("src/Fact.cs", 4, None),
                row("src/Ext.cs", 7, Some(graph::HeuristicTier::Ext)),
                row("src/Guess.cs", 9, Some(graph::HeuristicTier::Guess)),
            ],
            true,
        );
        let json = refs_model_to_json(&model);
        // A precise row is byte-identical to what it was before the tier
        // existed; a guessed one gains exactly one key, in the slot right after
        // the flag it refines and still before `source`.
        assert!(
            json.contains(
                r#""rows":[{"file":"src/Fact.cs","line":4,"why":"uses-member-precise"},{"file":"src/Ext.cs","line":7,"heuristic":true,"tier":"ext","why":"uses-member-ext"},{"file":"src/Guess.cs","line":9,"heuristic":true,"tier":"guess","why":"uses-member-guess"}]"#
            ),
            "{json}"
        );
        assert_eq!(
            json.matches(r#""tier""#).count(),
            2,
            "a precise row carries no trace of the key: {json}"
        );
    }

    #[test]
    fn refs_json_omits_the_outbound_key_entirely_without_out_and_keeps_its_slot_with_it() {
        let row = || {
            vec![query::InboundRow {
                file: "src/Fact.cs".into(),
                line: 4,
                heuristic: false,
                tier: None,
                source: String::new(),
            }]
        };
        let without = refs_model_to_json(&json_refs_model(row(), false));
        assert!(
            !without.contains(r#""outbound":{"inherits""#),
            "the default model must carry no outbound tables: {without}"
        );
        assert!(without.contains(r#""ambiguous":{"inbound""#), "{without}");

        let with = refs_model_to_json(&json_refs_model(row(), true));
        let outbound_at = with
            .find(r#""outbound":{"inherits""#)
            .expect("--out must emit the outbound tables");
        let inbound_at = with
            .find(r#""inbound":{"inherits""#)
            .expect("inbound is always emitted");
        let ambiguous_at = with
            .find(r#""ambiguous":{"inbound""#)
            .expect("ambiguous is always emitted");
        assert!(
            inbound_at < outbound_at && outbound_at < ambiguous_at,
            "outbound keeps JS's key slot: {with}"
        );
    }

    #[test]
    fn member_refs_json_wraps_unchanged_resolved_models_under_status_query_members() {
        let row = || {
            vec![query::InboundRow {
                file: "src/Fact.cs".into(),
                line: 4,
                heuristic: false,
                tier: None,
                source: String::new(),
            }]
        };
        let one = json_refs_model(row(), false);
        let json = member_refs_to_json("Widget", std::slice::from_ref(&one));
        assert!(
            json.starts_with(
                r#"{"schema_version":1,"status":"members","query":"Widget","members":[{"schema_version":1,"status":"resolved""#
            ),
            "schema_version is the first key, ahead of status, on both the wrapper and each member: {json}"
        );
        assert!(
            json.ends_with(r#"}],"outcome":"hit"}"#),
            "the wrapper's own outcome is appended last, after the members array: {json}"
        );
        assert!(
            json.contains(&refs_model_to_json(&one)),
            "a member entry is the resolved object unchanged: {json}"
        );
    }

    #[test]
    fn impact_json_leads_with_schema_version_ahead_of_query() {
        let model = query::ImpactModel {
            kind: query::SeedKind::Symbol,
            seed_files: vec!["src/Widget.cs".to_string()],
            hops: 2,
            total_affected: 0,
            rows: vec![],
            dropped: 0,
            manifest_gap: 0,
            heuristic_affected: 0,
            tests_affected: 0,
            braked: vec![],
            braked_files: vec![],
        };
        let json = impact_model_to_json("Widget", &model);
        assert!(
            json.starts_with(r#"{"schema_version":1,"query":"Widget","status":"resolved""#),
            "schema_version leads, ahead of the query key that used to be first: {json}"
        );
    }

    #[test]
    fn tests_json_leads_with_schema_version_ahead_of_status() {
        let model = query::TestsModel {
            query: "Order".to_string(),
            symbol: "App.Orders.Order".to_string(),
            def_files: vec!["src/Order.cs".to_string()],
            rows: vec![],
            test_file_count: 0,
            ref_count: 0,
            heuristic_file_count: 0,
            heuristic_ref_count: 0,
        };
        let json = tests_model_to_json(&model);
        assert!(
            json.starts_with(r#"{"schema_version":1,"status":"resolved","query":"Order""#),
            "schema_version leads, ahead of the status key that used to be first: {json}"
        );
    }

    #[test]
    fn impact_json_appends_heuristic_count_then_heuristic_after_score_and_heuristic_affected_after_manifest_gap(
    ) {
        let row = |file: &str, heuristic: bool| query::ImpactRow {
            file: file.to_string(),
            hop: 1,
            via_count: if heuristic { 0 } else { 1 },
            ambiguous_count: 0,
            top_symbols: vec!["Widget".to_string()],
            top_symbols_more: 0,
            score: 0.5,
            heuristic_count: if heuristic { 2 } else { 0 },
            heuristic,
            tier: None,
            iface_via: vec![],
            from_lines: vec![],
            infra: false,
            why: if heuristic {
                query::Why::UsesMemberGuess
            } else {
                query::Why::UsesMemberPrecise
            },
        };
        let model = query::ImpactModel {
            kind: query::SeedKind::Symbol,
            seed_files: vec!["src/Widget.cs".to_string()],
            hops: 2,
            total_affected: 1,
            rows: vec![row("src/Direct.cs", false), row("src/Guessed.cs", true)],
            dropped: 0,
            manifest_gap: 0,
            heuristic_affected: 1,
            tests_affected: 0,
            braked: vec![],
            braked_files: vec![],
        };
        let json = impact_model_to_json("Widget", &model);
        assert!(
            json.contains(
                r#"{"file":"src/Direct.cs","hop":1,"viaCount":1,"ambiguousCount":0,"topSymbols":["Widget"],"topSymbolsMore":0,"score":0.5,"why":"uses-member-precise"}"#
            ),
            "{json}"
        );
        assert!(
            json.contains(
                r#"{"file":"src/Guessed.cs","hop":1,"viaCount":0,"ambiguousCount":0,"topSymbols":["Widget"],"topSymbolsMore":0,"score":0.5,"heuristicCount":2,"heuristic":true,"why":"uses-member-guess"}"#
            ),
            "{json}"
        );
        assert!(
            json.ends_with(
                r#","dropped":0,"manifestGap":0,"heuristicAffected":1,"testsAffected":0,"outcome":"hit"}"#
            ),
            "{json}"
        );
    }

    #[test]
    fn impact_json_tier_sits_between_heuristic_and_iface_via() {
        let row = |file: &str, tier: graph::HeuristicTier| query::ImpactRow {
            file: file.to_string(),
            hop: 1,
            via_count: 0,
            ambiguous_count: 0,
            top_symbols: vec!["Widget".to_string()],
            top_symbols_more: 0,
            score: 0.5,
            heuristic_count: 2,
            heuristic: true,
            tier: Some(tier),
            iface_via: vec!["IWidget".to_string()],
            from_lines: vec![],
            infra: false,
            why: match tier {
                graph::HeuristicTier::Ext => query::Why::UsesMemberExt,
                graph::HeuristicTier::Guess => query::Why::UsesMemberGuess,
            },
        };
        let model = query::ImpactModel {
            kind: query::SeedKind::Symbol,
            seed_files: vec!["src/Widget.cs".to_string()],
            hops: 2,
            total_affected: 0,
            rows: vec![
                row("src/Extended.cs", graph::HeuristicTier::Ext),
                row("src/Guessed.cs", graph::HeuristicTier::Guess),
            ],
            dropped: 0,
            manifest_gap: 0,
            heuristic_affected: 2,
            tests_affected: 0,
            braked: vec![],
            braked_files: vec![],
        };
        let json = impact_model_to_json("Widget", &model);
        // `tier` takes the slot right after the flag it refines, which on this
        // row shape means BEFORE `ifaceVia` -- every key that was already
        // appended last stays appended last.
        assert!(
            json.contains(
                r#""score":0.5,"heuristicCount":2,"heuristic":true,"tier":"ext","ifaceVia":["IWidget"],"why":"uses-member-ext"}"#
            ),
            "{json}"
        );
        assert!(
            json.contains(
                r#""score":0.5,"heuristicCount":2,"heuristic":true,"tier":"guess","ifaceVia":["IWidget"],"why":"uses-member-guess"}"#
            ),
            "{json}"
        );
    }

    // The expected strings below pin the find output caps; the integration suite
    // also exercises the same caps end to end.
    fn find_cap_root(prefix: &str, entry_count: usize) -> PathBuf {
        let root = std::env::temp_dir().join(format!("scout-cli-{prefix}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let scout = root.join(".scout");
        std::fs::create_dir_all(&scout).unwrap();
        let entries: Vec<String> = (0..entry_count)
            .map(|i| {
                format!(
                    r#""src/widget-{i:02}.cs": {{ "purpose": "widget number {i}", "mtime": {i} }}"#
                )
            })
            .collect();
        let json = format!(r#"{{ "entries": {{ {} }} }}"#, entries.join(", "));
        std::fs::write(scout.join("manifest.json"), json).unwrap();
        root
    }

    #[test]
    fn cmd_find_caps_full_pool_at_25_with_honest_tail() {
        let root = find_cap_root("findcap-full", 30);
        let (code, out) = cmd_find(&root, "widget", false);
        assert_eq!(code, 0);
        let lines: Vec<&str> = out.split('\n').collect();
        assert_eq!(lines.len(), 26);
        assert_eq!(lines[25], "… +5 more (refine query)");
        assert!(lines[..25].iter().all(|l| l.contains("widget")));
    }

    #[test]
    fn cmd_find_caps_fallback_pool_at_10_with_honest_tail() {
        let root = find_cap_root("findcap-fallback", 12);
        let (code, out) = cmd_find(&root, "widget zzz123nomatch", false);
        assert_eq!(code, 0);
        let lines: Vec<&str> = out.split('\n').collect();
        assert_eq!(lines.len(), 11);
        assert_eq!(lines[10], "… +2 more (refine query)");
    }

    // The note text and the stream it lands on are pinned in
    // tests/cli_zero_hit.rs, which can see both streams; what belongs here is
    // that the code the verb computes is EXIT_NO_RESULT and not 0.
    #[test]
    fn cmd_find_reports_a_zero_hit_on_its_own_exit_code_with_stdout_unchanged() {
        let root = find_cap_root("findcap-zero", 5);
        let (code, out) = cmd_find(&root, "zzz123nosuchpurpose", false);
        assert_eq!(code, EXIT_NO_RESULT);
        assert_eq!(
            out,
            "no matches for \"zzz123nosuchpurpose\" (run 'devscout map' if manifest is missing)"
        );
    }

    #[test]
    fn cmd_find_below_cap_prints_no_tail() {
        let root = find_cap_root("findcap-small", 5);
        let (code, out) = cmd_find(&root, "widget", false);
        assert_eq!(code, 0);
        assert_eq!(out.split('\n').count(), 5);
        assert!(!out.contains("more (refine query)"));
    }

    // --- the declaration block ----------------------------------------------

    const LEDGER_CS: &str = "namespace Gadgets;\n\npublic class WidgetLedger\n{\n\tprivate int _entryCount;\n\n\tpublic string Label { get; set; }\n\n\tpublic event EventHandler Retired;\n\n\tprivate void PopulateSlots() { }\n}\n";
    const PANEL_XAML: &str = "<UserControl\n\tx:Class=\"Gadgets.PanelView\">\n\t<Button x:Name=\"ShipButton\" />\n</UserControl>\n";
    const STRINGS_RESW: &str =
        "<root>\n\t<resheader name=\"resmimetype\" />\n\t<data name=\"ShipButton.Content\" xml:space=\"preserve\" />\n</root>\n";

    // A repo whose graph.json is the one `map` would write for these three files
    // -- built through the real resolver, not hand-authored JSON, so the test
    // cannot drift from the index the map path actually produces.
    fn name_index_root(prefix: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("scout-cli-{prefix}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src").join("WidgetLedger.cs"), LEDGER_CS).unwrap();
        std::fs::write(root.join("src").join("Panel.xaml"), PANEL_XAML).unwrap();
        std::fs::write(root.join("src").join("Strings.resw"), STRINGS_RESW).unwrap();
        std::fs::create_dir_all(root.join(".scout")).unwrap();
        std::fs::write(
            root.join(".scout").join("manifest.json"),
            r#"{ "entries": {} }"#,
        )
        .unwrap();

        let fragments = vec![
            (
                "src/WidgetLedger.cs".to_string(),
                graph::fragment_from_extraction(&crate::extract::extract(LEDGER_CS)),
            ),
            (
                "src/Panel.xaml".to_string(),
                graph::markup_fragment(&root, "src/Panel.xaml").unwrap(),
            ),
            (
                "src/Strings.resw".to_string(),
                graph::markup_fragment(&root, "src/Strings.resw").unwrap(),
            ),
        ];
        let g = crate::resolve::resolve_graph(&root, &fragments);
        std::fs::create_dir_all(root.join(".scout").join("graph")).unwrap();
        std::fs::write(
            root.join(".scout").join("graph").join("graph.json"),
            serde_json::to_string(&g).unwrap(),
        )
        .unwrap();
        root
    }

    #[test]
    fn cmd_find_resolves_every_code_and_markup_category_to_its_declaration_line() {
        // A resource key (`ShipButton.Content`) is not in this loop: it is tier
        // 3, hidden by default. Its own default/--resources behavior is pinned
        // separately below.
        let root = name_index_root("findnames");
        for (query, expected) in [
            (
                "PopulateSlots",
                "src/WidgetLedger.cs:11  private void PopulateSlots() { }",
            ),
            (
                "Label",
                "src/WidgetLedger.cs:7  public string Label { get; set; }",
            ),
            (
                "_entryCount",
                "src/WidgetLedger.cs:5  private int _entryCount;",
            ),
            (
                "Retired",
                "src/WidgetLedger.cs:9  public event EventHandler Retired;",
            ),
            (
                "Gadgets.PanelView",
                "src/Panel.xaml:2  x:Class=\"Gadgets.PanelView\">",
            ),
        ] {
            let (code, out) = cmd_find(&root, query, false);
            assert_eq!(code, 0, "{query} should be a hit");
            assert_eq!(out, expected, "{query}");
        }
    }

    // --- kind tiering --------------------------------------------------------

    #[test]
    fn cmd_find_a_resource_key_only_query_brakes_correctly_by_default_and_is_a_hit_under_resources()
    {
        // `ShipButton.Content` matches ONLY the resource-key row -- no
        // markup-name row contains the full stop, and the manifest is empty -- so
        // this is the drowned-query case: a query that would otherwise return
        // resource keys (here, one) and nothing else brakes, correctly, instead
        // of looking like a hit.
        let root = name_index_root("findnames-resource-only");
        let (code, out) = cmd_find(&root, "ShipButton.Content", false);
        assert_eq!(
            code, EXIT_NO_RESULT,
            "a resource-key-only match is a zero hit by default"
        );
        assert_eq!(
            out,
            "no matches for \"ShipButton.Content\" (run 'devscout map' if manifest is missing)"
        );

        let (code, out) = cmd_find(&root, "ShipButton.Content", true);
        assert_eq!(code, 0, "--resources lifts the demotion");
        assert_eq!(
            out,
            "src/Strings.resw:3  <data name=\"ShipButton.Content\" xml:space=\"preserve\" />"
        );
    }

    #[test]
    fn cmd_find_default_view_hides_resource_keys_behind_a_trailer_and_resources_shows_them_inline()
    {
        // `ShipButton` matches BOTH the tier-2 markup-name `ShipButton` and
        // the tier-3 resource key `ShipButton.Content` (substring). Default:
        // the tier-2 row shows, the tier-3 row is a one-line trailer. Under
        // `--resources`: both rows show, inline, in build order, no trailer.
        let root = name_index_root("findnames-tiering");
        let (code, out) = cmd_find(&root, "ShipButton", false);
        assert_eq!(code, 0);
        assert_eq!(
            out,
            "src/Panel.xaml:3  <Button x:Name=\"ShipButton\" />\n+1 resource-key hits, use --resources",
            "tier 3 is a trailer, never an inline row, by default"
        );

        let (code, out) = cmd_find(&root, "ShipButton", true);
        assert_eq!(code, 0);
        assert_eq!(
            out,
            "src/Panel.xaml:3  <Button x:Name=\"ShipButton\" />\nsrc/Strings.resw:3  <data name=\"ShipButton.Content\" xml:space=\"preserve\" />",
            "--resources includes the resource-key row inline and drops the trailer"
        );
    }

    #[test]
    fn cmd_find_puts_the_declaration_block_above_the_manifest_block() {
        let root = name_index_root("findnames-order");
        std::fs::write(
            root.join(".scout").join("manifest.json"),
            r#"{ "entries": { "src/WidgetLedger.cs": { "purpose": "class WidgetLedger", "mtime": 1 } } }"#,
        )
        .unwrap();
        let (code, out) = cmd_find(&root, "WidgetLedger", false);
        assert_eq!(code, 0);
        assert_eq!(
            out,
            "src/WidgetLedger.cs:3  public class WidgetLedger\nsrc/WidgetLedger.cs:3: class WidgetLedger"
        );
    }

    // The manifest-pool row carries its own file's first declaration line, not
    // just the declaration block above it. `Panel.xaml` has no manifest entry in
    // `name_index_root`'s fixture, so this proves the line on a row that has ONE
    // (a real manifest hit for a file the name index also carries a declaration
    // for), independent of the declaration block.
    #[test]
    fn cmd_find_manifest_pool_row_carries_its_files_first_declaration_line() {
        let root = name_index_root("findnames-manifest-line");
        std::fs::write(
            root.join(".scout").join("manifest.json"),
            r#"{ "entries": { "src/WidgetLedger.cs": { "purpose": "class WidgetLedger; Ship", "mtime": 1 } } }"#,
        )
        .unwrap();
        let (code, out) = cmd_find(&root, "WidgetLedger", false);
        assert_eq!(code, 0);
        let lines: Vec<&str> = out.split('\n').collect();
        let manifest_row = lines
            .iter()
            .find(|l| l.starts_with("src/WidgetLedger.cs:") && l.contains("; Ship"));
        assert_eq!(
            manifest_row,
            Some(&"src/WidgetLedger.cs:3: class WidgetLedger; Ship"),
            "{out}"
        );
    }

    // A manifest hit whose file the name index carries NO declaration for at all
    // (a plain, undeclared source file) falls back to line 1 -- an always-valid
    // "open the file" anchor -- rather than emitting a row with no line.
    #[test]
    fn cmd_find_manifest_pool_row_for_a_file_with_no_declaration_falls_back_to_line_1() {
        let root = name_index_root("findnames-manifest-no-decl");
        std::fs::write(root.join("src").join("Notes.md"), "# widget notes\n").unwrap();
        std::fs::write(
            root.join(".scout").join("manifest.json"),
            r#"{ "entries": { "src/Notes.md": { "purpose": "widget notes doc", "mtime": 1 } } }"#,
        )
        .unwrap();
        let (code, out) = cmd_find(&root, "widget", false);
        assert_eq!(code, 0);
        assert!(out.contains("src/Notes.md:1: widget notes doc"), "{out}");
    }

    #[test]
    fn cmd_find_still_takes_the_zero_hit_exit_when_neither_index_matches() {
        let root = name_index_root("findnames-zero");
        let (code, out) = cmd_find(&root, "Zzzznomatch", false);
        assert_eq!(code, EXIT_NO_RESULT);
        assert_eq!(
            out,
            "no matches for \"Zzzznomatch\" (run 'devscout map' if manifest is missing)"
        );
    }

    // --- `clear` --------------------------------------------------------------
    //
    // In-process coverage of `cmd_clear`, fast and independent of the
    // subprocess byte-parity gate in the integration suite.

    fn clear_root(prefix: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("scout-cli-{prefix}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".scout")).unwrap();
        root
    }

    fn seed_row(conn: &rusqlite::Connection, session_id: &str, rel_path: &str) {
        store::record_fresh(
            conn,
            &store::RecordFresh {
                session_id,
                agent_id: "",
                rel_path,
                sha256: "sha",
                size: 10,
                mtime: 1,
                lines: 1,
                delivered: true,
            },
        )
        .unwrap();
    }

    #[test]
    fn cmd_clear_whole_store_removes_cache_db_and_answers_cache_cleared() {
        let root = clear_root("clear-whole");
        {
            let conn = store::open_store(&root).unwrap();
            seed_row(&conn, "s1", "a.ts");
        }
        assert!(root.join(".scout").join("cache.db").exists());
        let (code, out) = cmd_clear(&root, &[]);
        assert_eq!(code, 0);
        assert_eq!(out, "cache cleared");
        assert!(!root.join(".scout").join("cache.db").exists());
    }

    #[test]
    fn cmd_clear_older_than_rejects_negative_and_non_numeric_with_usage_code_2() {
        let root = clear_root("clear-older-bad");
        for bad in ["-5", "abc", ""] {
            let (code, out) = cmd_clear(&root, &["--older-than".to_string(), bad.to_string()]);
            assert_eq!(code, 2, "arg {bad:?}");
            assert_eq!(
                out, "usage: devscout clear --older-than <days>",
                "arg {bad:?}"
            );
        }
        // A bare `--older-than` with nothing after it (missing value entirely).
        let (code, out) = cmd_clear(&root, &["--older-than".to_string()]);
        assert_eq!(code, 2);
        assert_eq!(out, "usage: devscout clear --older-than <days>");
        assert!(
            !root.join(".scout").join("cache.db").exists(),
            "a rejected flag must not create a store"
        );
    }

    #[test]
    fn cmd_clear_session_unique_prefix_deletes_only_that_sessions_rows() {
        let root = clear_root("clear-session-unique");
        {
            let conn = store::open_store(&root).unwrap();
            seed_row(&conn, "abc-1111", "a.ts");
            seed_row(&conn, "abc-2222", "b.ts");
        }
        let (code, out) = cmd_clear(&root, &["--session".to_string(), "abc-1".to_string()]);
        assert_eq!(code, 0);
        assert_eq!(out, "deleted 1 row for session abc-1111");

        let conn = store::open_store(&root).unwrap();
        assert!(store::lookup_read(&conn, "abc-1111", "a.ts", "")
            .unwrap()
            .is_none());
        assert!(store::lookup_read(&conn, "abc-2222", "b.ts", "")
            .unwrap()
            .is_some());
    }

    // The ambiguous-prefix refusal: two sessions share the "abc" prefix; asking
    // for exactly that prefix must refuse on code 2, name both candidates, and
    // delete nothing.
    #[test]
    fn cmd_clear_session_ambiguous_prefix_refuses_with_code_2_and_lists_both_ids() {
        let root = clear_root("clear-session-ambiguous");
        {
            let conn = store::open_store(&root).unwrap();
            seed_row(&conn, "abc-1111", "a.ts");
            seed_row(&conn, "abc-2222", "b.ts");
        }
        let (code, out) = cmd_clear(&root, &["--session".to_string(), "abc".to_string()]);
        assert_eq!(code, 2);
        assert_eq!(out, "ambiguous session prefix \"abc\": abc-1111, abc-2222");

        // Refused, so read-only: both sessions' rows still stand.
        let conn = store::open_store(&root).unwrap();
        assert!(store::lookup_read(&conn, "abc-1111", "a.ts", "")
            .unwrap()
            .is_some());
        assert!(store::lookup_read(&conn, "abc-2222", "b.ts", "")
            .unwrap()
            .is_some());
    }

    #[test]
    fn cmd_clear_session_no_match_is_a_success_not_a_refusal() {
        let root = clear_root("clear-session-none");
        {
            let conn = store::open_store(&root).unwrap();
            seed_row(&conn, "abc-1111", "a.ts");
        }
        let (code, out) = cmd_clear(&root, &["--session".to_string(), "zzz".to_string()]);
        assert_eq!(code, 0);
        assert_eq!(out, "no sessions match \"zzz\"");
    }

    #[test]
    fn cmd_clear_session_missing_value_is_a_usage_error() {
        let root = clear_root("clear-session-missing");
        let (code, out) = cmd_clear(&root, &["--session".to_string()]);
        assert_eq!(code, 2);
        assert_eq!(out, "usage: devscout clear --session <id-or-prefix>");
    }

    #[test]
    fn cmd_clear_older_than_flag_takes_priority_over_session_flag_like_node_indexof_order() {
        // `clear` checks `--older-than` before `--session`, so a call carrying
        // both takes the age-based branch, whatever `--session` says.
        let root = clear_root("clear-precedence");
        {
            let conn = store::open_store(&root).unwrap();
            seed_row(&conn, "abc-1111", "a.ts");
        }
        let (code, out) = cmd_clear(
            &root,
            &[
                "--session".to_string(),
                "abc-1111".to_string(),
                "--older-than".to_string(),
                "9999".to_string(),
            ],
        );
        assert_eq!(code, 0);
        assert_eq!(
            out, "deleted 0 rows older than 9999d",
            "the --older-than branch must win"
        );
    }
}
