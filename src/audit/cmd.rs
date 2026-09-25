// `cmd_audit` -- the CLI entry point `cli.rs` dispatches `audit` to.

use std::path::Path;

use super::assert_check::evaluate_assert;
use super::load::load;
use super::model::AuditOptions;
use super::render::{render_json, render_text};
use super::score::score;

/// `devscout audit --semantic <refs.jsonl> [--units F] [--defs F] [--json]
/// [--assert F] [--fp-sites F]`. Exit 0 with the report (text, or one JSON
/// object with `--json`); exit 1 on any error (bad arguments, an
/// unreadable/malformed input file, no `.scout`/`.git` root, no graph.json)
/// or on any `--assert` violation.
///
/// Where the violation lines go depends on the report format, and the rule is
/// "stdout stays machine-readable": in TEXT mode they are appended after the
/// report on stdout, where they read as part of it. In `--json` mode they go
/// to STDERR instead, because stdout must remain exactly one JSON object a
/// caller can pipe into `jq` -- a CI step that both asserts and parses the
/// report is the whole point of the two flags together. Either way the exit
/// code is 1 and the lines themselves are identical.
pub(crate) fn cmd_audit(cwd: &Path, args: &[String]) -> (i32, String) {
    const USAGE: &str = "usage: devscout audit --semantic <refs.jsonl> [--units F] [--defs F] [--json] [--assert F] [--fp-sites F]";

    let mut semantic: Option<String> = None;
    let mut units: Option<String> = None;
    let mut defs: Option<String> = None;
    let mut assert_path: Option<String> = None;
    let mut fp_sites_path: Option<String> = None;
    let mut json = false;

    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        if matches!(
            flag,
            "--semantic" | "--units" | "--defs" | "--assert" | "--fp-sites"
        ) {
            let Some(val) = args.get(i + 1) else {
                return (1, format!("error: missing value for '{flag}'\n{USAGE}"));
            };
            match flag {
                "--semantic" => semantic = Some(val.clone()),
                "--units" => units = Some(val.clone()),
                "--defs" => defs = Some(val.clone()),
                "--assert" => assert_path = Some(val.clone()),
                "--fp-sites" => fp_sites_path = Some(val.clone()),
                _ => unreachable!(),
            }
            i += 2;
        } else if flag == "--json" {
            json = true;
            i += 1;
        } else {
            return (1, format!("error: unrecognized argument '{flag}'\n{USAGE}"));
        }
    }
    let Some(semantic) = semantic else {
        return (
            1,
            format!("error: --semantic <refs.jsonl> is required\n{USAGE}"),
        );
    };

    let root = match crate::cli::require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}")),
    };

    let semantic_path = crate::repo::resolve_from(cwd, Path::new(&semantic));
    let units_path = units.map(|u| crate::repo::resolve_from(cwd, Path::new(&u)));
    let defs_path = defs.map(|d| crate::repo::resolve_from(cwd, Path::new(&d)));
    let opts = AuditOptions {
        semantic: &semantic_path,
        units: units_path.as_deref(),
        defs: defs_path.as_deref(),
    };

    let mut inputs = match load(&root, &opts) {
        Ok(i) => i,
        Err(e) => return (1, format!("error: {e}")),
    };
    inputs.collect_fp_sites = fp_sites_path.is_some();
    let report = score(inputs);
    if let Some(fp_path) = fp_sites_path {
        let abs = crate::repo::resolve_from(cwd, Path::new(&fp_path));
        if let Err(e) = std::fs::write(&abs, super::fp_sites::render(&report.fp_sites)) {
            return (1, format!("error: failed to write {}: {e}", abs.display()));
        }
    }
    let json_string = render_json(&report);
    let mut out = if json {
        json_string.clone()
    } else {
        render_text(&report)
    };

    if let Some(assert_path) = assert_path {
        let abs = crate::repo::resolve_from(cwd, Path::new(&assert_path));
        let text = match std::fs::read_to_string(&abs) {
            Ok(t) => t,
            Err(e) => return (1, format!("error: failed to read {}: {e}", abs.display())),
        };
        let violations = match evaluate_assert(&json_string, &text) {
            Ok(v) => v,
            Err(e) => return (1, format!("error: {e}")),
        };
        if !violations.is_empty() {
            if json {
                // stdout keeps the bare JSON object; the violations are the
                // diagnostic half and belong on the other stream.
                eprintln!("{}", violations.join("\n"));
            } else {
                out.push('\n');
                out.push_str(&violations.join("\n"));
            }
            return (1, out);
        }
    }
    (0, out)
}
