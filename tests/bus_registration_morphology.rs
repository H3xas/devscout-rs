//! Integration tests for the name rule that decides whether a
//! one-type-argument call is a handler registration, driven against
//! `fixtures/bus-registration-morphology/`.
//!
//! The rule is permissive by design and the resolver is what discards a base
//! that earns no route, so the two sides are pinned separately: which
//! spellings produce a registration fact at all, and where admission stops a
//! registration-shaped call from adding a base. Every case maps the fixture's
//! shared core beside exactly one registration file, so a case that derives
//! nothing proves the registration file is why, never that another corpus
//! file stopped deriving.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

// The shared core every case maps: the bus, the registry, the house handler
// base, the messages, one publisher and one handler.
const CORE: [&str; 4] = ["Press.cs", "Messages.cs", "Handlers.cs", "Publishers.cs"];

struct Fixture {
    root: PathBuf,
    registry: PathBuf,
}

impl Fixture {
    fn subset(extra: &[&str]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "devscout-bus-registration-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/bus-registration-morphology");
        for name in CORE.iter().copied().chain(extra.iter().copied()) {
            fs::copy(source.join(name), root.join(name)).unwrap();
        }
        let registry = root.join("registry.json");
        let fx = Self { root, registry };
        fx.ok(&["init", "--no-hooks"]);
        fx.ok(&["map", "."]);
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
        let bytes = fs::read(self.root.join(".scout/graph/graph.json")).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // Every bus hop as `(publishing file, handler simple name, message
    // simple name, evidence)`, sorted so a case can assert on the whole set.
    fn hops(&self) -> Vec<(String, String, String, String)> {
        let graph = self.graph();
        let mut out: Vec<(String, String, String, String)> = graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "bus-hop")
            .map(|e| {
                (
                    e["from_file"].as_str().unwrap().to_string(),
                    simple(e["to"].as_str().unwrap()),
                    simple(e["message"].as_str().unwrap()),
                    e["evidence"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        out.sort();
        out
    }

    // `None` when no hop was emitted, so no vocabulary claim is made either
    // way -- distinct from a claim of `false`.
    fn derived(&self) -> Option<bool> {
        self.graph()["stats"]
            .get("bus_vocabulary_derived")
            .map(|v| v.as_bool().unwrap())
    }

    // The registration facts one source file contributes, as
    // `(handler, line)`. Read off the fragment cache `map` wrote: the
    // registration record is not carried into the graph, and this is where a
    // fact that contributed no base is still visible.
    fn registrations(&self, file: &str) -> Vec<(String, usize)> {
        let dir = self.root.join(".scout/graph");
        let mut caches: Vec<PathBuf> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("fragments-v") && name.ends_with(".json"))
            })
            .collect();
        caches.sort();
        assert_eq!(caches.len(), 1, "one fragment cache: {caches:?}");
        let bytes = fs::read(&caches[0]).unwrap();
        let cache: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        cache["entries"][file]["fragment"]["handlerRegistrations"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .map(|row| {
                        (
                            row["handler"].as_str().unwrap().to_string(),
                            row["line"].as_u64().unwrap() as usize,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_file(&self.registry);
    }
}

fn simple(id: &str) -> String {
    id.rsplit_once('.').map_or(id, |(_, t)| t).to_string()
}

fn hop(file: &str, to: &str) -> (String, String, String, String) {
    (
        file.to_string(),
        to.to_string(),
        "EditionNotice".to_string(),
        "base-arg".to_string(),
    )
}

#[test]
fn registration_without_generic_base_contributes_nothing() {
    let fx = Fixture::subset(&["RegistrationTargets.cs", "RegisterTargetWithoutBase.cs"]);
    assert_eq!(
        fx.registrations("RegisterTargetWithoutBase.cs"),
        vec![("LooseHandler".to_string(), 7)],
        "the call is admitted by its name, so the registration fact exists"
    );
    assert!(fx.hops().is_empty(), "{:?}", fx.hops());
    assert_eq!(fx.derived(), None, "no base, so no hop and no claim");
}

#[test]
fn middleware_shaped_registration_of_unsent_argument_adds_no_base() {
    let fx = Fixture::subset(&[
        "RegistrationTargets.cs",
        "RegisterMiddlewareOnUnsentArgument.cs",
    ]);
    assert_eq!(
        fx.registrations("RegisterMiddlewareOnUnsentArgument.cs"),
        vec![("LedgerBridge".to_string(), 7)],
        "an install-named one-type-argument call is a registration however the named type is shaped"
    );
    assert!(
        fx.hops().is_empty(),
        "LedgerBridge's own argument is never sent, and the same base on a type whose argument \
         is sent earns nothing while the base is unadmitted: {:?}",
        fx.hops()
    );
    assert_eq!(fx.derived(), None);
}

// The registration admits a base only when the registered type RECEIVES the
// base's resolved argument as a declared parameter; an override that only
// satisfies the base's own unrelated member never does.
#[test]
fn middleware_shaped_registration_without_receiver_adds_no_base() {
    let fx = Fixture::subset(&[
        "RegistrationTargets.cs",
        "RegisterMiddlewareOnUnreceivedArgument.cs",
    ]);
    assert_eq!(
        fx.registrations("RegisterMiddlewareOnUnreceivedArgument.cs"),
        vec![("QuietBridge".to_string(), 7)]
    );
    assert!(
        fx.hops().is_empty(),
        "the argument is sent, but no declaration of QuietBridge takes it, so the base stays out \
         of the vocabulary and the same base on a receiving type stays out with it: {:?}",
        fx.hops()
    );
    assert_eq!(fx.derived(), None);
}

#[test]
fn middleware_shaped_registration_of_sent_and_received_argument_is_admitted() {
    let fx = Fixture::subset(&[
        "RegistrationTargets.cs",
        "RegisterMiddlewareOnReceivedArgument.cs",
    ]);
    let hops = fx.hops();
    assert_eq!(
        hops,
        vec![
            hop("Publishers.cs", "EagerBridge"),
            hop("Publishers.cs", "EditionBridge"),
            hop("Publishers.cs", "QuietBridge"),
        ],
        "admitting the base is base-wide: every carrier of it becomes a handler, whatever the \
         registered type's own declaration happened to be"
    );
    assert!(
        hops.iter().all(|(file, ..)| file == "Publishers.cs"),
        "every hop starts at the message's own publish site: {hops:?}"
    );
    assert_eq!(fx.derived(), Some(true));
}

#[test]
fn method_type_parameter_registration_contributes_nothing() {
    let fx = Fixture::subset(&["RegisterMethodTypeParameter.cs"]);
    assert_eq!(
        fx.registrations("RegisterMethodTypeParameter.cs"),
        vec![("TShelf".to_string(), 7)],
        "the enclosing method's own type parameter is recorded as its name, not as a wildcard"
    );
    assert!(
        fx.hops().is_empty(),
        "no type of that name is declared, so the registration teaches no base: {:?}",
        fx.hops()
    );
    assert_eq!(fx.derived(), None);
}

// The nine spellings below each register the core handler, which does carry
// an admitted house base against a sent and received message. A spelling that
// started being recorded would therefore derive the vocabulary and show it,
// so the control beside them is what makes each negative mean something.

#[test]
fn control_registration_derives_the_vocabulary() {
    let fx = Fixture::subset(&["RegisterControl.cs"]);
    assert_eq!(fx.hops(), vec![hop("Publishers.cs", "EditionHandler")]);
    assert_eq!(fx.derived(), Some(true));
}

#[test]
fn unseen_registration_spelling_derives_nothing() {
    for file in [
        "RegisterNonGenericInstall.cs",
        "RegisterPluralRoleCall.cs",
        "RegisterWithName.cs",
        "RegisterEndpointName.cs",
        "RegisterTwoTypeArguments.cs",
        "RegisterFromAssembly.cs",
        "RegisterTypesFromAssembly.cs",
        "RegisterReceiverlessCall.cs",
        "RegisterConditionalAccessCall.cs",
    ] {
        let fx = Fixture::subset(&[file]);
        let hops = fx.hops();
        assert!(hops.is_empty(), "{file}: {hops:?}");
        assert_eq!(fx.derived(), None, "{file}");
    }
}
