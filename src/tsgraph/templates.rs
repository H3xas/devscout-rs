// The Angular template join: each exported `@Component` class gets a
// `graph.names` row per instance member, and each template read of a member
// becomes a precise `uses-member` edge from the template line to the class.
//
// The join runs over every fragment on every rebuild, cached ones included,
// which is what keeps a template edit correct: the `.html` file's own cache
// key moves, its fragment is extracted again, and this join pairs it with
// the unchanged component fragment it already had. A scanner gap reaches
// graph.json only through a component that names its template, so an `.html`
// file no component names adds nothing.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::extract::{TsComponent, TsFragment};
use crate::graph::{Edge, Graph, GraphName, HtmlFragment};
use crate::ngtemplate::{GapReason, TemplateGap, TemplateRead};

use super::{dir_of, join_rel, ts_def_id};

/// One template site the index could not bind: a graph.json
/// `template_unsupported` row. Rows sort by file, then line, then reason.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TemplateUnsupported {
    /// The file the site is in: the template, or the component's own file.
    pub file: String,
    /// The 1-based line of the site.
    pub line: usize,
    /// Why the site gave no edge.
    pub reason: GapReason,
}

#[derive(Default)]
struct Join {
    names: Vec<GraphName>,
    edges: Vec<Edge>,
    seen: HashSet<(String, usize, String, String)>,
    rows: BTreeSet<TemplateUnsupported>,
}

impl Join {
    fn row(&mut self, file: &str, line: usize, reason: GapReason) {
        self.rows.insert(TemplateUnsupported {
            file: file.to_string(),
            line,
            reason,
        });
    }

    // One template's reads and gaps, as `owner` (declared in `owner_file`)
    // sees them from `from`.
    fn template(
        &mut self,
        component: &TsComponent,
        (owner, owner_file): (&str, &str),
        from: &str,
        (reads, gaps): (&[TemplateRead], &[TemplateGap]),
    ) {
        for gap in gaps {
            self.row(from, gap.line, gap.reason);
        }
        for read in reads {
            if !component.members.iter().any(|m| m.name == read.name) {
                self.row(from, read.line, GapReason::UnknownMember);
                continue;
            }
            let key = (
                from.to_string(),
                read.line,
                owner.to_string(),
                read.name.clone(),
            );
            if self.seen.insert(key) {
                self.edges.push(Edge::uses_member(
                    from.to_string(),
                    read.line,
                    owner.to_string(),
                    owner_file.to_string(),
                    Some(read.name.clone()),
                    None,
                ));
            }
        }
    }

    fn component(
        &mut self,
        file: &str,
        component: &TsComponent,
        templates: &HashMap<&str, &HtmlFragment>,
    ) {
        let owner = ts_def_id(file, &component.name);
        self.names
            .extend(component.members.iter().map(|m| GraphName {
                name: m.name.clone(),
                kind: m.kind.clone(),
                file: file.to_string(),
                line: m.line,
                owner: owner.clone(),
            }));
        for gap in &component.gaps {
            self.row(file, gap.line, gap.reason);
        }
        if let Some(scan) = &component.inline {
            self.template(component, (&owner, file), file, (&scan.reads, &scan.gaps));
        }
        let Some(url) = &component.template_url else {
            return;
        };
        let target = join_rel(dir_of(file), &url.spec)
            .and_then(|rel| templates.get_key_value(rel.as_str()).map(|(r, f)| (*r, *f)));
        match target {
            Some((rel, html)) => {
                self.template(component, (&owner, file), rel, (&html.reads, &html.gaps));
            }
            None => self.row(file, url.line, GapReason::TemplateUrlUnresolved),
        }
    }
}

/// Appends the template facts to a resolved graph: member rows after the
/// names already there, `uses-member` edges counted with the others, and the
/// unsupported rows with their count in `stats.ts`. A graph with no exported
/// component keeps every byte.
pub(crate) fn append_templates(
    ts: &[(String, TsFragment)],
    html: &[(String, HtmlFragment)],
    graph: &mut Graph,
) {
    let templates: HashMap<&str, &HtmlFragment> =
        html.iter().map(|(rel, f)| (rel.as_str(), f)).collect();
    let defs: HashSet<&str> = graph.defs.iter().map(|d| d.id.as_str()).collect();
    let mut join = Join::default();
    for (file, fragment) in ts {
        for component in &fragment.components {
            if defs.contains(ts_def_id(file, &component.name).as_str()) {
                join.component(file, component, &templates);
            }
        }
    }
    graph.stats.edges_by_kind.uses_member += join.edges.len();
    if let Some(stats) = graph.stats.ts.as_mut() {
        stats.template_unsupported_count = join.rows.len();
    }
    graph.names.extend(join.names);
    graph.edges.extend(join.edges);
    graph.template_unsupported = join.rows.into_iter().collect();
}
