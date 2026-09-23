//! Integration tests for the compiler-fact enrichment consumer
//! (`src/semantic/`, wired into `resolve::assembly` and `mapcmd::map_repo`).
//!
//! Every admitted-artifact candidate here is constructed BY HAND, field by
//! field, inside this file -- never derived from a real `dotnet`/Roslyn run
//! or from devscout's own output -- matching the acceptance checks' own
//! evidence rule that a fixture's expected state is authored independently
//! of any producer. The one exception is the reference LINE NUMBER a
//! candidate names for each occurrence: read back from a syntax-only `map`
//! run's own `graph.json`, never hand-guessed, so a fixture edit can never
//! silently point an occurrence at the wrong line.
//!
//! Every test goes through the COMPILED BINARY as a subprocess (`map`,
//! `compiler-facts import`, `audit`), the same isolation rule
//! `tests/semantic_audit.rs` uses (`HOME`/`SCOUT_REGISTRY`/`SCOUT_CONTENT_DB`
//! pointed at a fresh temp dir each), plus a REAL git repository per fixture
//! -- unlike the oracle-audit fixture, this one needs a genuine commit head,
//! since both admission's source-snapshot check and the consumer's own
//! freshness check compare the admitted artifact's `sourceSnapshot.headSha`
//! against it.
//!
//! See `fixtures/csharp-enrichment/README.md` for what the fixture tree
//! itself proves and why.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use devscout_rs::graph;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-semantic-enrichment-{prefix}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    fs::canonicalize(&dir).expect("canonicalize temp dir")
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-enrichment")
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create dest dir");
    for entry in fs::read_dir(src).expect("read source dir") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name();
        let file_type = entry.file_type().expect("entry file type");
        if file_type.is_dir() {
            copy_tree(&entry.path(), &dst.join(&name));
        } else if file_type.is_file() {
            fs::copy(entry.path(), dst.join(&name)).expect("copy fixture file");
        }
    }
}

