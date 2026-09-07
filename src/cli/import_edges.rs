// The `import-edges` verb: validates a cross-repo edge export and writes the
// auxiliary artifact `impact` reads alongside `graph.json`. `--repo <id>` is
// required and never inferred -- see `graph::imports`'s own module header for
// why. A refusal here is loud (exit 1, naming the offending value) and
// leaves any artifact that already existed untouched: `write_imported_edges`
// is reached only once parsing has already returned `Ok`.

use std::fs;
use std::path::Path;

use crate::graph;

use super::root::require_repo;

const IMPORT_EDGES_USAGE: &str = "usage: devscout import-edges <file> --repo <id>";

// The `--repo <id>` value: its own presence check, distinct from a missing
// file path, so the usage message always fires for the argument actually
// missing.
fn parse_repo_flag(args: &[String]) -> Option<&str> {
    let idx = args.iter().position(|a| a == "--repo")?;
    let raw = args.get(idx + 1)?;
    if raw.is_empty() || raw.starts_with("--") {
        return None;
    }
    Some(raw.as_str())
}

// The first non-flag argument that is not `--repo`'s own value -- the same
// pattern `first_positional`/`parse_impact_args` use for their own verbs.
fn parse_file_arg(args: &[String]) -> Option<&str> {
    for (i, a) in args.iter().enumerate() {
        if a.starts_with("--") {
            continue;
        }
        if i > 0 && args[i - 1] == "--repo" {
            continue;
        }
        return Some(a.as_str());
    }
    None
}

/// `import-edges`. Check order: `--repo <id>` present, a file path present,
/// THEN `require_repo`, THEN the file read and validation.
pub(crate) fn cmd_import_edges(cwd: &Path, args: &[String]) -> (i32, String) {
    let Some(repo_id) = parse_repo_flag(args) else {
        return (2, IMPORT_EDGES_USAGE.to_string());
    };
    let Some(path) = parse_file_arg(args) else {
        return (2, IMPORT_EDGES_USAGE.to_string());
    };

    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}")),
    };

    let abs = crate::repo::resolve_from(cwd, Path::new(path));
    let bytes = match fs::read(&abs) {
        Ok(b) => b,
        Err(e) => return (1, format!("error: cannot read \"{path}\": {e}")),
    };

    let parsed = match graph::parse_imported_edges(&bytes, repo_id) {
        Ok(p) => p,
        Err(msg) => return (1, format!("error: {msg}")),
    };

    let count = parsed.edges.len();
    if let Err(e) = graph::write_imported_edges(&root, &parsed) {
        return (1, format!("error: cannot write imported edges: {e}"));
    }

    let suffix = if count == 1 { "" } else { "s" };
    (
        0,
        format!("imported {count} edge{suffix} from \"{path}\" for repo \"{repo_id}\""),
    )
}
