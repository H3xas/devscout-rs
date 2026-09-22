//! CLI coverage for the `compiler-facts` verb group: a versioned round trip
//! through both producers (`run` against a stub engine and `import` of a
//! build/CI-produced artifact), the full refusal matrix with its stable
//! tokens, and the offline default path `map`/`status` always keep.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    registry: PathBuf,
}

impl Fixture {
    fn uninitialized() -> Self {
        let root = std::env::temp_dir().join(format!(
            "devscout-compiler-facts-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let registry = std::env::temp_dir().join(format!(
            "devscout-compiler-facts-cli-registry-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        Self { root, registry }
    }

    fn new() -> Self {
        let fixture = Self::uninitialized();
        let init = fixture.run(&["init", "--no-hooks", "--no-map"]);
        assert!(init.status.success(), "init failed: {init:?}");
        fixture
    }

    // The graph directory's own location depends on whether a `.git`
    // ancestor exists at `init` time (see `graph::compiler_facts_json_path`
    // -> `graph_dir`), so a git checkout must exist BEFORE `init` runs, not
    // after -- initializing git first keeps every later command agreeing on
    // one location instead of `init` picking `.scout/graph` and a later
    // command picking `.git/scout/graph`.
    fn new_in_git_checkout() -> (Self, String) {
        let fixture = Self::uninitialized();
        let head = init_git_checkout(&fixture.root);
        let init = fixture.run(&["init", "--no-hooks", "--no-map"]);
        assert!(init.status.success(), "init failed: {init:?}");
        (fixture, head)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .current_dir(&self.root)
            .env("SCOUT_REGISTRY", &self.registry)
            .env_remove("SCOUT_COMPILER_ENGINE")
            .args(args)
            .output()
            .unwrap()
    }

    fn run_with_engine(&self, args: &[&str], engine: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .current_dir(&self.root)
            .env("SCOUT_REGISTRY", &self.registry)
            .env("SCOUT_COMPILER_ENGINE", engine)
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

    fn artifact_path(&self) -> PathBuf {
        self.root.join(".scout/graph/compiler-facts-v1.json")
    }

    // A `.git` ancestor present at `init` time moves the graph directory
    // under the shared git dir instead of `.scout` -- see
    // `graph::compiler_facts_json_path` -> `graph_dir` -> `git_common_dir`.
    fn artifact_path_in_git_checkout(&self) -> PathBuf {
        self.root.join(".git/scout/graph/compiler-facts-v1.json")
    }

    fn import(&self, path: &Path) {
        let out = self.run(&["compiler-facts", "import", path.to_str().unwrap()]);
        assert!(out.status.success(), "import failed: {out:?}");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_file(&self.registry);
    }
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/compiler-facts")
        .join(name)
}

fn base_candidate() -> Value {
    let bytes = fs::read(fixture_path("candidate.json")).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// Every other fixture in this file roots under a plain temp directory, never
// a git checkout, so the source-snapshot identity check -- the one class
// that actually gates a real producer's artifact against this checkout's
// own HEAD -- is never exercised end to end by an automated test. This
// turns a fixture root into a real, single-commit git checkout so that gap
// closes: `git_head` resolves to something real instead of `None`.
fn init_git_checkout(dir: &Path) -> String {
    git(dir, &["init", "-q"]);
    git(dir, &["config", "user.email", "fixture@example.com"]);
    git(dir, &["config", "user.name", "Fixture"]);
    fs::write(dir.join("README.md"), "fixture checkout\n").unwrap();
    git(dir, &["add", "README.md"]);
    git(dir, &["commit", "-q", "-m", "fixture commit"]);
    let out = Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(out.status.success(), "git rev-parse HEAD failed: {out:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn write_stub(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
    path
}

#[test]
fn import_admits_and_two_imports_of_the_same_bytes_are_byte_identical() {
    let fx = Fixture::new();
    let candidate = fixture_path("candidate.json");
    fx.import(&candidate);
    let first = fs::read(fx.artifact_path()).unwrap();
    let original = fs::read(&candidate).unwrap();
    assert_eq!(
        first, original,
        "published bytes are the original candidate, unchanged"
    );

    fx.import(&candidate);
    let second = fs::read(fx.artifact_path()).unwrap();
    assert_eq!(
        first, second,
        "two imports of the same bytes are byte-identical"
    );
}

#[test]
fn run_and_import_reach_the_same_outcome_for_the_same_candidate_bytes() {
    let imported = Fixture::new();
    imported.import(&fixture_path("candidate.json"));
    let imported_bytes = fs::read(imported.artifact_path()).unwrap();

    let ran = Fixture::new();
    let candidate = fixture_path("candidate.json");
    let stub = write_stub(
        &ran.root,
        "stub-engine.sh",
        &format!("#!/bin/sh\nexec cat \"{}\"\n", candidate.display()),
    );
    let out = ran.run_with_engine(
        &["compiler-facts", "run", "--solution", "Fixture.sln"],
        &stub,
    );
    assert!(
        out.status.success(),
        "{out:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ran_bytes = fs::read(ran.artifact_path()).unwrap();

    assert_eq!(
        imported_bytes, ran_bytes,
        "run and import must reach the same admitted bytes for the same candidate"
    );
}

#[test]
fn run_without_capabilities_omits_the_flag_letting_the_engine_pick_its_own_default() {
    // `compiler-facts run`'s own wrapper used to hardcode its own stale
    // default capability list and always pass `--capabilities` to the
    // spawned engine. The stub here fails loudly if it ever sees that flag
    // when the caller passed none, proving the engine's own
    // `SupportedCapabilities` default is now the single source of truth.
    let fx = Fixture::new();
    let script = format!(
        "#!/bin/sh\nfor a in \"$@\"; do\n  if [ \"$a\" = '--capabilities' ]; then\n    echo 'unexpected --capabilities flag on an unflagged run' >&2\n    exit 1\n  fi\ndone\nexec cat \"{}\"\n",
        fixture_path("candidate.json").display()
    );
    let stub = write_stub(&fx.root, "no-capabilities-flag-stub.sh", &script);
    let out = fx.run_with_engine(
        &["compiler-facts", "run", "--solution", "Fixture.sln"],
        &stub,
    );
    assert!(
        out.status.success(),
        "{out:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn run_with_explicit_capabilities_still_forwards_the_flag() {
    // The omission above is conditional on the caller passing nothing: an
    // explicit `--capabilities` must still reach the engine's own argv
    // unchanged.
    let fx = Fixture::new();
    let script = format!(
        "#!/bin/sh\nfound=0\nprev=none\nfor a in \"$@\"; do\n  if [ \"$prev\" = '--capabilities' ] && [ \"$a\" = 'symbols' ]; then\n    found=1\n  fi\n  prev=\"$a\"\ndone\nif [ \"$found\" != 1 ]; then\n  echo 'expected --capabilities symbols on argv' >&2\n  exit 1\nfi\nexec cat \"{}\"\n",
        fixture_path("candidate.json").display()
    );
    let stub = write_stub(&fx.root, "explicit-capabilities-stub.sh", &script);
    let out = fx.run_with_engine(
        &[
            "compiler-facts",
            "run",
            "--solution",
            "Fixture.sln",
            "--capabilities",
            "symbols",
        ],
        &stub,
    );
    assert!(
        out.status.success(),
        "{out:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn every_identity_and_malformed_class_is_refused_with_its_stable_token_and_writes_nothing() {
    let fx = Fixture::new();
    assert!(!fx.artifact_path().exists());

    let mutations: &[(&str, fn(&mut Value))] = &[
        ("contract-version-mismatch", |c| {
            c["contractVersion"] = Value::from(2);
        }),
        ("engine-revision-mismatch", |c| {
            c["producer"]["engineRevision"] = Value::from("bogus");
        }),
        ("profile-mismatch", |c| {
            c["profile"]["target"] = Value::from("net472");
        }),
        ("dependency-fingerprint-mismatch", |c| {
            c["dependencyFingerprint"] = Value::from("bogus");
        }),
        ("context-envelope-version-unrecognised", |c| {
            c["context"]["schemaVersion"] = Value::from(2);
        }),
        ("context-fingerprint-mismatch", |c| {
            c["context"]["envelope"]["compilations"][0]["fingerprint"] = Value::from("different");
        }),
        ("missing-completion-record", |c| {
            c["completion"]["terminal"] = Value::from(false);
        }),
        ("incoherent-inventory", |c| {
            c["units"]["missing"] = Value::from(vec!["Api|net9.0"]);
        }),
    ];

    for (token, mutate) in mutations {
        let mut candidate = base_candidate();
        mutate(&mut candidate);
        let bad = fx.root.join(format!("bad-{token}.json"));
        fs::write(&bad, serde_json::to_vec(&candidate).unwrap()).unwrap();
        let out = fx.run(&["compiler-facts", "import", bad.to_str().unwrap()]);
        assert_eq!(out.status.code(), Some(1), "case {token}: {out:?}");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains(&format!("refused: {token}")),
            "case {token}: {stdout}"
        );
        assert!(
            !fx.artifact_path().exists(),
            "case {token}: a refusal must never create the artifact"
        );
    }

    let bad = fx.root.join("bad-malformed-encoding.json");
    fs::write(&bad, "not json").unwrap();
    let out = fx.run(&["compiler-facts", "import", bad.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("refused: malformed-encoding"));
    assert!(!fx.artifact_path().exists());
}

#[test]
fn run_refuses_a_crashing_engine_as_engine_killed_and_leaves_the_prior_artifact_untouched() {
    let fx = Fixture::new();
    fx.import(&fixture_path("candidate.json"));
    let before = fs::read(fx.artifact_path()).unwrap();

    let stub = write_stub(&fx.root, "crash.sh", "#!/bin/sh\nexit 1\n");
    let out = fx.run_with_engine(
        &["compiler-facts", "run", "--solution", "Fixture.sln"],
        &stub,
    );
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("refused: engine-killed"));
    assert_eq!(fs::read(fx.artifact_path()).unwrap(), before);
}

#[test]
fn run_refuses_a_slow_engine_as_engine_timeout_and_leaves_the_prior_artifact_untouched() {
    let fx = Fixture::new();
    fx.import(&fixture_path("candidate.json"));
    let before = fs::read(fx.artifact_path()).unwrap();

    let stub = write_stub(&fx.root, "slow.sh", "#!/bin/sh\nsleep 5\n");
    let out = fx.run_with_engine(
        &[
            "compiler-facts",
            "run",
            "--solution",
            "Fixture.sln",
            "--timeout-ms",
            "100",
        ],
        &stub,
    );
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("refused: engine-timeout"));
    assert_eq!(fs::read(fx.artifact_path()).unwrap(), before);
}

#[test]
fn run_refuses_an_engine_exceeding_the_output_cap_and_leaves_the_prior_artifact_untouched() {
    let fx = Fixture::new();
    fx.import(&fixture_path("candidate.json"));
    let before = fs::read(fx.artifact_path()).unwrap();

    let stub = write_stub(
        &fx.root,
        "flood.sh",
        "#!/bin/sh\nyes AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n",
    );
    let out = fx.run_with_engine(
        &[
            "compiler-facts",
            "run",
            "--solution",
            "Fixture.sln",
            "--output-cap-bytes",
            "1000",
        ],
        &stub,
    );
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("refused: output-bounded"));
    assert_eq!(fs::read(fx.artifact_path()).unwrap(), before);
}

#[test]
fn run_refuses_no_located_engine_and_leaves_the_prior_artifact_untouched() {
    let fx = Fixture::new();
    fx.import(&fixture_path("candidate.json"));
    let before = fs::read(fx.artifact_path()).unwrap();

    let out = fx.run(&["compiler-facts", "run"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("SCOUT_COMPILER_ENGINE"));
    assert_eq!(fs::read(fx.artifact_path()).unwrap(), before);
}

#[test]
fn import_refuses_a_candidate_truncated_at_an_arbitrary_byte_offset_and_leaves_the_prior_artifact_untouched(
) {
    let fx = Fixture::new();
    fx.import(&fixture_path("candidate.json"));
    let before = fs::read(fx.artifact_path()).unwrap();

    let full = fs::read(fixture_path("candidate.json")).unwrap();
    // An arbitrary byte offset, two-thirds through the well-formed
    // candidate, landing mid-field so the cut bytes are neither valid JSON
    // nor terminated -- the exact class the crate's own terminal-completion
    // decision protects against, distinguished by an explicit record rather
    // than by size or by a successful parse.
    let offset = full.len() * 2 / 3;
    let bad = fx.root.join("truncated-candidate.json");
    fs::write(&bad, &full[..offset]).unwrap();

    let out = fx.run(&["compiler-facts", "import", bad.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("refused: "),
        "a truncated candidate must be refused with a stable reason: {stdout}"
    );
    assert_eq!(
        fs::read(fx.artifact_path()).unwrap(),
        before,
        "a truncated candidate must replace nothing"
    );
}

#[test]
fn import_inside_a_git_checkout_admits_a_matching_source_snapshot_and_refuses_a_mismatched_one() {
    let (fx, head) = Fixture::new_in_git_checkout();

    let mut matching = base_candidate();
    matching["sourceSnapshot"]["headSha"] = Value::from(head);
    let matching_path = fx.root.join("matching-source-snapshot.json");
    fs::write(&matching_path, serde_json::to_vec(&matching).unwrap()).unwrap();
    let out = fx.run(&["compiler-facts", "import", matching_path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "a candidate declaring this checkout's own HEAD must be admitted: {out:?}"
    );
    let before = fs::read(fx.artifact_path_in_git_checkout()).unwrap();

    // `candidate.json`'s own sourceSnapshot ("0000...000f") is never this
    // checkout's real HEAD, so this exercises the mismatch path in the same
    // git checkout, proving the check gates in both directions here.
    let mismatched_path = fx.root.join("mismatched-source-snapshot.json");
    fs::write(
        &mismatched_path,
        serde_json::to_vec(&base_candidate()).unwrap(),
    )
    .unwrap();
    let out = fx.run(&[
        "compiler-facts",
        "import",
        mismatched_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("refused: source-snapshot-mismatch"));
    assert_eq!(
        fs::read(fx.artifact_path_in_git_checkout()).unwrap(),
        before,
        "a source-snapshot refusal inside a git checkout must replace nothing"
    );
}

#[test]
fn status_reports_syntax_only_before_any_admission_and_coverage_after() {
    let fx = Fixture::new();
    let before = fx.ok(&["compiler-facts", "status"]);
    assert!(before.contains("syntax-only"), "{before}");

    fx.import(&fixture_path("candidate.json"));
    let after = fx.ok(&["compiler-facts", "status"]);
    assert!(after.contains("coverage: complete"), "{after}");
    assert!(!after.contains("syntax-only"), "{after}");
}

#[test]
fn status_reports_occurrence_coverage_admitted_with_occurrences() {
    let fx = Fixture::new();
    fx.import(&fixture_path("candidate-with-occurrences.json"));
    let status = fx.ok(&["compiler-facts", "status"]);
    assert!(status.contains("coverage: complete"), "{status}");
    assert!(status.contains("occurrences: 2 admitted"), "{status}");
    assert!(status.contains("1 unresolved"), "{status}");
}

#[test]
fn status_reports_occurrence_coverage_admitted_without_occurrences() {
    let fx = Fixture::new();
    fx.import(&fixture_path("candidate.json"));
    let status = fx.ok(&["compiler-facts", "status"]);
    assert!(status.contains("coverage: complete"), "{status}");
    assert!(
        !status.contains("occurrences:"),
        "an ordinary restricted import makes no occurrence claim at all: {status}"
    );
}

#[test]
fn status_reports_occurrence_coverage_requested_but_unavailable() {
    let fx = Fixture::new();
    fx.import(&fixture_path("candidate-occurrences-unavailable.json"));
    let status = fx.ok(&["compiler-facts", "status"]);
    assert!(status.contains("coverage: complete"), "{status}");
    assert!(
        status.contains("occurrences: unavailable (requested but not provided)"),
        "{status}"
    );
}

#[test]
fn map_and_refs_never_spawn_the_compiler_engine_even_when_one_is_configured() {
    let fx = Fixture::new();
    fs::write(fx.root.join("Program.cs"), "public class Program {}\n").unwrap();
    let marker = fx.root.join("engine-was-invoked.marker");
    let stub = write_stub(
        &fx.root,
        "marker-engine.sh",
        &format!(
            "#!/bin/sh\ntouch \"{}\"\ncat \"{}\"\n",
            marker.display(),
            fixture_path("candidate.json").display()
        ),
    );

    let map_out = fx.run_with_engine(&["map"], &stub);
    assert!(map_out.status.success(), "{map_out:?}");
    let refs_out = fx.run_with_engine(&["refs", "Program"], &stub);
    assert!(refs_out.status.success(), "{refs_out:?}");

    assert!(
        !marker.exists(),
        "map/refs must never spawn the compiler engine"
    );
}