// The same canonicalization and fold `graph::compiler_facts`'s own context
// summary recompute performs, reimplemented here because that function is
// test-only and crate-private, not reachable from an integration-test
// crate. Both sides use the same pinned `serde_json` (no `preserve_order`
// feature -- object keys serialize in sorted order), so `serde_json::to_string`
// canonicalizes identically on both sides.
fn recompute_context_fingerprint(compilations: &[(&Value, &str)]) -> String {
    let mut lines = Vec::with_capacity(compilations.len());
    for (identity, fingerprint) in compilations {
        let canonical = serde_json::to_string(identity).expect("identity serializes");
        lines.push(format!("{canonical}\u{1f}{fingerprint}"));
    }
    let preimage = lines.join("\n");
    let digest = Sha256::digest(preimage.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

struct Fixture {
    repo: PathBuf,
    home: PathBuf,
}

impl Fixture {
    /// Builds an isolated repo from the fixture tree, as a REAL git
    /// repository (one commit), and runs `devscout init --no-hooks
    /// --no-map`. Does not run `map` -- callers do that once they may also
    /// need to read the resulting graph before admitting an artifact.
    fn build(prefix: &str) -> Fixture {
        let base = temp_dir(prefix);
        let repo = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&repo).expect("create repo dir");
        fs::create_dir_all(&home).expect("create home dir");
        copy_tree(&fixture_root().join("src"), &repo.join("src"));

        let fx = Fixture { repo, home };
        fx.git(&["init", "-q"]);
        fx.git(&["config", "user.email", "test@example.com"]);
        fx.git(&["config", "user.name", "test"]);
        fx.git(&["add", "."]);
        fx.git(&["commit", "-q", "-m", "fixture commit"]);
        let init = fx.run(&["init", "--no-hooks", "--no-map"]);
        assert!(init.status.success(), "init failed: {init:?}");
        fx
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .args(args)
            .current_dir(&self.repo)
            .env("HOME", &self.home)
            .env("SCOUT_REGISTRY", self.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", self.home.join("content.db"))
            .output()
            .expect("devscout must run")
    }

    fn git(&self, args: &[&str]) -> Output {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.repo)
            .output()
            .expect("git must run");
        assert!(out.status.success(), "git {args:?} failed: {out:?}");
        out
    }

    fn head(&self) -> String {
        let out = self.git(&["rev-parse", "HEAD"]);
        String::from_utf8(out.stdout)
            .expect("git output is utf-8")
            .trim()
            .to_string()
    }

    fn map(&self) -> Output {
        let out = self.run(&["map", "src"]);
        assert!(out.status.success(), "map failed: {out:?}");
        out
    }

    fn graph(&self) -> Value {
        let path = graph::graph_json_path(&self.repo);
        let text =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        serde_json::from_str(&text).expect("graph.json is valid JSON")
    }

    /// Every `uses-member` edge at `(from_file, from_line)` naming `member`,
    /// in graph.json order.
    fn member_edges_at<'a>(
        graph: &'a Value,
        from_file: &str,
        from_line: u64,
        member: &str,
    ) -> Vec<&'a Value> {
        graph["edges"]
            .as_array()
            .expect("edges array")
            .iter()
            .filter(|e| {
                e["kind"] == "uses-member"
                    && e["from_file"] == from_file
                    && e["from_line"] == from_line
                    && e["member"] == member
            })
            .collect()
    }

    /// Writes and admits a candidate artifact from OUTSIDE the repo (so the
    /// candidate JSON file itself never dirties the working tree the
    /// consumer's own freshness check reads), and asserts the outcome
    /// matches `expect_success`.
    fn import(&self, candidate: &Value, expect_success: bool) -> Output {
        self.import_bytes(
            &serde_json::to_vec(candidate).expect("candidate serializes"),
            expect_success,
        )
    }

    /// Same as `import`, for a candidate that is not valid JSON at all (a
    /// truncated or otherwise malformed artifact) -- `build_candidate`
    /// always produces well-formed JSON, so a raw-bytes path is the only way
    /// to construct one of these.
    fn import_bytes(&self, bytes: &[u8], expect_success: bool) -> Output {
        let path = self.home.join(format!(
            "candidate-{}.json",
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        fs::write(&path, bytes).expect("write candidate file");
        let path_str = path.to_str().expect("utf-8 path").to_string();
        let out = self.run(&["compiler-facts", "import", &path_str]);
        assert_eq!(
            out.status.success(),
            expect_success,
            "compiler-facts import: {out:?}"
        );
        out
    }
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is utf-8")
}

// Candidate JSON construction.

const CALLER_FILE: &str = "src/Caller.cs";

fn compilation_identity() -> Value {
    json!({
        "projectPath": "Fixture.csproj",
        "projectName": "Fixture",
        "requestedTfm": "net9.0",
        "effectiveTfm": "net9.0",
        "configuration": "Debug",
        "platform": "AnyCPU",
    })
}

const COMPILATION_FINGERPRINT: &str = "1111111111111111111111111111111111111111";

/// Every zero-argument site this file admits shares one overload signature
/// (no fixture case here calls an overloaded zero/one-arg pair through this
/// path) -- the same-line-overload-survival test below builds its own sites
/// through `occurrence_site_with_overload` instead, which is the only place
/// a non-default signature or generic arity matters.
const DEFAULT_TARGET_ASSEMBLY: &str = "Fixture";
const DEFAULT_OVERLOAD_SIGNATURE: &str = "()->void";

fn occurrence_site(
    name_line: u64,
    resolution: &str,
    target_type: Option<&str>,
    target_member: Option<&str>,
    candidates: Vec<Value>,
    identity: &Value,
    fingerprint: &str,
) -> Value {
    occurrence_site_with_overload(
        name_line,
        resolution,
        target_type,
        target_member,
        DEFAULT_OVERLOAD_SIGNATURE,
        0,
        candidates,
        identity,
        fingerprint,
    )
}

/// Same as `occurrence_site`, with an explicit target generic arity and
/// overload signature -- the real producer's own per-occurrence identity
/// shape, matching `fixtures/csharp-compiler-facts/compiler-facts.json`'s
/// committed snapshot field for field (`caller`/`target`: `assembly`,
/// `type`, `member`, `genericArity`, `overloadSignature`; `span`: flat
/// `startLine`/`startChar`/`endLine`/`endChar`, never the nested
/// `{start:{...},end:{...}}` shape an earlier round of this fixture used).
/// This is the exact identity the consumer's own `RawOccurrence` must carry
/// alongside its `(file, startLine, member)` compatibility key.
#[allow(clippy::too_many_arguments)]
fn occurrence_site_with_overload(
    name_line: u64,
    resolution: &str,
    target_type: Option<&str>,
    target_member: Option<&str>,
    overload_signature: &str,
    generic_arity: u64,
    candidates: Vec<Value>,
    identity: &Value,
    fingerprint: &str,
) -> Value {
    json!({
        "file": CALLER_FILE,
        "shape": "call",
        "span": {
            "startLine": name_line, "startChar": 0,
            "endLine": name_line, "endChar": 1,
        },
        "name": {"line": name_line, "char": 0},
        "caller": {
            "assembly": DEFAULT_TARGET_ASSEMBLY,
            "type": "Fixtures.Enrichment.Caller",
            "member": "unspecified",
            "genericArity": 0,
            "overloadSignature": DEFAULT_OVERLOAD_SIGNATURE,
        },
        "resolution": resolution,
        "candidateReason": if resolution == "confirmed" { "" } else { "ambiguous-static-import" },
        "target": target_type.map(|t| json!({
            "assembly": DEFAULT_TARGET_ASSEMBLY,
            "type": t,
            "member": target_member,
            "genericArity": generic_arity,
            "overloadSignature": overload_signature,
        })).unwrap_or(Value::Null),
        "candidates": candidates,
        "compilation": {"identity": identity, "fingerprint": fingerprint},
        "documentContentIdentity": "doc-caller",
        "targetDocumentContentIdentities": Vec::<Value>::new(),
    })
}

/// Assembles a full candidate artifact: one compilation, whatever occurrence
/// sites the caller supplies. `compilation_state` lets the dependency-change
/// and incomplete-coverage tests name a non-`"complete"` compilation without
/// duplicating this whole builder.
#[allow(clippy::too_many_arguments)]
fn build_candidate(
    head_sha: &str,
    dirty: bool,
    dependency_fingerprint: &str,
    compilation_state: &str,
    identity: &Value,
    fingerprint: &str,
    occurrences: Vec<Value>,
) -> Value {
    let context_fp = recompute_context_fingerprint(&[(identity, fingerprint)]);
    json!({
        "format": graph::COMPILER_FACTS_FORMAT,
        "contractVersion": graph::COMPILER_FACTS_CONTRACT_VERSION,
        "artifactSchemaVersion": graph::COMPILER_FACTS_ARTIFACT_SCHEMA_VERSION,
        "producer": {"name": graph::EXPECTED_PRODUCER_NAME, "engineRevision": graph::EXPECTED_ENGINE_REVISION},
        "profile": {"target": "net9.0", "configuration": "Debug", "platform": "AnyCPU"},
        "dependencyFingerprint": dependency_fingerprint,
        "context": {
            "schemaVersion": graph::EXPECTED_CONTEXT_SCHEMA_VERSION,
            "contextFingerprint": context_fp,
            "envelope": {
                "compilations": [{
                    "identity": identity,
                    "state": compilation_state,
                    "reason": if compilation_state == "complete" { "" } else { "binding-error" },
                    "fingerprint": fingerprint,
                }],
            },
        },
        "sourceSnapshot": {"headSha": head_sha, "dirty": dirty, "dirtyDigest": ""},
        "capabilities": {"requested": ["occurrences"], "provided": ["occurrences"]},
        "completion": {"terminal": true},
        "units": {"processed": ["Fixture.csproj"], "missing": []},
        "coverage": {"state": "complete"},
        "diagnostics": [],
        "symbols": [],
        "occurrences": {
            "spanEncoding": "utf16-code-unit-line1-char0-end-exclusive",
            "identityEncoding": "fully-qualified-display-format",
            "sites": occurrences,
        },
    })
}

/// The two ambiguous `Config.Load()` call sites' own line numbers and every
/// `.Render()` call's line, read back from a syntax-only `map` -- never
/// hand-guessed. Also asserts the baseline shape every later test relies on:
/// two Guess edges at each `Config.Load()` site (Alpha and Beta), and NO
/// edge at all for any `.Render()` call -- each one's receiver type exists
/// only through a syntax-invisible route (generic-method inference,
/// inherited-generic-member substitution, a lambda nested inside another
/// lambda, or a generic indexer's own return type).
struct BaselineLines {
    same_context_override: u64,
    negative_collision: u64,
    local_call_result: u64,
    inherited_generic_callback: u64,
    nested_typed_lambda: u64,
    typed_indexer_result: u64,
    /// `QualifiedPropertyAccess`'s own `registry.Current.Render()`. Unlike
    /// every other case above, the receiver of `.Render()` here is itself a
    /// QUALIFIED (dotted) property-access expression (`registry.Current`),
    /// never stored in an intermediate local first. `Current`'s declared
    /// type is the generic parameter `T` on `Registry<T>`, substituted only
    /// through `WidgetRegistry : Registry<BetaWidget>`'s own inheritance --
    /// the same generic-substitution-across-inheritance blind spot
    /// `inherited_generic_callback` exploits for a method, applied here to a
    /// property instead. Because the receiver is reached through a PROPERTY
    /// hop rather than a recorded call or field/local type, the syntax
    /// ladder's own member-name-uniqueness fallback still fires here (it is
    /// silenced for the other cases above by their own recorded call-hop
    /// fact) -- so, with `GammaWidget` (`Widgets/GammaWidget.cs`) declaring
    /// a second, unrelated `Render()`, this site is baseline-AMBIGUOUS
    /// (two guessed edges, `BetaWidget` and `GammaWidget`), proved and
    /// overridden the same way `same_context_override` is, not a total miss
    /// like every other receiver category above.
    qualified_property_access: u64,
    /// `SameLineDistinctFacts`'s own single source line, carrying BOTH
    /// `thing.Render()` and `other.Paint()` -- two distinct members sharing
    /// one physical line, proving the compatibility join key is `(file,
    /// line, member)`, never `(file, line)` alone.
    same_line_distinct_facts: u64,
}

fn baseline_lines(fx: &Fixture) -> BaselineLines {
    let g = fx.graph();

    // Every named call's own source line: read from the fixture source
    // directly (most of them produce no edge to read a line back from) by
    // locating each exact statement -- the committed fixture's own fixed
    // shape. Read once, up front, since the qualified-property-access
    // ambiguity check below needs its own line before the render_line calls
    // further down.
    let caller_src =
        fs::read_to_string(fixture_root().join("src/Caller.cs")).expect("read fixture");
    let render_line = |needle: &str| -> u64 {
        caller_src
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains(needle))
            .map(|(i, _)| (i + 1) as u64)
            .unwrap_or_else(|| panic!("{needle} line present in fixture"))
    };

    let load_edges: Vec<(u64, String, Option<String>)> = g["edges"]
        .as_array()
        .expect("edges array")
        .iter()
        .filter(|e| {
            e["kind"] == "uses-member" && e["from_file"] == CALLER_FILE && e["member"] == "Load"
        })
        .map(|e| {
            (
                e["from_line"].as_u64().expect("from_line"),
                e["to"].as_str().expect("to").to_string(),
                e["tier"].as_str().map(str::to_string),
            )
        })
        .collect();
    let mut lines: Vec<u64> = load_edges.iter().map(|(l, ..)| *l).collect();
    lines.sort_unstable();
    lines.dedup();
    assert_eq!(
        lines.len(),
        2,
        "two distinct Config.Load() call sites, each ambiguous: {load_edges:?}"
    );
    for line in &lines {
        let at_line: Vec<&(u64, String, Option<String>)> =
            load_edges.iter().filter(|(l, ..)| l == line).collect();
        assert_eq!(
            at_line.len(),
            2,
            "each ambiguous Config.Load() call pushes one guess edge per candidate: {at_line:?}"
        );
        for (_, to, tier) in &at_line {
            assert_eq!(
                tier.as_deref(),
                Some("guess"),
                "{to} at line {line}: {at_line:?}"
            );
        }
        let targets: Vec<&str> = at_line.iter().map(|(_, to, _)| to.as_str()).collect();
        assert!(
            targets.contains(&"Fixtures.Enrichment.Alpha.Config"),
            "{targets:?}"
        );
        assert!(
            targets.contains(&"Fixtures.Enrichment.Beta.Config"),
            "{targets:?}"
        );
    }

    // The qualified-property-access site's own line, read early (before the
    // "every other .Render() call is unbound" check below, which must
    // exclude it): unlike every other receiver category, this one's own
    // fallback guess DOES fire (see `qualified_property_access`'s own doc
    // comment on `BaselineLines`), and `GammaWidget` makes it ambiguous
    // rather than a lucky single-candidate guess.
    let qualified_property_access_line = render_line("registry.Current.Render()");

    let render_edges_at_qualified_property_access: Vec<(String, Option<String>)> = g["edges"]
        .as_array()
        .expect("edges array")
        .iter()
        .filter(|e| {
            e["kind"] == "uses-member"
                && e["from_file"] == CALLER_FILE
                && e["member"] == "Render"
                && e["from_line"].as_u64() == Some(qualified_property_access_line)
        })
        .map(|e| {
            (
                e["to"].as_str().expect("to").to_string(),
                e["tier"].as_str().map(str::to_string),
            )
        })
        .collect();
    assert_eq!(
        render_edges_at_qualified_property_access.len(),
        2,
        "registry.Current.Render() is baseline-ambiguous, one guessed edge per Render() declarer: {render_edges_at_qualified_property_access:?}"
    );
    for (to, tier) in &render_edges_at_qualified_property_access {
        assert_eq!(
            tier.as_deref(),
            Some("guess"),
            "{to}: {render_edges_at_qualified_property_access:?}"
        );
    }
    let qpa_targets: Vec<&str> = render_edges_at_qualified_property_access
        .iter()
        .map(|(to, _)| to.as_str())
        .collect();
    assert!(
        qpa_targets.contains(&"Fixtures.Enrichment.Widgets.BetaWidget"),
        "{qpa_targets:?}"
    );
    assert!(
        qpa_targets.contains(&"Fixtures.Enrichment.Widgets.GammaWidget"),
        "{qpa_targets:?}"
    );

    let all_render_edges: Vec<&Value> = g["edges"]
        .as_array()
        .expect("edges array")
        .iter()
        .filter(|e| {
            e["kind"] == "uses-member"
                && e["from_file"] == CALLER_FILE
                && e["member"] == "Render"
                && e["from_line"].as_u64() != Some(qualified_property_access_line)
        })
        .collect();
    assert!(
        all_render_edges.is_empty(),
        "the syntax ladder must not resolve any OTHER .Render() call in the baseline -- each of those receivers' real type exists only through a syntax-invisible route: {all_render_edges:?}"
    );

    let all_paint_edges: Vec<&Value> = g["edges"]
        .as_array()
        .expect("edges array")
        .iter()
        .filter(|e| {
            e["kind"] == "uses-member" && e["from_file"] == CALLER_FILE && e["member"] == "Paint"
        })
        .collect();
    assert!(
        all_paint_edges.is_empty(),
        "the syntax ladder must not resolve .Paint() in the baseline either, the same generic-method-return route as thing.Render(): {all_paint_edges:?}"
    );

    BaselineLines {
        same_context_override: lines[0],
        negative_collision: lines[1],
        local_call_result: render_line("thing.Render()"),
        inherited_generic_callback: render_line("current.Render()"),
        nested_typed_lambda: render_line("widget.Render()"),
        typed_indexer_result: render_line("widgets[0].Render()"),
        qualified_property_access: qualified_property_access_line,
        same_line_distinct_facts: render_line("other.Paint()"),
    }
}

