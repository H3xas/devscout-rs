// The `find` verb: the manifest search plus the declaration block a code or
// markup name resolves to, each pool capped and ranked independently.

use std::path::Path;
use std::time::Instant;

use crate::graph;
use crate::manifest;
use crate::query;

use super::answer::{finish_query, EXIT_NO_RESULT};
use super::root::require_repo;

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
pub(crate) fn cmd_find(cwd: &Path, query_str: &str, resources: bool) -> (i32, String) {
    let start = Instant::now();
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
        Ok(r) => {
            let answer = find_result_out(
                graph.as_ref(),
                query_str,
                resources,
                resource_count,
                decl_lines,
                r,
            );
            finish_query(&root, "find", query_str, start, answer)
        }
        Err(e) => (1, format!("error: {e}")),
    }
}

// The rendering AND telemetry facts for a `find` answer, derived together at
// the single point the answer is rendered so the telemetry line can never
// disagree with the bytes that went to stdout.
fn find_result_out(
    graph: Option<&graph::Graph>,
    query_str: &str,
    resources: bool,
    resource_count: usize,
    decl_lines: Vec<String>,
    r: manifest::FindResult,
) -> (i32, String, query::Outcome, usize) {
    if r.hits.is_empty() && decl_lines.is_empty() {
        let out =
            format!("no matches for \"{query_str}\" (run 'devscout map' if manifest is missing)");
        return (EXIT_NO_RESULT, out, query::Outcome::FallbackAdvised, 0);
    }
    let cap = if r.fallback {
        FIND_FALLBACK_CAP
    } else {
        FIND_FULL_CAP
    };
    // The rows the answer carried, not the pool they were drawn from: neither
    // "+N more" tail is a row, and each pool holds at most its own cap of them.
    let candidate_count = decl_lines.len().min(FIND_NAMES_CAP) + r.hits.len().min(cap);
    let mut lines: Vec<String> = decl_lines;
    if !resources && resource_count > 0 {
        lines.push(format!(
            "+{resource_count} resource-key hits, use --resources"
        ));
    }
    // Every manifest-pool row carries a line too, same as the declaration
    // block above it: the file's own first declaration where the name index
    // has one, line 1 (an always-valid "open the file" anchor) for a file
    // the index carries no declared symbol for at all.
    let decl_line_by_file = graph
        .map(query::first_decl_line_by_file)
        .unwrap_or_default();
    lines.extend(r.hits.iter().take(cap).map(|h| {
        // An absent purpose renders as the literal text "undefined" -- not
        // an empty string. See manifest.rs's `FindHit::purpose` doc comment.
        let purpose = h.purpose.as_deref().unwrap_or("undefined");
        let agent = if h.source == "agent" { " [agent]" } else { "" };
        let line = decl_line_by_file.get(&h.path).copied().unwrap_or(1);
        format!("{}:{line}: {purpose}{agent}", h.path)
    }));
    if r.hits.len() > cap {
        lines.push(format!("… +{} more (refine query)", r.hits.len() - cap));
    }
    (0, lines.join("\n"), query::Outcome::Hit, candidate_count)
}
