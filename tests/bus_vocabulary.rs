//! Integration tests for the message vocabulary a repository declares about
//! itself, driven against `fixtures/bus-vocabulary/`.
//!
//! Nothing in that fixture is named after a bus this engine ships handling
//! for. Its bases, its registry and its verbs are all invented, so a hop
//! found there was found because the repository's own registrations said
//! where to look.

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
        Self::from_files(|_| true)
    }

    // The same fixture with only the files `keep` accepts, so a case can be
    // measured against a corpus that is missing one of them.
    fn from_files(keep: impl Fn(&str) -> bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "devscout-bus-vocab-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/bus-vocabulary");
        for entry in fs::read_dir(&source).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let name = name.to_str().unwrap();
            if name.ends_with(".cs") && keep(name) {
                fs::copy(entry.path(), root.join(name)).unwrap();
            }
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

    fn handlers_of(&self, message: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .hops()
            .into_iter()
            .filter(|(_, _, m, _)| m == message)
            .map(|(_, to, _, _)| to)
            .collect();
        out.sort();
        out.dedup();
        out
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

#[test]
fn registration_harvest_learns_a_consumer_base_the_engine_never_names() {
    let fx = Fixture::new();
    let handlers = fx.handlers_of("ReturnsDeskNotice");
    assert!(
        handlers.contains(&"ReturnsDeskWorker".to_string()),
        "a worker reached only through a base this engine ships no handling for: {handlers:?}"
    );
    assert_eq!(
        fx.graph()["stats"]["bus_vocabulary_derived"],
        serde_json::json!(true),
        "the vocabulary came from the repository's own registrations"
    );
}

#[test]
fn registration_harvest_reads_the_message_position_off_the_registered_type() {
    let fx = Fixture::new();
    let mut messages: Vec<String> = fx
        .hops()
        .into_iter()
        .filter(|(_, to, _, _)| to == "ReturnsDeskWorker")
        .map(|(_, _, message, _)| message)
        .collect();
    messages.dedup();
    assert_eq!(
        messages,
        vec!["ReturnsDeskNotice".to_string()],
        "the message is the base's own type argument, not the registered type's name"
    );
}

#[test]
fn one_argument_registration_does_not_become_a_di_implements_edge() {
    let fx = Fixture::new();
    let graph = fx.graph();
    let kinds = &graph["stats"]["edges_by_kind"];
    for scored in ["implements", "overrides", "dispatch"] {
        assert!(
            kinds.get(scored).is_none() || kinds[scored] == serde_json::json!(0),
            "a one-argument registration names a handler, not a service/implementation pair: {kinds}"
        );
    }
}

#[test]
fn a_consumer_inheriting_a_local_intermediate_base_reaches_its_publisher() {
    let fx = Fixture::new();
    assert_eq!(
        fx.handlers_of("OverdueNotice"),
        vec!["OverdueWorker".to_string()],
        "the intermediate passes its own type parameter through, so the subclass carries the message"
    );
}

#[test]
fn a_publish_wrapper_forwarding_to_a_recognized_verb_is_itself_a_publish_site() {
    let fx = Fixture::new();
    let wrapped: Vec<String> = fx
        .hops()
        .into_iter()
        .filter(|(file, _, _, _)| file == "ForwardingWrapper.cs")
        .map(|(_, to, _, _)| to)
        .collect();
    assert!(
        wrapped.contains(&"ReturnsDeskWorker".to_string()),
        "a call to the wrapper reaches what the wrapper publishes to: {wrapped:?}"
    );
}

#[test]
fn wrapper_forwarding_stops_at_one_hop() {
    let fx = Fixture::new();
    let reached: Vec<String> = fx
        .hops()
        .into_iter()
        .filter(|(_, _, message, _)| message == "OverdueNotice")
        .map(|(file, _, _, _)| file)
        .collect();
    assert!(
        !reached.contains(&"ForwardingWrapper.cs".to_string()),
        "a wrapper around a wrapper is not promoted, so the twice-wrapped publisher reaches nothing: {reached:?}"
    );
}

#[test]
fn a_saga_event_property_carries_its_message_to_the_publisher() {
    let fx = Fixture::new();
    let by_property: Vec<(String, String)> = fx
        .hops()
        .into_iter()
        .filter(|(_, _, _, evidence)| evidence == "property-arg")
        .map(|(_, to, message, _)| (to, message))
        .collect();
    assert_eq!(
        by_property,
        vec![(
            "ShelfAuditHandler".to_string(),
            "ShelfAuditNotice".to_string()
        )],
        "a handler's own property binds a message its base list never names"
    );
}

#[test]
fn a_property_type_argument_that_is_not_a_registered_message_emits_no_hop() {
    let fx = Fixture::new();
    assert!(
        fx.handlers_of("UnregisteredNotice").is_empty(),
        "a generic base nothing registered stays outside the vocabulary, however message-shaped it looks"
    );
}

#[test]
fn assembly_scan_registration_falls_back_to_built_in_roots_and_reports_the_gap() {
    // Every file that carries a registration call removed: what is left is a
    // repository that installs its handlers somewhere this pass cannot see.
    let fx = Fixture::from_files(|name| {
        !matches!(
            name,
            "RegisteredWorker.cs" | "BoundMessages.cs" | "HouseVerbOnly.cs"
        )
    });
    assert!(
        fx.hops().is_empty(),
        "with nothing registered, the house's own bases are not in the vocabulary: {:?}",
        fx.hops()
    );
    assert!(
        fx.graph()["stats"].get("bus_vocabulary_derived").is_none(),
        "no hop, so no vocabulary claim is made either way"
    );
}

#[test]
fn a_shared_message_reports_its_fan_out_degree_and_where_expansion_stopped() {
    let fx = Fixture::new();
    let json = fx.ok(&["refs", "ReturnsDeskWorker", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let rows = v["bus-hop"]["rows"]
        .as_array()
        .expect("a handler's refs carry its bus-hop rows");
    assert!(!rows.is_empty(), "{json}");
    for row in rows {
        assert!(
            row["handlers"].as_u64().unwrap() >= 1,
            "every row states how many handlers its message reaches: {row}"
        );
    }
    let shared = rows
        .iter()
        .find(|r| {
            r["message"]
                .as_str()
                .unwrap()
                .ends_with("ReturnsDeskNotice")
        })
        .expect("the shared message is among them");
    assert_eq!(
        shared["handlers"], 2,
        "a message two handlers share says so on every one of its rows: {shared}"
    );
}

// A repository whose publisher and handler sit in DIFFERENT projects, with
// a ProjectReference between them. Written out rather than copied from the
// fixture directory: the point of the case is the project layout, and the
// fixture above is deliberately flat.
fn two_project_tree() -> Fixture {
    let root = std::env::temp_dir().join(format!(
        "devscout-bus-projects-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join("Contracts")).unwrap();
    fs::create_dir_all(root.join("Workers")).unwrap();
    fs::create_dir_all(root.join("Api")).unwrap();

    fs::write(
        root.join("Contracts/Contracts.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup></Project>\n",
    )
    .unwrap();
    fs::write(
        root.join("Contracts/Shared.cs"),
        "using System.Threading.Tasks;\n\nnamespace Shared\n{\n    public interface IShelfBus\n    {\n        Task Publish<TNotice>(TNotice notice) where TNotice : class;\n    }\n\n    public abstract class ShelfWorkerBase<TNotice>\n    {\n        public abstract Task Work(TNotice notice);\n    }\n\n    public interface IWorkshopRegistry\n    {\n        IWorkshopRegistry AddShelfWorker<TWorker>();\n    }\n\n    public class BranchNotice\n    {\n        public string Branch { get; set; }\n    }\n}\n",
    )
    .unwrap();

    fs::write(
        root.join("Workers/Workers.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup><ItemGroup><ProjectReference Include=\"../Contracts/Contracts.csproj\" /></ItemGroup></Project>\n",
    )
    .unwrap();
    fs::write(
        root.join("Workers/BranchWorker.cs"),
        "using System.Threading.Tasks;\nusing Shared;\n\nnamespace Workers\n{\n    public class BranchWorker : ShelfWorkerBase<BranchNotice>\n    {\n        public override Task Work(BranchNotice notice) => Task.CompletedTask;\n    }\n\n    public static class WorkerInstallation\n    {\n        public static void Install(IWorkshopRegistry registry)\n        {\n            registry.AddShelfWorker<BranchWorker>();\n        }\n    }\n}\n",
    )
    .unwrap();

    fs::write(
        root.join("Api/Api.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup><ItemGroup><ProjectReference Include=\"../Contracts/Contracts.csproj\" /></ItemGroup></Project>\n",
    )
    .unwrap();
    fs::write(
        root.join("Api/BranchPublisher.cs"),
        "using System.Threading.Tasks;\nusing Shared;\n\nnamespace Api\n{\n    public class BranchPublisher\n    {\n        private readonly IShelfBus _bus;\n\n        public BranchPublisher(IShelfBus bus) => _bus = bus;\n\n        public Task Announce() => _bus.Publish(new BranchNotice { Branch = \"North\" });\n    }\n}\n",
    )
    .unwrap();

    let registry = root.join("registry.json");
    let fx = Fixture { root, registry };
    fx.ok(&["init", "--no-hooks"]);
    fx.ok(&["map", "."]);
    fx
}

#[test]
fn bus_hops_cross_project_boundaries_within_one_repository() {
    let fx = two_project_tree();
    let hops = fx.hops();
    assert_eq!(
        hops.len(),
        1,
        "the publisher's project and the handler's project are different, and the hop is still a hop: {hops:?}"
    );
    let (file, to, message, _) = &hops[0];
    assert_eq!(file, "Api/BranchPublisher.cs");
    assert_eq!(to, "BranchWorker");
    assert_eq!(message, "BranchNotice");
}