// Same-context authority: a confirmed compiler fact must determine the
// answer at an occurrence even where the syntax ladder already pushed one
// or more conflicting edges, and the displaced target must be preserved as
// a recorded disagreement, never as a graph edge.

#[test]
fn a_confirmed_compiler_fact_overrides_a_conflicting_guessed_edge_and_records_the_disagreement() {
    let fx = Fixture::build("override");
    fx.map();
    let lines = baseline_lines(&fx);
    let head = fx.head();
    let identity = compilation_identity();

    let candidate = build_candidate(
        &head,
        false,
        graph::EXPECTED_DEPENDENCY_FINGERPRINT,
        "complete",
        &identity,
        COMPILATION_FINGERPRINT,
        vec![occurrence_site(
            lines.same_context_override,
            "confirmed",
            Some("Fixtures.Enrichment.Beta.Config"),
            Some("Load"),
            vec![],
            &identity,
            COMPILATION_FINGERPRINT,
        )],
    );
    let imported = fx.import(&candidate, true);
    assert!(
        stdout_of(&imported).contains("coverage: complete"),
        "{imported:?}"
    );

    fx.map();
    let g = fx.graph();
    let edges = Fixture::member_edges_at(&g, CALLER_FILE, lines.same_context_override, "Load");
    assert_eq!(
        edges.len(),
        1,
        "both guessed edges are replaced by exactly one compiler-vouched edge: {edges:?}"
    );
    assert_eq!(edges[0]["to"], "Fixtures.Enrichment.Beta.Config");
    assert_eq!(edges[0]["source"], "semantic");
    assert!(
        edges[0]["tier"].is_null() && edges[0]["heuristic"].is_null(),
        "a semantic edge carries no heuristic tier: {edges:?}"
    );

    // The OTHER ambiguous call site, at a different line, carries no
    // occurrence in this artifact at all and must be unaffected.
    let untouched = Fixture::member_edges_at(&g, CALLER_FILE, lines.negative_collision, "Load");
    assert_eq!(
        untouched.len(),
        2,
        "unrelated site must be unaffected: {untouched:?}"
    );

    let stats = &g["stats"]["semantic"];
    assert_eq!(stats["confirmed"], 1, "{stats}");
    assert_eq!(
        stats["disagreements"], 1,
        "the displaced Alpha.Config guess is recorded as a disagreement: {stats}"
    );
}

