//! The registered fault-control battery: one control per registered wrong-
//! system row, plus the meta-check that the registered list is exhaustive
//! and every row is actually caught.

use devscout_rs::truth::fault_controls::{
    battery_is_exhaustive_and_all_caught, run_battery, FaultControl,
};

#[test]
fn every_registered_control_is_caught_and_the_list_is_exhaustive() {
    assert_eq!(battery_is_exhaustive_and_all_caught(), Ok(()));
}

#[test]
fn the_nine_named_rows_are_all_registered() {
    let labels: Vec<&str> = FaultControl::ALL.iter().map(|c| c.label()).collect();
    for expected in [
        "empty-output",
        "promoted-failed-binding",
        "promoted-ambiguity",
        "dropped-project",
        "dropped-generated-document",
        "wrong-requested-target",
        "stale-cache-reuse",
        "denominator-shrinkage",
        "self-agreeing-wrong-producer",
    ] {
        assert!(
            labels.contains(&expected),
            "control '{expected}' is not registered"
        );
    }
}

#[test]
fn a_run_reports_which_control_each_result_belongs_to() {
    for result in run_battery() {
        assert!(
            result.caught,
            "{} was not caught by the harness",
            result.control.label()
        );
    }
}
