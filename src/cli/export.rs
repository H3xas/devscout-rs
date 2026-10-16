//! The `export` verb: argument parsing, the usage line, and the exit codes.
//!
//! The verb writes its own stream rather than returning a rendered answer,
//! because the stream is not a message: `print_out` appends a newline to
//! whatever it is given, and the export already terminates every line it
//! writes, header and trailer included.

use std::path::Path;

use crate::export::{self, ExportError, ExportRefusal};

use super::root::require_repo;

const USAGE: &str = "usage: devscout export [--out FILE]";

// Exit codes: 0 for a written stream, 1 for a refusal or a write that failed,
// 2 for a malformed invocation. A refusal is not a usage error -- the
// invocation was well formed and the index cannot be published as it stands.
const EXIT_REFUSED: i32 = 1;
const EXIT_USAGE: i32 = 2;

fn usage_error(message: &str) -> i32 {
    eprintln!("devscout export: {message}\n{USAGE}");
    EXIT_USAGE
}

// `--out FILE` at most once. Anything else -- a positional argument, an unknown
// flag, a missing value, a value that is itself a flag -- is a usage error, so
// a mistyped invocation never writes a stream to a surprising place.
fn parse_out(args: &[String]) -> Result<Option<String>, i32> {
    let mut out: Option<String> = None;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg != "--out" {
            return Err(usage_error(&format!("unexpected argument `{arg}`")));
        }
        if out.is_some() {
            return Err(usage_error("`--out` given more than once"));
        }
        let Some(value) = args.get(index + 1) else {
            return Err(usage_error("`--out` needs a file"));
        };
        if value.starts_with('-') {
            return Err(usage_error("`--out` needs a file"));
        }
        out = Some(value.clone());
        index += 2;
    }
    Ok(out)
}

/// Runs `devscout export`, returning the process exit code.
pub(crate) fn cmd_export(cwd: &Path, args: &[String]) -> i32 {
    let out = match parse_out(args) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let root = match require_repo(cwd) {
        Ok(root) => root,
        Err(message) => {
            eprintln!("{}", ExportRefusal::NoIndex(message).line());
            return EXIT_REFUSED;
        }
    };
    match export::run(&root, cwd, out.as_deref().map(Path::new)) {
        Ok(()) => 0,
        Err(ExportError::Refused(refusal)) => {
            eprintln!("{}", refusal.line());
            EXIT_REFUSED
        }
        Err(ExportError::Output(message)) => {
            eprintln!("devscout export: {message}");
            EXIT_REFUSED
        }
        Err(ExportError::Stdout) => EXIT_REFUSED,
    }
}
