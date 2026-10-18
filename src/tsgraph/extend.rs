use std::path::Path;

use crate::extract::TsFragment;
use crate::graph::Graph;

use super::{read_ts_resolution, resolve_ts_graph_in};

pub(crate) fn extend_graph(root: &Path, fragments: &[(String, TsFragment)], graph: &mut Graph) {
    if fragments.is_empty() {
        return;
    }
    let (alias, config) = read_ts_resolution(root, fragments.iter().map(|(file, _)| file.as_str()));
    let ts = resolve_ts_graph_in(fragments, &alias, &config);
    graph.defs.extend(ts.defs);
    graph.names.extend(ts.names);
    graph.edges.extend(ts.edges);
    graph.stats.def_count = graph.defs.len();
    graph.stats.edges_by_kind.import = Some(ts.edges_by_kind.import);
    graph.stats.edges_by_kind.call = Some(ts.edges_by_kind.call);
    graph.stats.edges_by_kind.jsx_use = Some(ts.edges_by_kind.jsx_use);
    graph.stats.edges_by_kind.dispatch = Some(ts.edges_by_kind.dispatch);
    graph.stats.ts = Some(ts.stats);
    graph.unresolved_imports = ts.unresolved_imports;
}
