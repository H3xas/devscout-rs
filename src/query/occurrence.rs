// Query-time occurrence identity: distinguishes two rows of the same table
// that would otherwise serialize to byte-identical JSON objects (the "two
// calls on one target on one line" case -- see this crate's answer contract
// for the exact convention this assigns).
//
// This computes NO new fact and touches no persisted graph state: it groups
// rows already built by `refs.rs`'s existing ranking/capping pass, in that
// same stable emission order, and assigns a 0-based position within each
// group of size two or more. A row with no colliding sibling in its own
// table gets `None`, never `Some(0)` -- the field exists only where two rows
// would otherwise be indistinguishable.

use super::refs_tables::{InboundRow, OutboundRow};

// Two rows collide when every field but `occurrence_index` matches; comparing
// the whole struct is equivalent here because every row this module ever
// sees still carries `occurrence_index: None` at the point it is tagged --
// tagging runs exactly once, immediately after a table's rows are built and
// before the table is handed to `json.rs`.
fn inbound_collide(a: &InboundRow, b: &InboundRow) -> bool {
    a.file == b.file
        && a.line == b.line
        && a.heuristic == b.heuristic
        && a.tier == b.tier
        && a.source == b.source
}

fn outbound_collide(a: &OutboundRow, b: &OutboundRow) -> bool {
    a.file == b.file
        && a.line == b.line
        && a.to_file == b.to_file
        && a.to == b.to
        && a.heuristic == b.heuristic
        && a.tier == b.tier
        && a.source == b.source
}

/// Tags one inbound table's rows in place, in their existing emission order.
pub(super) fn tag_inbound_rows(rows: &mut [InboundRow]) {
    let assigned = assign_occurrence_indices(rows, inbound_collide);
    for (row, idx) in rows.iter_mut().zip(assigned) {
        row.occurrence_index = idx;
    }
}

/// Tags one outbound table's rows in place, in their existing emission order.
pub(super) fn tag_outbound_rows(rows: &mut [OutboundRow]) {
    let assigned = assign_occurrence_indices(rows, outbound_collide);
    for (row, idx) in rows.iter_mut().zip(assigned) {
        row.occurrence_index = idx;
    }
}

/// Assigns a 0-based occurrence index to every row of `rows` that shares
/// every field `eq` compares with at least one other row, in `rows`' own
/// order; a row with no such sibling gets `None`. `eq` must ignore whatever
/// field the caller intends to fill with the result (comparing it is
/// harmless here since every row starts with that field unset, but `eq`
/// stating the intent explicitly keeps this function honest about what it
/// groups on).
///
/// Quadratic in table size, which is bounded everywhere this is called by
/// the same inbound/outbound caps the rest of this module already applies
/// (tens of rows, not thousands).
pub(super) fn assign_occurrence_indices<T>(
    rows: &[T],
    eq: impl Fn(&T, &T) -> bool,
) -> Vec<Option<usize>> {
    let n = rows.len();
    let mut assigned: Vec<Option<usize>> = vec![None; n];
    for i in 0..n {
        if assigned[i].is_some() {
            continue;
        }
        let group: Vec<usize> = (i..n).filter(|&j| eq(&rows[i], &rows[j])).collect();
        if group.len() < 2 {
            continue;
        }
        for (position, &idx) in group.iter().enumerate() {
            assigned[idx] = Some(position);
        }
    }
    assigned
}

#[cfg(test)]
mod tests {
    use super::assign_occurrence_indices;

    #[derive(Debug, Clone, PartialEq)]
    struct Row {
        line: usize,
        target: &'static str,
    }

    fn same(a: &Row, b: &Row) -> bool {
        a.line == b.line && a.target == b.target
    }

    #[test]
    fn a_singleton_row_gets_no_occurrence_index() {
        let rows = vec![
            Row {
                line: 1,
                target: "A",
            },
            Row {
                line: 2,
                target: "B",
            },
        ];
        assert_eq!(assign_occurrence_indices(&rows, same), vec![None, None]);
    }

    #[test]
    fn two_colliding_rows_get_zero_and_one_in_emission_order() {
        let rows = vec![
            Row {
                line: 5,
                target: "A",
            },
            Row {
                line: 5,
                target: "A",
            },
        ];
        assert_eq!(
            assign_occurrence_indices(&rows, same),
            vec![Some(0), Some(1)]
        );
    }

    #[test]
    fn a_run_boundary_does_not_bleed_into_an_unrelated_group() {
        let rows = vec![
            Row {
                line: 5,
                target: "A",
            },
            Row {
                line: 5,
                target: "A",
            },
            Row {
                line: 9,
                target: "B",
            },
            Row {
                line: 9,
                target: "B",
            },
            Row {
                line: 9,
                target: "B",
            },
        ];
        assert_eq!(
            assign_occurrence_indices(&rows, same),
            vec![Some(0), Some(1), Some(0), Some(1), Some(2)]
        );
    }

    #[test]
    fn a_non_adjacent_collision_is_still_grouped_by_equality_not_adjacency() {
        let rows = vec![
            Row {
                line: 5,
                target: "A",
            },
            Row {
                line: 9,
                target: "B",
            },
            Row {
                line: 5,
                target: "A",
            },
        ];
        assert_eq!(
            assign_occurrence_indices(&rows, same),
            vec![Some(0), None, Some(1)]
        );
    }

    #[test]
    fn three_way_collision_assigns_a_position_to_every_member() {
        let rows = vec![
            Row {
                line: 5,
                target: "A",
            },
            Row {
                line: 5,
                target: "A",
            },
            Row {
                line: 5,
                target: "A",
            },
        ];
        assert_eq!(
            assign_occurrence_indices(&rows, same),
            vec![Some(0), Some(1), Some(2)]
        );
    }

    #[test]
    fn an_empty_table_assigns_nothing() {
        let rows: Vec<Row> = Vec::new();
        assert_eq!(
            assign_occurrence_indices(&rows, same),
            Vec::<Option<usize>>::new()
        );
    }
}
