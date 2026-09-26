// `MapOptions` -- see `mapcmd.rs`'s own module header for `hash_reuse`. Kept
// in its own sibling file (not inline in `mapcmd.rs`) because that file sits
// within two lines of its `tools/size-ratchet.toml` ceiling; adding
// `no_semantic` here, rather than growing the frozen file, keeps new code
// out of a capped file by moving the struct it belongs to into a sibling
// module.

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
    /// `--no-semantic` run always rebuilds and produces the exact syntax-only
    /// graph a build with no artifact admitted would, apart from its
    /// `stats.lane` record -- a zero-cost, always-available escape hatch back
    /// to pre-enrichment behavior. A CLI flag, not an
    /// environment switch, following this crate's own `--no-guess`-shaped
    /// precedent rather than `SCOUT_MTIME_REUSE`'s env-switch shape, so the
    /// lever is visible on the command line that produced a graph.
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
