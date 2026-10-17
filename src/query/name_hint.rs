// The `tests` name hint: when nothing tests a symbol, name the test files
// whose own file NAME matches it, and the production symbols each of those
// files references. Split out of `coverage.rs` to keep that file under this
// crate's per-file line budget, and because the rule is self-contained: it
// reads the index the verb already built and walks no edge outside the files
// it names.
//
// A file name is an observation about a name, never a test reference, so
// nothing here feeds a row, a count or an outcome. `coverage.rs` fills
// `TestsModel::name_hint` from this module on the zero case alone, so a
// symbol something really tests never carries a hint beside that answer.

use std::collections::{BTreeSet, HashMap};

use crate::graph;

use super::index::{def_files, GraphIndex};
use super::test_path;
use super::ts_edges;

/// How many reference ids one hint row lists. The count of distinct ids the
/// file references is reported separately as `reference_total`, so a capped
/// list still states how much it left out.
pub const NAME_HINT_REFERENCE_CAP: usize = 5;

/// One test file whose name matches the queried symbol.
///
/// Its `file` path, the production def ids it references most often
/// (`references`, capped at [`NAME_HINT_REFERENCE_CAP`]), and
/// `reference_total`, the distinct id count before that cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameHintRow {
    /// The file value.
    pub file: String,
    /// The production def ids this file references, most-referenced first.
    pub references: Vec<String>,
    /// How many distinct production def ids the file references.
    pub reference_total: usize,
}

// The three affixes a name-matching test file ends in, longest first: trying
// `tests` before `test` is what keeps `FooTests` from matching as `FooT` plus
// a `est` that no affix names.
const AFFIXES: [&str; 3] = ["tests", "test", "spec"];

/// Lowercase `s` and drop the characters a name convention treats as word
/// separators: `-`, `_` and `.` (the last one also joining a `foo.spec` stem
/// to its own affix).
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '-' | '_' | '.'))
        .flat_map(char::to_lowercase)
        .collect()
}

// The file name's last path segment without its last extension, or `None` for
// a segment that carries no extension at all -- a bare `.spec` has no stem to
// match on, so it is never a candidate rather than matching on its own dot.
fn stem(file: &str) -> Option<&str> {
    let segment = file.rsplit('/').next()?;
    let dot = segment.rfind('.')?;
    (dot > 0).then(|| &segment[..dot])
}

/// Whether the test file `file` is named after `name` under the stem rule.
///
/// The stem is compared in lowercase with `-`, `_` and `.` removed. It must
/// end in exactly one of `tests`, `test` or `spec`, and the rest of the stem
/// must equal `name` under the same normalization. A stem carrying no affix
/// never matches, so `Foo.cs` is not a hint for `Foo`.
pub fn stem_matches(name: &str, file: &str) -> bool {
    let Some(stem) = stem(file) else {
        return false;
    };
    let normalized = normalize(stem);
    let wanted = normalize(name);
    AFFIXES
        .iter()
        .any(|affix| normalized.strip_suffix(affix) == Some(wanted.as_str()))
}

// The def ids one precise edge targets. The C# kind and the three
// single-target TS kinds carry `to`; an `import` edge binds every definition
// it names, so it contributes each of them. Anything else -- a namespace
// `import`, a `bus-hop`, a heuristic kind -- targets nothing this module
// lists, and answers empty rather than a guess.
fn edge_targets(e: &graph::Edge) -> Vec<&str> {
    match e {
        graph::Edge::Inherits { to, .. }
        | graph::Edge::UsesType { to, .. }
        | graph::Edge::UsesMember { to, .. }
        | graph::Edge::Implements { to, .. }
        | graph::Edge::Overrides { to, .. } => vec![to.as_str()],
        other => ts_edges::admitted_targets(other).unwrap_or_default(),
    }
}

// A target counts only when it resolves to a def AND that def is declared
// outside a test file: a helper inside the test project is not a production
// symbol, so naming it would misstate what the hint observes.
fn is_production_target(index: &GraphIndex, to: &str) -> bool {
    index
        .by_id
        .get(to)
        .is_some_and(|&i| !index.is_test_file(&index.graph.defs[i].file))
}

// Every file the index has any record of. A test file with no precise edge at
// all still earns a hint row -- its name is the misleading fact, whatever it
// references -- so the heuristic adjacency and the test-def map are candidate
// sources too, not just the precise outbound one.
fn candidate_files<'a>(index: &'a GraphIndex<'_>) -> BTreeSet<&'a str> {
    index
        .by_file
        .keys()
        .chain(index.test_defs_by_file.keys())
        .chain(index.outbound_by_file.keys())
        .chain(index.heuristic_outbound_by_file.keys())
        .map(String::as_str)
        .collect()
}

// One row: the file's references to production symbols, most-referenced
// first, ties by id. `bus-hop` edges never contribute -- a possible route is
// not a reference the file makes -- and neither does the namespace `imports`
// list, whose entries name no def.
fn name_hint_row(index: &GraphIndex, file: &str) -> NameHintRow {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    if let Some(out) = index.outbound_by_file.get(file) {
        let precise = out
            .inherits
            .iter()
            .chain(&out.uses_type)
            .chain(&out.uses_member)
            .chain(&out.implements)
            .chain(&out.overrides)
            .chain(&out.ts.call)
            .chain(&out.ts.jsx_use)
            .chain(&out.ts.dispatch)
            .chain(&out.ts.import);
        for &i in precise {
            for to in edge_targets(&index.graph.edges[i]) {
                if is_production_target(index, to) {
                    *counts.entry(to).or_default() += 1;
                }
            }
        }
    }
    let reference_total = counts.len();
    let mut ranked: Vec<(&str, usize)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let references = ranked
        .into_iter()
        .take(NAME_HINT_REFERENCE_CAP)
        .map(|(id, _)| id.to_string())
        .collect();
    NameHintRow {
        file: file.to_string(),
        references,
        reference_total,
    }
}

/// The test files named after `id`, in path order, each with the production
/// symbols it references. A file is a candidate when its stem matches the
/// symbol's own name, when the verb's own test-file rule vouches for it, and
/// when it is not one of the symbol's own declaring files.
pub fn name_hint_rows(index: &GraphIndex, id: &str) -> Vec<NameHintRow> {
    let Some(&sym) = index.by_id.get(id) else {
        return Vec::new();
    };
    let name = index.graph.defs[sym].name.clone();
    let own = def_files(index, id);
    candidate_files(index)
        .into_iter()
        .filter(|file| !own.iter().any(|f| f == file))
        .filter(|file| stem_matches(&name, file))
        .filter(|file| test_path::test_via(index, file).is_some())
        .map(|file| name_hint_row(index, file))
        .collect()
}
