use crate::query;

use super::blocks::{compact_block, ref_kind_block, ref_kind_block_if_any};
use super::markers::{compact_marker, heuristic_suffix, source_suffix, BUS_HOP_UNVERIFIED};

// The one line that splits an enum's inbound member edges by which MEMBER they
// land on. `refs Toggles` already counted them all under `uses-member`; this
// says how many of that total were member-level and which members carried them,
// which is the half of the answer a caller otherwise has to run a second query
// (or a grep) to get. Printed only for an enum with at least one member-level
// reference, so no other symbol's output moves.
fn member_refs_line(m: &query::MemberRefs) -> String {
    let named = m
        .members
        .iter()
        .map(|e| format!("{} {}", e.name, e.count))
        .collect::<Vec<_>>()
        .join(", ");
    let more = if m.dropped != 0 {
        format!(" +{} more", m.dropped)
    } else {
        String::new()
    };
    format!(
        "member refs: {} across {} member(s): {named}{more}",
        m.total, m.member_count
    )
}

// One `bus-hop` provenance row: publisher (`file:line`), direction, the
// resolved message, the handler (`to`/`to_file`), the evidence word, and how
// many handlers the message reaches in all -- every fact a bus-hop row must
// show, regardless of which side of the edge the queried symbol sits on (see
// `BusHopRow`'s own doc comment). `handlers` is what separates one route
// from one shared contract every publisher appears to reach. `pub(super)` so
// `render::coverage`'s own `tests` bus table reuses this verbatim rather
// than respelling the disclosure a second time.
pub(super) fn bus_hop_line(r: &query::BusHopRow) -> String {
    format!(
        "{}:{}  {}  message={}  handler={}  handlerFile={}  evidence={}  handlers={}  \
         ({BUS_HOP_UNVERIFIED})",
        r.file,
        r.line,
        r.direction.as_str(),
        r.message,
        r.to,
        r.to_file,
        r.evidence,
        r.message_handlers
    )
}

