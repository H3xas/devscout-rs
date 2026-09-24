// `impact`'s per-row `why`: which rule or tier best explains why a file was
// reached, folded across every edge kind the walk saw for it. Split out of
// `impact.rs` to keep that file under its size ceiling; `pub(super)`
// throughout, so only `impact.rs` itself reaches in.

use crate::graph;

use super::impact::VisitedEntry;
use super::seq::SeqSet;
// Re-exported so `impact.rs` reaches both through this one module -- it
// never needs `super::why` directly.
use super::why::why_for_edge;
pub(super) use super::why::Why;

/// One representative referencing line PER EDGE KIND that reached this file,
/// the lowest line per kind.
///
/// `0` means "this kind never contributed", which
/// keeps a row a kind never touched free of that key in `--json`. `direct_amb`
/// is the ambiguous half of the `direct` kind, kept apart only so
/// `build_impact_model` can apply the resolved-over-ambiguous tie-break instead
/// of letting an ambiguous line win by being numerically smaller.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KindLines {
    /// The direct value.
    pub direct: usize,
    /// The direct amb value.
    pub direct_amb: usize,
    /// The ctor di value.
    pub ctor_di: usize,
    /// The heuristic value.
    pub heuristic: usize,
    /// The iface value.
    pub iface: usize,
    /// The lowest `bus-hop` line reaching this file, weakest of every slot.
    pub bus: usize,
}

// The lowest-line-wins guard for `ctor_di`, whose `why` is always `ctor-di`;
// every other `KindLines` slot routes through `Hit`'s `note_*` methods instead.
pub(super) fn note_line(slot: &mut usize, line: usize) {
    if line > 0 && (*slot == 0 || line < *slot) {
        *slot = line;
    }
}

/// One `bus-hop` edge's own identity: the route a possible-route row
/// discloses, and what a later hop's downstream row inherits when it is
/// reached only through this one.
///
/// Total order `(file, line, message, to)`
/// byte order (the same route-tuple ordering a `bus-hop` edge's own identity
/// uses elsewhere in this engine), `to_file` a final harmless tiebreak -- so
/// "the lowest by (publisher file, line, message, handler)" is a single
/// comparison, never a policy choice made ad hoc at each call site.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BusOrigin {
    /// The publish site's own file.
    pub file: String,
    /// The publish site's own line.
    pub line: usize,
    /// The resolved message identity.
    pub message: String,
    /// The handler def id.
    pub to: String,
    /// The handler's own file.
    pub to_file: String,
}

