// The nearest-name measure a zero-hit `find`/`refs` suggests from. Every
// ordering decision below is load-bearing: these rows land in the stable
// "did you mean" note, so their order must stay fixed.
//
// The order decides which candidate a caller sees, so it is stated once here
// and nowhere else: three tiers, lowest wins -- (0) the declared name contains
// the query case-insensitively, (1) they share at least one CamelCase/snake
// token, (2) their full names are within MAX_EDIT_DISTANCE.
// Inside a tier the smaller `penalty` wins; the remaining ties break by kind
// (type, then member, then markup, then resource), file, line, name. One row
// per distinct name -- a "did you mean" list of five spellings of one name is
// no list at all.

use crate::graph::GraphName;

// Kind buckets. Anything the index carries that is not a member, a markup
// name or a resource key is a declaration of a type or a top-level TS/JS
// entity, and ranks ahead of all three.
const MEMBER_KINDS: &[&str] = &["method", "property", "field", "event", "enum-member"];
// `markup-binding` (a `{Binding Path}` this index can name but not resolve)
// sits in the same bucket as the other markup names.
const MARKUP_KINDS: &[&str] = &["markup-class", "markup-name", "markup-binding"];

/// The SUGGESTION CAP value.
pub const SUGGESTION_CAP: usize = 5;
const MAX_EDIT_DISTANCE: usize = 2;

// Crate-visible so `find`'s kind-tiering (query.rs's `name_tier`) reuses this
// same bucket rule instead of restating the kind lists.
pub(crate) fn kind_rank(kind: &str) -> u8 {
    if MEMBER_KINDS.contains(&kind) {
        1
    } else if MARKUP_KINDS.contains(&kind) {
        2
    } else if kind == "resource-key" {
        3
    } else {
        0
    }
}

// Splits on every non-ASCII-alphanumeric character and at each CamelCase
// boundary (`lower|Upper`, and `Upper|Upper+lower` so `XMLHttpRequest` yields
// xml/http/request). Classification is ASCII-only on purpose: a non-ASCII
// character is treated as one or more separators regardless of how many bytes
// it occupies, and runs of separators collapse, so the token list stays stable.
// Iterating bytes rather than chars is what makes that true.
fn tokens(s: &str) -> Vec<String> {
    let bytes = s.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for (i, &c) in bytes.iter().enumerate() {
        if !c.is_ascii_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur).to_ascii_lowercase());
            }
            continue;
        }
        if c.is_ascii_uppercase() && !cur.is_empty() {
            let prev_upper = cur.as_bytes()[cur.len() - 1].is_ascii_uppercase();
            let next_lower = matches!(bytes.get(i + 1), Some(n) if n.is_ascii_lowercase());
            if !prev_upper || next_lower {
                out.push(std::mem::take(&mut cur).to_ascii_lowercase());
            }
        }
        cur.push(c as char);
    }
    if !cur.is_empty() {
        out.push(cur.to_ascii_lowercase());
    }
    let mut distinct: Vec<String> = Vec::new();
    for t in out {
        if !distinct.contains(&t) {
            distinct.push(t);
        }
    }
    distinct
}

// Levenshtein with an abort: the moment a whole row exceeds `max` no later row
// can come back under it, so the walk stops and the caller only learns the
// distance is out of range. Both arguments are ASCII (the caller gates on it),
// so byte length is character length.
fn edit_distance_within(a: &str, b: &str, max: usize) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len().abs_diff(b.len()) > max {
        return max + 1;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur: Vec<usize> = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        let mut best = cur[0];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            best = best.min(cur[j]);
        }
        if best > max {
            return max + 1;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

fn shared_count(a: &[String], b: &[String]) -> usize {
    a.iter().filter(|t| b.contains(t)).count()
}

// Code-point length, counted independently of UTF-16 units or UTF-8 bytes so
// the measure is stable across encodings.
fn code_points(s: &str) -> usize {
    s.chars().count()
}

struct Scored<'g> {
    row: &'g GraphName,
    rank: u8,
    penalty: usize,
    kind: u8,
}

