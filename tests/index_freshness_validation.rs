//! Generation, budget and legacy behaviour: what a query does when the index
//! is being replaced, when the change set is larger than the check reads, and
//! when the record on disk is one an earlier release wrote.

#[path = "freshness_fixture/mod.rs"]
mod fixture;

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use fixture::{state_of, stderr_of, stdout_of, Fixture, BASELINE};

fn spawn_map(fx: &Fixture) -> std::process::Child {
    Command::new(fixture::rust_bin())
        .args(["map", "."])
        .current_dir(&fx.root)
        .env("HOME", &fx.home)
        .env("SCOUT_REGISTRY", fx.home.join("repos.json"))
        .env("SCOUT_CONTENT_DB", fx.home.join("content.db"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("map must start")
}

fn lock_path(fx: &Fixture) -> std::path::PathBuf {
    fx.root.join(".git").join("scout").join("map.lock")
}

fn write_many(root: &Path, dir: &str, count: usize, body: &str) {
    fs::create_dir_all(root.join(dir)).unwrap();
    for i in 0..count {
        fs::write(root.join(dir).join(format!("F{i}.cs")), body).unwrap();
    }
}

#[test]
fn index_freshness_ac_6_two_maps_started_together_leave_one_valid_generation() {
    let (fx, _sha) = Fixture::mapped("ac6-race", BASELINE);
    let baseline = stdout_of(&fx.run(&["refs", "IWidget"]));

    let mut first = spawn_map(&fx);
    let mut second = spawn_map(&fx);
    let a = first.wait().unwrap();
    let b = second.wait().unwrap();
    assert!(a.success(), "the first map failed: {a:?}");
    assert!(b.success(), "the second map failed: {b:?}");

    let out = fx.run(&["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&out), "fresh", "the surviving run owns the index");
    assert_eq!(
        stdout_of(&fx.run(&["refs", "IWidget"])),
        baseline,
        "the answer is the later run's output, not a blend of the two"
    );
}

#[test]
fn index_freshness_ac_6_a_query_racing_a_locked_map_reports_index_changed() {
    let (fx, _sha) = Fixture::mapped("ac6-lock", BASELINE);

    let held = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path(&fx))
        .unwrap();
    held.lock().expect("take the map lock");

    let mut child = spawn_map(&fx);
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        child.try_wait().unwrap().is_none(),
        "a map must wait for the lock rather than run beside it"
    );

    held.unlock().expect("release the map lock");
    let status = child.wait().unwrap();
    assert!(status.success(), "the waiting map completes: {status:?}");
    assert_eq!(state_of(&fx.run(&["refs", "IWidget", "--json"])), "fresh");
}

#[test]
fn index_freshness_ac_6_an_in_progress_marker_reads_as_index_changed() {
    let (fx, _sha) = Fixture::mapped("ac6-marker", BASELINE);
    let owner = fx.owner();

    fx.write_marker(&owner);
    let out = fx.run(&["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&out), "unknown", "{out:?}");
    assert_eq!(
        fixture::freshness_of(&out)["reason"].as_str().unwrap(),
        "index-changed"
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("changed while this query ran"),
        "{stderr:?}"
    );

    // A marker left over an older graph is the same condition -- an
    // interrupted run whose artifacts are half-written.
    assert!(fx.run(&["map", "."]).status.success());
    fx.write_marker(&owner);
    assert_eq!(state_of(&fx.run(&["refs", "IWidget", "--json"])), "unknown");
}

#[test]
fn index_freshness_ac_6_an_artifact_rewritten_in_place_uncertifies_until_a_map() {
    let (fx, _sha) = Fixture::mapped("ac6-rewrite", BASELINE);

    let graph = fx.read_abs(&fx.graph_path());
    fs::write(fx.graph_path(), format!("{graph}\n")).unwrap();

    let out = fx.run(&["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&out), "unknown", "{out:?}");
    assert_eq!(
        fixture::freshness_of(&out)["reason"].as_str().unwrap(),
        "index-changed"
    );

    assert!(fx.run(&["map", "."]).status.success());
    assert_eq!(state_of(&fx.run(&["refs", "IWidget", "--json"])), "fresh");
}

#[test]
fn index_freshness_ac_6_a_legacy_record_certifies_nothing() {
    let (fx, sha) = Fixture::mapped("ac6-legacy", BASELINE);
    fx.write_legacy_record(&sha);

    let out = fx.run(&["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&out), "unknown", "{out:?}");
    assert_eq!(
        fixture::freshness_of(&out)["reason"].as_str().unwrap(),
        "no-index-state"
    );
    let stderr = stderr_of(&out);
    assert!(stderr.contains("rebuild with devscout map"), "{stderr:?}");
}

#[cfg(unix)]
#[test]
fn index_freshness_ac_6_a_map_that_cannot_read_status_publishes_nothing_certifiable() {
    use std::os::unix::fs::PermissionsExt;

    let (fx, _sha) = Fixture::mapped("ac6-status-fails", BASELINE);
    let real = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("locate git");
    let real = String::from_utf8(real.stdout).unwrap().trim().to_string();

    // A git whose status never succeeds, so `map` cannot establish what was
    // already dirty when it read the files.
    let shim_dir = fx.base.join("map-shim");
    fs::create_dir_all(&shim_dir).unwrap();
    let shim = shim_dir.join("git");
    fs::write(
        &shim,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do\n  if [ \"$a\" = \"status\" ]; then\n    exit 1\n  fi\ndone\nexec {real} \"$@\"\n"
        ),
    )
    .unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();

    let map = Command::new(fixture::rust_bin())
        .args(["map", "."])
        .current_dir(&fx.root)
        .env("HOME", &fx.home)
        .env("SCOUT_REGISTRY", fx.home.join("repos.json"))
        .env("SCOUT_CONTENT_DB", fx.home.join("content.db"))
        .env(
            "PATH",
            format!(
                "{}:{}",
                shim_dir.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .expect("map must run");
    assert!(map.status.success(), "the map still completes: {map:?}");

    let out = fx.run(&["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&out), "unknown", "{out:?}");
    assert_eq!(
        fixture::freshness_of(&out)["reason"].as_str().unwrap(),
        "no-index-state",
        "a run that could not establish its baseline certifies nothing"
    );
    assert!(stderr_of(&out).contains("rebuild with devscout map"));
}

#[test]
fn index_freshness_ac_7_more_status_entries_than_the_check_reads_is_a_budget_answer() {
    let (fx, _sha) = Fixture::mapped("ac7-status", BASELINE);
    write_many(
        &fx.root,
        "src/bulk",
        5_001,
        "namespace App.Bulk\n{\n    public class F { }\n}\n",
    );

    let out = fx.run(&["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&out), "unknown", "{out:?}");
    assert_eq!(
        fixture::freshness_of(&out)["reason"].as_str().unwrap(),
        "validation-budget"
    );
    let stderr = stderr_of(&out);
    assert!(stderr.contains("could not be verified"), "{stderr:?}");
}

#[test]
fn index_freshness_ac_7_more_candidate_paths_than_the_check_reads_is_a_budget_answer() {
    let (fx, _sha) = Fixture::mapped("ac7-paths", BASELINE);
    write_many(
        &fx.root,
        "src/bulk",
        2_049,
        "namespace App.Bulk\n{\n    public class F { }\n}\n",
    );

    let out = fx.run(&["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&out), "unknown", "{out:?}");
    assert_eq!(
        fixture::freshness_of(&out)["reason"].as_str().unwrap(),
        "validation-budget"
    );
}

#[test]
fn index_freshness_ac_7_more_candidate_bytes_than_the_check_reads_is_a_budget_answer() {
    let (fx, _sha) = Fixture::mapped("ac7-bytes", BASELINE);
    fs::create_dir_all(fx.root.join("src/big")).unwrap();
    for name in ["src/big/A.cs", "src/big/B.cs"] {
        let mut body = String::from("namespace App.Big\n{\n    public class F\n    {\n");
        while body.len() < 9 * 1024 * 1024 {
            body.push_str("        // padding padding padding padding padding padding\n");
        }
        body.push_str("    }\n}\n");
        fs::write(fx.root.join(name), body).unwrap();
    }

    let out = fx.run(&["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&out), "unknown", "{out:?}");
    assert_eq!(
        fixture::freshness_of(&out)["reason"].as_str().unwrap(),
        "validation-budget"
    );
    let stderr = stderr_of(&out);
    assert!(stderr.contains("could not be verified"), "{stderr:?}");
}

#[test]
fn index_freshness_ac_11_a_legacy_record_asks_for_a_map_and_a_missing_one_stays_silent() {
    let (fx, sha) = Fixture::mapped("ac11-legacy", BASELINE);
    let expected = "devscout: index for repo has no freshness record this version can verify — rebuild with devscout map\n";

    fx.write_legacy_record(&sha);
    let legacy = fx.run(&["refs", "IWidget"]);
    assert_eq!(stderr_of(&legacy), expected);

    // Rewriting it in the legacy shape changes nothing: only a map of this
    // build republishes a record that can certify.
    fx.write_legacy_record(&sha);
    assert_eq!(stderr_of(&fx.run(&["refs", "IWidget"])), expected);

    assert!(fx.run(&["map", "."]).status.success());
    assert_eq!(stderr_of(&fx.run(&["refs", "IWidget"])), "");

    // A record that is simply absent says nothing at all.
    fs::remove_file(fx.record_path()).unwrap();
    assert_eq!(stderr_of(&fx.run(&["refs", "IWidget"])), "");
    assert!(
        stdout_of(&fx.run(&["refs", "IWidget", "--json"]))
            .trim_end()
            .ends_with(r#","freshness":{"state":"unknown","reason":"no-index-state"}}"#),
        "a missing record is still reported on --json"
    );
}

#[test]
fn index_freshness_ac_11_the_stale_line_keeps_the_wording_earlier_releases_used() {
    let (fx, sha) = Fixture::mapped("ac11-line", BASELINE);
    let next = fx.advance_head();
    let short_indexed = &sha[..7];
    let short_current = &next[..7];

    let out = fx.run(&["refs", "IWidget"]);
    assert_eq!(
        stderr_of(&out),
        format!(
            "devscout: index for repo is stale (indexed at {short_indexed}, HEAD {short_current}; 0 changed files) — rebuild with devscout map\n"
        )
    );
    assert_eq!(sha.len(), 40);
}

#[cfg(unix)]
#[test]
fn index_freshness_ac_11_one_status_read_per_query() {
    use std::os::unix::fs::PermissionsExt;

    let (fx, _sha) = Fixture::mapped("ac11-spawn", BASELINE);
    let real = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("locate git");
    let real = String::from_utf8(real.stdout).unwrap().trim().to_string();

    let shim_dir = fx.base.join("count-shim");
    fs::create_dir_all(&shim_dir).unwrap();
    let tally = shim_dir.join("status-count");
    let shim = shim_dir.join("git");
    fs::write(
        &shim,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do\n  if [ \"$a\" = \"status\" ]; then\n    echo s >> {tally}\n  fi\ndone\nexec {real} \"$@\"\n",
            tally = tally.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();

    let out = Command::new(fixture::rust_bin())
        .args(["refs", "IWidget", "--json"])
        .current_dir(&fx.root)
        .env("HOME", &fx.home)
        .env("SCOUT_REGISTRY", fx.home.join("repos.json"))
        .env("SCOUT_CONTENT_DB", fx.home.join("content.db"))
        .env(
            "PATH",
            format!(
                "{}:{}",
                shim_dir.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .expect("devscout must run");

    assert!(out.status.success(), "{out:?}");
    assert_eq!(state_of(&out), "fresh", "{out:?}");
    let count = fs::read_to_string(&tally)
        .unwrap_or_default()
        .lines()
        .count();
    assert_eq!(
        count, 1,
        "one validation per invocation means one status read"
    );
}

#[test]
fn index_freshness_ac_11_a_non_git_root_is_silent_and_unknown() {
    let base = fixture::temp_dir("ac11-nongit");
    let root = base.join("plain");
    let home = base.join("home");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::write(
        root.join("src/IWidget.cs"),
        "namespace App.Widgets\n{\n    public interface IWidget\n    {\n        void Render();\n    }\n}\n",
    )
    .unwrap();

    let run = |args: &[&str]| {
        Command::new(fixture::rust_bin())
            .args(args)
            .current_dir(&root)
            .env("HOME", &home)
            .env("SCOUT_REGISTRY", home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", home.join("content.db"))
            .output()
            .expect("devscout must run")
    };

    let init = run(&["init", "--no-hooks", "--no-map"]);
    assert!(init.status.success(), "{init:?}");
    let map = run(&["map", "."]);
    assert!(map.status.success(), "{map:?}");
    assert!(
        !root.join(".scout").join("index-state.json").exists(),
        "a root with no Git administrative directory publishes no record"
    );

    let out = run(&["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&out), "unknown", "{out:?}");
    assert_eq!(
        fixture::freshness_of(&out)["reason"].as_str().unwrap(),
        "no-index-state"
    );
    assert_eq!(stderr_of(&out), "");
}

#[test]
fn index_freshness_ac_7_a_bounded_check_still_compares_bytes_not_timestamps() {
    let (fx, _sha) = Fixture::mapped("ac7-bytes-compare", BASELINE);
    let path = fx.root.join("src/IWidget.cs");
    let when = fs::metadata(&path).unwrap().modified().unwrap();
    let body = fx.read("src/IWidget.cs");

    // Same size, same modification time, different bytes.
    let edited = body.replacen("IWidget", "JWidget", 1);
    assert_eq!(edited.len(), body.len());
    fs::write(&path, &edited).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(when)
        .unwrap();

    let out = fx.run(&["refs", "IWidget", "--json"]);
    assert_eq!(state_of(&out), "stale", "{out:?}");
}
