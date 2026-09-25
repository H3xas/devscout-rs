// `load` -- filesystem. Returns a plain `String` error, the shape every
// `cmd_*` in cli.rs already wraps as `"error: {msg}"`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::model::{AuditOptions, DefRow, EdgeRow, Inputs, OracleRef, Tier, Unit};

/// Reads graph.json, the oracle's `refs.jsonl`/`units.jsonl`/`defs.jsonl`,
/// and the manifest, and assembles `Inputs`. No scoring happens here.
pub fn load(root: &Path, opts: &AuditOptions) -> Result<Inputs, String> {
    let graph_path = crate::graph::graph_json_path(root);
    let graph_text = std::fs::read_to_string(&graph_path).map_err(|_| {
        "no graph.json for this repo — run `devscout map` on a C# scope first".to_string()
    })?;
    let graph_value: serde_json::Value = serde_json::from_str(&graph_text).map_err(|e| {
        format!(
            "graph.json at {} is not valid JSON: {e}",
            graph_path.display()
        )
    })?;
    let (graph_defs, edges) = parse_graph(&graph_value)?;

    let records = read_jsonl::<OracleRef>(opts.semantic)?;
    let units = match opts.units {
        Some(p) => read_jsonl::<Unit>(p)?,
        None => Vec::new(),
    };
    let oracle_defs = match opts.defs {
        Some(p) => read_jsonl::<DefRow>(p)?,
        None => Vec::new(),
    };
    let universe = build_universe(root, &graph_value, &units);
    let lane = lane_of(&graph_value);
    let discovered_shapes = discovered_occurrence_shapes(root);

    Ok(Inputs {
        root: root.to_path_buf(),
        graph_defs,
        oracle_defs,
        edges,
        records,
        units,
        universe,
        lane,
        discovered_shapes,
        collect_fp_sites: false,
    })
}

/// Reads this checkout's own admitted compiler-facts artifact (if any) and
/// indexes every occurrence site's `shape` by `(file, name-line, target
/// member)` -- the same triple `SemanticDiscovered` edges join on. Never an
/// error: an absent or unreadable artifact, or one with no `occurrences`
/// array at all, simply yields an empty map, the same fail-open convention
/// `graph::read_compiler_facts` itself follows. Reads the artifact's raw
/// bytes directly (`crate::graph::read_compiler_facts`, the one admitted-
/// artifact reader this crate has), never `src/semantic/`'s own parse --
/// this module has no reason to translate a Roslyn type id or evaluate
/// freshness, only to read one field the artifact already writes verbatim
/// (`tools/scout-semantic/CompilerFacts.cs`'s `writer.WriteString("shape",
/// o.Shape)`).
fn discovered_occurrence_shapes(root: &Path) -> HashMap<(String, usize, String), String> {
    let mut shapes = HashMap::new();
    let Some(facts) = crate::graph::read_compiler_facts(root) else {
        return shapes;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&facts.bytes) else {
        return shapes;
    };
    let Some(sites) = value
        .get("occurrences")
        .and_then(|o| o.get("sites"))
        .and_then(|s| s.as_array())
    else {
        return shapes;
    };
    for site in sites {
        let (Some(file), Some(shape)) = (
            site.get("file").and_then(|x| x.as_str()),
            site.get("shape").and_then(|x| x.as_str()),
        ) else {
            continue;
        };
        let Some(line) = site
            .get("name")
            .and_then(|n| n.get("line"))
            .and_then(|x| x.as_u64())
        else {
            continue;
        };
        let Some(member) = site
            .get("target")
            .and_then(|t| t.get("member"))
            .and_then(|x| x.as_str())
        else {
            continue;
        };
        shapes.insert(
            (file.to_string(), line as usize, member.to_string()),
            shape.to_string(),
        );
    }
    shapes
}

/// `"enriched"` when graph.json's own top-level `stats.semantic` key is
/// present (regardless of whether it holds any nonzero counter -- a run
/// that admitted an artifact and confirmed nothing still ran the enriched
/// lane), else `"syntax"`. See `Inputs::lane`'s own doc comment.
fn lane_of(graph_value: &serde_json::Value) -> &'static str {
    if graph_value
        .get("stats")
        .and_then(|s| s.get("semantic"))
        .is_some()
    {
        "enriched"
    } else {
        "syntax"
    }
}

/// Parses one non-empty line per record via `T`'s `Deserialize`. The error
/// carries the file and 1-based line number, matching how a human would
/// point at a bad line in a `.jsonl` file.
fn read_jsonl<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let rec: T =
            serde_json::from_str(line).map_err(|e| format!("{}:{}: {e}", path.display(), i + 1))?;
        out.push(rec);
    }
    Ok(out)
}

