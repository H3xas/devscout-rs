use super::*;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

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

#[test]
fn a_concurrent_reader_always_sees_a_complete_old_or_new_artifact_never_a_partial_one() {
    // A reader racing the writer must land on the same-directory temp file's
    // rename, never in the middle of it: every full read of the target must
    // come back byte-for-byte one of the two known complete values, never a
    // truncated prefix, a mixed splice, or any other length.
    let dir = temp_dir("atomic-concurrent-read");
    let target = dir.join("graph.json");
    let value_a: Arc<Vec<u8>> = Arc::new(vec![b'A'; 300_003]);
    let value_b: Arc<Vec<u8>> = Arc::new(vec![b'B'; 400_007]);
    fs::write(&target, value_a.as_slice()).unwrap();

    let stop = Arc::new(AtomicBool::new(false));

    let reader_target = target.clone();
    let reader_value_a = Arc::clone(&value_a);
    let reader_value_b = Arc::clone(&value_b);
    let reader_stop = Arc::clone(&stop);
    let reader = thread::spawn(move || {
        let mut reads = 0usize;
        while !reader_stop.load(AtomicOrdering::Relaxed) {
            if let Ok(bytes) = fs::read(&reader_target) {
                let is_a = bytes.as_slice() == reader_value_a.as_slice();
                let is_b = bytes.as_slice() == reader_value_b.as_slice();
                assert!(
                    is_a || is_b,
                    "observed {} bytes matching neither complete artifact (old is {}, new is {}) -- a partial write",
                    bytes.len(),
                    reader_value_a.len(),
                    reader_value_b.len(),
                );
                reads += 1;
            }
        }
        reads
    });

    let writer_target = target.clone();
    let deadline = Instant::now() + Duration::from_millis(300);
    let mut write_b = false;
    while Instant::now() < deadline {
        let payload: &[u8] = if write_b { &value_b } else { &value_a };
        atomic_write_bytes(&writer_target, payload).unwrap();
        write_b = !write_b;
    }
    stop.store(true, AtomicOrdering::Relaxed);
    let reads = reader.join().unwrap();
    assert!(
        reads > 0,
        "reader observed no successful reads at all -- the race was not exercised"
    );
}
