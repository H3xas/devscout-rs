// Top-level argument dispatch: matches the subcommand, wires each verb's
// return value to stdout/stderr and the process exit code.
//
// Alongside the user-facing verbs (README.md), four dev/diagnostic subcommands
// are wired here and nowhere else: `noop` (cold-start floor), `parse` and
// `spans` (seed AST dumps), and `extract-dump` (full-extraction JSON).
//
// `init` dispatches through `initcmd::cmd_init_full`, NOT `cmd_init` directly --
// see initcmd.rs for why that split is load-bearing rather than stylistic.

use std::io::{Read, Write};
use std::process;

use crate::audit;
use crate::extract;
use crate::hookio;
use crate::initcmd;
use crate::parse;

use super::admin::{cmd_clear, cmd_map, cmd_stats};
use super::answer::{
    emit_freshness_warning, emit_zero_hit_note, ZERO_HIT_FIND, ZERO_HIT_IMPACT, ZERO_HIT_TESTS,
};
use super::args::first_positional;
use super::compiler_facts::cmd_compiler_facts;
use super::coverage::cmd_tests;
use super::find::cmd_find;
use super::impact::cmd_impact;
use super::import_edges::cmd_import_edges;
use super::read::cmd_read;
use super::refs::cmd_refs;
use super::root::{apply_global_options, current_dir};

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
  refs <symbol>              references to a symbol   [--out --all --no-guess --no-dispatch --no-bus --pick N --json|--compact]
  read <symbol>              decl span + inbound refs [--no-guess --no-dispatch --no-bus --pick N --json|--compact]
  impact <file|symbol>       blast radius             [--hops N --no-guess --no-dispatch --no-bus --no-imports --pick N --json|--compact]
  tests <symbol>             tests reaching a symbol  [--no-guess --no-dispatch --no-bus --pick N --json|--compact]
  import-edges <file> --repo <id>  load a cross-repo edge export for `impact` to read
  compiler-facts run|import|status  optional engine-derived facts [--target --configuration --platform]
  stats                      index + cache summary for this repo

plumbing
  parse <file.cs>            dump the parse tree
  spans <file.cs>            dump declaration spans
  extract-dump <file.cs>     dump extraction records
  audit --semantic <refs.jsonl>  score uses-member edges against a semantic oracle [--units F] [--defs F] [--json] [--assert F] [--fp-sites F]
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
            let (code, out, note) = cmd_refs(&cwd, &args[2..]);
            print_out(&out);
            emit_freshness_warning(&cwd);
            emit_zero_hit_note(code, note, &cwd, first_positional(&args[2..]));
            process::exit(code);
        }
        Some("read") => {
            let (code, out, note) = cmd_read(&cwd, &args[2..]);
            print_out(&out);
            emit_freshness_warning(&cwd);
            emit_zero_hit_note(code, note, &cwd, first_positional(&args[2..]));
            process::exit(code);
        }
        Some("impact") => {
            let (code, out) = cmd_impact(&cwd, &args[2..]);
            print_out(&out);
            emit_freshness_warning(&cwd);
            emit_zero_hit_note(code, Some(ZERO_HIT_IMPACT), &cwd, None);
            process::exit(code);
        }
        Some("tests") => {
            let (code, out) = cmd_tests(&cwd, &args[2..]);
            print_out(&out);
            emit_freshness_warning(&cwd);
            emit_zero_hit_note(code, Some(ZERO_HIT_TESTS), &cwd, None);
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
            emit_zero_hit_note(code, Some(ZERO_HIT_FIND), &cwd, Some(query_str.as_str()));
            process::exit(code);
        }
        Some("import-edges") => {
            let (code, out) = cmd_import_edges(&cwd, &args[2..]);
            print_out(&out);
            process::exit(code);
        }
        Some("compiler-facts") => {
            let (code, out) = cmd_compiler_facts(&cwd, &args[2..]);
            print_out(&out);
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
            eprintln!("usage: devscout <noop|parse|spans|extract-dump|hook|refs|read|impact|tests|find|import-edges|compiler-facts|map|stats|clear|init> [args]");
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
