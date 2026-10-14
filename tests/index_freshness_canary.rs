//! The index-reuse canary: a sealed sequence of working-tree transitions, each
//! compared against a cold index of the same tree.
//!
//! A warm answer labelled `fresh` states that the index describes the working
//! tree. The canary holds that claim to its one meaning: whenever the binary
//! labels an answer `fresh`, the answer must be byte-identical to what a cold
//! index of the same tree returns, and a tree nothing has touched must keep
//! reading `fresh`.

#[path = "freshness_fixture/mod.rs"]
mod fixture;

use std::fs;
use std::path::Path;

use fixture::{state_of, stdout_of, Fixture, BASELINE};

const REVISIONS: &[&str] = &[
    "namespace App.Widgets\n{\n    public class FirstRevision\n    {\n        public void A() { }\n    }\n}\n",
    "namespace App.Widgets\n{\n    public class LaterRevision\n    {\n        public void B() { }\n    }\n}\n",
];

fn copy_tree(src: &Path, dst: &Path, skip: &dyn Fn(&str) -> bool) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let from = entry.path();
        let to = dst.join(&name);
        let rel = from
            .strip_prefix(src)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        if skip(&rel) {
            continue;
        }
        if from.is_dir() {
            copy_tree(&from, &to, &|inner| skip(&format!("{rel}/{inner}")));
        } else {
            fs::copy(&from, &to).unwrap();
        }
    }
}

/// A cold index of the same tree: a copy with no index artifacts, mapped from
/// scratch. Its answer is the standard the warm one must meet.
fn cold_answer(fx: &Fixture, args: &[&str]) -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dst = fx.base.join(format!("cold-{n}"));
    let _ = fs::remove_dir_all(&dst);
    copy_tree(&fx.root, &dst, &|rel| {
        rel == ".git/scout" || rel == ".git/worktrees"
    });
    let cold = Fixture {
        base: dst.clone(),
        root: dst,
        home: fx.home.clone(),
    };
    let map = cold.run(&["map", "."]);
    assert!(map.status.success(), "cold map failed: {map:?}");
    stdout_of(&cold.run(args))
}

/// The answer with its `freshness` object removed: the canary compares what was
/// answered, not how the index was described.
fn without_freshness(answer: &str) -> String {
    let mut value: serde_json::Value = serde_json::from_str(answer.trim()).expect("answer is JSON");
    if let Some(obj) = value.as_object_mut() {
        obj.remove("freshness");
    }
    value.to_string()
}

/// One transition's two checks: a warm answer that calls itself fresh must
/// match the cold one, and after an incremental re-map the answer must be fresh
/// and still match.
fn check(fx: &Fixture, label: &str, args: &[&str]) {
    let warm = fx.run(args);
    if state_of(&warm) == "fresh" {
        assert_eq!(
            without_freshness(&stdout_of(&warm)),
            without_freshness(&cold_answer(fx, args)),
            "{label}: a fresh answer must be the cold answer"
        );
    }

    let remap = fx.run(&["map", "."]);
    assert!(remap.status.success(), "{label}: re-map failed: {remap:?}");
    let after = fx.run(args);
    assert_eq!(
        state_of(&after),
        "fresh",
        "{label}: a re-map leaves it fresh"
    );
    assert_eq!(
        without_freshness(&stdout_of(&after)),
        without_freshness(&cold_answer(fx, args)),
        "{label}: an incrementally mapped index must answer like a cold one"
    );
}

fn canary_fixture(prefix: &str) -> Fixture {
    let mut files = BASELINE.to_vec();
    files.push(("src/Rev.cs", REVISIONS[0]));
    files.push((
        "App.csproj",
        "<Project Sdk=\"Microsoft.NET.Sdk\"></Project>\n",
    ));
    let (fx, _sha) = Fixture::mapped(prefix, &files);
    fx
}

#[test]
fn index_freshness_ac_10_controls_stay_fresh() {
    let fx = canary_fixture("ac10-controls");
    for _ in 0..3 {
        check(&fx, "untouched", &["refs", "IWidget", "--json"]);
    }
}

#[test]
fn index_freshness_ac_10_the_transition_sequence_never_reports_a_false_fresh() {
    let fx = canary_fixture("ac10-sequence");
    let query: &[&str] = &["refs", "IWidget", "--json"];

    // An edit of a file that was clean when it was indexed.
    fx.write("src/WidgetImpl.cs", EDITED_IMPL);
    check(&fx, "edit of a clean file", query);

    // A second edit of a file that was already dirty at map time.
    fx.write("src/Rev.cs", REVISIONS[1]);
    check(&fx, "second edit of a dirty file", query);

    // A revert to the bytes the repository holds.
    fx.write("src/WidgetImpl.cs", BASELINE[1].1);
    check(&fx, "revert", query);

    // A rename.
    fs::rename(fx.root.join("src/Rev.cs"), fx.root.join("src/Moved.cs")).unwrap();
    check(&fx, "rename", query);

    // A delete.
    fx.remove("src/Moved.cs");
    check(&fx, "delete", query);

    // A new file the walk would index.
    fx.write(
        "src/Added.cs",
        "namespace App.Widgets\n{\n    public class Added { }\n}\n",
    );
    check(&fx, "new file", query);

    // A configuration input.
    fx.write(
        "App.csproj",
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup></PropertyGroup></Project>\n",
    );
    check(&fx, "configuration edit", query);

    // A second worktree publishing its own index.
    let linked = fx.add_worktree("linked");
    let map = fx.run_in(&linked, &["map", "."]);
    assert!(map.status.success(), "{map:?}");
    check(&fx, "another worktree published", query);

    // A branch switch.
    fixture::git(&fx.root, &["checkout", "-q", "-b", "feature"]);
    fs::write(
        fx.root.join("src/Added.cs"),
        "namespace App.Widgets\n{\n    public class Added { public void B() { } }\n}\n",
    )
    .unwrap();
    fixture::git(&fx.root, &["add", "-A"]);
    let tree = fixture::git(&fx.root, &["write-tree"]);
    let parent = fixture::git(&fx.root, &["rev-parse", "HEAD"]);
    let sha = fixture::git(
        &fx.root,
        &["commit-tree", &tree, "-p", &parent, "-m", "feature"],
    );
    fixture::git(&fx.root, &["update-ref", "refs/heads/feature", &sha]);
    check(&fx, "branch switch", query);

    // An interrupted map: the marker with no record behind it.
    fx.write_marker(&fx.owner());
    let interrupted = fx.run(query);
    assert_ne!(
        state_of(&interrupted),
        "fresh",
        "an interrupted run must not certify anything"
    );
    let map = fx.run(&["map", "."]);
    assert!(map.status.success(), "{map:?}");
    assert_eq!(state_of(&fx.run(query)), "fresh");
}

const EDITED_IMPL: &str =
    "using App.Widgets;\n\nnamespace App.Widgets.Impl\n{\n    public class WidgetImpl : IWidget\n    {\n        public void Render() { }\n        public void Extra() { }\n    }\n}\n";