/// Renders a reference-query model as human-readable text.
#[allow(
    clippy::too_many_lines,
    reason = "one ordered pass emitting every section of the refs model in the fixed order the text output promises"
)]
pub fn render_refs_text(model: &query::RefsModel) -> String {
    let mut out: Vec<String> = Vec::new();
    out.push(format!("{}  ({})", model.id, model.kind));
    out.push(format!(
        "def: {}",
        model
            .sites
            .iter()
            .map(|s| format!("{}:{}", s.file, s.line))
            .collect::<Vec<_>>()
            .join("  ")
    ));

    out.push("inbound:".to_string());
    ref_kind_block(
        &mut out,
        "inherits",
        Some(&model.inbound.inherits),
        |r: &query::InboundRow| {
            format!(
                "{}:{}  inherits{}{}",
                r.file,
                r.line,
                heuristic_suffix(r.heuristic, r.tier),
                source_suffix(&r.source)
            )
        },
    );
    ref_kind_block(
        &mut out,
        "uses-type",
        Some(&model.inbound.uses_type),
        |r: &query::InboundRow| {
            format!(
                "{}:{}  uses-type{}{}",
                r.file,
                r.line,
                heuristic_suffix(r.heuristic, r.tier),
                source_suffix(&r.source)
            )
        },
    );
    ref_kind_block(
        &mut out,
        "uses-member",
        Some(&model.inbound.uses_member),
        |r: &query::InboundRow| {
            format!(
                "{}:{}  uses-member{}{}",
                r.file,
                r.line,
                heuristic_suffix(r.heuristic, r.tier),
                source_suffix(&r.source)
            )
        },
    );
    ref_kind_block_if_any(
        &mut out,
        "implements",
        &model.inbound.implements,
        |r: &query::InboundRow| {
            format!(
                "{}:{}  implements{}",
                r.file,
                r.line,
                source_suffix(&r.source)
            )
        },
    );
    ref_kind_block_if_any(
        &mut out,
        "overrides",
        &model.inbound.overrides,
        |r: &query::InboundRow| {
            format!(
                "{}:{}  overrides{}",
                r.file,
                r.line,
                source_suffix(&r.source)
            )
        },
    );
    // One trailer for the five kinds, because they share one cap: the
    // per-kind headers say how much each kind lost, this says what the call as
    // a whole did not return. `--all` lifts this cap too, the same lever the
    // outbound trailer below names, but the text here already reports the true
    // drop count and is unchanged whether or not `--all` is set.
    let inbound_dropped = model.inbound.inherits.dropped
        + model.inbound.uses_type.dropped
        + model.inbound.uses_member.dropped
        + model.inbound.implements.dropped
        + model.inbound.overrides.dropped;
    if inbound_dropped != 0 {
        out.push(format!("  +{inbound_dropped} more"));
    }
    if let Some(m) = &model.member_refs {
        out.push(member_refs_line(m));
    }
    // Prints only when the symbol carries at least one bus-hop row -- the
    // same present-only-when-non-empty rule `implements`/`overrides` follow,
    // so a symbol untouched by a bus hop renders exactly as it did before
    // this section existed.
    ref_kind_block_if_any(&mut out, "bus-hop", &model.bus, bus_hop_line);

    if let Some(ob) = &model.outbound {
        out.push("outbound:".to_string());
        ref_kind_block(
            &mut out,
            "inherits",
            Some(&ob.inherits),
            |r: &query::OutboundRow| {
                format!(
                    "{}:{}  inherits  -> {}{}{}",
                    r.file,
                    r.line,
                    r.to_file,
                    heuristic_suffix(r.heuristic, r.tier),
                    source_suffix(&r.source)
                )
            },
        );
        ref_kind_block(
            &mut out,
            "uses-type",
            Some(&ob.uses_type),
            |r: &query::OutboundRow| {
                format!(
                    "{}:{}  uses-type  -> {}{}{}",
                    r.file,
                    r.line,
                    r.to_file,
                    heuristic_suffix(r.heuristic, r.tier),
                    source_suffix(&r.source)
                )
            },
        );
        ref_kind_block(
            &mut out,
            "uses-member",
            Some(&ob.uses_member),
            |r: &query::OutboundRow| {
                format!(
                    "{}:{}  uses-member  -> {}{}{}",
                    r.file,
                    r.line,
                    r.to_file,
                    heuristic_suffix(r.heuristic, r.tier),
                    source_suffix(&r.source)
                )
            },
        );
        ref_kind_block_if_any(
            &mut out,
            "implements",
            &ob.implements,
            |r: &query::OutboundRow| {
                format!(
                    "{}:{}  implements  -> {}{}",
                    r.file,
                    r.line,
                    r.to_file,
                    source_suffix(&r.source)
                )
            },
        );
        ref_kind_block_if_any(
            &mut out,
            "overrides",
            &ob.overrides,
            |r: &query::OutboundRow| {
                format!(
                    "{}:{}  overrides  -> {}{}",
                    r.file,
                    r.line,
                    r.to_file,
                    source_suffix(&r.source)
                )
            },
        );
        ref_kind_block(
            &mut out,
            "imports",
            Some(&ob.imports),
            |r: &query::ImportRow| {
                format!(
                    "{}:{}  imports  -> {}{}",
                    r.file,
                    r.line,
                    r.target,
                    source_suffix(&r.source)
                )
            },
        );
        // One trailer for the six outbound kinds, because they share one cap
        // (mirrors the inbound trailer above). Printing only ever happens when
        // `--all` would actually return more rows, so the hint is never dead
        // advice -- true of the inbound trailer above too, which is why that one
        // stays text-only rather than gaining the same hint.
        let outbound_dropped = ob.inherits.dropped
            + ob.uses_type.dropped
            + ob.uses_member.dropped
            + ob.implements.dropped
            + ob.overrides.dropped
            + ob.imports.dropped;
        if outbound_dropped != 0 {
            out.push(format!("  +{outbound_dropped} more, use --all"));
        }
    }

    let amb_in = &model.ambiguous.inbound;
    let amb_out = &model.ambiguous.outbound;
    if amb_in.total != 0 || amb_out.total != 0 {
        out.push("ambiguous:".to_string());
        let amb_row = |r: &query::AmbiguousRow| {
            format!(
                "{}:{}  {}  raw=\"{}\"  candidates={}",
                r.file, r.line, r.origin, r.raw, r.candidate_count
            )
        };
        if amb_in.total != 0 {
            ref_kind_block(&mut out, "inbound", Some(amb_in), amb_row);
        }
        if amb_out.total != 0 {
            ref_kind_block(&mut out, "outbound", Some(amb_out), amb_row);
        }
    }

    if model.manifest_gap != 0 {
        out.push(format!(
            "manifest gap: {} graph file(s) not in manifest",
            model.manifest_gap
        ));
    }
    out.join("\n")
}

