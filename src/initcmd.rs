// `scout init`: registry registration + `.git/info/exclude` handling (incl.
// worktree common-dir sharing), zero-questions on a normal repo. Also holds the
// registry WRITE side (the read side is repo.rs's, and deliberately read-only;
// see the "Registry write path" section below).
//
// ## What `cmd_init` actually does
//
// Parse `--label <l>` / positional scope-dir args; resolve the nearest `.git`
// ancestor of `cwd` or fall back to `resolve(cwd)`; on a non-git root, refuse
// (exit 2) if it directly contains OTHER git repos as immediate subdirectories
// (a `.scout` above several checkouts has no single HEAD, so the
// manifest-staleness check would silently stop firing -- refusing beats
// documenting a footgun); on a git root, ensure `.scout/` is listed in the git
// COMMON dir's `info/exclude` (git shares `info/` across every linked worktree,
// so one write covers all of them) and create `<root>/.scout/`; either way,
// register the root in the machine-local registry (`repos.json`) and print one
// summary line. The failure-prone step (resolving the common dir, writing the
// exclude) runs BEFORE anything is created or registered, so a failure there
// leaves nothing behind.
//
// ## Registry write path
//
// repo.rs exposes registry READ only, by its own module header's explicit
// design. The WRITE path lives here instead, round-tripped through
// `manifest::Value` (the schema-agnostic, order-preserving JSON type) rather
// than `repo::RegistryEntry` (a typed, six-known-field, read-only struct): a
// `Value`-based round-trip preserves any field beyond those six on an EXISTING
// entry untouched. Going through `RegistryEntry` first would silently drop
// anything else on every `devscout init` re-run.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::manifest;
use crate::mapcmd;
use crate::repo;
use crate::walk;

// ---------------------------------------------------------------------------
// calendar date as `YYYY-MM-DD`. No date/time crate is used --
// `civil_from_days` is Howard Hinnant's well-known constexpr
// days-since-epoch -> (y, m, d) algorithm
// (http://howardhinnant.github.io/date_algorithms.html), proleptic
// Gregorian, valid for every date this tool will ever see.
// ---------------------------------------------------------------------------

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

fn today() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

// ---------------------------------------------------------------------------
// `parse_init_args(args)`. `--label <l>` consumes its value (a missing value
// when `--label` is the last token collapses to `None`); every other token is a
// scope dir, in the order given.
// ---------------------------------------------------------------------------

fn parse_init_args(args: &[String]) -> (Option<String>, Vec<String>) {
    let mut label: Option<String> = None;
    let mut scope: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        if args[i] == "--label" {
            label = args.get(i + 1).cloned();
            i += 2;
            continue;
        }
        scope.push(args[i].clone());
        i += 1;
    }
    (label, scope)
}

// ---------------------------------------------------------------------------
// `nested_git_repos(root)` -- immediate subdirectories of `root` that themselves
// contain a `.git` entry (file or dir -- `Path::exists` does not distinguish).
// `read_dir` order is OS-dependent, so the result is sorted explicitly: it feeds
// directly into a user-facing error message, so its order is observable, not
// incidental (the same determinism discipline walk.rs applies to enumeration).
// ---------------------------------------------------------------------------

fn nested_git_repos(root: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if root.join(&name).join(".git").exists() {
                names.push(name);
            }
        }
    }
    names.sort();
    Ok(names)
}

// ---------------------------------------------------------------------------
// `.git/info/exclude` handling -- the body of `cmd_init`'s git-root branch. The
// exclude file is read with LOSSY UTF-8 decoding (see mapcmd.rs's
// `read_source_lossy` for the same convention and why `fs::read_to_string` --
// strict -- is the wrong primitive to reach for by default in this codebase); a
// git exclude file is effectively guaranteed plain ASCII/UTF-8 in practice, but
// the lossy read costs nothing and keeps the convention uniform.
// ---------------------------------------------------------------------------

fn ensure_scout_excluded(info_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(info_dir)?;
    let exclude_path = info_dir.join("exclude");
    let body = if exclude_path.exists() {
        String::from_utf8_lossy(&fs::read(&exclude_path)?).into_owned()
    } else {
        String::new()
    };
    let already_listed = body.split('\n').any(|l| l.trim() == ".scout/");
    if !already_listed {
        let prefix = if body.is_empty() || body.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        use io::Write as _;
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&exclude_path)?;
        write!(f, "{prefix}.scout/\n")?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Registry write path (`register_root`, `write_registry_value`, and the
// `today()` label-derivation helper above). See the module header for why this
// round-trips through `manifest::Value` rather than `repo::RegistryEntry`.
// ---------------------------------------------------------------------------

// Sets `key` on an object `Value`: on an EXISTING key the value is replaced and
// its POSITION preserved; a new key is appended at the end. Same pattern as
// mapcmd.rs's `with_ensured_source` (that module's own doc comment has the
// fuller rationale) -- duplicated rather than imported because that version is
// private and hardcodes `"source"`, while this one operates on `manifest::Value`
// generically (any key).
fn set_field(value: manifest::Value, key: &str, new_value: manifest::Value) -> manifest::Value {
    let fields = value.as_object().unwrap_or(&[]).to_vec();
    let mut out = Vec::with_capacity(fields.len() + 1);
    let mut replaced = false;
    for (k, v) in fields {
        if k == key {
            out.push((k, new_value.clone()));
            replaced = true;
        } else {
            out.push((k, v));
        }
    }
    if !replaced {
        out.push((key.to_string(), new_value));
    }
    manifest::Value::Object(out)
}

// Reads the registry, returning the WHOLE parsed value (not just its `roots`
// array) so a write-back preserves any top-level field beyond `roots` untouched
// -- see module header. A missing file reads as `{roots: []}`; a present-but-
// corrupt file, or one with no `roots` array, errors with the exact message text
// `repo::RegistryError`'s `Display` impl uses for the same two failures (kept in
// sync by hand: this module cannot `use` that error type -- it is private to
// repo.rs -- so the message text is quoted here verbatim).
fn read_registry_value() -> Result<manifest::Value, String> {
    let path = repo::registry_path();
    if !path.exists() {
        return Ok(manifest::Value::object(vec![(
            "roots",
            manifest::Value::array(vec![]),
        )]));
    }
    let bytes = fs::read(&path)
        .map_err(|e| format!("registry at {} is not valid JSON: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let value: manifest::Value = serde_json::from_str(&text)
        .map_err(|e| format!("registry at {} is not valid JSON: {e}", path.display()))?;
    match value.get("roots") {
        Some(manifest::Value::Array(_)) => Ok(value),
        _ => Err(format!(
            "registry at {} has no \"roots\" array",
            path.display()
        )),
    }
}

// Writes the registry as 2-space pretty-printed JSON with a trailing `\n`. NOTE
// the trailing `\n`: unlike `manifest::write_manifest` (which does NOT append
// one), the registry writer does.
//
// The write is atomic: tmp-file + rename -- the same rationale
// `manifest::write_manifest`'s doc comment states -- so a concurrent reader
// never observes a truncated file. This closes that window without changing the
// final bytes.
fn write_registry_value(value: &manifest::Value) -> Result<(), String> {
    let path = repo::registry_path();
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let body = format!("{json}\n");

    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_name = format!(
        "{}.tmp.{}.{suffix}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repos.json"),
        std::process::id()
    );
    let tmp_path = path.with_file_name(tmp_name);

    if let Err(e) = fs::write(&tmp_path, body.as_bytes()) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e.to_string());
    }
    if let Err(e) = fs::rename(&tmp_path, &path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e.to_string());
    }
    Ok(())
}

