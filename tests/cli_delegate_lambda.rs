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
        for name in ["Options.cs", "Registrar.cs", "Host.cs"] {
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
