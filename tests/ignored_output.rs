//! What a walk does with the untracked paths Git reports as ignored: in a Git
//! root they are skipped, in a plain root nothing is, and a query that cannot
//! be answered fails the map instead of narrowing it.
//!
//! Every test drives the compiled binary in a throwaway repository, because
//! which files land in the index is a property of one invocation.

#[path = "freshness_fixture/mod.rs"]
mod fixture;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use fixture::{state_of, stdout_of, Fixture};

const SLUGIFY_TS: &str = "export function slugify(s: string) { return s.trim(); }\n";
const A_CS: &str = "namespace App\n{\n    public class A { }\n}\n";

// --- shared helpers -------------------------------------------------------

/// A fixture mapped the way `Fixture::mapped` does, with one extra knob: the
/// `.git/info/exclude` body, which has to be written after `git init` and is
/// never staged.
fn mapped_with(prefix: &str, files: &[(&str, &str)], exclude: &str) -> Fixture {
    let fx = Fixture::empty(prefix);
    for (rel, body) in files {
        fx.write(rel, body);
    }
    if !exclude.is_empty() {
        fx.write(".git/info/exclude", exclude);
    }
    fx.commit_all("init");
    let init = fx.run(&["init", "--no-hooks", "--no-map"]);
    assert!(init.status.success(), "init failed: {init:?}");
    let map = fx.run(&["map", "."]);
    assert!(map.status.success(), "map failed: {map:?}");
    fx
}

// The shared manifest beside the Git directory, or the private one a root with
// no `.git` entry publishes under `.scout`.
fn manifest_path(fx: &Fixture) -> PathBuf {
    let shared = fx.root.join(".git/scout/manifest.json");
    if shared.exists() {
        shared
    } else {
        fx.root.join(".scout/manifest.json")
    }
}

