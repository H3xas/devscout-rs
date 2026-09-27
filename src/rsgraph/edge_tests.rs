use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::crates::{discover, RustCrates};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A scratch tree with the given files, outside any git work tree.
fn scratch(files: &[(&str, &str)]) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("scout-rsgraph-edges-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for (rel, body) in files {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// The packages `discover` finds over a scratch tree of `files`, with every
/// `.rs` file among them walked.
fn crates_of(files: &[(&str, &str)]) -> Option<RustCrates> {
    let dir = scratch(files);
    let rs: Vec<String> = files
        .iter()
        .filter(|(rel, _)| rel.ends_with(".rs"))
        .map(|(rel, _)| rel.to_string())
        .collect();
    let out = discover(&dir, &[".".to_string()], &rs).unwrap();
    let _ = fs::remove_dir_all(&dir);
    out
}

mod crates;
