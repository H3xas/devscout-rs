// `impact`'s per-row `why`: which rule or tier best explains why a file was
// reached, folded across every edge kind the walk saw for it. Split out of
// `impact.rs` to keep that file under its size ceiling; `pub(super)`
// throughout, so only `impact.rs` itself reaches in.

use crate::graph;

use super::impact::{Hit, KindLines, VisitedEntry};
// Re-exported so `impact.rs` reaches both through this one module -- it
// never needs `super::why` directly.
use super::why::why_for_edge;
pub(super) use super::why::Why;

/// Which edge (kind, and for `uses-member` its tier) explains each of a
/// file's non-zero `KindLines` slots -- `direct`/`direct_amb`/`heuristic`/
/// `iface`, in that order. `None` exactly when the paired line is `0`.
/// `lines.ctor_di` needs no partner: every edge that sets it is a `ctor-di`
/// edge, so `primary_why` names it directly.
#[derive(Debug, Clone, Default)]
pub(super) struct WhyTrack {
    pub(super) direct: Option<Why>,
    pub(super) direct_amb: Option<Why>,
    pub(super) heuristic: Option<Why>,
    pub(super) iface: Option<Why>,
}

// The lowest-line-wins guard, paired with the specific edge that explains the
// winning line. Both slots are set together so they can never name two
// different edges for the same line; the four `Hit` methods below are its
// only callers, one per `KindLines`/`WhyTrack` slot pair.
fn note_line_why(slot: &mut usize, why_slot: &mut Option<Why>, line: usize, why: Why) {
    if line > 0 && (*slot == 0 || line < *slot) {
        *slot = line;
        *why_slot = Some(why);
    }
}

impl Hit {
    /// Records a resolved (precise) reference reaching this file.
    pub(super) fn note_direct(&mut self, line: usize, edge: &graph::Edge) {
        let why = why_for_edge(edge);
        note_line_why(
            &mut self.lines.direct,
            &mut self.why_track.direct,
            line,
            why,
        );
    }
    /// Records an ambiguous reference reaching this file.
    pub(super) fn note_direct_amb(&mut self, line: usize, edge: &graph::Edge) {
        let why = why_for_edge(edge);
        note_line_why(
            &mut self.lines.direct_amb,
            &mut self.why_track.direct_amb,
            line,
            why,
        );
    }
    /// Records the interface hop reaching this file.
    pub(super) fn note_iface(&mut self, line: usize, edge: &graph::Edge) {
        let why = why_for_edge(edge);
        note_line_why(&mut self.lines.iface, &mut self.why_track.iface, line, why);
    }
    /// Records a heuristic (guessed) edge reaching this file.
    pub(super) fn note_heuristic(&mut self, line: usize, edge: &graph::Edge) {
        let why = why_for_edge(edge);
        note_line_why(
            &mut self.lines.heuristic,
            &mut self.why_track.heuristic,
            line,
            why,
        );
    }
}

// The single `why` a row reports when more than one edge kind reached the
// same file follows one priority order: ctor-di, direct, ambiguous, iface,
// guess -- the most specific evidence about the seed definition wins; an
// interface hop names the shared interface rather than the seed itself, and
// a guess is named only when nothing else fired at all.
fn primary_why(lines: &KindLines, track: &WhyTrack) -> Why {
    if lines.ctor_di != 0 {
        return Why::CtorDi;
    }
    if lines.direct != 0 {
        return track.direct.unwrap_or(Why::UsesMemberPrecise);
    }
    if lines.direct_amb != 0 {
        return track.direct_amb.unwrap_or(Why::UsesMemberPrecise);
    }
    if lines.iface != 0 {
        return track.iface.unwrap_or(Why::UsesMemberPrecise);
    }
    track.heuristic.unwrap_or(Why::UsesMemberGuess)
}

