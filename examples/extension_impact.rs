//! Offline replay of native impact answers with a fixed extension candidate cohort.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use devscout_rs::{graph, query};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct Spec {
    candidates: Vec<usize>,
    seeds: Vec<String>,
}

fn admit(index: &mut query::GraphIndex<'_>, numbers: &[usize]) -> Result<(), String> {
    for &number in numbers {
        let Some(graph::Edge::UsesMember {
            to,
            heuristic: true,
            tier: Some(graph::HeuristicTier::Ext),
            ..
        }) = index.graph.edges.get(number)
        else {
            return Err(format!("candidate {number} is not extension evidence"));
        };
        let entry = index
            .heuristic_inbound
            .get_mut(to)
            .ok_or_else(|| format!("candidate {number} has no heuristic adjacency"))?;
        let position = entry
            .uses_member
            .iter()
            .position(|&value| value == number)
            .ok_or_else(|| format!("candidate {number} is missing or duplicated"))?;
        entry.uses_member.remove(position);
        index
            .inbound
            .entry(to.clone())
            .or_default()
            .uses_member
            .push(number);
    }
    Ok(())
}

fn answer(index: &query::GraphIndex<'_>, seed: &str, cap: usize) -> Result<Value, String> {
    let query::ImpactResult::Resolved(model) = query::build_impact_model(
        index,
        seed,
        query::DEFAULT_HOPS,
        cap,
        true,
        query::DEFAULT_IFACE_MAX_FANIN,
        query::DEFAULT_HUB_MAX_INDEGREE,
    ) else {
        return Err(format!("seed is not uniquely resolved: {seed}"));
    };
    Ok(json!({
        "rows": model.rows.iter().map(|row| json!({
            "file": row.file, "hop": row.hop, "heuristic": row.heuristic,
            "why": row.why.as_str(), "from_lines": row.from_lines,
            "symbols": row.top_symbols, "infra": row.infra,
        })).collect::<Vec<_>>(),
        "dropped": model.dropped,
        "total_affected": model.total_affected,
        "heuristic_affected": model.heuristic_affected,
        "tests_affected": model.tests_affected,
        "braked_interfaces": model.braked.iter().map(|b| json!({
            "iface": b.iface, "fanin": b.fanin,
        })).collect::<Vec<_>>(),
        "braked_files": model.braked_files.iter().map(|b| json!({
            "file": b.file, "indegree": b.indegree,
        })).collect::<Vec<_>>(),
    }))
}

fn truth(index: &query::GraphIndex<'_>, seed: &str) -> Result<Value, String> {
    let query::SeedResolution::Resolved { ids, .. } = query::resolve_impact_seed(index, seed)
    else {
        return Err(format!("oracle seed is not uniquely resolved: {seed}"));
    };
    // Oracle truncation would turn valid predictions beyond a brake into false positives.
    let walk = query::impact_walk(index, &ids, query::DEFAULT_HOPS, true, 0, 0);
    Ok(json!({"rows": walk.visited.iter()
        .filter(|(_, row)| row.via_count > 0 || row.ambiguous_count > 0)
        .map(|(file, row)| json!({"file": file, "hop": row.hop, "heuristic": false}))
        .collect::<Vec<_>>() }))
}