/// Walks graph.json's `defs`/`edges` arrays by hand (a `serde_json::Value`,
/// not the typed `graph::Graph` -- see the `audit` module header) into the
/// reduced `DefRow`/`EdgeRow` shapes scoring needs. A missing/malformed
/// field reads as its type's default rather than erroring: graph.json is
/// devscout's own artifact and any shape it can produce is one this
/// function should survive (an edge with the wrong `kind` is simply
/// filtered out, never a hard error).
pub fn parse_graph(v: &serde_json::Value) -> Result<(Vec<DefRow>, Vec<EdgeRow>), String> {
    let defs_arr = v
        .get("defs")
        .and_then(|d| d.as_array())
        .ok_or("graph.json has no 'defs' array")?;
    let mut defs = Vec::with_capacity(defs_arr.len());
    for d in defs_arr {
        defs.push(DefRow {
            id: str_field(d, "id"),
            file: str_field(d, "file"),
            kind: str_field(d, "kind"),
            test: d
                .get("testMethods")
                .and_then(|x| x.as_array())
                .is_some_and(|a| !a.is_empty()),
        });
    }

    let edges_arr = v
        .get("edges")
        .and_then(|e| e.as_array())
        .ok_or("graph.json has no 'edges' array")?;
    let mut edges = Vec::new();
    for e in edges_arr {
        if e.get("kind").and_then(|k| k.as_str()) != Some("uses-member") {
            continue;
        }
        edges.push(EdgeRow {
            from_file: str_field(e, "from_file"),
            from_line: e.get("from_line").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
            to: str_field(e, "to"),
            to_file: str_field(e, "to_file"),
            tier: tier_of(e),
            member: e
                .get("member")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
        });
    }
    Ok((defs, edges))
}

fn str_field(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

/// Tier from an edge's raw JSON. Checked before anything else: the `source`
/// string an enriched-lane edge carries (`"semantic"` or
/// `"semantic-discovered"` -- `graph::edge::SemanticProvenance`'s own
/// kebab-case serialization), which no syntax-lane edge ever has, so this
/// check is a no-op for every edge a syntax-only build produces. Otherwise
/// the `tier` string if present (only `"ext"` and `"guess"` are meaningful
/// spellings today -- anything else falls through to the `heuristic` bool,
/// the same as no `tier` key at all), else legacy `heuristic: true` ->
/// `Heuristic`, else `Precise`.
fn tier_of(e: &serde_json::Value) -> Tier {
    match e.get("source").and_then(|x| x.as_str()) {
        Some("semantic") => return Tier::Semantic,
        Some("semantic-discovered") => return Tier::SemanticDiscovered,
        _ => {}
    }
    match e.get("tier").and_then(|x| x.as_str()) {
        Some("ext") => return Tier::Ext,
        Some("guess") => return Tier::Guess,
        _ => {}
    }
    if e.get("heuristic")
        .and_then(|x| x.as_bool())
        .unwrap_or(false)
    {
        Tier::Heuristic
    } else {
        Tier::Precise
    }
}

/// The audit's file universe: the manifest's mapped-file set (`entries`
/// object keys) if a manifest is present and non-empty; otherwise every file
/// named by a graph.json def or edge. A corrupt manifest fails open (falls
/// back to the graph-files set) rather than aborting the whole audit over an
/// unrelated artifact.
///
/// With no `--units`, that set IS the universe, unchanged (there is no
/// per-file compile status to narrow it by). With `--units`, the universe
/// instead becomes that set intersected with the UNION of `files` across
/// every unit whose `status` is `"ok"` -- not the older subtractive rule
/// (start from every mapped file, remove a failed unit's own files), because
/// subtracting only reaches a unit `units.jsonl` actually lists. A project
/// absent from the compiled `.sln` altogether -- never a `units` entry at
/// all, ok or failed -- has no ground truth either, and the old rule left
/// its files in the universe by omission; the new one excludes them by
/// construction, since they can never appear in the ok-unit union. Either
/// way, a file the oracle never compiled carries no reliable ground truth
/// (`score` drops both the oracle records AND the graph edges this filters
/// out).
fn build_universe(root: &Path, graph_value: &serde_json::Value, units: &[Unit]) -> HashSet<String> {
    let mut universe: HashSet<String> = HashSet::new();
    if let Ok(Some(m)) = crate::manifest::read_manifest(root) {
        if let Some(entries) = m.get("entries").and_then(crate::manifest::Value::as_object) {
            universe.extend(entries.iter().map(|(k, _)| k.clone()));
        }
    }
    if universe.is_empty() {
        for key in ["defs", "edges"] {
            if let Some(arr) = graph_value.get(key).and_then(|x| x.as_array()) {
                for row in arr {
                    for file_key in ["file", "from_file", "to_file"] {
                        if let Some(f) = row.get(file_key).and_then(|x| x.as_str()) {
                            universe.insert(f.to_string());
                        }
                    }
                }
            }
        }
    }
    if units.is_empty() {
        return universe;
    }
    let ok_unit_files: HashSet<&str> = units
        .iter()
        .filter(|u| u.status == "ok")
        .flat_map(|u| u.files.iter().map(String::as_str))
        .collect();
    universe.retain(|f| ok_unit_files.contains(f.as_str()));
    universe
}
