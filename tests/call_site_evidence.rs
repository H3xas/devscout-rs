//! The lossless call-site evidence contract, proved against
//! `fixtures/call-site-evidence/Witness.cs`.
//!
//! `fixtures/call-site-evidence/EXPECTED.md` records every witness's expected
//! call site, authored by reading the fixture source alone, before any
//! `devscout` command was run against it. Every test below checks the real
//! `--json` answer against that hand-authored list, never the reverse: a
//! test that instead asserted "whatever the tool currently prints" would
//! prove nothing about whether a site survived.
//!
//! `two_calls_on_one_line_...` is the one case expected to fail on
//! unmodified `main`: `InboundRow`/`OutboundRow` carried no field able to
//! tell two rows apart when every other field matched, so the pair collapsed
//! to two byte-identical JSON objects. This is the audit's own attribution
//! step -- every other named shape here already round-trips correctly
//! without `occurrenceIndex`, isolating that one genuine collision as the
//! proven gap `src/query/occurrence.rs` closes.
//!
//! The negative cases at the bottom are proof of absence, not a feature:
//! native devscout models no control-flow, await-ordering or dispatch-target
//! fact, so an unresolved target and an unmodeled construct must surface as
//! an explicit named state or an absent key, never a fabricated `false`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    registry: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "devscout-call-site-evidence-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/call-site-evidence");
        fs::copy(source.join("Witness.cs"), root.join("Witness.cs")).unwrap();
        let registry = root.join("registry.json");
        let fixture = Self { root, registry };
        fixture.ok(&["init", "--no-hooks"]);
        fixture
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .current_dir(&self.root)
            .env("SCOUT_REGISTRY", &self.registry)
            .args(args)
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{args:?}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn refs_json(&self, query: &str) -> serde_json::Value {
        let text = self.ok(&["refs", query, "--json"]);
        serde_json::from_str(&text).expect("valid JSON")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

// Every query this fixture resolves is a bare member name, so `refs` always
// answers under the `members` wrapper with exactly one entry -- unwrapped
// once here rather than in every test.
fn member(v: &serde_json::Value) -> &serde_json::Value {
    let members = v["members"].as_array().expect("a members wrapper");
    assert_eq!(members.len(), 1, "exactly one declaring type: {v}");
    &members[0]
}

fn uses_member_rows(model: &serde_json::Value) -> &Vec<serde_json::Value> {
    model["inbound"]["uses-member"]["rows"]
        .as_array()
        .expect("a uses-member rows array")
}

fn rows_at<'a>(rows: &'a [serde_json::Value], file: &str, line: u64) -> Vec<&'a serde_json::Value> {
    rows.iter()
        .filter(|r| r["file"] == file && r["line"].as_u64() == Some(line))
        .collect()
}

// -- Repeated calls, different lines -------------------------------------

#[test]
fn repeated_calls_on_different_lines_are_two_distinct_rows() {
    let fx = Fixture::new();
    let answer = fx.refs_json("Record");
    let model = member(&answer);
    let rows = uses_member_rows(model);
    assert_eq!(rows_at(rows, "Witness.cs", 33).len(), 1, "{rows:?}");
    assert_eq!(rows_at(rows, "Witness.cs", 34).len(), 1, "{rows:?}");
}

// -- Two calls, one line: the proven collision ---------------------------

#[test]
fn two_calls_on_one_line_serialize_as_two_distinguishable_rows_not_one_collapsed_row() {
    let fx = Fixture::new();
    let answer = fx.refs_json("Record");
    let model = member(&answer);
    let rows = uses_member_rows(model);
    let at_line_40 = rows_at(rows, "Witness.cs", 40);
    assert_eq!(
        at_line_40.len(),
        2,
        "both calls on line 40 must survive as rows: {rows:?}"
    );
    let mut occurrences: Vec<Option<u64>> = at_line_40
        .iter()
        .map(|r| r["occurrenceIndex"].as_u64())
        .collect();
    occurrences.sort();
    assert_eq!(
        occurrences,
        vec![Some(0), Some(1)],
        "the colliding pair must carry distinct, 0-based occurrenceIndex values: {at_line_40:?}"
    );
    // No two rows of the WHOLE table may serialize byte-identically -- the
    // property `occurrenceIndex` exists to guarantee.
    let mut serialized: Vec<String> = rows.iter().map(std::string::ToString::to_string).collect();
    let before = serialized.len();
    serialized.sort();
    serialized.dedup();
    assert_eq!(
        serialized.len(),
        before,
        "no two rows in one table may serialize identically: {rows:?}"
    );
}

// -- Overload ambiguity ---------------------------------------------------

#[test]
fn overload_ambiguity_keeps_both_call_sites_as_separate_rows() {
    let fx = Fixture::new();
    let answer = fx.refs_json("Record");
    let model = member(&answer);
    let rows = uses_member_rows(model);
    assert_eq!(rows_at(rows, "Witness.cs", 47).len(), 1, "{rows:?}");
    assert_eq!(rows_at(rows, "Witness.cs", 48).len(), 1, "{rows:?}");
    // Overload identity is a documented gap, not a false claim: neither row
    // names which overload it bound (native devscout has no such fact).
    for row in rows_at(rows, "Witness.cs", 47)
        .into_iter()
        .chain(rows_at(rows, "Witness.cs", 48))
    {
        assert!(row.get("overload").is_none(), "{row}");
        assert!(row.get("arity").is_none(), "{row}");
    }
}

