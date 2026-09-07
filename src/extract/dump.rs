use std::fs;
use std::path::Path;
use std::process;

use super::json::{extraction_to_json, ts_extraction_to_json};
use super::ts_fragment::extract_ts_file;
use super::ts_fragment_types::TsFragment;
use super::walk::extract;

// ---------------------------------------------------------------------------
// `extract-dump` subcommand -- canonical JSON for one file.
// ---------------------------------------------------------------------------

/// Extracts the C# file at `path` and prints the extraction as JSON.
pub fn run_extract_dump(path: &str) {
    let source = fs::read_to_string(path).unwrap_or_else(|err| {
        eprintln!("failed to read {path}: {err}");
        process::exit(1);
    });
    // The dump path dispatches on extension: a C# file yields a fragment read
    // as `{purpose, defs, usings, refs, names}`, while a TS/JS file yields a
    // `ts: 1` REFERENCE fragment carrying `defs` and `refs` but no `usings`
    // and no `names` -- and serialization drops an absent value's key, so the
    // TS dump is a three-key object. The C# dump below keeps all five keys.
    if let Some(grammar) = crate::parse::ts_grammar_for(path) {
        // Compute `root = dirname(path)`, `rel = basename(path)` and go
        // through `extract_ts_file` -- the SAME hybrid-aware worker path
        // `devscout map` uses, not the pure extractor -- so the dump exercises
        // the leading-comment prefix, off one single parse.
        let file_path = Path::new(path);
        let root = file_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let rel = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);
        match extract_ts_file(root, rel, &source, grammar) {
            Some(ts) => println!("{}", ts_extraction_to_json(&ts.purpose, &ts.fragment)),
            // A parse failure leaves the file out of both outputs, so its
            // dump is the empty fragment under a null purpose.
            None => println!("{}", ts_extraction_to_json(&None, &TsFragment::default())),
        }
        return;
    }
    let result = extract(&source);
    println!("{}", extraction_to_json(&result));
}
