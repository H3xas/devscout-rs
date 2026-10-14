// `MapOptions` -- kept in its own sibling file (not inline in `mapcmd.rs`)
// because that file is capped by `tools/size-ratchet.toml`; moving the struct
// it belongs to into a sibling module keeps new code out of a capped file.

/// Options for `map_repo`.
#[derive(Debug, Clone, Copy, Default)]
pub struct MapOptions {
    /// Retained for API compatibility and otherwise inert.
    ///
    /// Reuse is keyed on the content digest of the bytes a file holds, always,
    /// so there is no mtime mode left to select. The field stays because
    /// callers outside this crate construct it, and `SCOUT_MTIME_REUSE` is
    /// still accepted (and ignored) so an existing script does not break.
    pub hash_reuse: bool,
    /// The compiler-fact enrichment consumer's rollback lever (`devscout map
    /// --no-semantic`): when `true`, `map_repo` never loads or applies an
    /// admitted compiler-facts artifact even when one is present, so a
    /// `--no-semantic` run always rebuilds and produces the exact syntax-only
    /// graph a build with no artifact admitted would, apart from its
    /// `stats.lane` record -- a zero-cost, always-available escape hatch back
    /// to pre-enrichment behavior. A CLI flag, not an environment switch,
    /// following this crate's own `--no-guess`-shaped precedent, so the lever
    /// is visible on the command line that produced a graph.
    pub no_semantic: bool,
}

impl MapOptions {
    /// Binary defaults: content-derived reuse (always), `no_semantic` OFF (an
    /// admitted artifact, when present, is applied).
    ///
    /// The retired `SCOUT_MTIME_REUSE` and `SCOUT_HASH_REUSE` are deliberately
    /// not read any more: the former selected a mode that no longer exists and
    /// the latter names what is now the only behavior. A free function rather
    /// than folded into `map_repo` itself so tests (and any future caller) can
    /// construct `MapOptions` directly -- deterministic, no process-env
    /// mutation shared across parallel `cargo test` threads.
    #[must_use]
    pub fn from_env() -> Self {
        MapOptions {
            hash_reuse: true,
            no_semantic: false,
        }
    }
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
}
