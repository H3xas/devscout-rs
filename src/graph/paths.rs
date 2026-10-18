use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

use crate::layout;
use crate::repo::{git_common_dir, scout_dir};

// ---------------------------------------------------------------------------
// Path resolution (private helpers for the graph directory and cache paths).
// ---------------------------------------------------------------------------

// The def/ref graph lives in the git COMMON dir (shared by every linked
// worktree, alongside the manifest), one level down; a non-git root falls
// back to `<root>/.scout/graph/`.
pub(crate) fn graph_dir(root: &Path) -> PathBuf {
    match git_common_dir(root) {
        Some(common) => common.join(layout::SCOUT_DIR).join(layout::GRAPH_DIR),
        None => scout_dir(root).join(layout::GRAPH_DIR),
    }
}

/// Path to the graph artifact (`graph.json`) for `root`.
pub fn graph_json_path(root: &Path) -> PathBuf {
    graph_dir(root).join(layout::GRAPH_FILE)
}

/// Path to the project-model staleness sidecar for `root`.
///
/// Holds exactly the bytes `graph.json`'s `units` array would carry -- the
/// serialized `Vec<GraphUnit>` and nothing else. `map` compares this file
/// against a freshly discovered model to notice a `.csproj` edit, which no
/// mtime in the fragments index can see: `.csproj` is not a `SOURCE_EXT`, so
/// editing one moves no graph file and `index_is_stale` stays false. Written
/// by `rebuild_graph` only when a model exists and DELETED when one does not,
/// so a repo that never had a `.csproj` never grows the file and one whose
/// last `.csproj` was removed still sees a difference on the next run.
pub fn project_units_path(root: &Path) -> PathBuf {
    graph_dir(root).join(layout::PROJECT_UNITS_FILE)
}

/// Path to the Rust crate-model sidecar for `root`.
///
/// Holds the serialized Cargo package model (`rsgraph::crates::RustCrates`)
/// the last rebuild resolved Rust paths against. `map` compares it with a
/// freshly discovered model to notice a `Cargo.toml` edit, which moves no
/// walked mtime. Written only when a package is in scope and deleted
/// otherwise, the same rule as `project_units_path`.
pub fn rust_crates_path(root: &Path) -> PathBuf {
    graph_dir(root).join(layout::RUST_CRATES_FILE)
}

/// Path to the transport-input sidecar for `root`.
///
/// Holds the serialized `transport::TransportInputs` the last rebuild read.
/// `map` compares it with freshly discovered inputs to notice an edit of a
/// settings file or of a project's implicit usings, which moves no graph
/// source. Written only when a fragment holds a transport site and deleted
/// otherwise.
pub fn transport_inputs_path(root: &Path) -> PathBuf {
    graph_dir(root).join(layout::TRANSPORT_INPUTS_FILE)
}