// Explicit uncertainty: an ambiguous compiler fact is never treated as a
// confirmation, so the guessed edges it names must survive untouched.

#[test]
fn an_ambiguous_compiler_fact_never_overrides_and_the_guessed_edges_survive() {
    let fx = Fixture::build("ambiguous");
    fx.map();
    let lines = baseline_lines(&fx);
    let head = fx.head();
    let identity = compilation_identity();

    let candidate = build_candidate(
        &head,
        false,
        graph::EXPECTED_DEPENDENCY_FINGERPRINT,
        "complete",
        &identity,
        COMPILATION_FINGERPRINT,
        vec![occurrence_site(
            lines.negative_collision,
            "ambiguous",
            Some("Fixtures.Enrichment.Alpha.Config"),
            Some("Load"),
            vec![
                json!({"type": "Fixtures.Enrichment.Alpha.Config", "member": "Load"}),
                json!({"type": "Fixtures.Enrichment.Beta.Config", "member": "Load"}),
            ],
            &identity,
            COMPILATION_FINGERPRINT,
        )],
    );
    fx.import(&candidate, true);
    fx.map();
    let g = fx.graph();

    let edges = Fixture::member_edges_at(&g, CALLER_FILE, lines.negative_collision, "Load");
    assert_eq!(
        edges.len(),
        2,
        "an ambiguous compiler fact is never a confirmed answer -- both guessed edges stand: {edges:?}"
    );
    for e in &edges {
        assert!(
            e["source"].is_null(),
            "no edge here may carry enriched provenance: {e}"
        );
        assert_eq!(e["tier"], "guess");
    }

    let stats = &g["stats"]["semantic"];
    assert_eq!(stats["confirmed"], 0, "{stats}");
    assert_eq!(stats["disagreements"], 0, "{stats}");
}

// A miss the syntax ladder cannot reach at all still gets a compiler-vouched
// answer, through one of the two enriched populations.

#[test]
fn a_confirmed_compiler_fact_recovers_a_site_the_syntax_ladder_never_bound() {
    let fx = Fixture::build("recovered-miss");
    fx.map();
    let lines = baseline_lines(&fx);
    let head = fx.head();
    let identity = compilation_identity();

    let candidate = build_candidate(
        &head,
        false,
        graph::EXPECTED_DEPENDENCY_FINGERPRINT,
        "complete",
        &identity,
        COMPILATION_FINGERPRINT,
        vec![occurrence_site(
            lines.local_call_result,
            "confirmed",
            Some("Fixtures.Enrichment.Widgets.BetaWidget"),
            Some("Render"),
            vec![],
            &identity,
            COMPILATION_FINGERPRINT,
        )],
    );
    fx.import(&candidate, true);
    fx.map();
    let g = fx.graph();

    let edges = Fixture::member_edges_at(&g, CALLER_FILE, lines.local_call_result, "Render");
    assert_eq!(
        edges.len(),
        1,
        "a site the syntax ladder never bound now carries exactly one compiler-vouched edge: {edges:?}"
    );
    assert_eq!(edges[0]["to"], "Fixtures.Enrichment.Widgets.BetaWidget");
    // Whichever population this lands in (a per-reference override of a
    // reference the extractor tracked but could not resolve, or a
    // compiler-discovered site the extractor never referenced at all), it
    // must be tagged as ONE of the two enriched provenances, never left
    // unmarked as though the syntax ladder itself had produced it.
    let source = edges[0]["source"]
        .as_str()
        .expect("enriched edge carries a source tag");
    assert!(
        source == "semantic" || source == "semantic-discovered",
        "unexpected source tag: {source}"
    );
}

// Three more distinct routes by which a `.Render()` call's receiver type is
// invisible to the syntax ladder, each sharing the "recovers a miss" shape
// the local-call-result test above already proves, so the assertion body is
// factored out here rather than repeated three times.
fn assert_confirmed_fact_recovers_a_render_site(prefix: &str, line: u64) {
    let fx = Fixture::build(prefix);
    fx.map();
    baseline_lines(&fx); // re-asserts every `.Render()` call is still unbound
    let head = fx.head();
    let identity = compilation_identity();

    let candidate = build_candidate(
        &head,
        false,
        graph::EXPECTED_DEPENDENCY_FINGERPRINT,
        "complete",
        &identity,
        COMPILATION_FINGERPRINT,
        vec![occurrence_site(
            line,
            "confirmed",
            Some("Fixtures.Enrichment.Widgets.BetaWidget"),
            Some("Render"),
            vec![],
            &identity,
            COMPILATION_FINGERPRINT,
        )],
    );
    fx.import(&candidate, true);
    fx.map();
    let g = fx.graph();

    let edges = Fixture::member_edges_at(&g, CALLER_FILE, line, "Render");
    assert_eq!(
        edges.len(),
        1,
        "a site the syntax ladder never bound now carries exactly one compiler-vouched edge: {edges:?}"
    );
    assert_eq!(edges[0]["to"], "Fixtures.Enrichment.Widgets.BetaWidget");
    let source = edges[0]["source"]
        .as_str()
        .expect("enriched edge carries a source tag");
    assert!(
        source == "semantic" || source == "semantic-discovered",
        "unexpected source tag: {source}"
    );
}

// `WidgetHandler` inherits `Get()` from `Handler<BetaWidget>`, so
// `handler.Get()`'s own return type comes from a generic BASE class's own
// type argument, substituted at the derived, non-generic class -- a route
// the syntax ladder's own qualifier resolution does not walk, distinct from
// `LocalCallResult`'s own generic-method-return case (there the type
// argument is spelled out at the call site itself; here it never is).
#[test]
fn a_confirmed_compiler_fact_recovers_an_inherited_generic_callback_site() {
    let fx = Fixture::build("inherited-generic-callback");
    fx.map();
    let lines = baseline_lines(&fx);
    assert_confirmed_fact_recovers_a_render_site(
        "inherited-generic-callback",
        lines.inherited_generic_callback,
    );
}

// `widget`'s real type comes from `factory.Get<BetaWidget>()` (the same
// generic-method-return route `LocalCallResult` already proves), but the
// call this test targets sits inside a SECOND, inner lambda nested inside
// the outer one that declares `widget` -- proving the override reaches a
// reference two lambda scopes deep, not only a reference at a method's own
// top level.
#[test]
fn a_confirmed_compiler_fact_recovers_a_nested_typed_lambda_site() {
    let fx = Fixture::build("nested-typed-lambda");
    fx.map();
    let lines = baseline_lines(&fx);
    assert_confirmed_fact_recovers_a_render_site("nested-typed-lambda", lines.nested_typed_lambda);
}

// `widgets[0]`'s own type is `Container<BetaWidget>`'s indexer return type,
// resolved only through generic instantiation -- a route distinct from both
// a generic method's return and an inherited generic member.
#[test]
fn a_confirmed_compiler_fact_recovers_a_typed_indexer_result_site() {
    let fx = Fixture::build("typed-indexer-result");
    fx.map();
    let lines = baseline_lines(&fx);
    assert_confirmed_fact_recovers_a_render_site(
        "typed-indexer-result",
        lines.typed_indexer_result,
    );
}

