use std::collections::{HashMap, HashSet};

/// Default damping factor for the personalized PageRank pass.
pub const DEFAULT_DAMPING: f64 = 0.85;
/// Default iteration count for the personalized PageRank pass.
pub const DEFAULT_ITERATIONS: u32 = 20;
/// Personalized PageRank over the file-level subgraph. Plain power iteration,
/// no dependencies. Ranking only -- never removes a node.
///
/// **Bit-exactness**: `nodes`' array order drives `idx` (node -> array
/// position) and the `for i in 0..n` iteration order of the main loop, which
/// accumulates into `next[k]` via non-associative `f64` addition (both the
/// per-edge `next[j] += share` and, especially, the dangling-node
/// redistribution `next[k] += damping * r * teleport[k]` summed across every
/// dangling `i`). A different `nodes` order can therefore produce a
/// bit-different (not wrong) result -- callers MUST build `nodes` in a fixed
/// order (`seed_files` then `visited` keys, deduped) for a stable result; see
/// `build_impact_model`. Iterating one node's own out-edge set
/// (`fwd_adj.get(id)`) is NOT order-sensitive: every target in that set is
/// distinct, so each gets exactly one independent `+=` regardless of scan order
/// -- `fwd_adj`'s plain `HashMap<_, HashSet<_>>` is therefore safe.
pub fn personalized_page_rank(
    nodes: &[String],
    fwd_adj: &HashMap<String, HashSet<String>>,
    seeds: &[String],
    damping: f64,
    iterations: u32,
) -> HashMap<String, f64> {
    let n = nodes.len();
    if n == 0 {
        return HashMap::new();
    }
    let idx: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();

    let mut teleport = vec![0f64; n];
    let mut wsum = 0f64;
    for s in seeds {
        if let Some(&i) = idx.get(s.as_str()) {
            teleport[i] = 1.0;
            wsum += 1.0;
        }
    }
    if wsum == 0.0 {
        let v = 1.0 / n as f64;
        for t in teleport.iter_mut() {
            *t = v;
        }
    } else {
        for t in teleport.iter_mut() {
            *t /= wsum;
        }
    }

    let out_lists: Vec<Vec<usize>> = nodes
        .iter()
        .map(|id| match fwd_adj.get(id) {
            None => Vec::new(),
            Some(s) => s
                .iter()
                .filter_map(|t| idx.get(t.as_str()).copied())
                .collect(),
        })
        .collect();

    let mut rank = teleport.clone();
    for _ in 0..iterations {
        let mut next = vec![0f64; n];
        for i in 0..n {
            let r = rank[i];
            if r == 0.0 {
                continue;
            }
            let outs = &out_lists[i];
            if outs.is_empty() {
                for k in 0..n {
                    next[k] += damping * r * teleport[k]; // dangling node: no rank leakage
                }
                continue;
            }
            let share = (damping * r) / outs.len() as f64;
            for &j in outs {
                next[j] += share;
            }
        }
        for k in 0..n {
            next[k] += (1.0 - damping) * teleport[k];
        }
        rank = next;
    }

    let mut result = HashMap::with_capacity(n);
    for (i, id) in nodes.iter().enumerate() {
        result.insert(id.clone(), rank[i]);
    }
    result
}
