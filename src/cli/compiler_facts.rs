// The `compiler-facts` verb group: `run` (local one-shot engine
// acquisition), `import` (a build/CI-produced artifact), and `status`
// (read-only). `run` and `import` converge on the identical
// `graph::admit_and_publish` call -- the concrete mechanism behind one
// admission path for both producers. `status` never opens `graph.json` and
// is not a query verb; it is the one place the artifact's own
// engine-informed-vs-syntax-only state is reported.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::exit::{EXIT_FAILURE, EXIT_OK, EXIT_USAGE};
use crate::graph;
use crate::usage::{
    COMPILER_FACTS_IMPORT_USAGE, COMPILER_FACTS_RUN_USAGE, COMPILER_FACTS_STATUS_USAGE,
    COMPILER_FACTS_USAGE,
};

use super::root::require_repo;

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let idx = args.iter().position(|a| a == flag)?;
    let raw = args.get(idx + 1)?;
    if raw.starts_with("--") {
        return None;
    }
    Some(raw.as_str())
}

fn requested_profile(args: &[String]) -> graph::Profile {
    graph::Profile {
        target: flag_value(args, "--target").unwrap_or("net9.0").to_string(),
        configuration: flag_value(args, "--configuration")
            .unwrap_or("Debug")
            .to_string(),
        platform: flag_value(args, "--platform")
            .unwrap_or("AnyCPU")
            .to_string(),
    }
}

/// `None` when the caller passed no `--capabilities` flag -- `run`'s own
/// wrapper carries no second, independent definition of "the default
/// capability set" this way: the spawned engine's own `SupportedCapabilities`
/// default (see `tools/scout-semantic/CompilerFacts.cs`) is the single
/// source of truth for what an unflagged run produces, `compiler-facts
/// run`'s outcome included.
fn requested_capabilities(args: &[String]) -> Option<Vec<String>> {
    flag_value(args, "--capabilities").map(|raw| raw.split(',').map(str::to_string).collect())
}

fn parse_positive_usize(args: &[String], flag: &str, default: usize) -> Option<usize> {
    match flag_value(args, flag) {
        Some(raw) => raw.parse::<usize>().ok(),
        None => Some(default),
    }
}

fn publish_outcome(result: Result<graph::AdmittedFacts, graph::PublishError>) -> (i32, String) {
    match result {
        Ok(facts) => (EXIT_OK, coverage_line(&facts)),
        Err(graph::PublishError::Refused(reason)) => (EXIT_FAILURE, format!("refused: {reason}")),
        Err(graph::PublishError::Io(e)) => (
            EXIT_FAILURE,
            format!("error: cannot write compiler-facts artifact: {e}"),
        ),
    }
}

/// The one-line admission and status report. It reads `coverage: complete`
/// only when no unit is incomplete and every counted compilation is complete;
/// otherwise it names the incomplete compilations by reason.
fn coverage_line(facts: &graph::AdmittedFacts) -> String {
    let doc: Option<serde_json::Value> = serde_json::from_slice(&facts.bytes).ok();
    let tally = doc
        .as_ref()
        .map(graph::CompilationTally::from_artifact)
        .unwrap_or_default();
    let engine = &facts.header.engine_revision;
    let base = match (facts.coverage(), tally.summary()) {
        (graph::Coverage::Complete, None) => {
            format!("admitted compiler facts (engine {engine}), coverage: complete")
        }
        (graph::Coverage::Complete, Some(summary)) => {
            format!("admitted compiler facts (engine {engine}), coverage: partial ({summary})")
        }
        (graph::Coverage::Incomplete { units }, summary) => format!(
            "admitted compiler facts (engine {engine}), coverage: incomplete ({} unit{} affected{})",
            units.len(),
            if units.len() == 1 { "" } else { "s" },
            summary.map(|s| format!("; {s}")).unwrap_or_default()
        ),
    };
    match doc.as_ref().and_then(occurrence_coverage_line) {
        Some(occurrences) => format!("{base}, {occurrences}"),
        None => base,
    }
}