// `registry.Current`'s own type comes from `Registry<T>`'s generic parameter
// `T`, substituted only through `WidgetRegistry : Registry<BetaWidget>`'s
// inheritance -- the same blind spot `InheritedGenericCallback` proves for a
// method, here applied to a PROPERTY -- and, distinct from every case above,
// the receiver of `.Render()` is itself a QUALIFIED (dotted) property-access
// expression (`registry.Current`), never a bare local. This is the named
// "qualified property access" receiver category the Design's own list of
// invented cases requires; `Config.Load()` (`SameContextOverride` /
// `NegativeCollision`) is a type-qualified STATIC METHOD call, already
// resolvable (if ambiguously) by the syntax ladder via `using`, and is not
// this category -- it is the fixture's own same-context-override/negative-
// collision case.
//
// Unlike the four receiver categories above, this site is baseline-
// AMBIGUOUS, not unbound (see `qualified_property_access`'s own doc comment
// on `BaselineLines` for why the syntax ladder's fallback guess fires here
// specifically): `Render` has two declarers once `GammaWidget` exists, so
// the syntax-only build already pushes two guessed edges here, one of them
// WRONG. The shape this test proves is therefore the same-context-override
// shape (a confirmed fact displacing conflicting guesses, disagreement
// recorded), applied to this receiver category rather than to
// `Config.Load()`.
#[test]
fn a_confirmed_compiler_fact_overrides_the_qualified_property_access_sites_guesses() {
    let fx = Fixture::build("qualified-property-access");
    fx.map();
    let lines = baseline_lines(&fx);
    let head = fx.head();
    let identity = compilation_identity();

    let candidate = build_candidate(
        &head,
        false,
        graph::EXPECTED_DEPENDENCY_FINGERPRINT,
        "complete",
        &identity,
        COMPILATION_FINGERPRINT,
        vec![occurrence_site(
            lines.qualified_property_access,
            "confirmed",
            Some("Fixtures.Enrichment.Widgets.BetaWidget"),
            Some("Render"),
            vec![],
            &identity,
            COMPILATION_FINGERPRINT,
        )],
    );
    fx.import(&candidate, true);
    fx.map();
    let g = fx.graph();

    let edges =
        Fixture::member_edges_at(&g, CALLER_FILE, lines.qualified_property_access, "Render");
    assert_eq!(
        edges.len(),
        1,
        "both guessed edges (BetaWidget and the wrong GammaWidget) are replaced by exactly one compiler-vouched edge: {edges:?}"
    );
    assert_eq!(edges[0]["to"], "Fixtures.Enrichment.Widgets.BetaWidget");
    assert_eq!(edges[0]["source"], "semantic");
    assert!(
        edges[0]["tier"].is_null() && edges[0]["heuristic"].is_null(),
        "a semantic edge carries no heuristic tier: {edges:?}"
    );

    let stats = &g["stats"]["semantic"];
    assert_eq!(stats["confirmed"], 1, "{stats}");
    assert_eq!(
        stats["disagreements"], 1,
        "the displaced GammaWidget guess must be recorded as a disagreement diagnostic, never a graph edge: {stats}"
    );
}

// Offline and deterministic: two runs from the same pinned admitted
// artifact, with no compiler and no network involved, must be byte-identical.

#[test]
fn two_runs_from_the_same_admitted_artifact_are_byte_identical() {
    let fx = Fixture::build("deterministic");
    fx.map();
    let lines = baseline_lines(&fx);
    let head = fx.head();
    let identity = compilation_identity();

    let candidate = build_candidate(
        &head,
        false,
        graph::EXPECTED_DEPENDENCY_FINGERPRINT,
        "complete",
        &identity,
        COMPILATION_FINGERPRINT,
        vec![occurrence_site(
            lines.same_context_override,
            "confirmed",
            Some("Fixtures.Enrichment.Beta.Config"),
            Some("Load"),
            vec![],
            &identity,
            COMPILATION_FINGERPRINT,
        )],
    );
    fx.import(&candidate, true);
    fx.map();
    let graph_path = graph::graph_json_path(&fx.repo);
    let first = fs::read(&graph_path).expect("read graph.json");

    fx.map();
    let second = fs::read(&graph_path).expect("read graph.json again");
    assert_eq!(
        first, second,
        "two runs from the same pinned admitted artifact must be byte-identical"
    );
}

// Honest fallback, one failure mode per test: a candidate that fails
// admission on identity grounds never publishes anything.

#[test]
fn a_candidate_with_the_wrong_dependency_fingerprint_is_refused_and_never_admitted() {
    let fx = Fixture::build("refused");
    fx.map();
    let lines = baseline_lines(&fx);
    let head = fx.head();
    let identity = compilation_identity();

    let candidate = build_candidate(
        &head,
        false,
        "0000000000000000000000000000000000000000000000000000000000000000",
        "complete",
        &identity,
        COMPILATION_FINGERPRINT,
        vec![occurrence_site(
            lines.same_context_override,
            "confirmed",
            Some("Fixtures.Enrichment.Beta.Config"),
            Some("Load"),
            vec![],
            &identity,
            COMPILATION_FINGERPRINT,
        )],
    );
    let refused = fx.import(&candidate, false);
    let out = stdout_of(&refused);
    assert!(
        out.contains("refused: dependency-fingerprint-mismatch"),
        "{out}"
    );

    fx.map();
    let g = fx.graph();
    let edges = Fixture::member_edges_at(&g, CALLER_FILE, lines.same_context_override, "Load");
    assert_eq!(
        edges.len(),
        2,
        "a refused candidate publishes nothing -- the syntax-only graph stands unchanged: {edges:?}"
    );
    assert!(g["stats"]["semantic"].is_null(), "{}", g["stats"]);
}

// Honest fallback: an artifact admitted while the tree was dirty is stale
// from the moment it is captured, never confirming anything.

#[test]
fn an_artifact_admitted_dirty_at_capture_time_is_stale_and_never_confirms() {
    let fx = Fixture::build("dirty-at-admission");
    fx.map();
    let lines = baseline_lines(&fx);
    let head = fx.head();
    let identity = compilation_identity();

    let candidate = build_candidate(
        &head,
        true, // dirty at admission
        graph::EXPECTED_DEPENDENCY_FINGERPRINT,
        "complete",
        &identity,
        COMPILATION_FINGERPRINT,
        vec![occurrence_site(
            lines.same_context_override,
            "confirmed",
            Some("Fixtures.Enrichment.Beta.Config"),
            Some("Load"),
            vec![],
            &identity,
            COMPILATION_FINGERPRINT,
        )],
    );
    // Admission itself does not check `dirty` -- only the consumer's own
    // freshness leg does -- so this import still succeeds.
    fx.import(&candidate, true);
    fx.map();
    let g = fx.graph();
    let edges = Fixture::member_edges_at(&g, CALLER_FILE, lines.same_context_override, "Load");
    assert_eq!(
        edges.len(),
        2,
        "an artifact captured against a dirty tree is stale from the start and never confirms: {edges:?}"
    );
    let stats = &g["stats"]["semantic"];
    assert_eq!(stats["confirmed"], 0, "{stats}");
}