fn new_entry_value(
    root_abs: &str,
    kind: &str,
    label: Option<&str>,
    scope: &[String],
    stamp: &str,
) -> manifest::Value {
    manifest::Value::object(vec![
        ("root", manifest::Value::string(root_abs)),
        ("kind", manifest::Value::string(kind)),
        (
            "label",
            label
                .map(manifest::Value::string)
                .unwrap_or(manifest::Value::Null),
        ),
        (
            "scope",
            manifest::Value::array(
                scope
                    .iter()
                    .map(|s| manifest::Value::string(s.clone()))
                    .collect(),
            ),
        ),
        ("initialized", manifest::Value::string(stamp)),
        ("last_seen", manifest::Value::string(stamp)),
    ])
}

// Updates an EXISTING registry entry: `kind` is set unconditionally;
// `label`/`scope` overwrite only when non-empty (a `None`/empty label or empty
// scope leaves the prior value in place); `last_seen` always bumps to today.
fn update_entry_value(
    entry: manifest::Value,
    kind: &str,
    label: Option<&str>,
    scope: &[String],
    stamp: &str,
) -> manifest::Value {
    let mut e = set_field(entry, "kind", manifest::Value::string(kind));
    if let Some(l) = label {
        if !l.is_empty() {
            e = set_field(e, "label", manifest::Value::string(l));
        }
    }
    if !scope.is_empty() {
        e = set_field(
            e,
            "scope",
            manifest::Value::array(
                scope
                    .iter()
                    .map(|s| manifest::Value::string(s.clone()))
                    .collect(),
            ),
        );
    }
    e = set_field(e, "last_seen", manifest::Value::string(stamp));
    e
}

