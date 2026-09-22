//! Integration tests for lambda parameters typed from an in-graph callee's
//! delegate parameter: `reg.Register(x => x.Configure())` where
//! `Register(Action<Options> configure)` is declared in the graph types `x`
//! as `Options`, so `x.Configure()` resolves precisely.

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
            "devscout-delegate-lambda-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-delegate-lambda");
        for name in ["Options.cs", "Registrar.cs", "Host.cs", "Ledger.cs"] {
            fs::copy(source.join(name), root.join(name)).unwrap();
        }
        let registry = root.join("registry.json");
        let fx = Self { root, registry };
        fx.ok(&["init", "--no-hooks"]);
        fx
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
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn graph(&self) -> serde_json::Value {
        let text = fs::read_to_string(self.root.join(".scout/graph/graph.json"))
            .expect("graph.json must exist after init");
        serde_json::from_str(&text).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

// A precise row for the site line: `uses-member` followed directly by the
// source text, no tier word in between (a guess row reads `uses-member (guess)`).
fn precise_row(line: usize) -> String {
    format!("Host.cs:{line}  uses-member  ")
}

#[test]
fn a_lambda_parameter_is_typed_from_the_callee_delegate_parameter() {
    let fx = Fixture::new();

    let configure = fx.ok(&["refs", "Configure"]);
    assert!(
        configure.contains(&precise_row(12)),
        "Action<Options> types the lambda parameter: {configure}"
    );
    assert!(
        configure.contains(&precise_row(21)),
        "a bare call resolves through the enclosing type's own method: {configure}"
    );
    assert!(
        !configure.contains(&precise_row(17)),
        "overloads disagreeing on the delegate type bind nothing: {configure}"
    );
    assert!(
        !configure.contains(&precise_row(19)),
        "an explicitly generic callee records no slot: {configure}"
    );

    let enabled = fx.ok(&["refs", "Enabled"]);
    assert!(
        enabled.contains(&precise_row(13)),
        "Func<Options, bool> types the lambda parameter: {enabled}"
    );
    assert!(
        enabled.contains(&precise_row(14)),
        "Expression<Func<Options, object>> unwraps once: {enabled}"
    );

    let bind = fx.ok(&["refs", "Bind"]);
    assert!(
        bind.contains(&precise_row(15)),
        "the second lambda parameter is typed positionally: {bind}"
    );
    assert!(
        bind.contains(&precise_row(20)),
        "an extension callee types the lambda after its this parameter: {bind}"
    );

    let open = fx.ok(&["refs", "Open"]);
    assert!(
        open.contains(&precise_row(16)),
        "an in-graph delegate declaration types the lambda: {open}"
    );

    let tune = fx.ok(&["refs", "Tune"]);
    assert!(
        tune.contains(&precise_row(18)),
        "overloads agreeing on the delegate type bind: {tune}"
    );
}

#[test]
fn a_one_parameter_lambda_never_binds_an_overload_taking_a_two_parameter_delegate() {
    let fx = Fixture::new();

    // Two defs named `Splice` coexist on purpose (the instance method and
    // the extension), so `refs Splice` itself is ambiguous -- the graph's
    // own edges are what pins which one each call site actually bound.
    let graph = fx.graph();
    let splice_edges: Vec<&serde_json::Value> = graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            e["kind"] == "uses-member" && e["from_file"] == "Ledger.cs" && e["member"] == "Splice"
        })
        .collect();
    let edge_at = |line: u64| splice_edges.iter().find(|e| e["from_line"] == line);

    // A one-parameter lambda, with or without an explicit single type
    // argument, must never bind `Splice<TA,TB>(Func<TA,TB,object>)`: real
    // C# routes both to the one-parameter extension instead (confirmed
    // against the semantic oracle), which the resolver here reaches only
    // through the heuristic "ext" tier -- never a precise edge to `Ledger`.
    for line in [29, 34] {
        let edge = edge_at(line)
            .unwrap_or_else(|| panic!("no Splice edge at line {line}: {splice_edges:#?}"));
        assert_eq!(
            edge["to"], "Fixture.Ext.LedgerExtensions",
            "line {line} must bind the one-parameter extension, not the two-parameter instance method: {splice_edges:#?}"
        );
        assert_eq!(
            edge["heuristic"], true,
            "line {line} must not be a PRECISE edge to the two-parameter instance method: {splice_edges:#?}"
        );
    }

    // A genuine two-parameter lambda must still bind the two-parameter
    // overload precisely -- the positive control that the gate above is
    // arity-scoped, not a blanket refusal of `Splice`.
    let two_param =
        edge_at(39).unwrap_or_else(|| panic!("no Splice edge at line 39: {splice_edges:#?}"));
    assert_eq!(two_param["to"], "Fixture.Domain.Ledger");
    assert!(
        two_param["heuristic"].is_null(),
        "line 39 must resolve precisely: {splice_edges:#?}"
    );
}
