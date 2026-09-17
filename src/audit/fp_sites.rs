// One row per false-positive edge, written by `audit --fp-sites <file>`.
//
// Rows are built inside the scoring loop from the same evidence set that
// decided the edge's class, so a row can never disagree with the tier counts
// the report prints: the rows of one class and tier always number exactly
// that tier's counter for the class.

use super::{EdgeRow, OracleRef};

pub(super) struct FpSite {
    file: String,
    line: usize,
    tier: &'static str,
    class: &'static str,
    member: Option<String>,
    bound: String,
    bound_file: String,
    /// In-tree targets the oracle records for this reference, deduplicated
    /// in record order. Empty for a site the oracle never saw or whose every
    /// record is external.
    expected: Vec<String>,
    structural: bool,
}

impl FpSite {
    /// The tier counter this row belongs to: `no-site`, `external` or
    /// `wrong-target`.
    #[cfg(test)]
    pub(super) fn class(&self) -> &'static str {
        self.class
    }

    pub(super) fn new(
        e: &EdgeRow,
        class: &'static str,
        evidence: &[&OracleRef],
        structural: bool,
    ) -> FpSite {
        let mut expected: Vec<String> = Vec::new();
        for r in evidence.iter().filter(|r| !r.external) {
            if let Some(t) = &r.target {
                if !expected.contains(t) {
                    expected.push(t.clone());
                }
            }
        }
        FpSite {
            file: e.from_file.clone(),
            line: e.from_line,
            tier: e.tier.key(),
            class,
            member: e.member.clone(),
            bound: e.to.clone(),
            bound_file: e.to_file.clone(),
            expected,
            structural,
        }
    }
}

/// JSON Lines, one object per row, in the order the edges were scored.
pub(super) fn render(rows: &[FpSite]) -> String {
    let mut out = String::new();
    for s in rows {
        let row = serde_json::json!({
            "file": s.file,
            "line": s.line,
            "tier": s.tier,
            "class": s.class,
            "member": s.member,
            "bound": s.bound,
            "boundFile": s.bound_file,
            "expected": s.expected,
            "structural": s.structural,
        });
        out.push_str(&row.to_string());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::{score, DefRow, Inputs, Tier};
    use super::*;

    fn edge(line: usize, tier: Tier, member: &str) -> EdgeRow {
        EdgeRow {
            from_file: "F.cs".into(),
            from_line: line,
            to: "Ns.Order".into(),
            to_file: "F.cs".into(),
            tier,
            member: Some(member.to_string()),
        }
    }

    fn record(line: usize, member: &str, target: Option<&str>, external: bool) -> OracleRef {
        OracleRef {
            file: "F.cs".into(),
            start_line: line,
            shape: "access".into(),
            receiver_kind: "ident".into(),
            member: member.into(),
            target: target.map(str::to_string),
            target_kind: target.map(|_| "class".to_string()),
            target_file: None,
            external,
            ambiguous: false,
        }
    }

    fn inputs(edges: Vec<EdgeRow>, records: Vec<OracleRef>) -> Inputs {
        Inputs {
            root: PathBuf::from("/repo"),
            graph_defs: vec![DefRow {
                id: "Ns.Order".into(),
                file: "F.cs".into(),
                kind: "class".into(),
                test: false,
            }],
            oracle_defs: Vec::new(),
            edges,
            records,
            units: Vec::new(),
            universe: ["F.cs".to_string()].into_iter().collect(),
            collect_fp_sites: true,
        }
    }

    /// The sink reads the same evidence set the class split read, so the rows
    /// of each class always number exactly that class's tier counter.
    #[test]
    fn every_false_positive_earns_one_row_of_its_own_class() {
        let report = score(inputs(
            vec![
                edge(10, Tier::Guess, "Save"),
                edge(20, Tier::Guess, "Name"),
                edge(30, Tier::Guess, "Gone"),
            ],
            vec![
                record(10, "Save", None, true),
                record(20, "Name", Some("Ns.Invoice"), false),
            ],
        ));

        let (_, ts) = &report.tiers[0];
        assert_eq!(
            (ts.fp_external_site, ts.fp_wrong_target, ts.fp_no_site),
            (1, 1, 1)
        );
        let class_count = |c: &str| report.fp_sites.iter().filter(|r| r.class() == c).count();
        assert_eq!(class_count("external"), ts.fp_external_site);
        assert_eq!(class_count("wrong-target"), ts.fp_wrong_target);
        assert_eq!(class_count("no-site"), ts.fp_no_site);
        assert_eq!(report.fp_sites.len(), ts.fp);
    }

    /// The expected target is what makes a wrong-target row actionable: the
    /// edge bound `Ns.Order` where the oracle recorded `Ns.Invoice`.
    #[test]
    fn a_wrong_target_row_carries_the_target_the_oracle_expected() {
        let report = score(inputs(
            vec![edge(20, Tier::Precise, "Name")],
            vec![record(20, "Name", Some("Ns.Invoice"), false)],
        ));

        let rendered = render(&report.fp_sites);
        let row: serde_json::Value =
            serde_json::from_str(rendered.trim()).expect("one row of valid JSON");
        assert_eq!(row["class"], "wrong-target");
        assert_eq!(row["bound"], "Ns.Order");
        assert_eq!(row["expected"][0], "Ns.Invoice");
        assert_eq!(row["member"], "Name");
        assert_eq!(row["tier"], "precise");
    }
}
