// Imported-edge reach for `impact`: given the native walk's already-computed
// result and the validated artifact `import-edges` wrote, names every file
// reached only through a foreign edge. This is a lookup over `ImpactModel`,
// not a widening of `impact_walk` itself -- an imported edge reaches a file
// and stops there, the same rule a heuristic edge already follows, so it
// never feeds `add_adj` and never enters the next frontier. Kept out of
// `impact.rs`, which has no room left under its own size ceiling.

use std::collections::{HashMap, HashSet};

use crate::graph;

use super::impact::ImpactModel;
use super::why::Why;

/// One row of imported-edge reach: a file this graph never resolved an edge
/// to, named only because a foreign, producer-asserted record points at (or
/// out of) a file the native walk already reached.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedRow {
    /// The foreign file this row names.
    pub file: String,
    /// The foreign repo id this file belongs to.
    pub repo: String,
    /// One more than the local file's own hop -- the same "one hop of
    /// distance" `impact`'s native hops already mean.
    pub hop: u32,
    /// The producer's own kind for the record that reached this row
    /// (`calls`, `publishes`, `consumes`, or `enqueues`).
    pub imported_kind: String,
    /// Always [`Why::ImportedEdge`]; carried on the row so every row this
    /// module builds already carries the field its consumers read.
    pub why: Why,
}

/// The imported half of an `impact` answer: capped and counted apart from
/// the native rows, so a large import can never evict one of them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportedSection {
    /// The rows actually shown, ranked hop then repo then file.
    pub rows: Vec<ImportedRow>,
    /// How many more distinct foreign files the import named beyond `cap`.
    pub dropped: usize,
    /// Every distinct foreign file the import named, capped or not.
    pub affected: usize,
}

// Every file the native walk already reached, keyed to its own hop -- a seed
// file counts as hop 0, so a foreign edge landing on it reports at hop 1, the
// same "one more hop out" rule a file the walk reached at hop N gives a
// foreign edge landing there (hop N + 1).
fn local_hops(model: &ImpactModel) -> HashMap<&str, u32> {
    let mut hops = HashMap::new();
    for f in &model.seed_files {
        hops.entry(f.as_str()).or_insert(0);
    }
    for r in &model.rows {
        hops.entry(r.file.as_str()).or_insert(r.hop);
    }
    hops
}

fn is_mapped(end: &graph::EdgeEnd, mapped_repo: &str) -> bool {
    end.repo.as_deref() == Some(mapped_repo)
}

// Direct reach: a record whose `to` end resolves into the mapped repo at a
// file the walk already reached makes the record's own foreign `from` file a
// row -- the producer already joined the two sites, so no further widening
// through it is needed or done.
fn direct_rows(
    edges: &graph::ImportedEdges,
    hops: &HashMap<&str, u32>,
    seen: &mut HashSet<(String, String)>,
    out: &mut Vec<ImportedRow>,
) {
    for rec in &edges.edges {
        if is_mapped(&rec.from, &edges.mapped_repo) || !is_mapped(&rec.to, &edges.mapped_repo) {
            continue;
        }
        let Some(to_file) = rec.to.file.as_deref() else {
            continue;
        };
        let Some(&hop) = hops.get(to_file) else {
            continue;
        };
        let (Some(from_repo), Some(from_file)) =
            (rec.from.repo.as_deref(), rec.from.file.as_deref())
        else {
            continue;
        };
        if seen.insert((from_repo.to_string(), from_file.to_string())) {
            out.push(ImportedRow {
                file: from_file.to_string(),
                repo: from_repo.to_string(),
                hop: hop + 1,
                imported_kind: rec.kind.clone(),
                why: Why::ImportedEdge,
            });
        }
    }
}

// Composed reach: a `publishes` record out of the mapped repo, at a file the
// walk already reached, joined through its message `key` to a `consumes`/
// `enqueues` record whose `to` end is foreign, makes that foreign consumer a
// row. Costs one hop, not two -- the message node is not a file -- and never
// reports the arriving `publishes` record's own site as its own consumer.
fn composed_rows(
    edges: &graph::ImportedEdges,
    hops: &HashMap<&str, u32>,
    seen: &mut HashSet<(String, String)>,
    out: &mut Vec<ImportedRow>,
) {
    for pub_rec in &edges.edges {
        if pub_rec.kind != "publishes" || !is_mapped(&pub_rec.from, &edges.mapped_repo) {
            continue;
        }
        let Some(pub_file) = pub_rec.from.file.as_deref() else {
            continue;
        };
        let Some(&hop) = hops.get(pub_file) else {
            continue;
        };
        let Some(msg_key) = pub_rec.key.as_deref() else {
            continue;
        };
        for con_rec in &edges.edges {
            if con_rec.kind != "consumes" && con_rec.kind != "enqueues" {
                continue;
            }
            if con_rec.key.as_deref() != Some(msg_key) {
                continue;
            }
            if is_mapped(&con_rec.to, &edges.mapped_repo) {
                continue;
            }
            if con_rec.to.file.as_deref() == Some(pub_file) && con_rec.to.line == pub_rec.from.line
            {
                continue;
            }
            let (Some(con_repo), Some(con_file)) =
                (con_rec.to.repo.as_deref(), con_rec.to.file.as_deref())
            else {
                continue;
            };
            if seen.insert((con_repo.to_string(), con_file.to_string())) {
                out.push(ImportedRow {
                    file: con_file.to_string(),
                    repo: con_repo.to_string(),
                    hop: hop + 1,
                    imported_kind: con_rec.kind.clone(),
                    why: Why::ImportedEdge,
                });
            }
        }
    }
}

/// Builds the imported section of an `impact` answer.
///
/// Reads the native model's already-finished result and the validated
/// artifact. `cap` bounds `rows` the same way
/// [`super::refs_tables::cap_rows`] bounds the native ones, but
/// independently -- this count never competes with the native cap.
pub fn build_imported_section(
    model: &ImpactModel,
    edges: &graph::ImportedEdges,
    cap: usize,
) -> ImportedSection {
    let hops = local_hops(model);
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut rows = Vec::new();
    direct_rows(edges, &hops, &mut seen, &mut rows);
    composed_rows(edges, &hops, &mut seen, &mut rows);

    rows.sort_by(|a, b| {
        a.hop
            .cmp(&b.hop)
            .then_with(|| a.repo.cmp(&b.repo))
            .then_with(|| a.file.cmp(&b.file))
    });

    let affected = rows.len();
    let dropped = affected.saturating_sub(cap);
    rows.truncate(cap);
    ImportedSection {
        rows,
        dropped,
        affected,
    }
}