// `pub(super)` throughout: `build_visited_entry` reads every field of a
// finished `Hit` to assemble one `VisitedEntry`.
#[derive(Debug, Clone, Default)]
pub(super) struct Hit {
    pub(super) via_count: u32,
    pub(super) ambiguous_count: u32,
    pub(super) heuristic_count: u32,
    // How many of `heuristic_count` came from the EXTENSION tier. Counted
    // rather than flagged so the walk keeps one shape for both tiers, and one
    // is all the row needs to call itself an extension (see `row_tier`).
    pub(super) ext_count: u32,
    pub(super) symbols: SeqSet<String>,
    // The `via` labels an interface-hop hit at this file carries
    // (`"IFoo (ctor-di)"` or bare `"IFoo"`), first-seen order.
    pub(super) iface_via: SeqSet<String>,
    // One representative line per edge kind (see `KindLines`).
    pub(super) lines: KindLines,
    // Which edge explains each of `lines`' slots -- see `WhyTrack`.
    pub(super) why_track: WhyTrack,
    // Set by `note_taint` once an untainted edge reaches this file.
    pub(super) has_non_bus_path: bool,
    // The lowest-ordered bus-hop origin any path to this file crossed --
    // present whenever at least one tainted edge (direct or inherited)
    // reached it, regardless of whether an untainted edge also did; a row's
    // own disclosure gates on `bus_only` (`!has_non_bus_path`) instead of on
    // this being present, so a file with both kinds of path keeps its
    // stronger `why` and drops the marker without needing this cleared too.
    pub(super) bus_origin: Option<BusOrigin>,
}

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
    /// The weakest slot: a `bus-hop` edge is a possible, runtime-unverified
    /// route, so it names a file's `why` only when no other kind reached it
    /// at all -- one further step below `heuristic`, which is at least a
    /// confirmed edge the resolver merely guessed the target of.
    pub(super) bus: Option<Why>,
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
    /// Records a resolved (precise) reference reaching this file. `origin`
    /// is the bus-hop identity the frontier def this edge came from was
    /// itself only reachable through, when it was reachable only that way;
    /// every non-`note_bus` method takes it so taint threads through the
    /// walk, carrying the origin along with it, without a separate call at
    /// each site.
    pub(super) fn note_direct(
        &mut self,
        line: usize,
        edge: &graph::Edge,
        origin: Option<&BusOrigin>,
    ) {
        let why = why_for_edge(edge);
        note_line_why(
            &mut self.lines.direct,
            &mut self.why_track.direct,
            line,
            why,
        );
        self.note_taint(origin);
    }
    /// Records an ambiguous reference reaching this file.
    pub(super) fn note_direct_amb(
        &mut self,
        line: usize,
        edge: &graph::Edge,
        origin: Option<&BusOrigin>,
    ) {
        let why = why_for_edge(edge);
        note_line_why(
            &mut self.lines.direct_amb,
            &mut self.why_track.direct_amb,
            line,
            why,
        );
        self.note_taint(origin);
    }
    /// Records the interface hop reaching this file.
    pub(super) fn note_iface(
        &mut self,
        line: usize,
        edge: &graph::Edge,
        origin: Option<&BusOrigin>,
    ) {
        let why = why_for_edge(edge);
        note_line_why(&mut self.lines.iface, &mut self.why_track.iface, line, why);
        self.note_taint(origin);
    }
    /// Records a heuristic (guessed) edge reaching this file.
    pub(super) fn note_heuristic(
        &mut self,
        line: usize,
        edge: &graph::Edge,
        origin: Option<&BusOrigin>,
    ) {
        let why = why_for_edge(edge);
        note_line_why(
            &mut self.lines.heuristic,
            &mut self.why_track.heuristic,
            line,
            why,
        );
        self.note_taint(origin);
    }
    /// Records a `bus-hop` edge reaching this file, in either direction --
    /// always `Why::BusHop`, kept in its own slot so it never outranks a
    /// stronger kind merely by sitting on an earlier line. Never clears the
    /// taint: a bus hop is exactly what taints a path in the first place.
    /// Records the edge's OWN identity as this hit's origin (the lowest
    /// seen so far), distinct from `note_taint`'s inherited origin: this is
    /// the hop itself, not a hop some upstream def merely carried forward.
    pub(super) fn note_bus(&mut self, line: usize, edge: &graph::Edge) {
        let why = why_for_edge(edge);
        note_line_why(&mut self.lines.bus, &mut self.why_track.bus, line, why);
        if let graph::Edge::BusHop {
            from_file,
            from_line,
            message,
            to,
            to_file,
            ..
        } = edge
        {
            self.note_bus_origin(BusOrigin {
                file: from_file.clone(),
                line: *from_line,
                message: message.clone(),
                to: to.clone(),
                to_file: to_file.clone(),
            });
        }
    }
    // Keeps the lowest-ordered `BusOrigin` seen so far for this hit.
    fn note_bus_origin(&mut self, origin: BusOrigin) {
        if self
            .bus_origin
            .as_ref()
            .is_none_or(|existing| origin < *existing)
        {
            self.bus_origin = Some(origin);
        }
    }
    /// Clears the possible-route taint the moment ANY edge reaching this
    /// file is untainted (`origin` is `None`), and otherwise records the
    /// inherited origin -- the same taint/untaint decision as before, now
    /// carrying an identity rather than a bare flag. `ctor_di`'s own site
    /// has no `note_*` method of its own to fold this into, so it calls
    /// this directly.
    pub(super) fn note_taint(&mut self, origin: Option<&BusOrigin>) {
        match origin {
            Some(o) => self.note_bus_origin(o.clone()),
            None => self.has_non_bus_path = true,
        }
    }
}

