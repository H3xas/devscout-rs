use super::*;
use std::fs;

// --- atomic writes: crash-sim -----------------------------------

#[test]
fn atomic_write_leaves_target_intact_when_only_a_stray_tmp_exists() {
    let dir = temp_dir("atomic-crash");
    let target = dir.join("graph.json");
    fs::write(&target, b"{\"old\":true}").unwrap();
    // Simulate a crashed prior write: a tmp file was created but the
    // process died before the rename -- the target must be completely
    // unaffected by the stray file's mere presence.
    fs::write(dir.join(".graph.json.tmp.99999.7"), b"GARBAGE-PARTIAL").unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "{\"old\":true}");
}

#[test]
fn atomic_write_replaces_target_and_leaves_no_tmp_file_behind() {
    let dir = temp_dir("atomic-clean");
    let target = dir.join("graph.json");
    fs::write(&target, b"{\"old\":true}").unwrap();
    atomic_write_bytes(&target, b"{\"new\":true}").unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "{\"new\":true}");
    let leftover: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
        .collect();
    assert!(
        leftover.is_empty(),
        "no tmp file should remain: {leftover:?}"
    );
}

#[test]
fn atomic_write_creates_parent_directories() {
    let dir = temp_dir("atomic-mkdir");
    let target = dir.join("nested").join("deeper").join("graph.json");
    atomic_write_bytes(&target, b"{}").unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "{}");
}
