use super::*;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::paths::{
    atomic_write_bytes, fragments_cache_path, fragments_index_path, graph_dir,
    SUPERSEDED_CACHE_FILES,
};
use super::rebuild::graph_schema_is_current;

mod artifact;
mod cache;
mod def;
mod edge;
mod fragment_types;
mod imports;
mod ordered;
mod paths;
mod rebuild;

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("scout-graph-test-{label}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}
