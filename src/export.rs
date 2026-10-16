//! `devscout export`: the current repository's whole fact set as one versioned
//! NDJSON stream.
//!
//! The export is a VIEW over the index, never a promoted storage file: it is
//! built from the graph, the fragment cache's messaging arrays, the record
//! provenance and the digest table, so a change to how the index stores any of
//! them does not become a change to what a consumer parses.
//!
//! One repository per run, one fact per line, no cross-repository joining and
//! no inferred hop: everything published is recorded by `map`, in the tier and
//! the evidence class the index itself attached to it. Nothing here resolves
//! anything, and nothing here writes to the index.
//!
//! The reading order is the module list below: `snapshot` reads one consistent
//! generation or refuses, `facts` turns it into view lines, `evidence` states
//! how strongly each line is known, `header` states where the facts came from,
//! and `stream` orders them canonically and hashes their bytes.

mod evidence;
mod facts;
mod header;
mod refusal;
mod snapshot;
mod stream;

#[cfg(test)]
mod tests;

use std::io::Write;
use std::path::Path;

pub(crate) use refusal::ExportRefusal;

/// What stopped a run: a refusal a consumer can branch on, or a write that
/// could not be made.
pub(crate) enum ExportError {
    /// The index cannot be published as it stands, with the reason why.
    Refused(ExportRefusal),
    /// The stream could not be written where `--out` asked for it.
    Output(String),
    /// The stream could not be written to stdout -- a closed pipe, most often,
    /// where a message would be noise.
    Stdout,
}

impl From<ExportRefusal> for ExportError {
    fn from(refusal: ExportRefusal) -> ExportError {
        ExportError::Refused(refusal)
    }
}

/// Reads the index at `root` and writes one stream, to `out` or to stdout.
///
/// The whole stream is rendered before anything is written, so a refusal never
/// leaves a partial stream on stdout and never leaves a file at `out`.
pub(crate) fn run(root: &Path, invocation_dir: &Path, out: Option<&Path>) -> Result<(), ExportError> {
    let snapshot = snapshot::load(root, invocation_dir)?;
    // The freshness warning goes to stderr, as it does for every query: stdout
    // is the stream, and a warning is advice for a human reading alongside it.
    if let Some(warning) = &snapshot.freshness_warning {
        eprintln!("{warning}");
    }
    let stream = stream::render(&snapshot);
    let bytes = stream.bytes();
    match out {
        Some(path) => stream::write_out(path, &bytes).map_err(|error| {
            ExportError::Output(format!("cannot write {}: {error}", path.display()))
        }),
        None => {
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            lock.write_all(&bytes)
                .and_then(|()| lock.flush())
                .map_err(|_| ExportError::Stdout)
        }
    }
}
