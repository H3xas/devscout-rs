// Argument-parsing primitives shared by two or more verbs: the bare-query
// scan, `--pick`, the lenient integer parse `--hops`/`--iface-max-fanin`/
// `--hub-max-indegree` all use, `--no-guess`, and the `--json`/`--compact`
// conflict check.

use crate::query;

// The first non-flag argument -- the symbol `refs`/`read`/`tests` take.
// Shared with `dispatch`, which needs the same string to build the suggestion
// block for a query `cmd_refs`/`cmd_read` has already reported as a zero hit.
// Skips `--pick`'s OWN value: unlike every other flag these three verbs
// accept, `--pick N` carries a value that does not start with `--` and so
// would otherwise be mistaken for the query itself.
pub(crate) fn first_positional(args: &[String]) -> Option<&str> {
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

// `--pick <n>`, shared by `refs`/`read`/`impact`/`tests`: a strict positive
// integer (unlike `--hops`'s lenient `parse_int_js` -- a candidate index is
// never "close enough"). `Ok(None)` when the flag is absent, `Err(())` on a
// missing, non-numeric, or zero value, which every call site turns into its
// own usage error. Range-checking against the actual candidate count is the
// caller's job: that count is only known once resolution has already
// answered `MemberAmbiguous`.
pub(crate) fn parse_pick(args: &[String]) -> Result<Option<usize>, ()> {
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
pub(crate) fn parse_int_js(s: &str) -> Option<i64> {
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

// `--no-guess`/`--no-dispatch`, shared by the four verbs that read the graph
// index. Spelled once and read at INDEX BUILD rather than at each render
// site: a guess (or a dispatch edge) that never entered the adjacency cannot
// leak back out through a consumer that forgot to filter, which is the same
// reasoning that gives heuristic edges their own buckets in the first place.
//
// Both flags are plain presence tests like `--json`/`--out`, so neither takes
// a value and neither can be mistaken for the query -- no usage-error branch
// of its own, which is why the four call sites below just call this.
pub(crate) fn index_options(args: &[String]) -> query::IndexOptions {
    query::IndexOptions {
        include_guesses: !args.iter().any(|a| a == "--no-guess"),
        include_dispatch: !args.iter().any(|a| a == "--no-dispatch"),
    }
}

// `--json` and `--compact` name two different renderers, so a caller who asks
// for both gets a refusal rather than a precedence rule each verb would have to
// spell the same way. `verb` only names the command inside that message.
pub(crate) fn output_flags(verb: &str, args: &[String]) -> Result<(bool, bool), (i32, String)> {
    let json = args.iter().any(|a| a == "--json");
    let compact = args.iter().any(|a| a == "--compact");
    if json && compact {
        return Err((
            1,
            format!("devscout {verb}: --compact and --json are mutually exclusive"),
        ));
    }
    Ok((json, compact))
}