// Assembles a row's per-kind representative lines. The resolved-over-ambiguous
// tie-break decides the `direct` kind: a resolved site outranks an ambiguous
// one, and the ambiguous line is used only when the resolved half never fired.
pub(super) fn from_lines_of(lines: &KindLines) -> Vec<(&'static str, usize)> {
    let mut out: Vec<(&'static str, usize)> = Vec::new();
    let direct = if lines.direct != 0 {
        lines.direct
    } else {
        lines.direct_amb
    };
    if direct != 0 {
        out.push(("direct", direct));
    }
    if lines.ctor_di != 0 {
        out.push(("ctor-di", lines.ctor_di));
    }
    if lines.heuristic != 0 {
        out.push(("heuristic", lines.heuristic));
    }
    if lines.iface != 0 {
        out.push(("iface", lines.iface));
    }
    if lines.bus != 0 {
        out.push(("bus", lines.bus));
    }
    out
}

/// Routes one edge -- from either walk direction, reverse or forward -- to
/// `note_bus` or `note_direct`, whichever it is: the one call site both
/// kinds share, since a `bus-hop` edge always records its OWN identity via
/// `note_bus` regardless of `origin`, while every other kind carries the
/// frontier def's own inherited origin through to `note_direct`.
pub(super) fn note_kind_edge(
    h: &mut Hit,
    line: usize,
    edge: &graph::Edge,
    origin: Option<&BusOrigin>,
) {
    if matches!(edge, graph::Edge::BusHop { .. }) {
        h.note_bus(line, edge);
    } else {
        h.note_direct(line, edge, origin);
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
    if lines.heuristic != 0 || track.heuristic.is_some() {
        return track.heuristic.unwrap_or(Why::UsesMemberGuess);
    }
    track.bus.unwrap_or(Why::BusHop)
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
        bus_only: !h.has_non_bus_path,
        // Present only when `bus_only` is: a file also reached by a real
        // path keeps its stronger `why` and discloses no origin at all.
        bus_origin: if h.has_non_bus_path {
            None
        } else {
            h.bus_origin.clone()
        },
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
            bus: 0,
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
    fn bus_alone_reports_its_tracked_why() {
        let l = KindLines {
            bus: 1,
            ..lines(0, 0, 0, 0)
        };
        let track = WhyTrack {
            bus: Some(Why::BusHop),
            ..WhyTrack::default()
        };
        assert_eq!(primary_why(&l, &track), Why::BusHop);
    }

    #[test]
    fn heuristic_outranks_bus() {
        let l = KindLines {
            heuristic: 1,
            bus: 1,
            ..lines(0, 0, 0, 0)
        };
        let track = WhyTrack {
            heuristic: Some(Why::UsesMemberGuess),
            bus: Some(Why::BusHop),
            ..WhyTrack::default()
        };
        assert_eq!(primary_why(&l, &track), Why::UsesMemberGuess);
    }

    #[test]
    fn all_six_slots_set_still_picks_ctor_di() {
        let l = KindLines {
            ctor_di: 1,
            direct: 1,
            direct_amb: 1,
            iface: 1,
            heuristic: 1,
            bus: 1,
        };
        let track = WhyTrack {
            direct: Some(Why::UsesType),
            direct_amb: Some(Why::Inherits),
            iface: Some(Why::UsesMemberExt),
            heuristic: Some(Why::UsesMemberGuess),
            bus: Some(Why::BusHop),
        };
        assert_eq!(primary_why(&l, &track), Why::CtorDi);
    }
}
