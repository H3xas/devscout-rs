//! Every publish verb the seed names, driven through every shape a publish
//! call can take, end to end over a generated repository.
//!
//! The verbs under test are read from the seed rather than typed here, and the
//! fixture is written around them, so a verb the seed gains arrives without
//! anyone remembering to extend a fixture. Two gates read that one seed: the
//! extraction gate decides whether an identifier-argument call is recorded as
//! a publish site at all, and the resolution gate decides which recorded sites
//! emit a hop. They are one literal now, which is what makes a name they both
//! hold reach both layers; what this still catches is a DERIVED set diverging
//! from it, since resolution's own verb set is the seed plus the corpus's
//! forwarding wrappers. A verb that stops being recognized anywhere -- or a
//! shape that stops being recordable for it -- leaves a hole in this walk
//! rather than quietly shrinking the vocabulary.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use devscout_rs::bus_seed::PUBLISH_VERBS;

static NEXT: AtomicU64 = AtomicU64::new(0);

// One publisher file per shape, so a hop's own `from_file` says which shape
// produced it; the message says which verb.
const SHAPES: [&str; 3] = ["Identifier", "Generic", "Constructed"];

struct Fixture {
    root: PathBuf,
    registry: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "devscout-bus-seed-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        for (name, source) in generated_sources() {
            fs::write(root.join(name), source).unwrap();
        }
        let registry = root.join("registry.json");
        let fixture = Self { root, registry };
        fixture.ok(&["init", "--no-hooks"]);
        fixture.ok(&["map", "."]);
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
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    // Every bus hop as `(publishing file, message simple name, handler simple
    // name, evidence)`, in graph order.
    fn hops(&self) -> Vec<(String, String, String, String)> {
        let bytes = fs::read(self.root.join(".scout/graph/graph.json")).unwrap();
        let graph: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "bus-hop")
            .map(|e| {
                (
                    e["from_file"].as_str().unwrap().to_string(),
                    simple(e["message"].as_str().unwrap()),
                    simple(e["to"].as_str().unwrap()),
                    e["evidence"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn simple(id: &str) -> String {
    id.rsplit_once('.').map_or(id, |(_, t)| t).to_string()
}

// The notice a verb publishes and the handler that receives it, both named
// after the verb so one hop identifies its own verb without a lookup table.
fn notice(verb: &str) -> String {
    format!("{verb}Notice")
}

fn worker(verb: &str) -> String {
    format!("{verb}Worker")
}

fn generated_sources() -> Vec<(String, String)> {
    let mut files = vec![
        ("Framework.cs".to_string(), framework()),
        ("Messages.cs".to_string(), messages()),
        ("Handlers.cs".to_string(), handlers()),
    ];
    for shape in SHAPES {
        files.push((format!("{shape}Publisher.cs"), publishers(shape)));
    }
    files
}

// The local stand-ins every other file calls against: the consumer base the
// handlers derive from, and the bus itself with one method per seed verb. The
// methods are generic and their parameter is the method's OWN type parameter,
// so no overload here names a message and the resolution gate's non-dispatch
// refusal -- which fires only for a repository-declared method taking the
// published message itself -- stays out of this walk's way.
fn framework() -> String {
    let methods: Vec<String> = PUBLISH_VERBS
        .iter()
        .map(|verb| format!("        Task {verb}<TItem>(TItem item);"))
        .collect();
    format!(
        "using System.Threading.Tasks;\n\nnamespace SeedShapes\n{{\n    public interface IConsumer<TMessage>\n    {{\n    }}\n\n    public interface IMailbox\n    {{\n{}\n    }}\n}}\n",
        methods.join("\n")
    )
}

fn messages() -> String {
    let types: Vec<String> = PUBLISH_VERBS
        .iter()
        .map(|verb| {
            format!(
                "    public class {}\n    {{\n        public string Tag {{ get; set; }}\n    }}",
                notice(verb)
            )
        })
        .collect();
    format!("namespace SeedShapes\n{{\n{}\n}}\n", types.join("\n\n"))
}

fn handlers() -> String {
    let types: Vec<String> = PUBLISH_VERBS
        .iter()
        .map(|verb| {
            format!(
                "    public class {} : IConsumer<{}>\n    {{\n    }}",
                worker(verb),
                notice(verb)
            )
        })
        .collect();
    format!("namespace SeedShapes\n{{\n{}\n}}\n", types.join("\n\n"))
}

// One publisher class per shape, holding one method per seed verb. The
// identifier shape declares the notice in a local first, which is the only
// shape whose message is not written at the call site, and therefore the only
// one the extraction gate's verb list can refuse.
fn publishers(shape: &str) -> String {
    let methods: Vec<String> = PUBLISH_VERBS
        .iter()
        .map(|verb| match shape {
            "Identifier" => format!(
                "        public Task {verb}Announce(IMailbox mailbox)\n        {{\n            var notice = new {notice}();\n            return mailbox.{verb}(notice);\n        }}",
                notice = notice(verb)
            ),
            "Generic" => format!(
                "        public Task {verb}Announce(IMailbox mailbox) => mailbox.{verb}<{notice}>(default);",
                notice = notice(verb)
            ),
            _ => format!(
                "        public Task {verb}Announce(IMailbox mailbox) => mailbox.{verb}(new {notice} {{ Tag = \"{verb}\" }});",
                notice = notice(verb)
            ),
        })
        .collect();
    format!(
        "using System.Threading.Tasks;\n\nnamespace SeedShapes\n{{\n    public class {shape}Publisher\n    {{\n{}\n    }}\n}}\n",
        methods.join("\n\n")
    )
}

#[test]
fn every_seed_publish_verb_reaches_a_hop_in_every_publish_shape() {
    assert!(
        !PUBLISH_VERBS.is_empty(),
        "a seed with no verbs recognizes no publish site at all"
    );
    let fixture = Fixture::new();
    let hops = fixture.hops();

    for shape in SHAPES {
        let file = format!("{shape}Publisher.cs");
        let mut reached: Vec<(String, String)> = hops
            .iter()
            .filter(|(from, _, _, _)| *from == file)
            .map(|(_, message, handler, _)| (message.clone(), handler.clone()))
            .collect();
        reached.sort();
        let mut expected: Vec<(String, String)> = PUBLISH_VERBS
            .iter()
            .map(|verb| (notice(verb), worker(verb)))
            .collect();
        expected.sort();
        assert_eq!(
            reached, expected,
            "every seed verb's {shape} publish must reach the handler of the notice it sent"
        );
    }

    for (from, message, _, evidence) in &hops {
        assert_eq!(
            evidence, "base-arg",
            "a handler declared on the plain consumer base is read off its own base list: {from} {message}"
        );
    }
    assert_eq!(
        hops.len(),
        PUBLISH_VERBS.len() * SHAPES.len(),
        "one hop per verb per shape, and nothing else: {hops:?}"
    );
}