/// Assembles one `VisitedEntry` from a hop's finished `Hit` bookkeeping,
/// folding `why` in via `primary_why` alongside every other per-file summary
/// field -- the one place `impact_walk` hands a `Hit` off as a resolved row.
pub(super) fn build_visited_entry(hop: u32, infra: bool, h: &Hit) -> VisitedEntry {
    VisitedEntry {
        hop,
        via_count: h.via_count,
        ambiguous_count: h.ambiguous_count,
        heuristic_count: h.heuristic_count,
        ext_count: h.ext_count,
        symbols: h.symbols.clone().into_vec(),
        iface_via: h.iface_via.clone().into_vec(),
        lines: h.lines.clone(),
        infra,
        why: primary_why(&h.lines, &h.why_track),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(ctor_di: usize, direct: usize, direct_amb: usize, iface: usize) -> KindLines {
        KindLines {
            ctor_di,
            direct,
            direct_amb,
            iface,
            heuristic: 0,
        }
    }

    #[test]
    fn ctor_di_alone_wins() {
        let l = lines(1, 0, 0, 0);
        let track = WhyTrack::default();
        assert_eq!(primary_why(&l, &track), Why::CtorDi);
    }

    #[test]
    fn direct_alone_reports_its_tracked_why() {
        let l = lines(0, 1, 0, 0);
        let track = WhyTrack {
            direct: Some(Why::UsesType),
            ..WhyTrack::default()
        };
        assert_eq!(primary_why(&l, &track), Why::UsesType);
    }

    #[test]
    fn direct_amb_alone_reports_its_tracked_why() {
        let l = lines(0, 0, 1, 0);
        let track = WhyTrack {
            direct_amb: Some(Why::Inherits),
            ..WhyTrack::default()
        };
        assert_eq!(primary_why(&l, &track), Why::Inherits);
    }

    #[test]
    fn iface_alone_reports_its_tracked_why() {
        let l = lines(0, 0, 0, 1);
        let track = WhyTrack {
            iface: Some(Why::UsesMemberExt),
            ..WhyTrack::default()
        };
        assert_eq!(primary_why(&l, &track), Why::UsesMemberExt);
    }

    #[test]
    fn heuristic_alone_reports_its_tracked_why() {
        let l = lines(0, 0, 0, 0);
        let track = WhyTrack {
            heuristic: Some(Why::UsesMemberGuess),
            ..WhyTrack::default()
        };
        assert_eq!(primary_why(&l, &track), Why::UsesMemberGuess);
    }

    #[test]
    fn ctor_di_outranks_direct() {
        let l = lines(1, 1, 0, 0);
        let track = WhyTrack {
            direct: Some(Why::UsesType),
            ..WhyTrack::default()
        };
        assert_eq!(primary_why(&l, &track), Why::CtorDi);
    }

    #[test]
    fn direct_outranks_direct_amb() {
        let l = lines(0, 1, 1, 0);
        let track = WhyTrack {
            direct: Some(Why::UsesType),
            direct_amb: Some(Why::Inherits),
            ..WhyTrack::default()
        };
        assert_eq!(primary_why(&l, &track), Why::UsesType);
    }

    #[test]
    fn direct_amb_outranks_iface() {
        let l = lines(0, 0, 1, 1);
        let track = WhyTrack {
            direct_amb: Some(Why::Inherits),
            iface: Some(Why::UsesMemberExt),
            ..WhyTrack::default()
        };
        assert_eq!(primary_why(&l, &track), Why::Inherits);
    }

    #[test]
    fn iface_outranks_heuristic() {
        let l = lines(0, 0, 0, 1);
        let track = WhyTrack {
            iface: Some(Why::UsesMemberExt),
            heuristic: Some(Why::UsesMemberGuess),
            ..WhyTrack::default()
        };
        assert_eq!(primary_why(&l, &track), Why::UsesMemberExt);
    }

    #[test]
    fn all_five_slots_set_still_picks_ctor_di() {
        let l = KindLines {
            ctor_di: 1,
            direct: 1,
            direct_amb: 1,
            iface: 1,
            heuristic: 1,
        };
        let track = WhyTrack {
            direct: Some(Why::UsesType),
            direct_amb: Some(Why::Inherits),
            iface: Some(Why::UsesMemberExt),
            heuristic: Some(Why::UsesMemberGuess),
        };
        assert_eq!(primary_why(&l, &track), Why::CtorDi);
    }
}