fn score_row(
    row: &GraphName,
    query: &str,
    lower_query: &str,
    query_tokens: &[String],
    query_ascii: bool,
) -> Option<(u8, usize)> {
    let lower = row.name.to_lowercase();
    // Containment is one-directional. A name the query swallows whole is not a
    // near miss for it -- every two-letter field in the index sits inside some
    // long query -- and the token tier already reaches the case that matters, a
    // query carrying a real name plus a suffix.
    if lower.contains(lower_query) {
        return Some((0, code_points(&row.name).abs_diff(code_points(query))));
    }
    let name_tokens = tokens(&row.name);
    let shared = shared_count(query_tokens, &name_tokens);
    if shared > 0 {
        return Some((1, query_tokens.len() + name_tokens.len() - 2 * shared));
    }
    // The edit-distance tier is ASCII-only, and refuses rather than guesses
    // outside it: an edit distance is ambiguous for a non-ASCII name because it
    // depends on whether characters are counted as bytes or code units, and an
    // unstable suggestion list is worse than a shorter one. Such a name is
    // still reachable through the substring and token tiers, which are stable.
    if query_ascii && row.name.is_ascii() {
        let d = edit_distance_within(lower_query, &lower, MAX_EDIT_DISTANCE);
        if d <= MAX_EDIT_DISTANCE {
            return Some((2, d));
        }
    }
    None
}

/// The nearest `SUGGESTION_CAP` names in the index, nearest first. Never
/// substituted for the query and never run: the caller prints these and stops.
pub fn did_you_mean<'g>(names: &'g [GraphName], query: &str) -> Vec<&'g GraphName> {
    if query.is_empty() {
        return Vec::new();
    }
    let lower_query = query.to_lowercase();
    let query_tokens = tokens(query);
    let query_ascii = query.is_ascii();

    let mut scored: Vec<Scored> = names
        .iter()
        .filter_map(|row| {
            score_row(row, query, &lower_query, &query_tokens, query_ascii).map(
                |(rank, penalty)| Scored {
                    row,
                    rank,
                    penalty,
                    kind: kind_rank(&row.kind),
                },
            )
        })
        .collect();
    // Text keys compare by UTF-8 byte order, the same seam
    // `ambiguous_candidates_out` sorts on. `sort_by` is stable, and every key
    // below is total anyway.
    scored.sort_by(|a, b| {
        a.rank
            .cmp(&b.rank)
            .then(a.penalty.cmp(&b.penalty))
            .then(a.kind.cmp(&b.kind))
            .then(a.row.file.cmp(&b.row.file))
            .then(a.row.line.cmp(&b.row.line))
            .then(a.row.name.cmp(&b.row.name))
    });

    let mut seen: Vec<&str> = Vec::new();
    let mut out: Vec<&GraphName> = Vec::new();
    for s in scored {
        if seen.contains(&s.row.name.as_str()) {
            continue;
        }
        seen.push(&s.row.name);
        out.push(s.row);
        if out.len() == SUGGESTION_CAP {
            break;
        }
    }
    out
}