fn manifest_entries(fx: &Fixture) -> Vec<String> {
    let text = fx.read_abs(&manifest_path(fx));
    let value: serde_json::Value = serde_json::from_str(&text).expect("manifest is JSON");
    let mut keys: Vec<String> = value["entries"]
        .as_object()
        .expect("a mapped root has an entries object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

fn record(fx: &Fixture) -> serde_json::Value {
    serde_json::from_str(&fx.read_abs(&fx.record_path())).expect("record is JSON")
}

/// The names of the paths model discovery certified reading.
fn record_config_inputs(fx: &Fixture) -> Vec<String> {
    let mut keys: Vec<String> = record(fx)["config_inputs"]
        .as_object()
        .expect("the record carries config_inputs")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

// The four artifacts a refused map must leave exactly as the last good one did.
const INDEX_ARTIFACTS: &[&str] = &[
    ".git/scout/manifest.json",
    ".git/scout/index-state.json",
    ".git/scout/index-digests.json",
    ".git/scout/graph/graph.json",
];

fn artifacts_under(fx: &Fixture, paths: &[&str]) -> Vec<(String, Option<Vec<u8>>)> {
    paths
        .iter()
        .map(|rel| ((*rel).to_string(), fs::read(fx.root.join(rel)).ok()))
        .collect()
}

fn run_with_path(fx: &Fixture, args: &[&str], path: &str) -> Output {
    Command::new(fixture::rust_bin())
        .args(args)
        .current_dir(&fx.root)
        .env("HOME", &fx.home)
        .env("XDG_CONFIG_HOME", &fx.home)
        .env("SCOUT_REGISTRY", fx.home.join("repos.json"))
        .env("SCOUT_CONTENT_DB", fx.home.join("content.db"))
        .env("PATH", path)
        .output()
        .expect("devscout must run")
}

fn real_git() -> String {
    let out = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("locate git");
    let path = String::from_utf8(out.stdout).unwrap().trim().to_string();
    assert!(path.starts_with('/'), "git must be found: {path:?}");
    path
}

fn path_with(dir: &Path) -> String {
    format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

/// A directory holding a `git` shim that replaces the `ls-files` answer and
/// delegates every other subcommand to the real binary.
fn shim_dir(fx: &Fixture, name: &str, body: &str) -> PathBuf {
    let dir = fx.base.join(name);
    fs::create_dir_all(&dir).unwrap();
    let shim = dir.join("git");
    fs::write(
        &shim,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do\n  if [ \"$a\" = \"ls-files\" ]; then\n    {body}\n  fi\ndone\nexec {} \"$@\"\n",
            real_git()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
    }
    dir
}

fn assert_ignore_query_refused(out: &Output) {
    assert_eq!(out.status.code(), Some(1), "expected a refusal: {out:?}");
    let text = stdout_of(out);
    let line = text.trim_end();
    assert!(
        line.starts_with(
            "error: ignore query failed (git ls-files --others --ignored --exclude-standard --directory):"
        ),
        "one error naming the query: {line:?}"
    );
    assert!(!line.contains('\n'), "exactly one line: {line:?}");
}

// --- built output beside its source ---------------------------------------

// The five built duplicates the review that filed this found: three module
// suffixes and a plain `.js` in an output directory, plus one more directory.
fn built_bundles() -> Vec<(&'static str, &'static str)> {
    vec![
        (".gitignore", "/build/\n/out/\n"),
        ("src/slug.ts", SLUGIFY_TS),
        (
            "build/slug.mjs",
            "export function slugify(s) { return s; }\n",
        ),
        (
            "build/slug.cjs",
            "module.exports.slugify = function (s) { return s; };\n",
        ),
        (
            "build/slug.js",
            "function slugify(s) { return s; }\nmodule.exports = { slugify };\n",
        ),
        (
            "lib/slug.js",
            "exports.slugify = function (s) { return s; };\n",
        ),
        ("out/slug.mjs", "export const slugify = (s) => s;\n"),
    ]
}

#[test]
fn ignored_output_ac_1_built_bundles_beside_source_are_not_indexed() {
    let fx = mapped_with("ac1", &built_bundles(), "/lib/\n");

    assert_eq!(
        manifest_entries(&fx),
        vec!["src/slug.ts"],
        "one .gitignore rule, one .git/info/exclude rule, one directory each"
    );

    let refs = fx.run(&["refs", "slugify", "--json"]);
    assert_eq!(refs.status.code(), Some(0), "{refs:?}");
    let value: serde_json::Value = serde_json::from_str(stdout_of(&refs).trim()).unwrap();
    assert_eq!(value["outcome"], "hit");
    assert_eq!(value["id"], "src/slug.ts#slugify");
}

#[test]
fn ignored_output_ac_1_global_excludes_file_is_honoured() {
    let fx = Fixture::empty("ac1-excludes");
    let excludes = fx.home.join("global-ignore");
    fs::write(&excludes, "/build/\n").unwrap();
    fs::write(
        fx.home.join(".gitconfig"),
        format!("[core]\n\texcludesFile = {}\n", excludes.display()),
    )
    .unwrap();
    fx.write("src/slug.ts", SLUGIFY_TS);
    fx.commit_all("init");
    // Written after the commit so it stays untracked, which is the only state
    // the excludes file can hide it in.
    fx.write(
        "build/slug.mjs",
        "export function slugify(s) { return s; }\n",
    );

    let init = fx.run(&["init", "--no-hooks", "--no-map"]);
    assert!(init.status.success(), "init failed: {init:?}");
    let map = fx.run(&["map", "."]);
    assert!(map.status.success(), "map failed: {map:?}");

    assert_eq!(manifest_entries(&fx), vec!["src/slug.ts"]);
}

// --- what stays indexed ---------------------------------------------------

#[test]
fn ignored_output_ac_2_tracked_file_under_ignored_pattern_is_indexed() {
    let fx = Fixture::empty("ac2-tracked");
    fx.write(".gitignore", "/lib/\n");
    fx.write("lib/keep.ts", "export function keepMe() { return 1; }\n");
    fixture::git(&fx.root, &["add", "-f", "lib/keep.ts"]);
    fx.commit_all("init");
    fx.write(
        "lib/slug.js",
        "exports.slugify = function (s) { return s; };\n",
    );

    let init = fx.run(&["init", "--no-hooks", "--no-map"]);
    assert!(init.status.success(), "init failed: {init:?}");
    let map = fx.run(&["map", "."]);
    assert!(map.status.success(), "map failed: {map:?}");

    assert_eq!(
        manifest_entries(&fx),
        vec!["lib/keep.ts"],
        "a tracked file is never in Git's ignored set; its untracked sibling is"
    );
}

#[test]
fn ignored_output_ac_2_negation_reincludes_a_file() {
    let fx = Fixture::empty("ac2-negation");
    fx.write(".gitignore", "/gen/*\n!/gen/keep.ts\n");
    fx.write("src/a.ts", "export const a = 1;\n");
    fx.commit_all("init");
    fx.write("gen/keep.ts", "export const kept = 1;\n");
    fx.write("gen/drop.ts", "export const dropped = 1;\n");

    let init = fx.run(&["init", "--no-hooks", "--no-map"]);
    assert!(init.status.success(), "init failed: {init:?}");
    let map = fx.run(&["map", "."]);
    assert!(map.status.success(), "map failed: {map:?}");

    assert_eq!(
        manifest_entries(&fx),
        vec!["gen/keep.ts", "src/a.ts"],
        "the re-included file is indexed as Git answers it"
    );
}

#[test]
fn ignored_output_ac_2_unignored_build_lib_out_dirs_stay_indexed() {
    let fx = Fixture::empty("ac2-unignored");
    fx.write(".gitignore", "/unrelated-output/\n");
    fx.write("packages/a/src/lib/x.ts", "export const x = 1;\n");
    fx.write("build/Build.cs", A_CS);
    fx.write("out/Out.cs", A_CS);
    fx.commit_all("init");

    let init = fx.run(&["init", "--no-hooks", "--no-map"]);
    assert!(init.status.success(), "init failed: {init:?}");
    let map = fx.run(&["map", "."]);
    assert!(map.status.success(), "map failed: {map:?}");

    assert_eq!(
        manifest_entries(&fx),
        vec!["build/Build.cs", "out/Out.cs", "packages/a/src/lib/x.ts"],
        "a directory name alone is not a rule: only Git's answer is"
    );
}

#[test]
fn ignored_output_ac_2_walk_order_is_unchanged() {
    let fx = Fixture::empty("ac2-order");
    // Unanchored on purpose, so the rule reaches a `build` directory at any
    // depth and the test can place one inside the scope as well as above it.
    fx.write(".gitignore", "build/\n");
    fx.write("src/a.ts", "export const a = 1;\n");
    fx.write("src/m/n.ts", "export const n = 1;\n");
    fx.write("src/z.ts", "export const z = 1;\n");
    fx.commit_all("init");
    let init = fx.run(&["init", "--no-hooks", "--no-map"]);
    assert!(init.status.success(), "init failed: {init:?}");
    let map = fx.run(&["map", "."]);
    assert!(map.status.success(), "map failed: {map:?}");

    // A skipped name inside the scope and one at its top: neither may reorder
    // or drop the files that remain.
    fx.write(
        "src/build/slug.mjs",
        "export function slugify(s) { return s; }\n",
    );
    fx.write(
        "build/slug.mjs",
        "export function slugify(s) { return s; }\n",
    );
    let map = fx.run(&["map", "."]);
    assert!(map.status.success(), "map failed: {map:?}");

    assert_eq!(
        manifest_entries(&fx),
        vec!["src/a.ts", "src/m/n.ts", "src/z.ts"],
        "the skipped pair changes neither the set nor the order of the rest"
    );
}

// --- a root with no Git ---------------------------------------------------

#[test]
fn ignored_output_ac_3_plain_root_walks_by_name_only() {
    let fx = Fixture::empty("ac3-plain");
    fs::remove_dir_all(fx.root.join(".git")).unwrap();
    fx.write(".gitignore", "build/\n");
    fx.write("src/a.ts", "export const a = 1;\n");
    fx.write(
        "build/slug.mjs",
        "export function slugify(s) { return s; }\n",
    );
    fx.write("node_modules/pkg/x.js", "module.exports = {};\n");

    let init = fx.run(&["init", "--no-hooks", "--no-map"]);
    assert!(init.status.success(), "init failed: {init:?}");

    let no_git = fx.base.join("no-git");
    fs::create_dir_all(&no_git).unwrap();
    let map = run_with_path(&fx, &["map", "."], &no_git.display().to_string());
    assert_eq!(map.status.code(), Some(0), "map failed: {map:?}");

    assert_eq!(
        manifest_entries(&fx),
        vec!["build/slug.mjs", "src/a.ts"],
        "a root with no .git entry applies the fixed name list alone, so the \
         named build/ directory stays and the named node_modules does not"
    );
}

// --- a query that cannot be answered --------------------------------------

fn assert_refused_query_keeps_the_index(fx: &Fixture, path: &str) {
    let before = artifacts_under(fx, INDEX_ARTIFACTS);
    let out = run_with_path(fx, &["map", "."], path);
    assert_ignore_query_refused(&out);
    assert_eq!(
        artifacts_under(fx, INDEX_ARTIFACTS),
        before,
        "the prior publication is untouched by a refused query"
    );
    assert_eq!(
        state_of(&fx.run(&["refs", "slugify", "--json"])),
        "fresh",
        "and a query afterwards answers as it did before"
    );
}

fn mapped_twice(prefix: &str) -> Fixture {
    let fx = Fixture::empty(prefix);
    fx.write(".gitignore", "/build/\n");
    fx.write("src/slug.ts", SLUGIFY_TS);
    fx.commit_all("init");
    let init = fx.run(&["init", "--no-hooks", "--no-map"]);
    assert!(init.status.success(), "init failed: {init:?}");
    let map = fx.run(&["map", "."]);
    assert!(map.status.success(), "map failed: {map:?}");
    fx.write(
        "build/slug.mjs",
        "export function slugify(s) { return s; }\n",
    );
    let map = fx.run(&["map", "."]);
    assert!(map.status.success(), "second map failed: {map:?}");
    fx
}

#[test]
fn ignored_output_ac_4_failed_query_refuses_and_keeps_the_prior_index() {
    let fx = mapped_twice("ac4-causes");

    let no_git = fx.base.join("no-git");
    fs::create_dir_all(&no_git).unwrap();
    assert_refused_query_keeps_the_index(&fx, &no_git.display().to_string());

    let exits = shim_dir(&fx, "shim-128", "exit 128");
    assert_refused_query_keeps_the_index(&fx, &path_with(&exits));

    let empty = shim_dir(&fx, "shim-empty", "printf 'a\\0\\0'; exit 0");
    assert_refused_query_keeps_the_index(&fx, &path_with(&empty));

    let non_utf8 = shim_dir(&fx, "shim-non-utf8", "printf 'out/\\377\\0'; exit 0");
    assert_refused_query_keeps_the_index(&fx, &path_with(&non_utf8));

    let unterminated = shim_dir(&fx, "shim-unterminated", "printf 'build/'; exit 0");
    assert_refused_query_keeps_the_index(&fx, &path_with(&unterminated));
}

#[test]
fn ignored_output_ac_4_unusable_gitdir_file_refuses() {
    let fx = Fixture::empty("ac4-gitfile");
    // A plain root first, so the prior publication lives under `.scout` and
    // stays reachable once `.git` becomes a file Git cannot read.
    fs::remove_dir_all(fx.root.join(".git")).unwrap();
    fx.write("src/slug.ts", SLUGIFY_TS);
    let init = fx.run(&["init", "--no-hooks", "--no-map"]);
    assert!(init.status.success(), "init failed: {init:?}");
    let map = fx.run(&["map", "."]);
    assert!(map.status.success(), "map failed: {map:?}");

    let plain: &[&str] = &[
        ".scout/manifest.json",
        ".scout/index-state.json",
        ".scout/index-digests.json",
        ".scout/graph/graph.json",
    ];
    let before = artifacts_under(&fx, plain);

    fx.write(".git", "not a gitdir pointer\n");
    let out = fx.run(&["map", "."]);
    assert_ignore_query_refused(&out);
    assert_eq!(
        artifacts_under(&fx, plain),
        before,
        "a refused map writes no artifact"
    );
}

// --- model discovery and the census ---------------------------------------

#[test]
fn ignored_output_ac_5_model_discovery_skips_ignored_project_files() {
    let fx = Fixture::empty("ac5-model");
    fx.write(".gitignore", "/out/\n/build/\n");
    fx.write(
        "src/App.csproj",
        "<Project Sdk=\"Microsoft.NET.Sdk\"></Project>\n",
    );
    fx.write("src/A.cs", A_CS);
    fx.write(
        "out/Gen.csproj",
        "<Project Sdk=\"Microsoft.NET.Sdk\"></Project>\n",
    );
    fx.write("build/Cargo.toml", "[package]\nname = \"gen\"\n");
    fx.write("build/x.rs", "pub fn x() {}\n");
    fx.commit_all("init");

    let init = fx.run(&["init", "--no-hooks", "--no-map"]);
    assert!(init.status.success(), "init failed: {init:?}");
    let map = fx.run(&["map", "."]);
    assert!(map.status.success(), "map failed: {map:?}");

    assert_eq!(
        record_config_inputs(&fx),
        vec!["src/App.csproj"],
        "an ignored project file is neither read nor certified"
    );
    let units: serde_json::Value =
        serde_json::from_str(&fx.read_abs(&fx.root.join(".git/scout/graph/project-units.json")))
            .unwrap();
    assert_eq!(units[0]["id"], "src/App.csproj");
    assert_eq!(units.as_array().unwrap().len(), 1);
    assert!(
        !fx.root.join(".git/scout/graph/rust-crates.json").exists(),
        "an ignored Cargo.toml names no package"
    );
}

#[test]
fn ignored_output_ac_5_ancestor_scan_honours_the_ignore_set() {
    let fx = Fixture::empty("ac5-ancestor");
    fx.write(".gitignore", "/Root.csproj\n");
    fx.write(
        "Root.csproj",
        "<Project Sdk=\"Microsoft.NET.Sdk\"></Project>\n",
    );
    fx.write(
        "src/app/App.csproj",
        "<Project Sdk=\"Microsoft.NET.Sdk\"></Project>\n",
    );
    fx.write("src/app/A.cs", A_CS);
    fx.commit_all("init");

    let init = fx.run(&["init", "--no-hooks", "--no-map"]);
    assert!(init.status.success(), "init failed: {init:?}");
    let map = fx.run(&["map", "src/app"]);
    assert!(map.status.success(), "map failed: {map:?}");

    assert_eq!(
        record_config_inputs(&fx),
        vec!["src/app/App.csproj"],
        "the narrow scope's ancestor scan reads no ignored child"
    );
}

#[test]
fn ignored_output_ac_5_init_census_counts_the_walked_set() {
    let fx = Fixture::empty("ac5-census");
    fx.write(".gitignore", "/build/\n");
    fx.write("src/A.cs", A_CS);
    fx.commit_all("init");
    fx.write("build/x.mjs", "export const x = 1;\n");

    let out = fx.run(&["init", "--no-hooks", "--no-map"]);
    assert!(out.status.success(), "init failed: {out:?}");
    let census = stdout_of(&out)
        .lines()
        .find(|l| l.starts_with("languages:"))
        .expect("init prints a census line")
        .to_string();
    assert_eq!(census, "languages: 1 .cs (fully supported)");
}

#[test]
fn ignored_output_ac_5_init_reports_a_failed_query_and_completes() {
    let fx = Fixture::empty("ac5-init-fails");
    fx.write(".gitignore", "/build/\n");
    fx.write("src/A.cs", A_CS);
    fx.commit_all("init");

    let exits = shim_dir(&fx, "shim-init-128", "exit 128");
    let out = run_with_path(&fx, &["init", "--no-hooks", "--no-map"], &path_with(&exits));
    assert_eq!(out.status.code(), Some(0), "init still completes: {out:?}");
    let text = stdout_of(&out);
    assert!(
        text.contains("languages: error walking sources (ignore query failed ("),
        "the census reports the walk error in its own line: {text}"
    );
}

#[test]
fn ignored_output_ac_5_init_map_line_reports_the_refusal() {
    let fx = Fixture::empty("ac5-init-map");
    fx.write(".gitignore", "/build/\n");
    fx.write("src/A.cs", A_CS);
    fx.commit_all("init");

    let exits = shim_dir(&fx, "shim-init-map-128", "exit 128");
    let out = run_with_path(&fx, &["init", "--no-hooks"], &path_with(&exits));
    assert_eq!(out.status.code(), Some(0), "init still completes: {out:?}");
    let text = stdout_of(&out);
    assert!(
        text.contains("map: error: ignore query failed ("),
        "the first map reports the refusal in its own line: {text}"
    );
}

// --- freshness ------------------------------------------------------------

#[test]
fn ignored_output_ac_6_freshness_matches_the_walk() {
    let fx = Fixture::empty("ac6");
    fx.write(".gitignore", "/build/\n/lib/\n");
    fx.write("src/slug.ts", SLUGIFY_TS);
    fx.write("lib/keep.ts", "export function keepMe() { return 1; }\n");
    fixture::git(&fx.root, &["add", "-f", "lib/keep.ts"]);
    fx.commit_all("init");
    let init = fx.run(&["init", "--no-hooks", "--no-map"]);
    assert!(init.status.success(), "init failed: {init:?}");
    let map = fx.run(&["map", "."]);
    assert!(map.status.success(), "map failed: {map:?}");
    assert_eq!(state_of(&fx.run(&["refs", "slugify", "--json"])), "fresh");

    fx.write("build/new.mjs", "export const added = 1;\n");
    fx.write(
        "build/slug.mjs",
        "export function slugify(s) { return s; }\n",
    );
    assert_eq!(
        state_of(&fx.run(&["refs", "slugify", "--json"])),
        "fresh",
        "a new file and an edit under an ignored directory are not indexed paths"
    );

    fx.write("lib/keep.ts", "export function keepMe() { return 2; }\n");
    assert_eq!(
        state_of(&fx.run(&["refs", "slugify", "--json"])),
        "stale",
        "a tracked file under an ignored pattern is walked, so its edit counts"
    );
}