// Registers or updates `root_abs` in the registry. `root_abs` is already the
// fully resolved absolute root (`cmd_init`'s `root`), so no further resolution
// happens here.
fn register_root(
    root_abs: &str,
    kind: &str,
    label: Option<&str>,
    scope: &[String],
) -> Result<(), String> {
    let registry = read_registry_value()?;
    let roots: Vec<manifest::Value> = match registry.get("roots") {
        Some(manifest::Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    };

    let stamp = today();
    let mut found = false;
    let mut new_roots = Vec::with_capacity(roots.len() + 1);
    for entry in roots {
        if entry.get("root").and_then(manifest::Value::as_str) == Some(root_abs) {
            found = true;
            new_roots.push(update_entry_value(entry, kind, label, scope, &stamp));
        } else {
            new_roots.push(entry);
        }
    }
    if !found {
        new_roots.push(new_entry_value(root_abs, kind, label, scope, &stamp));
    }

    let updated_registry = set_field(registry, "roots", manifest::Value::Array(new_roots));
    write_registry_value(&updated_registry)
}

// ---------------------------------------------------------------------------
// `cmd_init(cwd, args)`.
// ---------------------------------------------------------------------------

/// Runs `scout init`. Returns `(code, out)` like every other command in cli.rs
/// (the `run(argv, cwd)` -> `{code, out}` convention documented at the top of
/// that file's refs/impact/find section).
pub fn cmd_init(cwd: &Path, args: &[String]) -> (i32, String) {
    let (label, scope) = parse_init_args(args);
    let git_root = repo::find_repo_root(cwd);
    let root = git_root.clone().unwrap_or_else(|| repo::resolve_path(cwd));

    if git_root.is_none() {
        match nested_git_repos(&root) {
            Ok(nested) if !nested.is_empty() => {
                return (
                    2,
                    format!(
                        "refusing to init {}: it contains git repos ({}).\nA .scout above several repos has no single HEAD, so manifest staleness detection silently stops firing. Run devscout init inside each repo instead.",
                        root.display(),
                        nested.join(", "),
                    ),
                );
            }
            Ok(_) => {}
            Err(e) => return (1, format!("error: {e}")),
        }
    }

    let scope_note = if scope.is_empty() {
        String::new()
    } else {
        format!(", scope {}", scope.join(", "))
    };
    let scout_dir = repo::scout_dir(&root);
    let root_abs = root.to_string_lossy().into_owned();

    if let Some(git_root) = &git_root {
        // The failure-prone step (resolving the common dir, writing the
        // exclude) runs FIRST, so a failure here leaves nothing created and
        // nothing registered.
        let common = match repo::git_common_dir(git_root) {
            Some(c) => c,
            // Unreachable in practice: `git_root` was itself found via a `.git`
            // marker, so `git_common_dir` resolving it should never fail except
            // under a TOCTOU race (the marker vanishing between the two calls).
            // This aborts with an error rather than panicking.
            None => {
                return (
                    1,
                    format!(
                        "error: unable to resolve git common dir for {}",
                        git_root.display()
                    ),
                )
            }
        };
        if let Err(e) = ensure_scout_excluded(&common.join("info")) {
            return (1, format!("error: {e}"));
        }
        if let Err(e) = fs::create_dir_all(&scout_dir) {
            return (1, format!("error: {e}"));
        }
        if let Err(e) = register_root(&root_abs, "git", label.as_deref(), &scope) {
            return (1, format!("error: {e}"));
        }
        return (
            0,
            format!(
                "devscout initialized at {}{scope_note}",
                scout_dir.display()
            ),
        );
    }

    if let Err(e) = fs::create_dir_all(&scout_dir) {
        return (1, format!("error: {e}"));
    }
    if let Err(e) = register_root(&root_abs, "plain", label.as_deref(), &scope) {
        return (1, format!("error: {e}"));
    }
    (
        0,
        format!(
            "devscout initialized at {} (non-git root: {}){scope_note}",
            scout_dir.display(),
            root.display()
        ),
    )
}

// ---------------------------------------------------------------------------
// Out-of-the-box productization.
//
// Hook installation lives here, NOT in `cmd_init`: the init tests call
// `cmd_init` directly with the real `HOME`, so folding it in would write the
// operator's live settings on every test run.
// ---------------------------------------------------------------------------

// C# gets the complete extraction path. TypeScript-family files are indexed
// and resolved into the graph, but with narrower edge coverage. The remaining
// extensions enumerated by `walk::SOURCE_EXT` are counted but not indexed.
const FULLY_SUPPORTED_EXT: &[&str] = &[".cs"];
const INDEXED_AND_GRAPHED_EXT: &[&str] = &[".ts", ".tsx", ".js", ".jsx"];

// Flags handled only here -- stripped from `args` before they ever reach
// `parse_init_args` (which treats unknown tokens as scope-dir positionals), so
// `--no-hooks`/`--no-map` are never mistaken for a scope directory and never
// leak into the registry's `scope` field.
fn extract_rust_only_flags(args: &[String]) -> (bool, bool, Vec<String>) {
    let mut no_hooks = false;
    let mut no_map = false;
    let mut rest = Vec::with_capacity(args.len());
    for a in args {
        match a.as_str() {
            "--no-hooks" => no_hooks = true,
            "--no-map" => no_map = true,
            _ => rest.push(a.clone()),
        }
    }
    (no_hooks, no_map, rest)
}

/// The CLI entry point for `init` (wired from cli.rs, replacing a direct
/// `cmd_init` call). Runs the core `cmd_init` first; on any core failure
/// (nonzero code) returns immediately, unchanged, with NOTHING below this point
/// attempted, so the exit code is nonzero only when the core steps failed. On
/// success, runs the three extra steps (language census, hook install, first
/// map) and appends one status line each, always exiting 0: a failure in any of
/// them is reported on its own line and does not change the exit code or block
/// the other steps (each step is independent -- a map failure does not skip the
/// hooks line or vice versa).
pub fn cmd_init_full(cwd: &Path, args: &[String]) -> (i32, String) {
    let (no_hooks, no_map, parity_args) = extract_rust_only_flags(args);
    let (code, out) = cmd_init(cwd, &parity_args);
    if code != 0 {
        return (code, out);
    }

    // Same root-resolution `cmd_init` itself used -- not returned by that
    // function, so re-derived here rather than plumbed through a changed
    // signature (keeping `cmd_init`'s signature untouched is deliberate).
    let git_root = repo::find_repo_root(cwd);
    let root = git_root.unwrap_or_else(|| repo::resolve_path(cwd));

    let lines = [
        out,
        census_line(&root),
        hooks_line(no_hooks),
        map_line(&root, no_map),
    ];
    (0, lines.join("\n"))
}

// ---------------------------------------------------------------------------
// Language census. Rather than a second, independent recursive walker, this
// counts extensions over exactly the file set `walk::list_source_files` already
// enumerates (its `SKIP_DIRS`/`SOURCE_EXT` contract). Bounded consequence, noted
// rather than solved: a real-but-unsupported language outside `walk::SOURCE_EXT`
// entirely (Python, Go, ...) is invisible to this census, same as it is
// invisible to `devscout map` itself.
// ---------------------------------------------------------------------------

fn census_line(root: &Path) -> String {
    let files = match walk::list_source_files(root, &[".".to_string()]) {
        Ok(f) => f,
        Err(e) => return format!("languages: error walking sources ({e})"),
    };
    if files.is_empty() {
        return "languages: no source files found".to_string();
    }

    let mut counts: Vec<(String, usize)> = Vec::new();
    for f in &files {
        let ext = match f.rfind('.') {
            Some(i) => f[i..].to_string(),
            None => String::new(),
        };
        match counts.iter_mut().find(|(e, _)| *e == ext) {
            Some(entry) => entry.1 += 1,
            None => counts.push((ext, 1)),
        }
    }
    counts.sort();
    let (fully_supported, other): (Vec<_>, Vec<_>) = counts
        .into_iter()
        .partition(|(e, _)| FULLY_SUPPORTED_EXT.contains(&e.as_str()));
    let (indexed_and_graphed, not_indexed): (Vec<_>, Vec<_>) = other
        .into_iter()
        .partition(|(e, _)| INDEXED_AND_GRAPHED_EXT.contains(&e.as_str()));
    let fmt_group = |g: &[(String, usize)]| {
        g.iter()
            .map(|(e, c)| format!("{c} {e}"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let mut groups = Vec::new();
    if !fully_supported.is_empty() {
        groups.push(format!("{} (fully supported)", fmt_group(&fully_supported)));
    }
    if !indexed_and_graphed.is_empty() {
        groups.push(format!(
            "{} (indexed and graphed, narrower edge coverage)",
            fmt_group(&indexed_and_graphed)
        ));
    }
    if !not_indexed.is_empty() {
        groups.push(format!(
            "{} (present, not indexed)",
            fmt_group(&not_indexed)
        ));
    }
    format!("languages: {}", groups.join("; "))
}

// ---------------------------------------------------------------------------
// Hook install. Reads `$HOME/.claude/settings.json` (read-only, for
// SHAPE) to decide whether it is safe to mirror that structure; writes only
// when the shape is recognized AND at least one of the two entries
// (`hook read` / `hook bash`) is actually missing. Fail-safe on anything
// else: missing file, invalid JSON, or a shape this code does not
// recognize all take the SAME "print the snippet, do not write" path,
// deliberately collapsed into one outcome, since "is this safe to merge
// into" is a single yes/no question regardless of which specific way the
// answer came out no.
// ---------------------------------------------------------------------------

enum HookOutcome {
    AlreadyInstalled,
    Installed {
        added: Vec<&'static str>,
        backup: PathBuf,
    },
    NeedsManualInsert {
        reason: String,
        snippet: String,
    },
}

fn hooks_line(no_hooks: bool) -> String {
    if no_hooks {
        return "hooks: skipped (--no-hooks)".to_string();
    }
    match install_hooks() {
        Ok(HookOutcome::AlreadyInstalled) => "hooks: already installed (idempotent)".to_string(),
        Ok(HookOutcome::Installed { added, backup }) => {
            format!(
                "hooks: installed ({}); backup {}",
                added.join(", "),
                backup.display()
            )
        }
        Ok(HookOutcome::NeedsManualInsert { reason, snippet }) => {
            format!("hooks: {reason} -- add manually:\n{snippet}")
        }
        Err(e) => format!("hooks: error: {e}"),
    }
}

// `$HOME/.claude/settings.json` -- respects the `HOME` env var (the test seam),
// never hardcodes a path. `Err` only when `HOME` itself is unset, which this
// function treats as a hard error (there is no live-settings hazard here: with
// no `HOME`, there is no path to touch).
fn settings_path() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    Ok(PathBuf::from(home).join(".claude").join("settings.json"))
}

// The absolute path to the CURRENTLY RUNNING binary -- what the installed hook
// command actually invokes. `current_exe` + `canonicalize` (falling back to the
// uncanonicalized path if that fails, e.g. a since-deleted binary mid-run)
// resolves through any symlink so the installed command keeps working even if a
// PATH symlink is later repointed.
fn current_binary_path() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = fs::canonicalize(&exe).unwrap_or(exe);
    Ok(exe.to_string_lossy().into_owned())
}

fn install_hooks() -> Result<HookOutcome, String> {
    let bin = current_binary_path()?;
    let path = settings_path()?;

    if !path.exists() {
        return Ok(HookOutcome::NeedsManualInsert {
            reason: format!("no settings file at {}", path.display()),
            snippet: hook_snippet(&bin),
        });
    }

    let bytes = fs::read(&path).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let value: manifest::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => {
            return Ok(HookOutcome::NeedsManualInsert {
                reason: format!("settings file at {} is not valid JSON", path.display()),
                snippet: hook_snippet(&bin),
            })
        }
    };

    if !matches!(value, manifest::Value::Object(_)) || !hooks_shape_ok(&value) {
        return Ok(HookOutcome::NeedsManualInsert {
            reason: format!(
                "settings file at {} has an unexpected shape",
                path.display()
            ),
            snippet: hook_snippet(&bin),
        });
    }

    let (updated, added) = merge_hooks(value, &bin);
    if added.is_empty() {
        return Ok(HookOutcome::AlreadyInstalled);
    }

    // Backup happens ONLY here -- right before the one write path, and only
    // once we know a write is actually about to happen (an idempotent
    // re-run with nothing to add returns above and never reaches this
    // line, so it never creates a second backup).
    let backup = backup_settings(&path, &bytes)?;
    write_settings(&path, &updated)?;
    Ok(HookOutcome::Installed { added, backup })
}

// `true` when `value`'s `hooks`/`hooks.PostToolUse` shape (if either key is
// even present) is one this code is willing to merge into. An ABSENT `hooks` key,
// or an absent `hooks.PostToolUse` key, is fine (nothing to validate -- both get
// created fresh by `merge_hooks`); anything PRESENT but not the expected shape is
// not.
fn hooks_shape_ok(value: &manifest::Value) -> bool {
    let Some(hooks) = value.get("hooks") else {
        return true;
    };
    let manifest::Value::Object(_) = hooks else {
        return false;
    };
    let Some(post_tool_use) = hooks.get("PostToolUse") else {
        return true;
    };
    let manifest::Value::Array(entries) = post_tool_use else {
        return false;
    };
    entries.iter().all(entry_shape_ok)
}

// One `hooks.PostToolUse[]` element: `{matcher?: string, hooks?: [{type?:
// string, command?: string}]}`. Every field is optional (some real-world entries
// omit `matcher`), but any field that IS present must have the right JSON type --
// a present-but-wrong-typed field is exactly the "shape I don't recognize, don't
// touch it" case this whole check exists for.
fn entry_shape_ok(entry: &manifest::Value) -> bool {
    if !matches!(entry, manifest::Value::Object(_)) {
        return false;
    }
    if let Some(matcher) = entry.get("matcher") {
        if !matches!(matcher, manifest::Value::String(_)) {
            return false;
        }
    }
    let Some(hooks) = entry.get("hooks") else {
        return true;
    };
    let manifest::Value::Array(items) = hooks else {
        return false;
    };
    items.iter().all(|item| {
        if !matches!(item, manifest::Value::Object(_)) {
            return false;
        }
        if let Some(t) = item.get("type") {
            if !matches!(t, manifest::Value::String(_)) {
                return false;
            }
        }
        if let Some(c) = item.get("command") {
            if !matches!(c, manifest::Value::String(_)) {
                return false;
            }
        }
        true
    })
}

// `true` when some existing `hooks.PostToolUse[].hooks[].command` already
// contains both `"scout"` and `marker` (`"hook read"`/`"hook bash"`) -- the
// idempotency rule. Scans every entry regardless of its `matcher` (an
// already-installed command under any matcher still counts), matching the rule as
// stated rather than narrowing it.
fn already_has_hook_command(entries: &[manifest::Value], marker: &str) -> bool {
    entries.iter().any(|entry| {
        let Some(manifest::Value::Array(items)) = entry.get("hooks") else {
            return false;
        };
        items.iter().any(|item| {
            item.get("command")
                .and_then(manifest::Value::as_str)
                .is_some_and(|c| c.contains("scout") && c.contains(marker))
        })
    })
}

fn hook_entry(matcher: &str, bin: &str, subcmd: &str) -> manifest::Value {
    manifest::Value::object(vec![
        ("matcher", manifest::Value::string(matcher)),
        (
            "hooks",
            manifest::Value::array(vec![manifest::Value::object(vec![
                ("type", manifest::Value::string("command")),
                (
                    "command",
                    manifest::Value::string(format!("{bin} hook {subcmd}")),
                ),
            ])]),
        ),
    ])
}

// Appends whichever of the Read/Bash entries are missing to
// `hooks.PostToolUse`, preserving every other top-level key (`PreToolUse`,
// `UserPromptSubmit`, ...) and every existing `PostToolUse` entry unchanged (via
// `set_field`'s position-preserving merge, the same primitive `register_root`
// uses on the registry). Never removes or reorders anything that was already
// there.
fn merge_hooks(value: manifest::Value, bin: &str) -> (manifest::Value, Vec<&'static str>) {
    let hooks_val = value
        .get("hooks")
        .cloned()
        .unwrap_or_else(|| manifest::Value::object(vec![]));
    let ptu_val = hooks_val
        .get("PostToolUse")
        .cloned()
        .unwrap_or_else(|| manifest::Value::array(vec![]));
    let mut entries = match ptu_val {
        manifest::Value::Array(items) => items,
        _ => Vec::new(),
    };

    let mut added: Vec<&'static str> = Vec::new();
    if !already_has_hook_command(&entries, "hook read") {
        entries.push(hook_entry("Read", bin, "read"));
        added.push("Read");
    }
    if !already_has_hook_command(&entries, "hook bash") {
        entries.push(hook_entry("Bash", bin, "bash"));
        added.push("Bash");
    }

    let updated_hooks = set_field(hooks_val, "PostToolUse", manifest::Value::Array(entries));
    let updated = set_field(value, "hooks", updated_hooks);
    (updated, added)
}

// The exact JSON a human should paste into `settings.json` by hand when this
// code declines to write it itself. Built through `manifest::Value` +
// `serde_json`'s pretty printer (not hand-formatted string interpolation) so the
// binary path is correctly JSON-escaped even in the pathological case of a path
// containing a `"` or backslash.
fn hook_snippet(bin: &str) -> String {
    let v = manifest::Value::object(vec![(
        "hooks",
        manifest::Value::object(vec![(
            "PostToolUse",
            manifest::Value::array(vec![
                hook_entry("Read", bin, "read"),
                hook_entry("Bash", bin, "bash"),
            ]),
        )]),
    )]);
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

// `YYYYMMDD-HHMMSS-nnnnnnnnn` (UTC, sub-second suffix for collision safety on
// rapid successive installs within the same test process). Reuses
// `civil_from_days` rather than pulling in a date/time crate.
fn now_stamp() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let days = (secs / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    let sod = secs % 86_400;
    let (h, mi, s) = (sod / 3600, (sod % 3600) / 60, sod % 60);
    format!(
        "{y:04}{m:02}{d:02}-{h:02}{mi:02}{s:02}-{:09}",
        dur.subsec_nanos()
    )
}

// Copies the ORIGINAL bytes (pre-parse, pre-modification) next to the settings
// file, timestamped. Never overwrites an existing backup (the
// nanosecond-resolution stamp makes a same-name collision practically
// impossible; on the vanishingly unlikely collision `fs::write` just overwrites
// it, which is still strictly safer than skipping the backup).
fn backup_settings(path: &Path, original_bytes: &[u8]) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("settings.json");
    let backup_path = path.with_file_name(format!("{name}.bak.{}", now_stamp()));
    fs::write(&backup_path, original_bytes).map_err(|e| e.to_string())?;
    Ok(backup_path)
}

// Same atomic tmp-file + rename convention as `write_registry_value` /
// `manifest::write_manifest` -- never leaves a half-written `settings.json` for
// Claude Code (or anything else reading it live) to observe mid-write.
fn write_settings(path: &Path, value: &manifest::Value) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let body = format!("{json}\n");

    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_name = format!(
        "{}.tmp.{}.{suffix}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("settings.json"),
        std::process::id()
    );
    let tmp_path = path.with_file_name(tmp_name);

    if let Err(e) = fs::write(&tmp_path, body.as_bytes()) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e.to_string());
    }
    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e.to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// First map. Thin wrapper over `mapcmd::map_repo` (the same function `cli.rs`'s
