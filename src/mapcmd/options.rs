// `MapOptions` -- see `mapcmd.rs`'s own module header for `hash_reuse`. Kept
// in its own sibling file (not inline in `mapcmd.rs`) because that file sits
// at its exact `tools/size-ratchet.toml` ceiling with zero headroom; adding
// `no_semantic` here, rather than growing the frozen file, is what "new code
// lands in new sibling modules" (this ticket's own Decision) means in
// practice for a struct that already lived in the file being protected.
//
// `compiler_facts_artifact_present` lives here for the same reason: it is
// `map_repo`'s rebuild-trigger input for the `--no-semantic` rollback lever,
// and `mapcmd.rs` has no line budget left to host it directly.

use std::path::Path;

/// Options for `map_repo` -- see `mapcmd.rs`'s own module header.
#[derive(Debug, Clone, Copy, Default)]
pub struct MapOptions {
    /// Content-hash reuse (`mapcmd.rs`'s own module header). `MapOptions::default()`
    /// keeps `false` (mtime keying); the binary's env default is `true` -- see
    /// `from_env`.
    pub hash_reuse: bool,
    /// The compiler-fact enrichment consumer's rollback lever (`devscout map
    /// --no-semantic`): when `true`, `map_repo` never loads or applies an
    /// admitted compiler-facts artifact even when one is present, so a
    /// `--no-semantic` run always produces the exact syntax-only graph a
    /// build with no artifact admitted would -- a zero-cost, always-available
    /// escape hatch back to pre-enrichment behavior. A CLI flag, not an
    /// environment switch, following this crate's own `--no-guess`-shaped
    /// precedent rather than `SCOUT_MTIME_REUSE`'s env-switch shape: the
    /// consumer's design explicitly retains D11 ("a flag, not an environment
    /// switch") for this lever.
    pub no_semantic: bool,
}

impl MapOptions {
    /// Binary default: hash reuse ON, `no_semantic` OFF (an admitted artifact,
    /// when present, is applied). Exactly `SCOUT_MTIME_REUSE=1` drops back to
    /// mtime keying; anything else (including unset) keeps hash reuse. The
    /// retired opt-in `SCOUT_HASH_REUSE` is deliberately not read any more --
    /// it named what is now the default. A free function rather than folded
    /// into `map_repo` itself so tests (and any future caller) can construct
    /// `MapOptions` directly -- deterministic, no process-env mutation shared
    /// across parallel `cargo test` threads.
    pub fn from_env() -> Self {
        MapOptions {
            hash_reuse: std::env::var("SCOUT_MTIME_REUSE").as_deref() != Ok("1"),
            no_semantic: false,
        }
    }
}

/// Whether an admitted compiler-facts artifact sits on disk at `root`,
/// checked by presence alone (`Path::exists`), never by loading or parsing
/// it. `map_repo`'s rebuild trigger needs this independently of whether
/// `MapOptions::no_semantic` is set: when the flag forces `semantic_layer` to
/// `None`, `semantic_layer.is_some()` can never fire, so a `--no-semantic`
/// run over an otherwise-unchanged tree with a still-enriched `graph.json` on
/// disk would leave that stale enriched graph in place forever (the rollback
/// lever's whole point is to undo exactly that). Presence-only, not
/// freshness or parseability, so a rebuild is triggered even for a stale or
/// malformed artifact -- the same "fires safely, not perfectly" cost
/// `mapcmd.rs`'s own trigger comment already accepts for its other two
/// disjuncts.
pub(crate) fn compiler_facts_artifact_present(root: &Path) -> bool {
    crate::graph::compiler_facts_json_path(root).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_keeps_semantic_enrichment_on() {
        assert!(!MapOptions::default().no_semantic);
        assert!(!MapOptions::from_env().no_semantic);
    }

    #[test]
    fn no_semantic_is_independently_settable() {
        let opts = MapOptions {
            hash_reuse: true,
            no_semantic: true,
        };
        assert!(opts.hash_reuse);
        assert!(opts.no_semantic);
    }

    #[test]
    fn compiler_facts_artifact_present_checks_disk_only_never_parses() {
        let n = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "scout-mapopts-test-{}-{}",
            std::process::id(),
            n.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        assert!(!compiler_facts_artifact_present(&dir));

        let path = crate::graph::compiler_facts_json_path(&dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create artifact parent dir");
        }
        // Deliberately malformed bytes -- presence, not parseability, is what
        // this function checks.
        std::fs::write(&path, b"not json").expect("write artifact stub");
        assert!(compiler_facts_artifact_present(&dir));

        std::fs::remove_dir_all(&dir).ok();
    }
}