/// `--compact` `refs` rendering.
#[allow(
    clippy::too_many_lines,
    reason = "one ordered pass mirroring render_refs_text's section order in the compact shape"
)]
pub fn render_refs_compact(model: &query::RefsModel) -> String {
    let mut out: Vec<String> = Vec::new();
    out.push(format!("{}  ({})", model.id, model.kind));
    out.push(format!(
        "def: {}",
        model
            .sites
            .iter()
            .map(|s| format!("{}:{}", s.file, s.line))
            .collect::<Vec<_>>()
            .join("  ")
    ));

    // `file_of_*` are explicitly typed as `fn(&R) -> &str` (not left to
    // inference) so the closure is assigned its higher-ranked signature
    // (`for<'a> fn(&'a R) -> &'a str`) at the point of declaration --
    // without the annotation, an unannotated `let` binds the closure to
    // one concrete (non-reusable) lifetime instead, which then fails to
    // coerce back down at `compact_block`'s `fn` parameter on the second
    // and later call sites below.
    // The one-character marker is `compact_marker`'s (`x` extension, `h`
    // guess); neither collides with a line number, and the run-length collapse
    // treats `5`, `5h` and `5x` as the three distinct entries they are -- a `5x`
    // seen twice reads `5xx2`.
    //
    // Compact groups a file's hits into one `path:line,line` entry, which
    // leaves no slot for a per-hit source line -- the snippet is a
    // default-renderer and `--json` affordance only.
    let file_of_ib: fn(&query::InboundRow) -> &str = |r| r.file.as_str();
    let line_ib =
        |r: &query::InboundRow| format!("{}{}", r.line, compact_marker(r.heuristic, r.tier));
    compact_block(
        &mut out,
        "in:inherits",
        Some(&model.inbound.inherits),
        file_of_ib,
        line_ib,
    );
    compact_block(
        &mut out,
        "in:uses-type",
        Some(&model.inbound.uses_type),
        file_of_ib,
        line_ib,
    );
    compact_block(
        &mut out,
        "in:uses-member",
        Some(&model.inbound.uses_member),
        file_of_ib,
        line_ib,
    );
    compact_block(
        &mut out,
        "in:implements",
        Some(&model.inbound.implements),
        file_of_ib,
        line_ib,
    );
    compact_block(
        &mut out,
        "in:overrides",
        Some(&model.inbound.overrides),
        file_of_ib,
        line_ib,
    );
    // The direction letter ('i'/'o') stands in for the full `in`/`out` word --
    // compact strips message/evidence to the same terse shape every other
    // block here already takes. The trailing '?' is the possible-route
    // marker: every bus-hop row is unverified, and compact mode has no room
    // for the full disclosure text, only a marker plus where to find it.
    let file_of_bus: fn(&query::BusHopRow) -> &str = |r| r.file.as_str();
    let line_bus = |r: &query::BusHopRow| format!("{}{}?", r.line, &r.direction.as_str()[..1]);
    compact_block(
        &mut out,
        "bus-hop (? = possible route, rerun without --compact for the full row)",
        Some(&model.bus),
        file_of_bus,
        line_bus,
    );

    if let Some(ob) = &model.outbound {
        let file_of_ob: fn(&query::OutboundRow) -> &str = |r| r.file.as_str();
        let line_ob =
            |r: &query::OutboundRow| format!("{}{}", r.line, compact_marker(r.heuristic, r.tier));
        compact_block(
            &mut out,
            "out:inherits",
            Some(&ob.inherits),
            file_of_ob,
            line_ob,
        );
        compact_block(
            &mut out,
            "out:uses-type",
            Some(&ob.uses_type),
            file_of_ob,
            line_ob,
        );
        compact_block(
            &mut out,
            "out:uses-member",
            Some(&ob.uses_member),
            file_of_ob,
            line_ob,
        );
        compact_block(
            &mut out,
            "out:implements",
            Some(&ob.implements),
            file_of_ob,
            line_ob,
        );
        compact_block(
            &mut out,
            "out:overrides",
            Some(&ob.overrides),
            file_of_ob,
            line_ob,
        );

        let file_of_imp: fn(&query::ImportRow) -> &str = |r| r.file.as_str();
        let line_imp = |r: &query::ImportRow| r.line.to_string();
        compact_block(
            &mut out,
            "out:imports",
            Some(&ob.imports),
            file_of_imp,
            line_imp,
        );
    }

    let amb_in = &model.ambiguous.inbound;
    let amb_out = &model.ambiguous.outbound;
    let file_of_amb: fn(&query::AmbiguousRow) -> &str = |r| r.file.as_str();
    let line_amb =
        |r: &query::AmbiguousRow| format!("{}(candidates={})", r.line, r.candidate_count);
    compact_block(&mut out, "amb:in", Some(amb_in), file_of_amb, line_amb);
    compact_block(&mut out, "amb:out", Some(amb_out), file_of_amb, line_amb);

    // The missing-table tolerance documented at the top of this file is what an
    // absent `outbound` falls through here: the three outbound tables plus
    // imports contribute nothing to any of the three sums when the caller did not
    // ask for them.
    let ob = model.outbound.as_ref();
    let ob_sum = |f: fn(&query::OutboundTables) -> usize| ob.map_or(0, f);
    let edges = model.inbound.inherits.total
        + model.inbound.uses_type.total
        + model.inbound.uses_member.total
        + model.inbound.implements.total
        + model.inbound.overrides.total
        + model.bus.total
        + ob_sum(|o| {
            o.inherits.total
                + o.uses_type.total
                + o.uses_member.total
                + o.implements.total
                + o.overrides.total
                + o.imports.total
        });
    let shown = model.inbound.inherits.rows.len()
        + model.inbound.uses_type.rows.len()
        + model.inbound.uses_member.rows.len()
        + model.inbound.implements.rows.len()
        + model.inbound.overrides.rows.len()
        + model.bus.rows.len()
        + ob_sum(|o| {
            o.inherits.rows.len()
                + o.uses_type.rows.len()
                + o.uses_member.rows.len()
                + o.implements.rows.len()
                + o.overrides.rows.len()
                + o.imports.rows.len()
        });
    let dropped = model.inbound.inherits.dropped
        + model.inbound.uses_type.dropped
        + model.inbound.uses_member.dropped
        + model.inbound.implements.dropped
        + model.inbound.overrides.dropped
        + model.bus.dropped
        + ob_sum(|o| {
            o.inherits.dropped
                + o.uses_type.dropped
                + o.uses_member.dropped
                + o.implements.dropped
                + o.overrides.dropped
                + o.imports.dropped
        });
    let ambiguous = amb_in.total + amb_out.total;
    let gap = if model.manifest_gap != 0 {
        format!(" gap={}", model.manifest_gap)
    } else {
        String::new()
    };
    // Same fact as the default renderer's line, spelled the way every other
    // compact block is: `name=count`, no prose, and absent entirely when there is
    // nothing to say.
    if let Some(m) = &model.member_refs {
        let named = m
            .members
            .iter()
            .map(|e| format!("{}={}", e.name, e.count))
            .collect::<Vec<_>>()
            .join(",");
        let more = if m.dropped != 0 {
            format!(",+{}", m.dropped)
        } else {
            String::new()
        };
        out.push(format!("mem: {named}{more}"));
    }
    out.push(format!(
        "summary: edges={edges} shown={shown} dropped={dropped} ambiguous={ambiguous}{gap}"
    ));
    out.join("\n")
}