// Dependency-change invalidation: the repository head moving on a file the
// admitted occurrence never named must invalidate the whole artifact, even
// though the consuming file is byte-identical across the pair.

#[test]
fn a_repository_head_moving_on_an_unrelated_file_invalidates_the_whole_artifact() {
    let fx = Fixture::build("dependency-change");
    fx.map();
    let lines = baseline_lines(&fx);
    let head_at_capture = fx.head();
    let identity = compilation_identity();

    let candidate = build_candidate(
        &head_at_capture,
        false,
        graph::EXPECTED_DEPENDENCY_FINGERPRINT,
        "complete",
        &identity,
        COMPILATION_FINGERPRINT,
        vec![occurrence_site(
            lines.same_context_override,
            "confirmed",
            Some("Fixtures.Enrichment.Beta.Config"),
            Some("Load"),
            vec![],
            &identity,
            COMPILATION_FINGERPRINT,
        )],
    );
    fx.import(&candidate, true);
    fx.map();
    let before = fx.graph();
    let before_edges =
        Fixture::member_edges_at(&before, CALLER_FILE, lines.same_context_override, "Load");
    assert_eq!(
        before_edges.len(),
        1,
        "the override must apply first: {before_edges:?}"
    );

    // Move the repository head on a file `Caller.cs` never touches, and
    // confirm `Caller.cs`'s own bytes are unchanged across the pair.
    let caller_path = fx.repo.join(CALLER_FILE);
    let caller_before = fs::read(&caller_path).expect("read Caller.cs");
    fs::write(fx.repo.join("src/Unrelated.cs"), "// an unrelated file\n")
        .expect("write unrelated file");
    fx.git(&["add", "."]);
    fx.git(&["commit", "-q", "-m", "unrelated change"]);
    let caller_after = fs::read(&caller_path).expect("read Caller.cs again");
    assert_eq!(
        caller_before, caller_after,
        "the consuming file must be byte-identical across the pair"
    );

    fx.map();
    let after = fx.graph();
    let after_edges =
        Fixture::member_edges_at(&after, CALLER_FILE, lines.same_context_override, "Load");
    assert_eq!(
        after_edges.len(),
        2,
        "the moved head invalidates the whole artifact even though Caller.cs never changed: {after_edges:?}"
    );
    for e in &after_edges {
        assert_eq!(e["tier"], "guess");
    }
}

// Honest fallback: an occurrence inside a diagnostically incomplete
// compilation must never report a clean confirmation, even carrying
// resolution "confirmed" on its own.

#[test]
fn an_occurrence_in_a_partial_compilation_never_reports_a_clean_confirmation() {
    let fx = Fixture::build("partial-compilation");
    fx.map();
    let lines = baseline_lines(&fx);
    let head = fx.head();
    let identity = compilation_identity();

    let candidate = build_candidate(
        &head,
        false,
        graph::EXPECTED_DEPENDENCY_FINGERPRINT,
        "partial",
        &identity,
        COMPILATION_FINGERPRINT,
        vec![occurrence_site(
            lines.same_context_override,
            "confirmed",
            Some("Fixtures.Enrichment.Beta.Config"),
            Some("Load"),
            vec![],
            &identity,
            COMPILATION_FINGERPRINT,
        )],
    );
    fx.import(&candidate, true);
    fx.map();
    let g = fx.graph();
    let edges = Fixture::member_edges_at(&g, CALLER_FILE, lines.same_context_override, "Load");
    assert_eq!(
        edges.len(),
        2,
        "a partial compilation's own occurrence is not a clean confirmation, even with resolution \"confirmed\": {edges:?}"
    );
}

// Honest fallback: a candidate that parses as JSON but never declares itself
// finished -- indistinguishable, by parse success alone, from a run killed
// or truncated mid-write -- is refused outright and never replaces a valid
// artifact.

#[test]
fn a_truncated_candidate_missing_its_completion_record_is_refused_and_never_admitted() {
    let fx = Fixture::build("truncated-completion");
    fx.map();
    let lines = baseline_lines(&fx);
    let head = fx.head();
    let identity = compilation_identity();

    let mut candidate = build_candidate(
        &head,
        false,
        graph::EXPECTED_DEPENDENCY_FINGERPRINT,
        "complete",
        &identity,
        COMPILATION_FINGERPRINT,
        vec![occurrence_site(
            lines.same_context_override,
            "confirmed",
            Some("Fixtures.Enrichment.Beta.Config"),
            Some("Load"),
            vec![],
            &identity,
            COMPILATION_FINGERPRINT,
        )],
    );
    // A run cut off mid-write never gets to append its own terminal record --
    // this is that state, with every other field otherwise well-formed.
    candidate["completion"]["terminal"] = json!(false);
    let refused = fx.import(&candidate, false);
    let out = stdout_of(&refused);
    assert!(out.contains("refused: missing-completion-record"), "{out}");

    fx.map();
    let g = fx.graph();
    let edges = Fixture::member_edges_at(&g, CALLER_FILE, lines.same_context_override, "Load");
    assert_eq!(
        edges.len(),
        2,
        "a candidate missing its own completion record publishes nothing -- the syntax-only graph stands unchanged: {edges:?}"
    );
    assert!(g["stats"]["semantic"].is_null(), "{}", g["stats"]);
}

// Honest fallback: bytes that are not valid JSON at all -- a run killed hard
// enough to leave a half-written file -- are refused on the same footing,
// never partially parsed for whatever prefix happens to be well-formed.

#[test]
fn a_malformed_candidate_that_is_not_valid_json_is_refused_and_never_admitted() {
    let fx = Fixture::build("malformed-json");
    fx.map();
    let lines = baseline_lines(&fx);
    let head = fx.head();
    let identity = compilation_identity();

    let candidate = build_candidate(
        &head,
        false,
        graph::EXPECTED_DEPENDENCY_FINGERPRINT,
        "complete",
        &identity,
        COMPILATION_FINGERPRINT,
        vec![occurrence_site(
            lines.same_context_override,
            "confirmed",
            Some("Fixtures.Enrichment.Beta.Config"),
            Some("Load"),
            vec![],
            &identity,
            COMPILATION_FINGERPRINT,
        )],
    );
    let full_bytes = serde_json::to_vec(&candidate).expect("candidate serializes");
    // Cut the well-formed bytes off partway through -- not valid JSON by
    // construction (a closing brace cannot appear this early), the same
    // shape a process killed mid-write would leave on disk.
    let truncated = &full_bytes[..full_bytes.len() / 2];
    assert!(
        serde_json::from_slice::<Value>(truncated).is_err(),
        "the truncated prefix must not itself be valid JSON, or this test proves nothing"
    );
    let refused = fx.import_bytes(truncated, false);
    let out = stdout_of(&refused);
    assert!(out.contains("refused: malformed-encoding"), "{out}");

    fx.map();
    let g = fx.graph();
    let edges = Fixture::member_edges_at(&g, CALLER_FILE, lines.same_context_override, "Load");
    assert_eq!(
        edges.len(),
        2,
        "a malformed candidate publishes nothing -- the syntax-only graph stands unchanged: {edges:?}"
    );
    assert!(g["stats"]["semantic"].is_null(), "{}", g["stats"]);
}

// Exact identity: the compatibility join key is `(file, startLine, member)`,
// never `(file, startLine)` alone -- two distinct member facts sharing one
// physical source line must resolve independently, never conflated just
// because they share a line.