// `cmd_map` calls for a plain `devscout map`), run unscoped (matching a bare
// `devscout map` with no dir args). `MapReport::summary_line()` already reports
// files/defs/edges/elapsed in one line -- reused as-is rather than re-deriving
// the same four numbers into a second format.
// ---------------------------------------------------------------------------

fn map_line(root: &Path, no_map: bool) -> String {
    if no_map {
        return "map: skipped (--no-map)".to_string();
    }
    match mapcmd::map_repo(root, &[], mapcmd::MapOptions::from_env()) {
        Ok(report) => format!("map: {}", report.summary_line()),
        Err(e) => format!("map: error: {e}"),
    }
}

// ---------------------------------------------------------------------------
// Unit tests -- pure-function coverage only (arg parsing, date math, the
// exclude-file body logic, `set_field`'s position semantics, and
// `nested_git_repos` on a plain filesystem fixture with no registry
// involvement). Anything touching `SCOUT_REGISTRY` (the registry round-trip,
// `cmd_init` end to end) lives in the integration suite instead, on purpose:
// manifest.rs's own `#[cfg(test)]` block already mutates that same
// process-global env var behind a LOCAL mutex scoped to its own tests, and
// `cargo test`'s default `--lib` run puts every `src/*.rs` module's unit tests
// in ONE shared binary -- two independent local mutexes in two different modules
// do NOT synchronize with each other, so adding a second SCOUT_REGISTRY-mutating
// test site here would be a real, silent race, not a hypothetical one. The
// integration suite runs as its own separate process (cargo test's standard
// treatment of each `tests/*.rs` file), sidestepping the hazard entirely -- the
// same reasoning repo.rs's registry-read tests rely on.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(prefix: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "scout-initcmd-rs-{prefix}-{}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    // -- parse_init_args ----------------------------------------------------

    #[test]
    fn no_args_is_no_label_no_scope() {
        let (label, scope) = parse_init_args(&[]);
        assert_eq!(label, None);
        assert!(scope.is_empty());
    }

    #[test]
    fn label_flag_consumes_its_value() {
        let args: Vec<String> = ["--label", "backend-cs"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (label, scope) = parse_init_args(&args);
        assert_eq!(label, Some("backend-cs".to_string()));
        assert!(scope.is_empty());
    }

    #[test]
    fn label_as_last_token_with_no_value_is_none() {
        let args: Vec<String> = ["src", "--label"].iter().map(|s| s.to_string()).collect();
        let (label, scope) = parse_init_args(&args);
        assert_eq!(label, None);
        assert_eq!(scope, vec!["src".to_string()]);
    }

    #[test]
    fn scope_dirs_collected_in_order_around_label() {
        let args: Vec<String> = ["src", "--label", "l", "lib", "app"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (label, scope) = parse_init_args(&args);
        assert_eq!(label, Some("l".to_string()));
        assert_eq!(
            scope,
            vec!["src".to_string(), "lib".to_string(), "app".to_string()]
        );
    }

    // -- civil_from_days / today ------------------------------------------

    #[test]
    fn civil_from_days_epoch_is_1970_01_01() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn civil_from_days_known_reference_points() {
        // 2000-03-01 is Hinnant's own reference point for this algorithm.
        // (11017 = days between 1970-01-01 and 2000-03-01, cross-checked
        // with `python3 -c "import datetime; print((datetime.date(2000,3,1)
        // - datetime.date(1970,1,1)).days)"`.)
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
        // 2024-01-01 (a leap year's first day) -- cross-checked against
        // `date -u -d @1704067200 +%Y-%m-%d` / `date -u -r 1704067200
        // +%Y-%m-%d` (1704067200 / 86400 = 19723).
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        // 2024-02-29 -- the leap day itself, 59 days into a leap year.
        assert_eq!(civil_from_days(19_723 + 59), (2024, 2, 29));
    }

    #[test]
    fn today_is_well_formed_yyyy_mm_dd() {
        let t = today();
        assert_eq!(t.len(), 10);
        assert_eq!(t.as_bytes()[4], b'-');
        assert_eq!(t.as_bytes()[7], b'-');
        assert!(t.chars().all(|c| c.is_ascii_digit() || c == '-'));
    }

    // -- ensure_scout_excluded ------------------------------------------

    #[test]
    fn exclude_created_fresh_when_missing() {
        let dir = temp_dir("exclude-fresh");
        let info_dir = dir.join("info");
        ensure_scout_excluded(&info_dir).unwrap();
        let body = fs::read_to_string(info_dir.join("exclude")).unwrap();
        assert_eq!(body, ".scout/\n");
    }

    #[test]
    fn exclude_appended_after_existing_content_without_trailing_newline() {
        let dir = temp_dir("exclude-append-no-nl");
        let info_dir = dir.join("info");
        fs::create_dir_all(&info_dir).unwrap();
        fs::write(info_dir.join("exclude"), "*.log").unwrap();
        ensure_scout_excluded(&info_dir).unwrap();
        let body = fs::read_to_string(info_dir.join("exclude")).unwrap();
        assert_eq!(body, "*.log\n.scout/\n");
    }

    #[test]
    fn exclude_appended_after_existing_content_with_trailing_newline() {
        let dir = temp_dir("exclude-append-nl");
        let info_dir = dir.join("info");
        fs::create_dir_all(&info_dir).unwrap();
        fs::write(info_dir.join("exclude"), "*.log\n").unwrap();
        ensure_scout_excluded(&info_dir).unwrap();
        let body = fs::read_to_string(info_dir.join("exclude")).unwrap();
        assert_eq!(body, "*.log\n.scout/\n");
    }

    #[test]
    fn exclude_is_idempotent_when_already_listed() {
        let dir = temp_dir("exclude-idempotent");
        let info_dir = dir.join("info");
        fs::create_dir_all(&info_dir).unwrap();
        fs::write(info_dir.join("exclude"), "*.log\n.scout/\nmore\n").unwrap();
        ensure_scout_excluded(&info_dir).unwrap();
        let body = fs::read_to_string(info_dir.join("exclude")).unwrap();
        assert_eq!(body, "*.log\n.scout/\nmore\n", "must not double-append");
    }

    #[test]
    fn exclude_matches_on_trimmed_line_not_exact_bytes() {
        // The listed-line check trims each line, so a line carrying trailing
        // whitespace or a CRLF still counts as already-listed.
        let dir = temp_dir("exclude-trim-match");
        let info_dir = dir.join("info");
        fs::create_dir_all(&info_dir).unwrap();
        fs::write(info_dir.join("exclude"), "  .scout/  \n").unwrap();
        ensure_scout_excluded(&info_dir).unwrap();
        let body = fs::read_to_string(info_dir.join("exclude")).unwrap();
        assert_eq!(
            body, "  .scout/  \n",
            "already-listed (after trim) must not append again"
        );
    }

    // -- set_field -----------------------------------------------------

    #[test]
    fn set_field_overwrites_in_place_preserving_position() {
        let v = manifest::Value::object(vec![
            ("a", manifest::Value::string("1")),
            ("b", manifest::Value::string("2")),
        ]);
        let out = set_field(v, "a", manifest::Value::string("9"));
        let fields = out.as_object().unwrap();
        assert_eq!(fields[0].0, "a");
        assert_eq!(out.get("a").and_then(manifest::Value::as_str), Some("9"));
        assert_eq!(fields[1].0, "b");
    }

    #[test]
    fn set_field_appends_when_absent() {
        let v = manifest::Value::object(vec![("a", manifest::Value::string("1"))]);
        let out = set_field(v, "z", manifest::Value::string("new"));
        let fields = out.as_object().unwrap();
        assert_eq!(fields.last().unwrap().0, "z");
    }

    // -- nested_git_repos --------------------------------------------------

    #[test]
    fn nested_git_repos_finds_immediate_subdirs_with_dot_git_only() {
        let root = temp_dir("nested-detect");
        fs::create_dir_all(root.join("repo-a/.git")).unwrap();
        fs::create_dir_all(root.join("repo-b/.git")).unwrap();
        fs::create_dir_all(root.join("plain-dir")).unwrap();
        fs::create_dir_all(root.join("repo-a/nested-repo/.git")).unwrap(); // NOT immediate -- must not count

        let found = nested_git_repos(&root).unwrap();
        assert_eq!(
            found,
            vec!["repo-a".to_string(), "repo-b".to_string()],
            "sorted, immediate subdirs only"
        );
    }

    #[test]
    fn nested_git_repos_empty_when_none_present() {
        let root = temp_dir("nested-none");
        fs::create_dir_all(root.join("plain-dir")).unwrap();
        assert!(nested_git_repos(&root).unwrap().is_empty());
    }

    // -- new_entry_value / update_entry_value -------------------------------

    #[test]
    fn update_entry_ignores_falsy_label_and_empty_scope() {
        let entry = new_entry_value(
            "/x",
            "git",
            Some("orig"),
            &["src".to_string()],
            "2026-01-01",
        );
        let updated = update_entry_value(entry, "git", None, &[], "2026-01-02");
        assert_eq!(
            updated.get("label").and_then(manifest::Value::as_str),
            Some("orig"),
            "no label given -- keeps existing"
        );
        let scope = updated.get("scope").unwrap();
        assert!(
            matches!(scope, manifest::Value::Array(items) if items.len() == 1),
            "empty scope given -- keeps existing"
        );
        assert_eq!(
            updated.get("last_seen").and_then(manifest::Value::as_str),
            Some("2026-01-02")
        );
    }

    #[test]
    fn update_entry_overwrites_truthy_label_and_nonempty_scope() {
        let entry = new_entry_value(
            "/x",
            "git",
            Some("orig"),
            &["src".to_string()],
            "2026-01-01",
        );
        let updated = update_entry_value(
            entry,
            "git",
            Some("fresh"),
            &["lib".to_string(), "app".to_string()],
            "2026-01-02",
        );
        assert_eq!(
            updated.get("label").and_then(manifest::Value::as_str),
            Some("fresh")
        );
        let scope = updated.get("scope").unwrap();
        assert!(matches!(scope, manifest::Value::Array(items) if items.len() == 2));
    }

    // -- pure-function coverage only. Anything touching HOME or the real
    // filesystem's settings.json path lives in the integration suite instead,
    // for the same env-var-race reason SCOUT_REGISTRY tests do (this module's
    // own header, above).
    // -------------------------------------------------------------------

    #[test]
    fn extract_rust_only_flags_strips_no_hooks_and_no_map_only() {
        let args: Vec<String> = ["--no-hooks", "src", "--no-map", "--label", "l"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (no_hooks, no_map, rest) = extract_rust_only_flags(&args);
        assert!(no_hooks);
        assert!(no_map);
        assert_eq!(
            rest,
            vec!["src".to_string(), "--label".to_string(), "l".to_string()],
            "everything else passes through untouched, in order"
        );
    }

    #[test]
    fn extract_rust_only_flags_defaults_false_when_absent() {
        let args: Vec<String> = ["--label", "l", "src"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (no_hooks, no_map, rest) = extract_rust_only_flags(&args);
        assert!(!no_hooks);
        assert!(!no_map);
        assert_eq!(rest, args);
    }

    // -- census_line ---------------------------------------------------

    #[test]
    fn census_line_reports_all_three_support_tiers() {
        let dir = temp_dir("census-mixed");
        fs::write(dir.join("A.cs"), "namespace X { class A {} }").unwrap();
        fs::write(dir.join("B.cs"), "namespace X { class B {} }").unwrap();
        fs::write(dir.join("c.ts"), "export const x = 1;").unwrap();
        fs::write(dir.join("d.md"), "# doc").unwrap();
        let line = census_line(&dir);
        assert_eq!(line, "languages: 2 .cs (fully supported); 1 .ts (indexed and graphed, narrower edge coverage); 1 .md (present, not indexed)");
    }

    #[test]
    fn census_line_all_supported_omits_the_unsupported_clause() {
        let dir = temp_dir("census-cs-only");
        fs::write(dir.join("A.cs"), "namespace X { class A {} }").unwrap();
        let line = census_line(&dir);
        assert_eq!(line, "languages: 1 .cs (fully supported)");
    }

    #[test]
    fn census_line_no_supported_still_reports_unsupported() {
        let dir = temp_dir("census-unsupported-only");
        fs::write(dir.join("a.md"), "# doc").unwrap();
        let line = census_line(&dir);
        assert_eq!(line, "languages: 1 .md (present, not indexed)");
    }

    #[test]
    fn census_line_empty_repo_reports_no_source_files() {
        let dir = temp_dir("census-empty");
        assert_eq!(census_line(&dir), "languages: no source files found");
    }

    // -- hooks_shape_ok / entry_shape_ok --------------------------------

    #[test]
    fn hooks_shape_ok_on_absent_hooks_key() {
        let v = manifest::Value::object(vec![("other", manifest::Value::string("x"))]);
        assert!(hooks_shape_ok(&v));
    }

    #[test]
    fn hooks_shape_ok_on_the_live_shape() {
        // Mirrors the real ~/.claude/settings.json PostToolUse shape this
        // code was designed to recognize.
        let v = manifest::Value::object(vec![(
            "hooks",
            manifest::Value::object(vec![(
                "PostToolUse",
                manifest::Value::array(vec![
                    manifest::Value::object(vec![
                        ("matcher", manifest::Value::string("Read")),
                        (
                            "hooks",
                            manifest::Value::array(vec![manifest::Value::object(vec![
                                ("type", manifest::Value::string("command")),
                                (
                                    "command",
                                    manifest::Value::string("node scout-read-hook.js"),
                                ),
                            ])]),
                        ),
                    ]),
                    manifest::Value::object(vec![("matcher", manifest::Value::string("Bash"))]),
                ]),
            )]),
        )]);
        assert!(hooks_shape_ok(&v));
    }

    #[test]
    fn hooks_shape_rejects_hooks_as_non_object() {
        let v = manifest::Value::object(vec![("hooks", manifest::Value::array(vec![]))]);
        assert!(!hooks_shape_ok(&v));
    }

    #[test]
    fn hooks_shape_rejects_post_tool_use_as_non_array() {
        let v = manifest::Value::object(vec![(
            "hooks",
            manifest::Value::object(vec![("PostToolUse", manifest::Value::string("nope"))]),
        )]);
        assert!(!hooks_shape_ok(&v));
    }

    #[test]
    fn hooks_shape_rejects_entry_with_wrong_typed_matcher() {
        let entries = manifest::Value::array(vec![manifest::Value::object(vec![(
            "matcher",
            manifest::Value::number(1),
        )])]);
        let v = manifest::Value::object(vec![(
            "hooks",
            manifest::Value::object(vec![("PostToolUse", entries)]),
        )]);
        assert!(!hooks_shape_ok(&v));
    }

    #[test]
    fn hooks_shape_rejects_entry_that_is_not_an_object() {
        let entry = manifest::Value::string("not an object");
        assert!(!entry_shape_ok(&entry));
    }

    // -- already_has_hook_command ----------------------------------------

    #[test]
    fn already_has_hook_command_matches_scout_plus_marker() {
        let entries = vec![manifest::Value::object(vec![(
            "hooks",
            manifest::Value::array(vec![manifest::Value::object(vec![(
                "command",
                manifest::Value::string("/abs/devscout hook read"),
            )])]),
        )])];
        assert!(already_has_hook_command(&entries, "hook read"));
        assert!(
            !already_has_hook_command(&entries, "hook bash"),
            "different marker must not match"
        );
    }

    #[test]
    fn already_has_hook_command_ignores_scout_without_the_marker() {
        // An existing hook whose command contains "scout" (a path segment) but
        // never the literal text "hook read"/"hook bash" -- must NOT be treated
        // as already-installed.
        let entries = vec![manifest::Value::object(vec![(
            "hooks",
            manifest::Value::array(vec![manifest::Value::object(vec![(
                "command",
                manifest::Value::string("node /opt/x/scout/scout-read-hook.js"),
            )])]),
        )])];
        assert!(!already_has_hook_command(&entries, "hook read"));
    }

    #[test]
    fn already_has_hook_command_false_on_empty_entries() {
        assert!(!already_has_hook_command(&[], "hook read"));
    }

    // -- merge_hooks -----------------------------------------------------

    #[test]
    fn merge_hooks_adds_both_entries_and_preserves_other_top_level_keys() {
        let v = manifest::Value::object(vec![
            ("model", manifest::Value::string("x")),
            ("hooks", manifest::Value::object(vec![])),
        ]);
        let (updated, added) = merge_hooks(v, "/abs/devscout");
        assert_eq!(added, vec!["Read", "Bash"]);
        assert_eq!(
            updated.get("model").and_then(manifest::Value::as_str),
            Some("x"),
            "unrelated top-level key preserved"
        );
        let ptu = updated.get("hooks").unwrap().get("PostToolUse").unwrap();
        assert!(matches!(ptu, manifest::Value::Array(items) if items.len() == 2));
    }

    #[test]
    fn merge_hooks_is_idempotent_on_second_call() {
        let v = manifest::Value::object(vec![("hooks", manifest::Value::object(vec![]))]);
        let (once, added1) = merge_hooks(v, "/abs/devscout");
        assert_eq!(added1, vec!["Read", "Bash"]);
        let (twice, added2) = merge_hooks(once.clone(), "/abs/devscout");
        assert!(
            added2.is_empty(),
            "second merge over its own output must add nothing"
        );
        assert_eq!(once, twice, "no-op merge must not alter the value at all");
    }

    #[test]
    fn merge_hooks_preserves_existing_post_tool_use_entries() {
        let existing = manifest::Value::array(vec![manifest::Value::object(vec![(
            "matcher",
            manifest::Value::string("Write"),
        )])]);
        let v = manifest::Value::object(vec![(
            "hooks",
            manifest::Value::object(vec![("PostToolUse", existing)]),
        )]);
        let (updated, added) = merge_hooks(v, "/abs/devscout");
        assert_eq!(added, vec!["Read", "Bash"]);
        let ptu = updated.get("hooks").unwrap().get("PostToolUse").unwrap();
        assert!(
            matches!(ptu, manifest::Value::Array(items) if items.len() == 3),
            "1 existing + 2 new"
        );
        if let manifest::Value::Array(items) = ptu {
            assert_eq!(
                items[0].get("matcher").and_then(manifest::Value::as_str),
                Some("Write"),
                "existing entry stays first, untouched"
            );
        }
    }

    // -- hook_snippet ------------------------------------------------------

    #[test]
    fn hook_snippet_is_valid_json_containing_both_subcommands() {
        let snippet = hook_snippet("/abs/devscout");
        let parsed: manifest::Value =
            serde_json::from_str(&snippet).expect("snippet must be valid JSON");
        let ptu = parsed.get("hooks").unwrap().get("PostToolUse").unwrap();
        assert!(matches!(ptu, manifest::Value::Array(items) if items.len() == 2));
        assert!(snippet.contains("/abs/devscout hook read"));
        assert!(snippet.contains("/abs/devscout hook bash"));
    }

    // -- now_stamp / backup_settings / write_settings ---------------------

    #[test]
    fn now_stamp_is_well_formed() {
        let s = now_stamp();
        // YYYYMMDD-HHMMSS-nnnnnnnnn
        assert_eq!(s.len(), 8 + 1 + 6 + 1 + 9);
        assert!(
            s.chars().enumerate().all(|(i, c)| if i == 8 || i == 15 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }),
            "got {s:?}"
        );
    }

    #[test]
    fn backup_settings_writes_original_bytes_verbatim() {
        let dir = temp_dir("backup");
        let path = dir.join("settings.json");
        let original = b"{\"a\":1}";
        let backup = backup_settings(&path, original).unwrap();
        assert!(backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("settings.json.bak."));
        assert_eq!(fs::read(&backup).unwrap(), original);
    }

    #[test]
    fn write_settings_then_read_round_trips_and_ends_with_newline() {
        let dir = temp_dir("write-settings");
        let path = dir.join("settings.json");
        let v = manifest::Value::object(vec![("a", manifest::Value::string("1"))]);
        write_settings(&path, &v).unwrap();
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.ends_with('\n'));
        let reread: manifest::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(reread, v);
    }

    // -- language tier sanity ----------------------------------------------

    #[test]
    fn census_language_tiers_are_explicit() {
        assert_eq!(FULLY_SUPPORTED_EXT, &[".cs"]);
        assert_eq!(INDEXED_AND_GRAPHED_EXT, &[".ts", ".tsx", ".js", ".jsx"]);
    }
}