// The version in both cache filenames is the fragment SCHEMA version, bumped
// whenever the extractor starts recording something old cached fragments
// lack -- v10 added def `type_params`/`base_generic_args` and the new
// `ctor-param` ref kind (with its `args` field), v11 added markup graph
// facts (a `.xaml` file's fragment now carries the `x:Class` def and its
// element/`x:Bind` refs, where every cached markup fragment before it carried
// names only), v12 added the TS/JS reference fragment, v13 added def
// `propertyTypes` and ref `receiverPropertyOwner` plus the ref
// `receiverCallOwner`/`receiverCallMember` pair, plus the foreach
// element-type fact -- no new field (it settles into the existing
// `receiver_type`), but a cached fragment from before it can still disagree
// with a fresh one for the same unchanged file, so it rides the same bump
// rather than skipping it. v16 added the ref `receiverBase` flag, set only
// for a `base.` qualifier -- a cached v15 fragment carries none, so every
// `base.` receiver would silently resolve (or fail to resolve) as if it
// were a plain `this.` receiver. v17 added no field: the extractor now
// blanks the inactive arms of `#if`/`#else` groups before parsing, so a
// cached v16 fragment of an unchanged file can carry twin defs and refs
// from both arms that a fresh one no longer records -- the same
// "disagrees for an unchanged file" case that moved v13, so it rides a
// bump too. v18 added def `methodParams` and ref `receiverLambda`, the
// untyped-lambda-parameter callee slot -- a cached v17 fragment carries
// neither, so every per-overload parameter shape and every untyped-lambda
// callee-slot lookup they back would silently see no candidates. v19
// added def `overrideMethods` (the `override`-marked method names a type
// declares) and the fragment-level `registrations` list (each two-type-argument
// DI service registration a file's invocations record) -- a cached v18
// fragment carries neither, so every override-chain lookup and every
// registration-driven `implements` edge would silently see no facts. v20
// added ref `receiverNullable` and `lambdaArgArity` -- a cached v19
// fragment carries neither, so a `T?` receiver's own `Nullable<T>.Value`/
// `HasValue`/`GetValueOrDefault` unwrap would silently keep resolving
// against `T`'s own same-named member, and a delegate-shaped argument whose
// own parameter count disagrees with a candidate's delegate parameter
// shape would silently keep binding it. v21 fills ref `typeArgCount` on
// every `uses-member` ref -- a cached v20 fragment carries `null` there, so
// a member qualifier written with a type-argument list would silently keep
// binding a same-named sibling of the WRONG arity, and a bare qualifier
// would silently keep binding whichever same-named sibling the def index
// happened to order first. v22 added def `baseTypeArgs` (a base's NESTED
// generic-argument descriptors, alongside the already-flat
// `baseGenericArgs`), the fragment-level `publishes` list (each
// message-bus publish-site fact a file's invocations record), def
// `propertyMessageArgs` (the single type arguments a type's own properties
// wrap), publish `enclosingMethod` (set when a publish hands on its own
// caller's message, which makes the enclosing method a forwarding
// wrapper), the fragment-level `handlerRegistrations` list (each
// one-type-argument handler registration a file's invocations record), and
// publish sites whose message is named by the call itself under a verb
// this engine does not ship -- a cached v21 fragment carries none of them,
// so a nested base's inner type argument (`IConsumer<Batch<T>>`'s `T`),
// every publish-site fact and every message vocabulary a repository
// declares about itself would silently be missing from a reused cache.
// v23 added def `arrayMessageBases` (the array-marked subset of a def's own
// `bases`, for a base whose message-position argument is an array type) --
// a cached v22 fragment carries none, so a single message and an array of
// it would read as the same base-list argument. v22 never shipped in an
// accepted or released build; every run that wrote one, including this
// lane's own measurement runs, is superseded by the rename same as any
// other generation.
// v24 drops explicit interface implementations (`IFace.Member` declarations)
// from a def's `methods`, `methodReturns`, `methodReturnArgs`,
// `nonPublicMethods`, `properties` and `propertyTypes`, and from the
// class-scope receiver facts a bare property name reads -- a cached v23
// fragment of an unchanged file still lists them, so `this.M()`, a property
// hop and a bare name inside the implementing type would keep binding a
// member C# lookup never finds there.
// v25 adds `namespace` to a plain `usings` entry: the namespace the directive
// is declared in, which bare type-name resolution reads to consult a using
// only at its own namespace level, after that level's types. A cached v24
// fragment of an unchanged file carries no such key and reads every using as
// declared at compilation-unit level, so a using written inside a namespace
// block would lose to a type in a shallower namespace and one written in a
// sibling block would still apply. The same generation keeps the written
// qualifier on a publish `message` the call names itself (`new Ns.M()` is
// `Ns.M`, no longer `M`), so the message resolves through that qualifier; a
// cached v24 fragment of an unchanged file still carries the bare name, which
// the call site's namespace can bind to a different same-named type.
// v26 adds ref `methodTypeArgCount` (the length of a type-argument list
// written on the invoked member itself), def `methodTypeArities` (each
// overload's own type-parameter count, aligned with `methodArities`) and
// extension-entry `typeParamCount`, and keeps two extension overloads that
// differ only in their type-parameter count as separate entries. A cached v25
// fragment of an unchanged file carries none of them, so `x.M<A, B>()` would
// keep binding an `M<T>` overload C# refuses, where a fresh fragment of the
// same file binds only an overload with two type parameters.
// v27 adds ref `lambdaArgShape` (whether each positional lambda literal's body
// can only produce a value, and whether its parameters are implicitly typed),
// def `methodTypeParamSites` (where each overload's own type parameters occur
// in its parameter types, aligned with `methodParams`) and def
// `delegateSignature` (a delegate's return and where its type parameters
// occur). A cached v26 fragment of an unchanged file carries none of them, so
// `list.Any(m => true)` would keep binding an `Any(Action<F>)` or an
// uninferable `Any<T>(FilterDelegate<IX<T>>)` overload C# refuses, where a
// fresh fragment of the same file skips both.
// v28 adds the Rust fragment arm (`rs: 1`). No fragment an earlier build
// wrote changes shape; the bump exists so a build without that arm never
// meets a Rust fragment, because one cache entry no arm accepts makes the
// whole cache fail to parse and every file reparse on every run.
// v29 adds Rust resolution facts (`modFiles`, `ctors`, `tests`, `impls`,
// `uses`, `blocks`, `sites`) and module scopes extended by item macros.
// A cached v28 Rust fragment lacks these facts, so edges of an unchanged
// file would be missing and globs reaching an item-macro module could be
// resolved incorrectly.
// v30 appends Rust `modPaths` (the file a `#[path]` attribute names for a
// `mod x;` declaration) and lists in `tests` the functions a `cfg_attr`
// marks as tests. A cached v29 Rust fragment of an unchanged file carries
// neither, so its `#[path]` modules would keep reaching the default file
// rustc does not compile, and its `cfg_attr` tests would stay unmarked.
// v31 adds `bound_defs` to a TS `import` edge: the definitions its named,
// default and type-only bindings resolve to. The fragments themselves are
// unchanged, but an older cache pair of an unchanged repository would let `map`
// skip the rebuild and keep a `graph.json` whose import edges bind nothing,
// so every TS import row would stay missing until some file changed.
// v32 adds ref `receiverTypeQualified` and fact `qualifiedType` -- the
// written path of a receiver's own declared type, alongside the bare last
// segment `receiverType`/`type` already carry. A cached v31 fragment of an
// unchanged file carries neither, so a receiver declared `Outer.Type` would
// keep resolving through its tail, which the ladder's using and
// graph-wide-uniqueness steps answer with whatever same-named type they
// reach, where a fresh fragment of the same file resolves the written path.
// v33 adds ref `argTypes`, an ordered static-type fact for each written call
// argument, and records conversion, variance and selected result descriptors
// in the existing `methodParams` table. A cached v32 fragment has none of
// these facts, so a same-count overload that rejects the arguments could
// still bind precisely ahead of an applicable base or extension method.
// v34 records every dotted name the C# extractor builds out of a qualified
// name's own tokens, with the whitespace, line breaks and comments between
// them left out, so `Widgets . Widget` reads as `Widgets.Widget`. A cached
// v33 fragment of an unchanged file keeps the raw text in its receiver and
// type-reference fields, and no lookup matches that text: the written path
// reaches the ladder with trivia inside it, the last-dot split yields a name
// with a blank or a comment in it, and every precise edge the name earned
// degrades to a bare-name guess. A fresh extraction of the same unchanged
// bytes disagrees, which is why the pair has to move.
// v35 records an interpolated string argument under its own descriptor and marks a type declared
// `[InterpolatedStringHandler]` in the `methodParams` table. A cached v33 fragment has neither, and
// a cached v34 pair carries the other change's facts only, so a map that reused either would still
// compare the interpolated string as a plain `string` and veto the overload C# selects.
// v36 adds no extraction field: it stamps the pair with the worktree that
// wrote it, and changes the file shape from a bare rel-keyed object to an
// `{owner, entries}` one. The owner is the writing worktree's Git
// administrative directory, so a fragment another worktree extracted is
// never reused here even when the relative path and the content key both
// match: another worktree's scope and its own configuration are what those
// bytes were read under. A v35 payload carries no owner and does not parse
// as this generation either, so the bump is also what stops a pre-stamp
// cache from being read.
// v37 records identifier-argument publish sites for unguarded dispatch-root
// verbs. A cached v36 fragment of an unchanged file lacks those records, so
// its newly admitted calls would still produce no hop until re-extracted.
// v38 appends import-equals, export-equals, namespace re-export and dynamic
// import facts to TS fragments. Older fragments cannot supply those facts.
// v39 is another extractor change's generation (file-scope TS/JS
// declarations), so it is skipped here rather than reused for different facts.
// v40 scopes every C# member-table entry to the constructs that declare it and
// enters `var (…)` deconstruction names as taken-but-unknown. A cached v38
// fragment of an unchanged file still types a bare argument or receiver from a
// declaration its use never sees, so a map reusing it would keep vetoing the
// overload C# selects.
// v41 appends the transport configuration sites a C# file states. A cached v40
// fragment of an unchanged file carries none, so a map reusing it would record
// no transport fact for that file.
// v42 appends the `@Component` facts of a TS file and adds the `.html`
// template fragment. A cached v41 fragment of an unchanged component carries
// no members and no template, so a map reusing it would give no template edge.
// v43 joins the v42 facts with the file-scope TS/JS declarations and local
// references that v39 carried on its own. A cached v42 fragment has no local
// declarations, and a cached v39 one has no v40-v42 facts, so neither is reused.
// The rename IS the invalidation mechanism:
// pre-bump caches stop being found, every file reparses
// once, no reader carries version-compat logic. Writers delete every
// superseded generation (see `remove_superseded_caches`).
pub(crate) fn fragments_cache_path(root: &Path) -> PathBuf {
    graph_dir(root).join(layout::FRAGMENT_CACHE_FILE)
}

