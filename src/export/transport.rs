//! The `transport-address` family: one line per transport fact the graph
//! records, its keys in the fact's own order after `family` and `evidence`.

use serde::Serialize;

use crate::transport::TransportFact;

use super::evidence;
use super::facts::Fact;
use super::snapshot::Snapshot;

/// The family's rank in the canonical order.
pub(super) const RANK: usize = 7;

#[derive(Serialize)]
struct TransportAddressLine<'a> {
    family: &'static str,
    evidence: &'static str,
    #[serde(flatten)]
    fact: &'a TransportFact,
}

pub(super) fn collect_transport(snapshot: &Snapshot, facts: &mut Vec<Fact>) {
    for fact in &snapshot.graph.transport {
        facts.push(Fact::new(
            RANK,
            fact.file.clone(),
            fact.line,
            &TransportAddressLine {
                family: "transport-address",
                evidence: evidence::DECLARED,
                fact,
            },
        ));
    }
}