#[test]
fn two_distinct_members_sharing_one_source_line_resolve_independently() {
    let fx = Fixture::build("same-line-distinct-facts");
    fx.map();
    let lines = baseline_lines(&fx);
    let head = fx.head();
    let identity = compilation_identity();

    let candidate = build_candidate(
        &head,
        false,
        graph::EXPECTED_DEPENDENCY_FINGERPRINT,
        "complete",
        &identity,
        COMPILATION_FINGERPRINT,
        vec![
            occurrence_site(
                lines.same_line_distinct_facts,
                "confirmed",
                Some("Fixtures.Enrichment.Widgets.BetaWidget"),
                Some("Render"),
                vec![],
                &identity,
                COMPILATION_FINGERPRINT,
            ),
            occurrence_site(
                lines.same_line_distinct_facts,
                "confirmed",
                Some("Fixtures.Enrichment.Widgets.BetaWidget"),
                Some("Paint"),
                vec![],
                &identity,
                COMPILATION_FINGERPRINT,
            ),
        ],
    );
    fx.import(&candidate, true);
    fx.map();
    let g = fx.graph();

    let render_edges =
        Fixture::member_edges_at(&g, CALLER_FILE, lines.same_line_distinct_facts, "Render");
    let paint_edges =
        Fixture::member_edges_at(&g, CALLER_FILE, lines.same_line_distinct_facts, "Paint");
    assert_eq!(
        render_edges.len(),
        1,
        "the Render fact at this line must resolve on its own: {render_edges:?}"
    );
    assert_eq!(
        paint_edges.len(),
        1,
        "the Paint fact at the SAME line must resolve independently, not be swallowed by or merged with the Render fact: {paint_edges:?}"
    );
    assert_eq!(render_edges[0]["member"], "Render");
    assert_eq!(paint_edges[0]["member"], "Paint");
    assert_eq!(
        render_edges[0]["to"],
        "Fixtures.Enrichment.Widgets.BetaWidget"
    );
    assert_eq!(
        paint_edges[0]["to"],
        "Fixtures.Enrichment.Widgets.BetaWidget"
    );
}

// Exact identity: a repeated occurrence -- the SAME site admitted more than
// once, naming the SAME target -- must survive deduplication as ONE
// confirmed fact, never misread as a conflict between "two different
// answers" just because more than one occurrence record names it.

#[test]
fn a_repeated_identical_occurrence_survives_as_one_confirmed_fact_not_an_ambiguity() {
    let fx = Fixture::build("repeated-occurrence");
    fx.map();
    let lines = baseline_lines(&fx);
    let head = fx.head();
    let identity = compilation_identity();

    let one_occurrence = || {
        occurrence_site(
            lines.local_call_result,
            "confirmed",
            Some("Fixtures.Enrichment.Widgets.BetaWidget"),
            Some("Render"),
            vec![],
            &identity,
            COMPILATION_FINGERPRINT,
        )
    };
    let candidate = build_candidate(
        &head,
        false,
        graph::EXPECTED_DEPENDENCY_FINGERPRINT,
        "complete",
        &identity,
        COMPILATION_FINGERPRINT,
        vec![one_occurrence(), one_occurrence()],
    );
    fx.import(&candidate, true);
    fx.map();
    let g = fx.graph();

    let edges = Fixture::member_edges_at(&g, CALLER_FILE, lines.local_call_result, "Render");
    assert_eq!(
        edges.len(),
        1,
        "two identical occurrence records naming the same target must still resolve to exactly one edge: {edges:?}"
    );
    assert_eq!(edges[0]["to"], "Fixtures.Enrichment.Widgets.BetaWidget");
    let source = edges[0]["source"]
        .as_str()
        .expect("enriched edge carries a source tag");
    assert!(
        source == "semantic" || source == "semantic-discovered",
        "a repeated occurrence must resolve cleanly, never falling back to Ambiguous/Other: {source}"
    );
}

// Rollback lever: `--no-semantic` must roll back an ALREADY-ENRICHED graph
// on an otherwise-unchanged tree, not merely prevent enrichment on a fresh
// one. Before this fix, `map_repo`'s rebuild trigger only fired on
// `semantic_layer.is_some()`, which `--no-semantic` itself always forces to
// `false` -- so the flag could never trigger the very rebuild it needs to
// take effect once a graph was already enriched, and `rebuild_graph` left
// the stale enriched graph in place (`NotRebuilt`).

#[test]
fn no_semantic_rolls_back_an_already_enriched_graph_on_an_unchanged_tree() {
    let fx = Fixture::build("no-semantic-rollback");

    // Captured before any artifact is ever admitted -- the exact bytes a
    // build with no artifact admitted produces, from the same repo and head
    // the rollback run below reuses.
    let no_artifact = fx.map();
    assert!(no_artifact.status.success());
    let no_artifact_graph =
        fs::read(graph::graph_json_path(&fx.repo)).expect("read no-artifact graph.json");

    let lines = baseline_lines(&fx);
    let head = fx.head();
    let identity = compilation_identity();
    let candidate = build_candidate(
        &head,
        false,
        graph::EXPECTED_DEPENDENCY_FINGERPRINT,
        "complete",
        &identity,
        COMPILATION_FINGERPRINT,
        vec![occurrence_site(
            lines.same_context_override,
            "confirmed",
            Some("Fixtures.Enrichment.Beta.Config"),
            Some("Load"),
            vec![],
            &identity,
            COMPILATION_FINGERPRINT,
        )],
    );
    fx.import(&candidate, true);

    let enriched = fx.map();
    assert!(enriched.status.success(), "map failed: {enriched:?}");
    let enriched_graph = fx.graph();
    assert!(
        enriched_graph
            .get("stats")
            .and_then(|s| s.get("semantic"))
            .is_some(),
        "map with an admitted artifact and no rollback flag must produce the enriched lane"
    );

    // No file touched between the enriched run above and this one -- only
    // the flag changes.
    let rollback = fx.run(&["map", "src", "--no-semantic"]);
    assert!(
        rollback.status.success(),
        "map --no-semantic failed: {rollback:?}"
    );
    let rollback_bytes =
        fs::read(graph::graph_json_path(&fx.repo)).expect("read rollback graph.json");
    let rollback_graph: Value =
        serde_json::from_slice(&rollback_bytes).expect("graph.json is valid JSON");
    assert!(
        rollback_graph
            .get("stats")
            .and_then(|s| s.get("semantic"))
            .is_none(),
        "map --no-semantic on an unchanged tree with an artifact admitted must roll back to \
         the syntax lane, not leave the already-enriched graph in place"
    );
    assert_eq!(
        rollback_bytes, no_artifact_graph,
        "--no-semantic must produce exactly the graph a build with no artifact admitted would"
    );
}

// Exact identity, the same-line-OVERLOAD case: `(file, startLine, member)`
// alone cannot tell `Configure()` and `Configure(true)` apart -- both share
// one physical line, one file and one member name. `Options.cs` gives both
// a real overload each other so the fixture stays valid C#; the admitted
// facts carry each one's own `overloadSignature`, the exact identity
// `SemanticLayer::lookup` reads alongside the bare compatibility key to keep
// them as distinct facts instead of collapsing to one ambiguous outcome.

