// The `compiler-facts` verb group: `run` (local one-shot engine
// acquisition), `import` (a build/CI-produced artifact), and `status`
// (read-only). `run` and `import` converge on the identical
// `graph::admit_and_publish` call -- the concrete mechanism behind one
// admission path for both producers. `status` never opens `graph.json` and
// is not a query verb; it is the one place this ticket's own
// engine-informed-vs-syntax-only state is reported.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::graph;

use super::root::require_repo;

const RUN_USAGE: &str = "usage: devscout compiler-facts run --solution <path.sln|path.csproj> [--target T] [--configuration C] [--platform P] [--capabilities a,b] [--timeout-ms N] [--output-cap-bytes N]";
const IMPORT_USAGE: &str =
    "usage: devscout compiler-facts import <file> [--target T] [--configuration C] [--platform P]";
const STATUS_USAGE: &str = "usage: devscout compiler-facts status";

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

fn requested_capabilities(args: &[String]) -> Vec<String> {
    match flag_value(args, "--capabilities") {
        Some(raw) => raw.split(',').map(str::to_string).collect(),
        None => vec!["symbols".to_string(), "diagnostics".to_string()],
    }
}

fn parse_positive_usize(args: &[String], flag: &str, default: usize) -> Option<usize> {
    match flag_value(args, flag) {
        Some(raw) => raw.parse::<usize>().ok(),
        None => Some(default),
    }
}

fn publish_outcome(result: Result<graph::AdmittedFacts, graph::PublishError>) -> (i32, String) {
    match result {
        Ok(facts) => (0, coverage_line(&facts)),
        Err(graph::PublishError::Refused(reason)) => (1, format!("refused: {reason}")),
        Err(graph::PublishError::Io(e)) => (
            1,
            format!("error: cannot write compiler-facts artifact: {e}"),
        ),
    }
}

fn coverage_line(facts: &graph::AdmittedFacts) -> String {
    match facts.coverage() {
        graph::Coverage::Complete => {
            format!(
                "admitted compiler facts (engine {}), coverage: complete",
                facts.header.engine_revision
            )
        }
        graph::Coverage::Incomplete { units } => format!(
            "admitted compiler facts (engine {}), coverage: incomplete ({} unit{} affected)",
            facts.header.engine_revision,
            units.len(),
            if units.len() == 1 { "" } else { "s" }
        ),
    }
}

// Reuses the engine's existing `--tfm`/`-p Name=Value` machinery for the
// target/configuration/platform profile rather than inventing new engine
// flags for what that surface already carries; `--capabilities` and
// `--compiler-facts` (stdout via `-`, the same convention `--facts -`
// already follows) are the only genuinely new engine flags.
fn engine_command(engine: &Path, root: &Path, solution: &Path, args: &[String]) -> Command {
    let profile = requested_profile(args);
    let capabilities = requested_capabilities(args);
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
        .arg(format!("Platform={}", profile.platform))
        .arg("--capabilities")
        .arg(capabilities.join(","));
    command
}

/// `compiler-facts run`: launches the engine located by
/// `SCOUT_COMPILER_ENGINE`, captures and admits its output, and publishes
/// it on success. No code path here writes to the artifact path except
/// through `graph::admit_and_publish`.
pub(crate) fn cmd_compiler_facts_run(cwd: &Path, args: &[String]) -> (i32, String) {
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}")),
    };
    let Some(engine) = graph::locate_engine() else {
        return (
            1,
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
        return (2, RUN_USAGE.to_string());
    };
    let Some(output_cap) =
        parse_positive_usize(args, "--output-cap-bytes", graph::DEFAULT_OUTPUT_CAP)
    else {
        return (2, RUN_USAGE.to_string());
    };
    let Some(solution) = flag_value(args, "--solution") else {
        return (2, RUN_USAGE.to_string());
    };
    let solution_abs = crate::repo::resolve_from(cwd, Path::new(solution));

    let command = engine_command(&engine, &root, &solution_abs, args);
    let bytes = match graph::run_engine(
        command,
        Duration::from_millis(timeout_ms as u64),
        output_cap,
    ) {
        Ok(b) => b,
        Err(reason) => return (1, format!("refused: {reason}")),
    };

    let expected = graph::expectations_for(&root, requested_profile(args));
    publish_outcome(graph::admit_and_publish(&root, &bytes, &expected))
}

/// `compiler-facts import <file>`: admits and publishes a build/CI-produced
/// artifact through the same `graph::admit_and_publish` call `run` uses.
pub(crate) fn cmd_compiler_facts_import(cwd: &Path, args: &[String]) -> (i32, String) {
    let Some(path) = args.iter().find(|a| !a.starts_with("--")) else {
        return (2, IMPORT_USAGE.to_string());
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
    let expected = graph::expectations_for(&root, requested_profile(args));
    publish_outcome(graph::admit_and_publish(&root, &bytes, &expected))
}

/// `compiler-facts status`: read-only. Never opens `graph.json`. Reports
/// `coverage: syntax-only` when no artifact has ever been admitted -- the
/// ordinary state for a repository `map` already covers fully offline.
pub(crate) fn cmd_compiler_facts_status(cwd: &Path, _args: &[String]) -> (i32, String) {
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}")),
    };
    match graph::read_compiler_facts(&root) {
        Some(facts) => (0, coverage_line(&facts)),
        None => (
            0,
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
            2,
            format!("usage: devscout compiler-facts <run|import|status>\n{RUN_USAGE}\n{IMPORT_USAGE}\n{STATUS_USAGE}"),
        ),
    }
}
