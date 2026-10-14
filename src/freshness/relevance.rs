//! Which paths a change to can invalidate an index.
//!
//! Two shapes qualify, and both are defined by reusing the constants the
//! consumers already obey rather than by a second copy of their rules:
//!
//! - an **indexed path**: inside the record's scope, with no [`SKIP_DIRS`]
//!   component, and an extension from [`SOURCE_EXT`] -- exactly the files
//!   `walk::list_source_files` would hand `map`;
//! - a **configuration input**: a `.csproj`, `Directory.Build.props` or
//!   `Cargo.toml` that `project::discover` or `rsgraph::crates::discover`
//!   reads.
//!
//! Whether a path that passes the shape test is really a regular file (the walk
//! skips symlinks) is a candidate-evaluation question, not this predicate's: the
//! shape alone selects candidates, and the digest comparison then decides.

use crate::walk::{SKIP_DIRS, SOURCE_EXT};

/// Whether `rel` (a `/`-joined repo-relative path) falls under one of `scope`'s
/// directories. `"."` covers everything; otherwise `dir` itself or anything
/// under `dir/`.
#[must_use]
pub fn is_within_scope(rel: &str, scope: &[String]) -> bool {
    scope
        .iter()
        .any(|d| d == "." || rel == d || rel.starts_with(&format!("{d}/")))
}

/// The directory part of a `/`-joined relative path, `""` at the repo root.
fn dir_of(rel: &str) -> &str {
    match rel.rfind('/') {
        Some(i) => &rel[..i],
        None => "",
    }
}

/// The last segment of a `/`-joined relative path.
fn name_of(rel: &str) -> &str {
    match rel.rfind('/') {
        Some(i) => &rel[i + 1..],
        None => rel,
    }
}

/// Whether any directory component of `dir` is a [`SKIP_DIRS`] name -- the
/// walk never descends into one, so nothing under it is indexed or read.
fn has_skipped_component(dir: &str) -> bool {
    dir.split('/').any(|part| SKIP_DIRS.contains(&part))
}

/// The substring from a file name's last `.` onward, matching
/// `walk::extension_of` (and therefore [`SOURCE_EXT`]'s own matching rule).
fn extension_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) => &name[i..],
        None => "",
    }
}

/// Whether `rel` is a source file `map` would index inside `scope`.
#[must_use]
pub fn is_indexed_path(rel: &str, scope: &[String]) -> bool {
    if rel.is_empty() || !is_within_scope(rel, scope) {
        return false;
    }
    if has_skipped_component(dir_of(rel)) {
        return false;
    }
    SOURCE_EXT.contains(&extension_of(name_of(rel)))
}

/// The directories strictly between a scope directory and the repo root.
///
/// `/`-joined relative paths, `""` for the root itself, matching
/// `project::ancestors_up_to`'s walk from a scope directory's parent upward.
#[must_use]
pub fn ancestor_dirs_of_scope(scope: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in scope {
        if d == "." {
            // `ancestors_up_to(root, root)` is empty: a root scope has nothing
            // above it inside the repo.
            continue;
        }
        let mut cur = d.as_str();
        while let Some(i) = cur.rfind('/') {
            let parent = &cur[..i];
            out.push(parent.to_string());
            cur = parent;
        }
        out.push(String::new());
    }
    out.sort();
    out.dedup();
    out
}

/// Whether `rel` is a configuration input model discovery reads.
///
/// `Cargo.toml` only ever counts inside `scope` (`rsgraph::crates::discover`
/// scans scope, never its ancestors); `.csproj` and `Directory.Build.props`
/// also count as direct children of an ancestor directory of a scope directory,
/// which is where `project::discover`'s non-recursive ancestor scan looks.
#[must_use]
pub fn is_config_input(rel: &str, scope: &[String]) -> bool {
    let name = name_of(rel);
    let dir = dir_of(rel);
    let in_scope = is_within_scope(rel, scope) && !has_skipped_component(dir);
    if name == "Cargo.toml" {
        return in_scope;
    }
    if !(name.ends_with(".csproj") || name == "Directory.Build.props") {
        return false;
    }
    in_scope || ancestor_dirs_of_scope(scope).iter().any(|a| a == dir)
}

/// Whether a changed `rel` can invalidate the index -- an indexed path or a
/// configuration input.
#[must_use]
pub fn is_relevant(rel: &str, scope: &[String]) -> bool {
    is_indexed_path(rel, scope) || is_config_input(rel, scope)
}

/// The pathspecs a `git status` read needs beyond [`scope_pathspecs`].
///
/// They cover every configuration input the relevance predicate accepts from
/// the ancestor scan: each ancestor directory's `Directory.Build.props` and
/// `*.csproj`.
#[must_use]
pub fn config_pathspecs(scope: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for dir in ancestor_dirs_of_scope(scope) {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        out.push(format!(":(literal){prefix}Directory.Build.props"));
        out.push(format!(":(glob){prefix}*.csproj"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn indexed_paths_follow_scope_skip_dirs_and_extensions() {
        let s = scope(&["src"]);
        assert!(is_indexed_path("src/a.cs", &s));
        assert!(is_indexed_path("src/deep/a.ts", &s));
        assert!(!is_indexed_path("other/a.cs", &s), "outside the scope");
        assert!(
            !is_indexed_path("src/notes.txt", &s),
            "not a source extension"
        );
        assert!(
            !is_indexed_path("src/bin/a.cs", &s),
            "a skipped directory is never indexed"
        );
        assert!(is_indexed_path("anything/a.cs", &scope(&["."])));
    }

    #[test]
    fn a_file_with_no_extension_is_never_indexed() {
        // `walk::extension_of` reads the whole name for a leading-dot file, so
        // `.gitignore` is not a `.gitignore` extension and never matches.
        assert!(!is_indexed_path(".gitignore", &scope(&["."])));
    }

    #[test]
    fn config_inputs_cover_scope_and_ancestors() {
        let s = scope(&["src"]);
        assert!(is_config_input("src/App.csproj", &s));
        assert!(is_config_input("App.csproj", &s), "root is an ancestor dir");
        assert!(is_config_input("Directory.Build.props", &s));
        assert!(
            !is_config_input("vendor/App.csproj", &s),
            "a sibling directory is neither in scope nor an ancestor"
        );
        assert!(
            !is_config_input("vendor/nested/App.csproj", &s),
            "the ancestor scan is non-recursive: only a direct child counts"
        );
        assert!(
            !is_config_input("src/App.props", &s),
            "only that exact props file"
        );
    }

    #[test]
    fn cargo_manifests_count_in_scope_only() {
        let s = scope(&["src"]);
        assert!(is_config_input("src/Cargo.toml", &s));
        assert!(is_config_input("src/member/Cargo.toml", &s));
        assert!(
            !is_config_input("Cargo.toml", &s),
            "crate discovery never scans ancestors"
        );
    }

    #[test]
    fn a_root_scope_covers_the_whole_repo_with_no_ancestor_specs() {
        let s = scope(&["."]);
        assert!(is_config_input("Cargo.toml", &s));
        assert!(is_config_input("deep/App.csproj", &s));
        assert!(config_pathspecs(&s).is_empty());
    }

    #[test]
    fn config_pathspecs_name_each_ancestor_directory() {
        let specs = config_pathspecs(&scope(&["a/b"]));
        assert!(specs.contains(&":(literal)a/Directory.Build.props".to_string()));
        assert!(specs.contains(&":(glob)a/*.csproj".to_string()));
        assert!(specs.contains(&":(literal)Directory.Build.props".to_string()));
        assert!(specs.contains(&":(glob)*.csproj".to_string()));
    }
}
