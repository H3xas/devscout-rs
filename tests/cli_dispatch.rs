//! Integration tests for the DI-registration `implements`/`overrides` edges
//! and `--no-dispatch`, driven against `fixtures/dispatch-signals/`.

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
            "devscout-dispatch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/dispatch-signals");
        for entry in fs::read_dir(&source).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            if Path::new(&name).extension().and_then(|e| e.to_str()) == Some("cs") {
                fs::copy(entry.path(), root.join(&name)).unwrap();
            }
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

#[test]
fn refs_on_the_service_interface_lists_both_of_its_implementations_by_default() {
    let fx = Fixture::new();
    let out = fx.ok(&["refs", "IBeacon"]);
    assert!(out.contains("implements ("), "{out}");
    assert!(out.contains("LightBeacon.cs"), "{out}");
    assert!(out.contains("LoudBeacon.cs"), "{out}");
}

#[test]
fn no_dispatch_drops_the_implements_table_from_refs_entirely() {
    let fx = Fixture::new();
    let out = fx.ok(&["refs", "IBeacon", "--no-dispatch"]);
    assert!(
        !out.contains("implements ("),
        "the whole table must be absent under --no-dispatch: {out}"
    );
}

#[test]
fn refs_on_the_base_class_lists_the_override_chain() {
    let fx = Fixture::new();
    let out = fx.ok(&["refs", "BaseBeacon"]);
    assert!(out.contains("overrides ("), "{out}");
    assert!(out.contains("LightBeacon.cs"), "{out}");

    let narrowed = fx.ok(&["refs", "BaseBeacon", "--no-dispatch"]);
    assert!(
        !narrowed.contains("overrides ("),
        "the whole table must be absent under --no-dispatch: {narrowed}"
    );
}

#[test]
fn an_arity_tied_overload_pair_leaves_the_member_level_implements_edge_unemitted() {
    let fx = Fixture::new();
    let out = fx.ok(&["refs", "IRelay", "--json"]);
    let v: serde_json::Value = serde_json::from_str(out.trim()).expect("valid JSON");
    // The type-level fact resolves (the registration itself is unambiguous);
    // no member-level edge names the arity-tied `Notify` -- exactly one row,
    // never two.
    assert_eq!(
        v["inbound"]["implements"]["total"], 1,
        "only the type-level implements edge survives the arity tie: {out}"
    );
}

#[test]
fn impact_on_a_registered_implementation_reaches_the_interfaces_caller_and_no_dispatch_removes_it()
{
    let fx = Fixture::new();
    // `SilentBeacon` declares no `: IBeacon` base at all -- the type-level
    // `implements` edge is the only path to `IBeacon` here, which is what
    // isolates this widening from the ordinary base-list `inherits` case
    // `LightBeacon`/`LoudBeacon` also exercise.
    let reached = fx.ok(&["impact", "SilentBeacon.cs", "--json"]);
    assert!(reached.contains("Caller.cs"), "{reached}");

    let narrowed = fx.ok(&["impact", "SilentBeacon.cs", "--no-dispatch", "--json"]);
    assert!(
        !narrowed.contains("Caller.cs"),
        "--no-dispatch must remove the widening the implements edge provided: {narrowed}"
    );
}

#[test]
fn a_one_type_argument_registration_call_is_invisible_to_refs() {
    let fx = Fixture::new();
    // `Wiring.cs` also calls `AddSingleton<IRelay>()` with one type argument;
    // it must contribute no second implementation to `IRelay`'s answer.
    let out = fx.ok(&["refs", "IRelay", "--json"]);
    let v: serde_json::Value = serde_json::from_str(out.trim()).expect("valid JSON");
    assert_eq!(v["inbound"]["implements"]["total"], 1);
}
