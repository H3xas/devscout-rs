use std::collections::HashMap;

use super::super::scope::{glob_cost, reset_glob_cost, Bound, Ns, Tables, Target};

fn arc(t: &mut Tables, from: usize, to: usize) {
    let index = t.resolved.len();
    t.resolved.push(Bound {
        types: vec![Target::Module(to)],
        values: Vec::new(),
    });
    t.module_globs[from].push(index);
}

fn tables(topology: &str, size: usize, outcome: &str, name: &str, ns: Ns) -> Tables {
    let mut t = Tables {
        defs: Vec::new(),
        def_by_path: HashMap::new(),
        items: vec![HashMap::new(); size + 2],
        leaves: Vec::new(),
        module_named: vec![HashMap::new(); size + 2],
        module_globs: vec![Vec::new(); size + 2],
        block_named: HashMap::new(),
        block_globs: HashMap::new(),
        block_parent: vec![vec![None]],
        resolved: Vec::new(),
        assoc: HashMap::new(),
        impl_self: HashMap::new(),
        opaque_modules: vec![false; size + 2],
    };
    match topology {
        "chain" => {
            for from in 0..size {
                arc(&mut t, from, from + 1);
            }
        }
        "wide" => {
            for to in 1..=size {
                arc(&mut t, 0, to);
            }
        }
        "diamond" => {
            arc(&mut t, 0, 1);
            arc(&mut t, 0, 2);
            for from in 1..size {
                arc(&mut t, from, from + 1);
                if from + 2 <= size {
                    arc(&mut t, from, from + 2);
                }
            }
        }
        "cycle" => {
            arc(&mut t, 0, 1);
            for from in 1..size {
                arc(&mut t, from, from + 1);
            }
            arc(&mut t, size, 1);
        }
        _ => panic!("unknown topology: {topology}"),
    }
    if outcome == "found" || outcome == "ambiguous" {
        let entry = t.items[size].entry(name.to_string()).or_default();
        match ns {
            Ns::Type => entry.types.push(Target::Def(0)),
            Ns::Value => entry.values.push(Target::Def(0)),
        }
    }
    if outcome == "ambiguous" {
        arc(&mut t, 0, size);
        let entry = t.items[1].entry(name.to_string()).or_default();
        match ns {
            Ns::Type => entry.types.push(Target::Def(1)),
            Ns::Value => entry.values.push(Target::Def(1)),
        }
    }
    if outcome == "opaque" {
        t.opaque_modules[size] = true;
    }
    t.block_globs.insert((0, 0), t.module_globs[0].clone());
    t
}

fn lookup(t: &Tables, location: &str, name: &str, ns: Ns) -> (Vec<Target>, Vec<usize>) {
    let mut probe = Vec::new();
    let found = match location {
        "module" => t.in_module(0, name, ns, false, &mut probe),
        "block" => t.in_blocks(0, Some(0), name, ns, false, &mut probe),
        _ => panic!("unknown location: {location}"),
    };
    (found.unwrap_or_default(), probe)
}

fn change_first_glob(t: &mut Tables, size: usize) {
    t.resolved[0].types = vec![Target::Module(size + 1)];
    t.invalidate_glob_cache();
}

#[test]
fn generated_glob_cost_row() {
    let Ok(spec) = std::env::var("RESOLVER_COST_GLOB") else {
        return;
    };
    let parts: Vec<&str> = spec.split(',').collect();
    assert_eq!(parts.len(), 7);
    let topology = parts[0];
    let size: usize = parts[1].parse().unwrap();
    let queries: usize = parts[2].parse().unwrap();
    let outcome = parts[3];
    let ns = match parts[4] {
        "type" => Ns::Type,
        "value" => Ns::Value,
        other => panic!("unknown namespace: {other}"),
    };
    let location = parts[5];
    let shape = parts[6];
    let mut t = tables(topology, size, outcome, "needle", ns);
    let names: Vec<String> = (0..queries)
        .map(|i| {
            if shape != "distinct" {
                "needle".to_string()
            } else {
                format!("needle{i}")
            }
        })
        .collect();
    let distinct: Vec<&str> = if shape != "distinct" {
        vec!["needle"]
    } else {
        names.iter().map(String::as_str).collect()
    };
    let mut ref_visits = 0;
    let mut ref_arcs = 0;
    for name in distinct {
        let reference = tables(topology, size, outcome, "needle", ns);
        reset_glob_cost();
        lookup(&reference, location, name, ns);
        let cost = glob_cost();
        ref_visits += cost.visits;
        ref_arcs += cost.arcs;
    }
    if shape == "mutate" {
        let mut reference = tables(topology, size, outcome, "needle", ns);
        change_first_glob(&mut reference, size);
        reset_glob_cost();
        lookup(&reference, location, "needle", ns);
        let cost = glob_cost();
        ref_visits += cost.visits;
        ref_arcs += cost.arcs;
    }
    reset_glob_cost();
    let mut result_bytes = 0;
    let mut probe_entries = 0;
    let mut first_found = None;
    let mut last_found = None;
    for (index, name) in names.iter().enumerate() {
        if shape == "mutate" && index == queries / 2 {
            change_first_glob(&mut t, size);
        }
        let (found, probe) = lookup(&t, location, name, ns);
        if index == 0 {
            first_found = Some(found.clone());
        }
        last_found = Some(found.clone());
        result_bytes += found.len() * std::mem::size_of::<Target>();
        probe_entries += probe.len();
    }
    if shape == "mutate" {
        assert_ne!(
            first_found, last_found,
            "changed import leaf must change lookup result"
        );
    }
    let cost = glob_cost();
    let row = serde_json::json!({
        "family": "glob", "topology": topology, "size": size, "queries": queries,
        "outcome": outcome, "namespace": parts[4], "location": location, "shape": shape,
        "vertices": size + 2, "arcs": t.resolved.len(), "distinct_keys": if shape == "distinct" {queries} else if shape == "mutate" {2} else {1},
        "vwork": cost.visits, "ework": cost.arcs, "vref": ref_visits, "eref": ref_arcs,
        "probes": probe_entries, "candidates": cost.candidates,
        "copied_entries": cost.probes + cost.candidates, "result_bytes": result_bytes,
    });
    println!("COST_ROW={row}");
}