/// One row per candidate: two leading spaces, two between fields. This is the
/// exact text of the zero-hit "did you mean" note printed to stderr.
pub fn suggestion_lines(names: &[GraphName], query: &str) -> Vec<String> {
    did_you_mean(names, query)
        .into_iter()
        .map(|n| format!("  {}  {}  {}:{}", n.name, n.kind, n.file, n.line))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, kind: &str, file: &str, line: usize) -> GraphName {
        GraphName {
            name: name.into(),
            kind: kind.into(),
            file: file.into(),
            line,
            owner: String::new(),
        }
    }

    #[test]
    fn tokens_split_camel_case_snake_and_acronym_runs() {
        assert_eq!(
            tokens("PopulateToolbarItems"),
            vec!["populate", "toolbar", "items"]
        );
        assert_eq!(tokens("_shelfCount"), vec!["shelf", "count"]);
        assert_eq!(tokens("XMLHttpRequest"), vec!["xml", "http", "request"]);
        assert_eq!(
            tokens("Catalog.Shelf.Title"),
            vec!["catalog", "shelf", "title"]
        );
        assert_eq!(tokens("Total total"), vec!["total"], "distinct tokens only");
    }

    #[test]
    fn a_near_miss_name_leads_the_list_on_the_substring_tier() {
        let names = vec![
            row("PopulateToolbarOverflow", "method", "src/Toolbar.cs", 20),
            row("PopulateToolbarItems", "method", "src/Toolbar.cs", 10),
        ];
        let got = suggestion_lines(&names, "PopulateToolbarItem");
        assert_eq!(got[0], "  PopulateToolbarItems  method  src/Toolbar.cs:10");
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn a_name_the_query_swallows_whole_is_not_a_near_miss_for_it() {
        let names = vec![
            row("op", "field", "src/Shell.cs", 15),
            row("ToolbarItem", "class", "src/Item.cs", 8),
        ];
        let got: Vec<&str> = did_you_mean(&names, "PopulateToolbarItem")
            .iter()
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(
            got,
            vec!["ToolbarItem"],
            "the token tier keeps the real name and drops the two-letter field"
        );
    }

    #[test]
    fn a_type_outranks_a_member_a_markup_name_and_a_resource_key_at_equal_distance() {
        let names = vec![
            row("Shelf.Title", "resource-key", "a.resw", 3),
            row("ShelfRoot", "markup-name", "a.xaml", 3),
            row("ShelfCount", "field", "a.cs", 3),
            row("ShelfLoader", "class", "a.cs", 3),
        ];
        let got: Vec<&str> = did_you_mean(&names, "Shelf Missing")
            .iter()
            .map(|n| n.kind.as_str())
            .collect();
        assert_eq!(got, vec!["class", "field", "markup-name", "resource-key"]);
    }

    #[test]
    fn the_list_stops_at_five_distinct_names_however_many_rows_match() {
        let mut names: Vec<GraphName> = (0..9)
            .map(|i| row(&format!("ShelfPart{i}"), "method", "a.cs", i + 1))
            .collect();
        names.push(row("ShelfPart0", "method", "b.cs", 1));
        let got = did_you_mean(&names, "ShelfPart");
        assert_eq!(got.len(), SUGGESTION_CAP);
        assert_eq!(
            got[0].file, "a.cs",
            "the first row of a repeated name wins, the later one is dropped"
        );
    }

    #[test]
    fn nothing_within_the_measure_suggests_nothing() {
        let names = vec![
            row("ShelfLoader", "class", "a.cs", 3),
            row("Anchor", "class", "b.cs", 1),
        ];
        assert!(did_you_mean(&names, "Zzzznomatch").is_empty());
        assert!(did_you_mean(&names, "").is_empty());
    }

    #[test]
    fn edit_distance_reaches_two_and_refuses_three() {
        let names = vec![row("Anchor", "class", "b.cs", 1)];
        assert_eq!(did_you_mean(&names, "Anchxr").len(), 1, "one substitution");
        assert_eq!(did_you_mean(&names, "Anchxy").len(), 1, "two substitutions");
        assert!(
            did_you_mean(&names, "Axchxy").is_empty(),
            "three substitutions is past the bound"
        );
    }

    #[test]
    fn a_non_ascii_name_is_reachable_by_token_but_never_by_edit_distance() {
        let names = vec![
            row("Größe", "property", "a.cs", 1),
            row("GrößeReader", "class", "a.cs", 2),
        ];
        assert!(
            did_you_mean(&names, "Grosse").is_empty(),
            "the edit tier refuses a non-ASCII pair"
        );
        assert_eq!(
            did_you_mean(&names, "GrößeWriter").len(),
            2,
            "the token tier still reaches it"
        );
    }
}