fn run(directory: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let baseline: graph::Graph =
        serde_json::from_slice(&fs::read(directory.join("baseline.json"))?)?;
    let oracle: graph::Graph = serde_json::from_slice(&fs::read(directory.join("oracle.json"))?)?;
    let spec: Spec = serde_json::from_slice(&fs::read(directory.join("spec.json"))?)?;
    let root = directory.join("unmapped");
    let baseline_index = query::load_graph_index(&baseline, &root);
    let mut candidate_index = query::load_graph_index(&baseline, &root);
    let oracle_index = query::load_graph_index(&oracle, &root);
    admit(&mut candidate_index, &spec.candidates)?;
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    for seed in spec.seeds {
        let record = json!({
            "seed": seed,
            "baseline": answer(&baseline_index, &seed, query::DEFAULT_CAP)?,
            "candidate": answer(&candidate_index, &seed, query::DEFAULT_CAP)?,
            "oracle": truth(&oracle_index, &seed)?,
        });
        serde_json::to_writer(&mut out, &record)?;
        writeln!(out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> graph::Graph {
        serde_json::from_value(json!({
            "schema_version": 3, "built_at_head": null,
            "defs": (["Alpha", "Beta", "Gamma", "Delta", "Epsilon"].iter().map(|name| json!({
                "id": name, "name": name, "namespace": "", "kind": "class",
                "file": format!("{name}.cs"), "line": 1, "methods": ["Run"],
            })).collect::<Vec<_>>()),
            "edges": [
                graph::Edge::uses_member("Beta.cs".into(), 7, "Alpha".into(), "Alpha.cs".into(),
                    Some("Run".into()), Some(graph::HeuristicTier::Ext)),
                graph::Edge::uses_member("Gamma.cs".into(), 7, "Beta".into(), "Beta.cs".into(),
                    Some("Run".into()), Some(graph::HeuristicTier::Ext)),
                graph::Edge::uses_member("Delta.cs".into(), 7, "Gamma".into(), "Gamma.cs".into(),
                    Some("Run".into()), Some(graph::HeuristicTier::Guess)),
                graph::Edge::uses_member("Epsilon.cs".into(), 7, "Delta".into(), "Delta.cs".into(),
                    Some("Run".into()), None),
            ],
            "stats": {"def_count": 5, "file_count": 5, "edges_by_kind": graph::EdgesByKind::default(),
                "ambiguous_count": 0, "ambiguous_pct": 0.0, "unresolved_external_count": 0},
        })).unwrap()
    }

    #[test]
    fn two_extension_hops_add_a_file_without_changing_graph_evidence() {
        let graph = fixture();
        let before = graph.clone();
        let mut index = query::load_graph_index(&graph, Path::new("unmapped"));
        assert_eq!(
            answer(&index, "Alpha.cs", 50).unwrap()["rows"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        admit(&mut index, &[0, 1]).unwrap();
        let result = answer(&index, "Alpha.cs", 50).unwrap();
        let rows = result["rows"].as_array().unwrap();
        assert!(rows
            .iter()
            .any(|row| row["file"] == "Gamma.cs" && row["hop"] == 2));
        assert!(!rows.iter().any(|row| row["file"] == "Delta.cs"));
        assert_eq!(graph, before);
    }

    #[test]
    fn guess_cannot_be_admitted_or_used_to_reach_its_caller() {
        let graph = fixture();
        let mut index = query::load_graph_index(&graph, Path::new("unmapped"));
        assert!(admit(&mut index, &[2]).is_err());
        let result = answer(&index, "Gamma.cs", 50).unwrap();
        let rows = result["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["file"], "Delta.cs");
        assert_eq!(rows[0]["heuristic"], true);
    }

    #[test]
    fn oracle_truth_is_not_truncated_by_a_numerical_hub_brake() {
        let mut graph = fixture();
        graph.edges[0] = graph::Edge::uses_member(
            "Beta.cs".into(),
            7,
            "Alpha".into(),
            "Alpha.cs".into(),
            Some("Run".into()),
            None,
        );
        graph.edges[1] = graph::Edge::uses_member(
            "Gamma.cs".into(),
            7,
            "Beta".into(),
            "Beta.cs".into(),
            Some("Run".into()),
            None,
        );
        for number in 0..35 {
            let mut definition = graph.defs[0].clone();
            definition.id = format!("Caller{number}");
            definition.name = definition.id.clone();
            definition.file = format!("Caller{number}.cs");
            graph.edges.push(graph::Edge::uses_member(
                definition.file.clone(),
                7,
                "Beta".into(),
                "Beta.cs".into(),
                Some("Run".into()),
                None,
            ));
            graph.defs.push(definition);
        }
        let index = query::load_graph_index(&graph, Path::new("unmapped"));
        let predicted = answer(&index, "Alpha.cs", 50).unwrap();
        let expected = truth(&index, "Alpha.cs").unwrap();
        assert!(!predicted["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["file"] == "Gamma.cs"));
        assert!(expected["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["file"] == "Gamma.cs"));
    }

    #[test]
    fn an_empty_cohort_restores_the_baseline_answer() {
        let graph = fixture();
        let baseline = query::load_graph_index(&graph, Path::new("unmapped"));
        let mut suppressed = query::load_graph_index(&graph, Path::new("unmapped"));
        admit(&mut suppressed, &[]).unwrap();
        assert_eq!(
            answer(&baseline, "Alpha.cs", 50),
            answer(&suppressed, "Alpha.cs", 50)
        );
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args_os()
        .nth(1)
        .ok_or("usage: extension_impact <replay-directory>")?;
    run(Path::new(&directory))
}
