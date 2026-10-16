//! The `map` side of a published generation: the exclusive lock, the
//! in-progress marker, and the record that certifies the run.
//!
//! The order is what makes a query safe. A `map` takes the lock, replaces the
//! record with a marker, writes every artifact, and only then publishes the
//! record that names them -- so a query either sees the previous generation
//! intact, or the marker (a run in flight or one that never finished), and never
//! a half-written pair. A run that fails after the marker leaves the marker
//! behind deliberately: an uncertified index is honest, a certifying record over
//! artifacts this run did not finish is not.
//!
//! The lock is [`std::fs::File::lock`] on `<state dir>/map.lock`, released when
//! the file handle drops or the process exits. The lock file itself is never
//! deleted -- unlinking it would let a second process lock a different inode and
//! run concurrently.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use crate::graph::graph_json_path;
use crate::layout;

use super::record::{
    new_generation, owner_of, record_path, remove_state, state_dir, write_digests, write_marker,
    write_record, ArtifactIdentity, Artifacts, Record, RECORD_FORMAT, RECORD_VERSION,
};

/// A `map` run's claim on a root's index, held for as long as the run lasts.
pub struct MapSession {
    root: PathBuf,
    owner: Option<String>,
    generation: Option<String>,
    // Held only to keep the lock; dropped with the session.
    _lock: Option<File>,
}

impl MapSession {
    /// The publishing worktree's identity, `None` for a root with no Git
    /// administrative directory (which publishes no record at all).
    #[must_use]
    pub fn owner(&self) -> Option<&str> {
        self.owner.as_deref()
    }

    /// This run's generation token.
    #[must_use]
    pub fn generation(&self) -> &str {
        self.generation.as_deref().unwrap_or("")
    }

    /// Enters the in-progress state: the marker record, written before the run
    /// touches any index artifact. A root with no owner instead clears whatever
    /// record it finds, because it must never carry a certification.
    pub fn mark_in_progress(&self) -> io::Result<()> {
        match self.owner.as_deref() {
            Some(owner) => write_marker(&self.root, owner),
            None => remove_state(&self.root),
        }
    }

    /// Publishes the record that certifies this run, as the last write.
    pub fn publish(
        &self,
        head: Option<String>,
        scope: &[String],
        file_count: u64,
        dirty_at_map: Option<Vec<String>>,
        config_inputs: BTreeMap<String, String>,
    ) -> io::Result<()> {
        let Some(owner) = self.owner.as_deref() else {
            return Ok(());
        };
        let record = Record {
            format: RECORD_FORMAT.to_string(),
            version: RECORD_VERSION,
            state: "published".to_string(),
            owner: owner.to_string(),
            generation: self.generation().to_string(),
            indexed_head: head,
            scope: scope.to_vec(),
            indexed_at: crate::hookio::iso8601_now(),
            file_count,
            dirty_at_map,
            config_inputs,
            artifacts: Artifacts {
                graph: ArtifactIdentity::of(&graph_json_path(&self.root)),
                manifest: ArtifactIdentity::of(&state_dir(&self.root).join(layout::MANIFEST_FILE)),
            },
        };
        write_record(&self.root, &record)
    }

    /// Writes the content-digest table, which must carry this run's generation
    /// so a query can tell a table from another run apart.
    pub fn write_digests(&self, files: &BTreeMap<String, String>) -> io::Result<()> {
        write_digests(&self.root, self.generation(), files)
    }
}

/// Takes the root's exclusive map lock and opens a session.
///
/// A held lock means another `map` of the same repository is running -- in this
/// worktree or a sibling -- so the run waits, announcing itself once on stderr
/// rather than interleaving two writers over one index.
pub fn begin(root: &Path) -> io::Result<MapSession> {
    let lock = acquire_lock(root)?;
    let owner = owner_of(root);
    let generation = owner.as_deref().map(new_generation);
    Ok(MapSession {
        root: root.to_path_buf(),
        owner,
        generation,
        _lock: Some(lock),
    })
}

fn acquire_lock(root: &Path) -> io::Result<File> {
    let path = super::record::lock_path(root);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // Neither `truncate` nor `append`: the lock rides the handle, and the file's
    // contents carry no meaning.
    let file = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => {
            eprintln!("devscout map: waiting for another map of this repository to finish");
            file.lock()?;
            Ok(file)
        }
        // Any other lock error (an unsupported filesystem, say) leaves the run
        // unserialized rather than refusing to map at all -- the marker and the
        // artifact identities still bound what a reader can certify.
        Err(std::fs::TryLockError::Error(_)) => Ok(file),
    }
}

/// Whether a freshness record file exists at all, distinct from whether it
/// certifies anything.
#[must_use]
pub fn record_exists(root: &Path) -> bool {
    record_path(root).exists()
}
