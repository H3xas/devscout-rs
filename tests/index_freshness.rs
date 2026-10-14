//! Index freshness, end to end: an index is `fresh` only when the worktree
//! asking published it and the bytes it indexed still match the working tree.

#[path = "freshness_fixture/mod.rs"]
mod fixture;

use std::fs;
use std::path::Path;

use fixture::{state_of, stderr_of, Fixture, BASELINE};

// Two revisions of one file with EXACTLY the same byte length, so a rewrite
// cannot be told apart by size.
const BEFORE: &str =
    "namespace App.Widgets\n{\n    public class BeforeRevision\n    {\n        public void A() { }\n    }\n}\n";
const FIRST: &str =
    "namespace App.Widgets\n{\n    public class FirstRevision\n    {\n        public void A() { }\n    }\n}\n";
const LATER: &str =
    "namespace App.Widgets\n{\n    public class LaterRevision\n    {\n        public void A() { }\n    }\n}\n";

fn restore_mtime(path: &Path, when: std::time::SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .expect("open for mtime restore")
        .set_modified(when)
        .expect("restore mtime");
}

/// A fixture whose revision file was already dirty when `map` indexed it.
fn mapped_with_a_dirty_revision(prefix: &str) -> (Fixture, std::time::SystemTime) {
    let mut files = BASELINE.to_vec();
    files.push(("src/Rev.cs", BEFORE));
    let (fx, _sha) = Fixture::mapped(prefix, &files);
    // Rewrite to the first revision and re-map, so the index describes bytes
    // that already differ from HEAD before the second edit lands.
    fx.write("src/Rev.cs", FIRST);
    let map = fx.run(&["map", "."]);
    assert!(map.status.success(), "map failed: {map:?}");
    let when = fs::metadata(fx.root.join("src/Rev.cs"))
        .unwrap()
        .modified()
        .unwrap();
    (fx, when)
}

#[test]
fn index_freshness_ac_1_a_second_edit_of_an_already_dirty_file_is_stale() {
    let (fx, when) = mapped_with_a_dirty_revision("ac1");
    let path = fx.root.join("src/Rev.cs");
    let before = fs::read(&path).unwrap().len();

    fx.write("src/Rev.cs", LATER);
    assert_eq!(
        fs::read(&path).unwrap().len(),
        before,
        "the second edit must keep the file's size for this to mean anything"
    );
    restore_mtime(&path, when);

    for verb in [
        ["refs", "FirstRevision"],
        ["read", "FirstRevision"],
        ["impact", "src/Rev.cs"],
        ["tests", "FirstRevision"],
    ] {
        let mut args: Vec<&str> = verb.to_vec();
        args.push("--json");
        let out = fx.run(&args);
        assert_eq!(
            state_of(&out),
            "stale",
            "{verb:?} must not certify bytes it never indexed: {out:?}"
        );
        assert!(
            fixture::freshness_of(&out)["changedFiles"]
                .as_u64()
                .unwrap()
                >= 1,
            "{verb:?} must count the rewritten file: {out:?}"
        );
    }

    let refs = fx.run(&["refs", "FirstRevision"]);
    let stderr = stderr_of(&refs);
    assert!(
        stderr.starts_with("devscout: index for repo is stale (indexed at "),
        "{stderr:?}"
    );
    assert!(
        stderr.contains("; 1 changed files) — rebuild with devscout map\n"),
        "{stderr:?}"
    );
}

#[test]
fn index_freshness_ac_1_an_unchanged_dirty_file_stays_fresh() {
    let (fx, _when) = mapped_with_a_dirty_revision("ac1-control");
    let out = fx.run(&["refs", "FirstRevision", "--json"]);
    assert_eq!(state_of(&out), "fresh", "{out:?}");
    assert_eq!(stderr_of(&out), "");
}

