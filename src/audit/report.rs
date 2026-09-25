// `TierStats` -- per-tier counters `score.rs` fills and `render.rs` reads.
// Every count is a `usize`; ratios are computed at render time
// (`ratio_text`/`ratio_j`) from a hit count and a denominator, both kept,
// rather than as a pre-divided `f64` -- there is exactly one place (each
// renderer) that has to decide how a zero denominator prints, instead of
// that decision being baked into the data.

#[derive(Debug, Clone, Copy, Default)]
pub struct TierStats {
    pub edges: usize,
    pub tp: usize,
    pub fp: usize,
    pub fp_no_site: usize,
    pub fp_external_site: usize,
    pub fp_wrong_target: usize,
    /// Structurally-impossible FALSE POSITIVES only -- a structurally
    /// impossible edge that also happens to be the right answer to its site
    /// (the oracle sees the caller's project as unable to reach the
    /// target's, but a `uses-member` edge to that exact member still landed
    /// there and is a TP) is real signal about a project-boundary edge case,
    /// not resolver noise, and counting it here would silently make `guess`
    /// tier's own name-collision false positives look worse than they are.
    pub structural: usize,
    /// Edges scoring deliberately did not classify as either a true or a
    /// false positive, because the oracle has no vocabulary to judge them by
    /// at all -- today, only `SemanticDiscovered` edges whose originating
    /// compiler occurrence carries `shape == "identifier"` (a bare
    /// field/property/event read the oracle's own walker never records a
    /// case for). Always 0 for every other tier. Excluded from both `tp`+
    /// `fp` and from `precision`'s own denominator, so a large unjudged
    /// population never dilutes or inflates the judged figure either way.
    pub unjudged: usize,
}

impl TierStats {
    pub fn precision(&self) -> f64 {
        let judged = self.edges - self.unjudged;
        if judged == 0 {
            0.0
        } else {
            self.tp as f64 / judged as f64
        }
    }
}