#[test]
fn the_record_table_totals_exactly_the_hand_authored_witness_count() {
    let fx = Fixture::new();
    let answer = fx.refs_json("Record");
    let model = member(&answer);
    // EXPECTED.md: 6 rows -- 33, 34, 40 (x2), 47, 48. Regenerated from a real
    // run, not copied from that document; the two are cross-checked by hand
    // whenever either changes.
    assert_eq!(model["inbound"]["uses-member"]["total"].as_u64(), Some(6));
    assert_eq!(model["inbound"]["uses-member"]["dropped"].as_u64(), Some(0));
}

// -- Recursion --------------------------------------------------------------

#[test]
fn recursion_is_its_own_inbound_row_on_the_callee_it_recurses_into() {
    let fx = Fixture::new();
    let answer = fx.refs_json("Recurse");
    let model = member(&answer);
    let rows = uses_member_rows(model);
    assert_eq!(rows_at(rows, "Witness.cs", 64).len(), 1, "{rows:?}");
}

// -- Awaited sequence ------------------------------------------------------

#[test]
fn awaited_sequence_keeps_both_awaited_calls_as_separate_rows() {
    let fx = Fixture::new();
    let answer = fx.refs_json("RecordAsync");
    let model = member(&answer);
    let rows = uses_member_rows(model);
    assert_eq!(rows_at(rows, "Witness.cs", 71).len(), 1, "{rows:?}");
    assert_eq!(rows_at(rows, "Witness.cs", 72).len(), 1, "{rows:?}");
    // Sequence order is a documented gap: no row claims to be "first" or
    // "second" in execution order, only its own source line.
    for row in rows {
        assert!(row.get("sequence").is_none(), "{row}");
        assert!(row.get("awaitOrder").is_none(), "{row}");
    }
}

// -- Parallel launch/join ----------------------------------------------------

#[test]
fn parallel_launch_and_join_keeps_both_launch_sites_as_separate_rows() {
    let fx = Fixture::new();
    let answer = fx.refs_json("RecordAsync");
    let model = member(&answer);
    let rows = uses_member_rows(model);
    assert_eq!(rows_at(rows, "Witness.cs", 79).len(), 1, "{rows:?}");
    assert_eq!(rows_at(rows, "Witness.cs", 80).len(), 1, "{rows:?}");
}

// -- Negative cases: capability gaps stay absent, never a fabricated value --

#[test]
fn an_unresolved_target_answers_an_explicit_named_outcome_never_a_default_row() {
    let fx = Fixture::new();
    let out = fx.run(&["refs", "NoSuchWitnessTarget", "--json"]);
    let text = String::from_utf8(out.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(text.trim()).expect("valid JSON");
    assert_eq!(v["outcome"], "fallback-advised", "{v}");
    // A seed the graph never named carries no invented row, tier or
    // ordering claim of any kind.
    assert!(v.get("inbound").is_none(), "{v}");
    assert!(v.get("rows").is_none(), "{v}");
}

#[test]
fn no_answer_this_fixture_produces_ever_claims_an_unmodeled_control_or_dispatch_fact() {
    // Native devscout models no branch/await-ordering/dispatch-target fact at
    // all (the sidecar's optional facts are a separate, explicitly gated
    // mode -- see `tests/flowtrace_facts.rs`). Every key below must be
    // absent from every query this fixture answers, on every audited verb:
    // an absent key is the honest gap, and a literal `false` standing in for
    // "not supported" would be the violation this test guards against.
    const NEVER_EMITTED: &[&str] = &[
        "branchPoint",
        "paramSource",
        "exceptionMap",
        "callOrder",
        "awaitOrder",
        "sequence",
        "dispatchTarget",
        "overload",
        "arity",
    ];
    let fx = Fixture::new();
    for query in ["Record", "RecordAsync", "Recurse", "Ledger", "Caller"] {
        for verb in ["refs", "read", "tests", "impact"] {
            let out = fx.run(&[verb, query, "--json"]);
            let text = String::from_utf8(out.stdout).unwrap();
            if text.trim().is_empty() {
                continue;
            }
            for key in NEVER_EMITTED {
                assert!(
                    !text.contains(&format!("\"{key}\"")),
                    "{verb} {query} --json must never carry {key:?}: {text}"
                );
            }
        }
    }
}

#[test]
fn occurrence_index_never_appears_as_a_boolean_or_on_a_row_with_no_collision() {
    let fx = Fixture::new();
    let answer = fx.refs_json("Record");
    let model = member(&answer);
    let rows = uses_member_rows(model);
    for row in rows {
        let at_40 = row["line"].as_u64() == Some(40);
        match row.get("occurrenceIndex") {
            Some(v) => {
                assert!(
                    at_40,
                    "only the line-40 pair may carry occurrenceIndex: {row}"
                );
                assert!(
                    v.is_u64(),
                    "occurrenceIndex must be an unsigned integer: {row}"
                );
            }
            None => assert!(
                !at_40,
                "a colliding row must carry occurrenceIndex, never omit it silently: {row}"
            ),
        }
    }
}