/// Occurrence coverage for the status line -- the number admitted, the
/// number in each non-confirmed resolution state, and an explicit
/// unavailable state where the capability was requested but not provided,
/// rather than silence. Computed by a read-only, independent parse of the
/// already-admitted, already-trusted candidate bytes; distinct from
/// `graph::CandidateHeader`'s own minimal occurrence model, which never
/// reads a field beyond an occurrence's own `compilation.identity`/
/// `fingerprint`. `None` when the capability was neither requested nor
/// provided -- the ordinary non-occurrence case, where the base coverage
/// line already says everything this status line has to say.
fn occurrence_coverage_line(doc: &serde_json::Value) -> Option<String> {
    let names = |key: &str| -> Vec<String> {
        doc["capabilities"][key]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let requested_occurrences = names("requested").iter().any(|c| c == "occurrences");
    let provided_occurrences = names("provided").iter().any(|c| c == "occurrences");

    if !provided_occurrences {
        return if requested_occurrences {
            Some("occurrences: unavailable (requested but not provided)".to_string())
        } else {
            None
        };
    }

    let sites = doc["occurrences"]["sites"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut non_confirmed: std::collections::BTreeMap<String, usize> = Default::default();
    for site in &sites {
        if let Some(resolution) = site["resolution"].as_str() {
            if resolution != "confirmed" {
                *non_confirmed.entry(resolution.to_string()).or_insert(0) += 1;
            }
        }
    }

    let mut line = format!("occurrences: {} admitted", sites.len());
    for (state, count) in &non_confirmed {
        line.push_str(&format!(", {count} {state}"));
    }
    Some(line)
}

// Reuses the engine's existing `--tfm`/`-p Name=Value` machinery for the
// target/configuration/platform profile rather than inventing new engine
// flags for what that surface already carries; `--capabilities` and
// `--compiler-facts` (stdout via `-`, the same convention `--facts -`
// already follows) are the only genuinely new engine flags.
fn engine_command(engine: &Path, root: &Path, solution: &Path, args: &[String]) -> Command {
    let profile = requested_profile(args);
    let mut command = Command::new(engine);
    command
        .arg(solution)
        .arg("--root")
        .arg(root)
        .arg("--emit")
        .arg("compiler-facts")
        .arg("--compiler-facts")
        .arg("-")
        .arg("--tfm")
        .arg(&profile.target)
        .arg("-p")
        .arg(format!("Configuration={}", profile.configuration))
        .arg("-p")
        .arg(format!("Platform={}", profile.platform));
    if let Some(capabilities) = requested_capabilities(args) {
        command.arg("--capabilities").arg(capabilities.join(","));
    }
    command
}

/// `compiler-facts run`: launches the engine located by
/// `SCOUT_COMPILER_ENGINE`, captures and admits its output, and publishes
/// it on success. No code path here writes to the artifact path except
/// through `graph::admit_and_publish`.
pub(crate) fn cmd_compiler_facts_run(cwd: &Path, args: &[String]) -> (i32, String) {
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (EXIT_FAILURE, format!("error: {e}")),
    };
    let Some(engine) = graph::locate_engine() else {
        return (
            EXIT_FAILURE,
            format!(
                "error: no engine located; set {} to the built engine executable",
                graph::SCOUT_COMPILER_ENGINE
            ),
        );
    };
    let Some(timeout_ms) = parse_positive_usize(
        args,
        "--timeout-ms",
        graph::DEFAULT_TIMEOUT.as_millis() as usize,
    ) else {
        return (EXIT_USAGE, COMPILER_FACTS_RUN_USAGE.to_string());
    };
    let Some(output_cap) =
        parse_positive_usize(args, "--output-cap-bytes", graph::DEFAULT_OUTPUT_CAP)
    else {
        return (EXIT_USAGE, COMPILER_FACTS_RUN_USAGE.to_string());
    };
    let Some(solution) = flag_value(args, "--solution") else {
        return (EXIT_USAGE, COMPILER_FACTS_RUN_USAGE.to_string());
    };
    let solution_abs = crate::repo::resolve_from(cwd, Path::new(solution));

    let command = engine_command(&engine, &root, &solution_abs, args);
    let bytes = match graph::run_engine(
        command,
        Duration::from_millis(timeout_ms as u64),
        output_cap,
    ) {
        Ok(b) => b,
        Err(reason) => return (EXIT_FAILURE, format!("refused: {reason}")),
    };

    let expected = graph::expectations_for(&root, requested_profile(args));
    publish_outcome(graph::admit_and_publish(&root, &bytes, &expected))
}

/// `compiler-facts import <file>`: admits and publishes a build/CI-produced
/// artifact through the same `graph::admit_and_publish` call `run` uses.
pub(crate) fn cmd_compiler_facts_import(cwd: &Path, args: &[String]) -> (i32, String) {
    let Some(path) = args.iter().find(|a| !a.starts_with("--")) else {
        return (EXIT_USAGE, COMPILER_FACTS_IMPORT_USAGE.to_string());
    };
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (EXIT_FAILURE, format!("error: {e}")),
    };
    let abs = crate::repo::resolve_from(cwd, Path::new(path));
    let bytes = match fs::read(&abs) {
        Ok(b) => b,
        Err(e) => return (EXIT_FAILURE, format!("error: cannot read \"{path}\": {e}")),
    };
    let expected = graph::expectations_for(&root, requested_profile(args));
    publish_outcome(graph::admit_and_publish(&root, &bytes, &expected))
}

/// `compiler-facts status`: read-only. Never opens `graph.json`. Reports
/// `coverage: syntax-only` when no artifact has ever been admitted -- the
/// ordinary state for a repository `map` already covers fully offline.
pub(crate) fn cmd_compiler_facts_status(cwd: &Path, _args: &[String]) -> (i32, String) {
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (EXIT_FAILURE, format!("error: {e}")),
    };
    match graph::read_compiler_facts(&root) {
        Some(facts) => (EXIT_OK, coverage_line(&facts)),
        None => (
            EXIT_OK,
            "coverage: syntax-only (no compiler-facts artifact admitted)".to_string(),
        ),
    }
}

/// Dispatches `compiler-facts <run|import|status>`.
pub(crate) fn cmd_compiler_facts(cwd: &Path, args: &[String]) -> (i32, String) {
    match args.first().map(String::as_str) {
        Some("run") => cmd_compiler_facts_run(cwd, &args[1..]),
        Some("import") => cmd_compiler_facts_import(cwd, &args[1..]),
        Some("status") => cmd_compiler_facts_status(cwd, &args[1..]),
        _ => (
            EXIT_USAGE,
            format!(
                "{}\n{}\n{}\n{}",
                COMPILER_FACTS_USAGE,
                COMPILER_FACTS_RUN_USAGE,
                COMPILER_FACTS_IMPORT_USAGE,
                COMPILER_FACTS_STATUS_USAGE
            ),
        ),
    }
}