/// The fragment generation this build reads and writes.
///
/// Parsed from the cache file name: the rename IS the invalidation mechanism,
/// so the number a reader reports can only ever be the one this build's own
/// path carries.
pub fn fragment_generation() -> u32 {
    layout::FRAGMENT_CACHE_FILE
        .trim_start_matches("fragments-v")
        .trim_end_matches(".json")
        .parse()
        .expect("the fragment cache file name carries its generation")
}

pub(crate) fn fragments_index_path(root: &Path) -> PathBuf {
    graph_dir(root).join(layout::FRAGMENT_INDEX_FILE)
}

// Every generation below the current one, not just the immediately previous:
// a repo mapped last under v1 and never since would otherwise keep its v1
// pair forever.
pub(crate) const SUPERSEDED_CACHE_FILES: &[&str] = &[
    "fragments.json",
    "fragments-index.json",
    "fragments-v2.json",
    "fragments-index-v2.json",
    "fragments-v3.json",
    "fragments-index-v3.json",
    "fragments-v4.json",
    "fragments-index-v4.json",
    "fragments-v5.json",
    "fragments-index-v5.json",
    "fragments-v6.json",
    "fragments-index-v6.json",
    "fragments-v7.json",
    "fragments-index-v7.json",
    "fragments-v8.json",
    "fragments-index-v8.json",
    "fragments-v9.json",
    "fragments-index-v9.json",
    "fragments-v10.json",
    "fragments-index-v10.json",
    "fragments-v11.json",
    "fragments-index-v11.json",
    "fragments-v12.json",
    "fragments-index-v12.json",
    "fragments-v13.json",
    "fragments-index-v13.json",
    "fragments-v14.json",
    "fragments-index-v14.json",
    "fragments-v15.json",
    "fragments-index-v15.json",
    "fragments-v16.json",
    "fragments-index-v16.json",
    "fragments-v17.json",
    "fragments-index-v17.json",
    "fragments-v18.json",
    "fragments-index-v18.json",
    "fragments-v19.json",
    "fragments-index-v19.json",
    "fragments-v20.json",
    "fragments-index-v20.json",
    "fragments-v21.json",
    "fragments-index-v21.json",
    "fragments-v22.json",
    "fragments-index-v22.json",
    "fragments-v23.json",
    "fragments-index-v23.json",
    "fragments-v24.json",
    "fragments-index-v24.json",
    "fragments-v25.json",
    "fragments-index-v25.json",
    "fragments-v26.json",
    "fragments-index-v26.json",
    "fragments-v27.json",
    "fragments-index-v27.json",
    "fragments-v28.json",
    "fragments-index-v28.json",
    "fragments-v29.json",
    "fragments-index-v29.json",
    "fragments-v30.json",
    "fragments-index-v30.json",
    "fragments-v31.json",
    "fragments-index-v31.json",
    "fragments-v32.json",
    "fragments-index-v32.json",
    "fragments-v33.json",
    "fragments-index-v33.json",
    "fragments-v34.json",
    "fragments-index-v34.json",
    "fragments-v35.json",
    "fragments-index-v35.json",
    "fragments-v36.json",
    "fragments-index-v36.json",
    "fragments-v37.json",
    "fragments-index-v37.json",
    "fragments-v38.json",
    "fragments-index-v38.json",
    "fragments-v39.json",
    "fragments-index-v39.json",
    "fragments-v40.json",
    "fragments-index-v40.json",
    "fragments-v41.json",
    "fragments-index-v41.json",
    "fragments-v42.json",
    "fragments-index-v42.json",
];

pub(crate) fn remove_superseded_caches(root: &Path) {
    for name in SUPERSEDED_CACHE_FILES {
        // Best-effort cleanup only -- a leftover superseded file is inert.
        let _ = fs::remove_file(graph_dir(root).join(name));
    }
}

// ---------------------------------------------------------------------------
// Atomic writes -- tmp file in the SAME directory as the target (so
// the final `rename` is same-filesystem and therefore atomic), unique name
// so concurrent writers (this process's own sequential graph/fragments/index
// writes, or a genuinely concurrent process) never collide. Writing to a
// tmp file and renaming is a deliberate upgrade over an in-place write --
// output BYTES are unaffected, only the write's crash-safety.
// ---------------------------------------------------------------------------

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("artifact");
    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_path = dir.join(format!(".{file_name}.tmp.{}.{counter}", std::process::id()));
    let write_result = fs::write(&tmp_path, bytes);
    if write_result.is_err() {
        let _ = fs::remove_file(&tmp_path);
        write_result?;
    }
    fs::rename(&tmp_path, path)?;
    Ok(())
}

pub(crate) fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    atomic_write_bytes(path, &bytes)
}