#[test]
fn index_freshness_ac_2_another_worktrees_index_is_unknown() {
    let mut files = BASELINE.to_vec();
    files.push(("src/Rev.cs", FIRST));
    let (fx, _sha) = Fixture::mapped("ac2", &files);
    let linked = fx.add_worktree("linked");

    let from_linked = fx.run_in(&linked, &["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&from_linked), "unknown", "{from_linked:?}");
    assert_eq!(
        fixture::freshness_of(&from_linked)["reason"]
            .as_str()
            .unwrap(),
        "other-worktree"
    );
    let stderr = stderr_of(&from_linked);
    assert!(
        stderr.contains("was built in another worktree"),
        "{stderr:?}"
    );

    // The publishing worktree keeps answering for itself.
    let from_main = fx.run(&["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&from_main), "fresh", "{from_main:?}");

    // Once the linked worktree maps, it is the owner and the main worktree
    // reads the index as someone else's.
    let map = fx.run_in(&linked, &["map", "."]);
    assert!(map.status.success(), "map in the linked worktree: {map:?}");
    assert_eq!(
        state_of(&fx.run_in(&linked, &["refs", "IWidget", "--json"])),
        "fresh"
    );
    assert_eq!(state_of(&fx.run(&["refs", "IWidget", "--json"])), "unknown");

    // A worktree nested inside a mapped checkout still resolves to itself, not
    // to the outer `.scout` root.
    let nested = fx.add_worktree("dist/nested");
    let from_nested = fx.run_in(&nested, &["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&from_nested), "unknown", "{from_nested:?}");
    assert_eq!(
        fixture::freshness_of(&from_nested)["reason"]
            .as_str()
            .unwrap(),
        "other-worktree"
    );
}

#[test]
fn index_freshness_ac_3_content_changes_are_stale_and_untouched_dirt_is_fresh() {
    let (fx, _sha) = Fixture::mapped("ac3", BASELINE);

    // A file dirty at map time that has not moved since stays fresh.
    assert_eq!(state_of(&fx.run(&["refs", "IWidget", "--json"])), "fresh");

    fx.write("src/IWidget.cs", "namespace App.Widgets\n{\n    public interface IWidget\n    {\n        void Render();\n        void Resize();\n    }\n}\n");
    let out = fx.run(&["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&out), "stale", "an edited indexed file");
    assert_eq!(
        fixture::freshness_of(&out)["changedFiles"].as_u64(),
        Some(1)
    );

    // Restoring it to the committed bytes is a change too: the index holds the
    // edited bytes, so going back must not read as fresh.
    assert!(fx.run(&["map", "."]).status.success());
    assert_eq!(state_of(&fx.run(&["refs", "IWidget", "--json"])), "fresh");
    fx.write("src/IWidget.cs", "namespace App.Widgets\n{\n    public interface IWidget\n    {\n        void Render();\n    }\n}\n");
    assert_eq!(state_of(&fx.run(&["refs", "IWidget", "--json"])), "stale");
}

#[test]
fn index_freshness_ac_3_a_restore_delete_and_rename_are_all_stale() {
    let mut files = BASELINE.to_vec();
    files.push((
        "src/Extra.cs",
        "namespace App.Widgets\n{\n    public class Extra { }\n}\n",
    ));
    let (fx, _sha) = Fixture::mapped("ac3-shapes", &files);

    // Rename an indexed file.
    fs::rename(fx.root.join("src/Extra.cs"), fx.root.join("src/Moved.cs")).unwrap();
    let renamed = fx.run(&["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&renamed), "stale", "a renamed indexed file");
    assert!(
        fixture::freshness_of(&renamed)["changedFiles"]
            .as_u64()
            .unwrap()
            >= 2,
        "a rename is a deletion and an addition: {renamed:?}"
    );

    // A re-map makes the move part of the index, so deleting it afterwards is
    // the only change left.
    assert!(fx.run(&["map", "."]).status.success());
    fx.remove("src/Moved.cs");
    let deleted = fx.run(&["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&deleted), "stale", "a deleted indexed file");

    // Restoring the deleted file's bytes makes the index current again.
    let restore = fx.run(&["map", "."]);
    assert!(restore.status.success());
    assert_eq!(state_of(&fx.run(&["refs", "IWidget", "--json"])), "fresh");
}

#[test]
fn index_freshness_ac_3_names_with_spaces_quotes_and_non_ascii_are_counted() {
    let mut files = BASELINE.to_vec();
    files.push((
        "src/plain.cs",
        "namespace App.Widgets\n{\n    public class Plain { }\n}\n",
    ));
    let (fx, _sha) = Fixture::mapped("ac3-names", &files);
    let committed = fx.read("src/plain.cs");

    for name in ["src/has space.cs", "src/has\"quote.cs", "src/café.cs"] {
        fx.write(name, &committed);
    }
    let out = fx.run(&["refs", "Plain", "--json"]);
    assert_eq!(state_of(&out), "stale", "{out:?}");
    assert_eq!(
        fixture::freshness_of(&out)["changedFiles"].as_u64(),
        Some(3),
        "{out:?}"
    );
}

#[test]
fn index_freshness_ac_3_unavailable_git_reports_git_unavailable() {
    let (fx, _sha) = Fixture::mapped("ac3-git", BASELINE);
    fs::write(fx.root.join(".git").join("index"), b"not an index").unwrap();

    let out = fx.run(&["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&out), "unknown", "{out:?}");
    assert_eq!(
        fixture::freshness_of(&out)["reason"].as_str().unwrap(),
        "git-unavailable"
    );
    assert_eq!(stderr_of(&out), "", "unavailable git stays silent");
}

#[cfg(unix)]
#[test]
fn index_freshness_ac_3_a_malformed_status_answer_is_git_unavailable() {
    use std::os::unix::fs::PermissionsExt;

    let (fx, _sha) = Fixture::mapped("ac3-shim", BASELINE);
    let real_git = fixture::git(&fx.root, &["--version"]);
    assert!(real_git.starts_with("git version"));
    let real = std::process::Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("locate git");
    let real = String::from_utf8(real.stdout).unwrap().trim().to_string();

    let shim_dir = fx.base.join("shim");
    fs::create_dir_all(&shim_dir).unwrap();
    let shim = shim_dir.join("git");
    fs::write(
        &shim,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do\n  if [ \"$a\" = \"status\" ]; then\n    printf 'nonsense'\n    exit 0\n  fi\ndone\nexec {real} \"$@\"\n"
        ),
    )
    .unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();

    let path = format!(
        "{}:{}",
        shim_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = std::process::Command::new(fixture::rust_bin())
        .args(["refs", "IWidget", "--json"])
        .current_dir(&fx.root)
        .env("HOME", &fx.home)
        .env("SCOUT_REGISTRY", fx.home.join("repos.json"))
        .env("SCOUT_CONTENT_DB", fx.home.join("content.db"))
        .env("PATH", path)
        .output()
        .expect("devscout must run");

    assert_eq!(state_of(&out), "unknown", "{out:?}");
    assert_eq!(
        fixture::freshness_of(&out)["reason"].as_str().unwrap(),
        "git-unavailable"
    );
}

#[test]
fn index_freshness_ac_4_new_relevant_files_and_configuration_inputs() {
    let mut files = BASELINE.to_vec();
    files.push((
        "App.csproj",
        "<Project Sdk=\"Microsoft.NET.Sdk\"></Project>\n",
    ));
    let (fx, _sha) = Fixture::mapped("ac4", &files);
    assert_eq!(state_of(&fx.run(&["refs", "IWidget", "--json"])), "fresh");

    // A new file the walk would index.
    fx.write(
        "src/New.cs",
        "namespace App.Widgets\n{\n    public class New { }\n}\n",
    );
    let added = fx.run(&["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&added), "stale", "a new indexed file");
    assert_eq!(
        fixture::freshness_of(&added)["changedFiles"].as_u64(),
        Some(1)
    );

    // A new file the walk would not index.
    assert!(fx.run(&["map", "."]).status.success());
    fx.write("notes.txt", "not indexed\n");
    fx.write(
        "bin/Generated.cs",
        "namespace App.Widgets\n{\n    public class Generated { }\n}\n",
    );
    assert_eq!(
        state_of(&fx.run(&["refs", "IWidget", "--json"])),
        "fresh",
        "a text file and a file under a skipped directory are not indexed"
    );

    // A configuration input, edited.
    assert!(fx.run(&["map", "."]).status.success());
    fx.write("App.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><TargetFramework>net9.0</TargetFramework></PropertyGroup></Project>\n");
    assert_eq!(
        state_of(&fx.run(&["refs", "IWidget", "--json"])),
        "stale",
        "an edited csproj is a change even though no walked file moved"
    );
}

#[test]
fn index_freshness_ac_4_a_scoped_map_watches_only_its_own_configuration() {
    let mut files = BASELINE.to_vec();
    files.push((
        "Directory.Build.props",
        "<Project><PropertyGroup></PropertyGroup></Project>\n",
    ));
    files.push(("tooling/Cargo.toml", "[package]\nname = \"tooling\"\n"));
    let (fx, _sha) = Fixture::mapped("ac4-scoped", &files);

    let scoped = fx.run(&["map", "src"]);
    assert!(scoped.status.success(), "{scoped:?}");

    // The root props file is an ancestor input of a `src` scope.
    fx.write(
        "Directory.Build.props",
        "<Project><PropertyGroup><X>1</X></PropertyGroup></Project>\n",
    );
    assert_eq!(
        state_of(&fx.run(&["refs", "IWidget", "--json"])),
        "stale",
        "the ancestor props file is an input of a scoped map"
    );

    // A `Cargo.toml` outside the scope is not: crate discovery scans scope
    // only.
    assert!(fx.run(&["map", "src"]).status.success());
    fx.write("tooling/Cargo.toml", "[package]\nname = \"tooling2\"\n");
    assert_eq!(
        state_of(&fx.run(&["refs", "IWidget", "--json"])),
        "fresh",
        "crate discovery never reads an ancestor manifest"
    );
}

#[test]
fn index_freshness_ac_4_a_cargo_manifest_inside_scope_is_a_configuration_input() {
    let mut files = BASELINE.to_vec();
    files.push(("Cargo.toml", "[package]\nname = \"fixture\"\n"));
    let (fx, _sha) = Fixture::mapped("ac4-cargo", &files);
    fx.write(
        "Cargo.toml",
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    );
    assert_eq!(state_of(&fx.run(&["refs", "IWidget", "--json"])), "stale");
}

#[test]
fn index_freshness_ac_5_reuse_is_keyed_on_the_bytes_indexed() {
    let mut files = BASELINE.to_vec();
    files.push(("src/Rev.cs", BEFORE));
    let (fx, _sha) = Fixture::mapped("ac5", &files);
    let path = fx.root.join("src/Rev.cs");
    let when = fs::metadata(&path).unwrap().modified().unwrap();

    fx.write("src/Rev.cs", FIRST);
    restore_mtime(&path, when);

    for mtime_reuse in [None, Some("1")] {
        let mut cmd = std::process::Command::new(fixture::rust_bin());
        cmd.args(["map", "."])
            .current_dir(&fx.root)
            .env("HOME", &fx.home)
            .env("SCOUT_REGISTRY", fx.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", fx.home.join("content.db"));
        if let Some(v) = mtime_reuse {
            cmd.env("SCOUT_MTIME_REUSE", v);
        }
        let out = cmd.output().expect("map must run");
        assert!(out.status.success(), "{out:?}");
        let graph = fx.read_abs(&fx.graph_path());
        assert!(
            graph.contains("FirstRevision"),
            "a same-path, same-mtime, different-bytes change must be re-extracted (SCOUT_MTIME_REUSE={mtime_reuse:?})"
        );
    }

    // The recorded digest is of the bytes that were extracted.
    let digests: serde_json::Value =
        serde_json::from_str(&fx.read_abs(&fx.digests_path())).unwrap();
    let recorded = digests["files"]["src/Rev.cs"].as_str().unwrap();
    let expected = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(fs::read(&path).unwrap());
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    assert_eq!(recorded, expected);
}

#[test]
fn index_freshness_ac_5_a_second_worktree_reuses_nothing_and_carries_nothing() {
    let mut files = BASELINE.to_vec();
    files.push((
        "src/MainOnly.cs",
        "namespace App.Widgets\n{\n    public class MainOnly { }\n}\n",
    ));
    let (fx, _sha) = Fixture::mapped("ac5-worktree", &files);
    let linked = fx.add_worktree("linked");

    // Give the linked worktree a file the main one does not have, so the main
    // manifest has an entry the linked run must not carry forward.
    fx.write(
        "src/MainOnly.cs",
        "namespace App.Widgets\n{\n    public class MainOnly { public void A() { } }\n}\n",
    );
    assert!(fx.run(&["map", "."]).status.success());

    let map = fx.run_in(&linked, &["map", "."]);
    assert!(map.status.success(), "{map:?}");
    assert_eq!(
        state_of(&fx.run_in(&linked, &["refs", "IWidget", "--json"])),
        "fresh",
        "the worktree that just mapped answers for itself"
    );

    // The old generation is gone from the shared graph directory.
    let shared =
        fs::read_dir(fx.root.join(".git").join("scout").join("graph")).expect("shared graph dir");
    for entry in shared.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            !name.contains("v33"),
            "a superseded generation must be deleted: {name}"
        );
    }
}