#[test]
fn two_same_line_overloads_of_one_member_survive_as_distinct_facts() {
    let fx = Fixture::build("same-line-overloads");
    fx.map();
    let head = fx.head();
    let identity = compilation_identity();

    let caller_src =
        fs::read_to_string(fixture_root().join("src/Caller.cs")).expect("read fixture");
    let line = caller_src
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("options.Configure();"))
        .map(|(i, _)| (i + 1) as u64)
        .expect("options.Configure(); line present in fixture");

    let zero_arg = occurrence_site_with_overload(
        line,
        "confirmed",
        Some("Fixtures.Enrichment.Options"),
        Some("Configure"),
        "()->void",
        0,
        vec![],
        &identity,
        COMPILATION_FINGERPRINT,
    );
    let one_arg = occurrence_site_with_overload(
        line,
        "confirmed",
        Some("Fixtures.Enrichment.Options"),
        Some("Configure"),
        "(bool)->void",
        0,
        vec![],
        &identity,
        COMPILATION_FINGERPRINT,
    );
    let candidate = build_candidate(
        &head,
        false,
        graph::EXPECTED_DEPENDENCY_FINGERPRINT,
        "complete",
        &identity,
        COMPILATION_FINGERPRINT,
        vec![zero_arg, one_arg],
    );
    fx.import(&candidate, true);
    fx.map();
    let g = fx.graph();

    let edges = Fixture::member_edges_at(&g, CALLER_FILE, line, "Configure");
    assert_eq!(
        edges.len(),
        2,
        "both overloads at this line must resolve, never collapsing to one ambiguous outcome: {edges:?}"
    );
    for e in &edges {
        assert_eq!(e["to"], "Fixtures.Enrichment.Options");
        assert_eq!(e["source"], "semantic");
    }

    // Both overloads resolve to the SAME type (`to`/`to_file` alone cannot
    // tell them apart), so the projected edges carry each one's own
    // `overload_signature` -- without it, the two edges below would be
    // byte-identical and a reader could never recover which fact is which.
    let mut signatures: Vec<Option<&str>> = edges
        .iter()
        .map(|e| e["overload_signature"].as_str())
        .collect();
    signatures.sort_unstable();
    assert_eq!(
        signatures,
        vec![Some("()->void"), Some("(bool)->void")],
        "each overload's own projected edge must carry its own signature: {edges:?}"
    );
    assert_ne!(
        edges[0], edges[1],
        "the two overloads' projected edges must not be byte-identical: {edges:?}"
    );

    let stats = &g["stats"]["semantic"];
    assert_eq!(
        stats["confirmed"], 2,
        "both same-line overloads confirm independently: {stats}"
    );
    assert_eq!(
        stats["disagreements"], 0,
        "the syntax ladder already bound both calls correctly by receiver type alone (arity plays no part in devscout's own resolution), so neither confirmation displaces anything: {stats}"
    );

    // The query-join leg: a `refs` lookup on the member must return both
    // overloads as two distinct sites, not collapse them the way a
    // content-keyed join (rather than an edge-index one) would.
    let refs_json = fx.run(&["refs", "Configure", "--json"]);
    assert!(
        refs_json.status.success(),
        "refs Configure --json: {refs_json:?}"
    );
    let refs: Value =
        serde_json::from_str(&stdout_of(&refs_json)).expect("refs --json is valid JSON");
    // A bare member seed always answers through the per-declaring-type
    // `"members"` wrapper, one entry per type even when only one exists.
    let rows = refs["members"][0]["inbound"]["uses-member"]["rows"]
        .as_array()
        .unwrap_or_else(|| panic!("inbound uses-member rows: {refs}"));
    assert_eq!(
        rows.len(),
        2,
        "the query join must return both same-line overloads as distinct facts, never collapsed to one: {rows:?}"
    );
}

// The evidence gap the round-2 technical review named: the fixture above
// proves two same-line overloads survive, but never proves the arity
// narrowing in `SemanticLayer::lookup` is load-bearing, because both
// overloads resolve to the SAME `SemanticTarget` (the type-level identity
// `resolve_target` returns) regardless of whether narrowing runs at all --
// with narrowing disabled, `resolved_targets` still collapses the two
// identical targets to one via `dedup`, and every enrichment test passed
// unchanged. This fixture closes that gap: the two same-line, same-member
// occurrences below resolve to GENUINELY DIFFERENT target types (two
// pre-existing fixture types that happen to share no relationship other than
// both declaring a zero/one-arg call site here). Without narrowing, EVERY
// reference at this shared `(file, line, member)` key sees BOTH occurrences,
// so `resolved_targets` collects two DIFFERENT targets that `dedup` cannot
// collapse -- the outcome is `Ambiguous`, the override never fires, and the
// syntax ladder's own precise edge (bound directly off the `Options`-typed
// local, which the ladder can already do without any compiler fact) survives
// unchanged, naming `Options`, not either fabricated target. With narrowing,
// each call's own argument count picks out the ONE candidate whose signature
// matches, and each resolves to its own correct, distinct target -- proven
// by re-running this exact test with the narrowing step of `lookup` disabled
// in a scratch edit and observing the failure (recorded in the journal,
// never shipped as a runtime toggle).
#[test]
fn arity_narrowing_resolves_two_same_line_overloads_to_their_own_distinct_targets() {
    let fx = Fixture::build("same-line-overloads-distinct-targets");
    fx.map();
    let head = fx.head();
    let identity = compilation_identity();

    let caller_src =
        fs::read_to_string(fixture_root().join("src/Caller.cs")).expect("read fixture");
    let line = caller_src
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("options.Configure();"))
        .map(|(i, _)| (i + 1) as u64)
        .expect("options.Configure(); line present in fixture");

    let zero_arg = occurrence_site_with_overload(
        line,
        "confirmed",
        Some("Fixtures.Enrichment.Alpha.Config"),
        Some("Configure"),
        "()->void",
        0,
        vec![],
        &identity,
        COMPILATION_FINGERPRINT,
    );
    let one_arg = occurrence_site_with_overload(
        line,
        "confirmed",
        Some("Fixtures.Enrichment.Beta.Config"),
        Some("Configure"),
        "(bool)->void",
        0,
        vec![],
        &identity,
        COMPILATION_FINGERPRINT,
    );
    let candidate = build_candidate(
        &head,
        false,
        graph::EXPECTED_DEPENDENCY_FINGERPRINT,
        "complete",
        &identity,
        COMPILATION_FINGERPRINT,
        vec![zero_arg, one_arg],
    );
    fx.import(&candidate, true);
    fx.map();
    let g = fx.graph();

    let edges = Fixture::member_edges_at(&g, CALLER_FILE, line, "Configure");
    assert_eq!(
        edges.len(),
        2,
        "both differently-targeted overloads must resolve, never collapsing to Ambiguous: {edges:?}"
    );
    for e in &edges {
        assert_eq!(
            e["source"], "semantic",
            "an ambiguous outcome (narrowing not applied) never overrides, leaving the ladder's own precise edge in place instead: {edges:?}"
        );
    }
    let mut targets: Vec<&str> = edges
        .iter()
        .map(|e| e["to"].as_str().expect("to"))
        .collect();
    targets.sort_unstable();
    assert_eq!(
        targets,
        vec!["Fixtures.Enrichment.Alpha.Config", "Fixtures.Enrichment.Beta.Config"],
        "each overload must resolve to ITS OWN confirmed target, never the other's and never falling back to the ladder's own `Options` guess: {edges:?}"
    );
}
