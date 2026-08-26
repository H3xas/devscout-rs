// scout-root/repo-root discovery, git common dir, worktree resolution, plus
// registry read.
//
// `git_dir_for`/`git_common_dir` deliberately read `.git`'s type and contents
// with single syscalls rather than a separate existence check followed by a
// read; a TOCTOU race that would otherwise surface as an error simply yields
// `None` here ("safer, not narrower").

use std::env;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

// ---------------------------------------------------------------------------
// Path resolution helpers -- lexical only, never touching symlinks (distinct
// from `fs::canonicalize`): resolve, dirname, and `.`/`..` normalization.
// ---------------------------------------------------------------------------

// Makes `path` absolute against the process cwd when relative, then lexically
// normalizes (`.`/`..` collapsed, no symlink resolution, cannot climb above the
// root). The result may name a path that does not exist; nothing here touches
// disk beyond `current_dir()`.
pub(crate) fn resolve_path(path: &Path) -> PathBuf {
    let base = if path.is_absolute() {
        path.to_path_buf()
    } else {
        // An unavailable cwd is effectively unreachable in practice, so this
        // falls back to `/` rather than propagating a nonexistent failure mode
        // into every caller's signature.
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    };
    normalize(&base)
}

// Lexical `.`/`..` collapse on an already-absolute input: a `..` that would
// climb above the root is a no-op (there is no path above `/`), which is what
// makes `resolve_path` a real resolve instead of a mere concatenation.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
                // else: already at the root (or empty) -- no-op, clamping at
                // the root.
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolves `path` against `base`: `path` alone when it is absolute, otherwise
/// `path` joined onto `base`; lexically normalized either way, with no symlink
/// resolution and no requirement that the result exist. `base` is expected
/// absolute (callers pass a process cwd or an already-resolved directory),
/// which is what lets this skip prefixing the cwd.
pub fn resolve_from(base: &Path, path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    normalize(&joined)
}

// Parent directory with a fixed point at the root (`dirname("/") == "/"`),
// unlike `Path::parent()` which returns `None` there. Preserving that fixed
// point is what lets `climb`'s loop-termination check (`parent == current`)
// terminate cleanly.
fn dirname(p: &Path) -> PathBuf {
    p.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| p.to_path_buf())
}

// ---------------------------------------------------------------------------
// Root discovery
// ---------------------------------------------------------------------------

// Walks `dir` and its ancestors for an entry literally named `marker`
// (existence does not distinguish file vs. directory, so a worktree's `.git`
// FILE matches exactly like a normal repo's `.git` DIRECTORY). Returns the
// first ancestor that has one, or `None` once the search reaches the filesystem
// root without a match.
fn climb(dir: &Path, marker: &str) -> Option<PathBuf> {
    let mut current = dir.to_path_buf();
    loop {
        if current.join(marker).exists() {
            return Some(current);
        }
        let parent = dirname(&current);
        if parent == current {
            return None;
        }
        current = parent;
    }
}

// Resolves `start_path` to an absolute path, then uses that path's containing
// directory unless the path itself is already a directory. A path that does not
// exist is treated the same as "not a directory": its dirname. `fs::metadata`
// follows symlinks (not `symlink_metadata`).
fn start_dir(start_path: &Path) -> PathBuf {
    let abs = resolve_path(start_path);
    match fs::metadata(&abs) {
        Ok(meta) if meta.is_dir() => abs,
        _ => dirname(&abs),
    }
}

/// Nearest ancestor of `start_path` containing a `.scout` entry, or `None`.
pub fn find_scout_root(start_path: &Path) -> Option<PathBuf> {
    climb(&start_dir(start_path), ".scout")
}

/// Nearest ancestor of `start_path` containing a `.git` entry, or `None`. This
/// matches on *any* `.git` entry regardless of type. Inside a linked worktree,
/// `<worktree>/.git` is a FILE (see `git_dir_for`), so this returns the
/// worktree's own directory, not the main repo root -- resolving through to the
/// shared root is `git_common_dir`'s job, not this one's.
pub fn find_repo_root(start_path: &Path) -> Option<PathBuf> {
    climb(&start_dir(start_path), ".git")
}

/// The `.scout` directory for a repo at `root` -- `root.join(".scout")`. Never fails.
pub fn scout_dir(root: &Path) -> PathBuf {
    root.join(".scout")
}

/// Repo-relative path: resolves both paths, slices `abs_path` from
/// `resolve(root).len() + 1`, and rejoins on `/`. No containment check: if
/// `abs_path` is not actually nested under `root`, the result is whatever that
/// slice produces (unhardened, unspecified). `.get()` (not raw indexing) is
/// used only to avoid a UTF-8 char-boundary panic on that not-nested case; a
/// well-formed call (`abs_path` under `root`) always slices at the ASCII
/// separator byte, and an out-of-range start yields `""`.
pub fn rel_path(root: &Path, abs_path: &Path) -> String {
    let root_s = resolve_path(root).to_string_lossy().into_owned();
    let abs_s = resolve_path(abs_path).to_string_lossy().into_owned();
    let start = root_s.len() + 1;
    let sliced = abs_s.get(start..).unwrap_or("");
    sliced
        .split(std::path::MAIN_SEPARATOR)
        .collect::<Vec<_>>()
        .join("/")
}

// ---------------------------------------------------------------------------
// Git dir resolution -- pure filesystem parsing. Nothing here shells out to
// `git`; `git_dir_for`/`git_common_dir` read `.git`'s type and contents
// directly. The one `git rev-parse` shell-out in this crate lives in
// manifest.rs, not here.
// ---------------------------------------------------------------------------

/// The git dir for a checkout at `root`: a plain directory `<root>/.git` for a
/// normal repo, or the `gitdir:` pointer parsed out of `<root>/.git` when it is
/// a FILE (linked worktree). `None` when `<root>/.git` does not exist at all, or
/// when a FILE exists but has no parseable `gitdir:` line / an empty target.
pub fn git_dir_for(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    let meta = fs::metadata(&dot_git).ok()?;
    if meta.is_dir() {
        return Some(resolve_path(&dot_git));
    }
    let contents = fs::read_to_string(&dot_git).ok()?;
    let line = contents
        .split('\n')
        .find(|l| l.trim().starts_with("gitdir:"))?;
    let target = line.trim()["gitdir:".len()..].trim();
    if target.is_empty() {
        return None;
    }
    Some(resolve_path(&root.join(target)))
}

/// The SHARED git dir every worktree of the repo has in common:
/// `git_dir_for(root)` unless that dir carries a `commondir` file (linked
/// worktree), in which case the file's contents (trimmed) are resolved relative
/// to the git dir. `None` when `root` has no git dir at all (delegates to
/// `git_dir_for`).
pub fn git_common_dir(root: &Path) -> Option<PathBuf> {
    let git_dir = git_dir_for(root)?;
    let common_file = git_dir.join("commondir");
    if !common_file.exists() {
        return Some(git_dir);
    }
    let target = fs::read_to_string(&common_file).ok()?;
    let target = target.trim();
    if target.is_empty() {
        return Some(git_dir);
    }
    Some(resolve_path(&git_dir.join(target)))
}

// ---------------------------------------------------------------------------
// Minimal JSON reader -- dependency-free, sufficient for the registry's fixed,
// small schema and not a general-purpose substitute for a real JSON crate (no
// streaming, no arbitrary-precision numbers). Only `Object`/`Array`/`String`
// are consumed by the registry reader today -- `Bool`/`Number`/`Null` exist
// for a JSON parser to be a JSON parser, not because a registry field needs
// them.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
mod json {
    #[derive(Debug, Clone, PartialEq)]
    pub enum Value {
        Null,
        Bool(bool),
        Number(f64),
        String(String),
        Array(Vec<Value>),
        Object(Vec<(String, Value)>),
    }

    pub fn parse(input: &str) -> Result<Value, String> {
        let chars: Vec<char> = input.chars().collect();
        let mut pos = 0usize;
        skip_ws(&chars, &mut pos);
        let value = parse_value(&chars, &mut pos)?;
        skip_ws(&chars, &mut pos);
        if pos != chars.len() {
            return Err(format!("unexpected trailing data at position {pos}"));
        }
        Ok(value)
    }

    fn skip_ws(chars: &[char], pos: &mut usize) {
        while matches!(chars.get(*pos), Some(' ' | '\t' | '\n' | '\r')) {
            *pos += 1;
        }
    }

    fn peek(chars: &[char], pos: usize) -> Result<char, String> {
        chars
            .get(pos)
            .copied()
            .ok_or_else(|| "unexpected end of input".to_string())
    }

    fn parse_value(chars: &[char], pos: &mut usize) -> Result<Value, String> {
        skip_ws(chars, pos);
        match peek(chars, *pos)? {
            '{' => parse_object(chars, pos),
            '[' => parse_array(chars, pos),
            '"' => parse_string(chars, pos).map(Value::String),
            't' => parse_literal(chars, pos, "true", Value::Bool(true)),
            'f' => parse_literal(chars, pos, "false", Value::Bool(false)),
            'n' => parse_literal(chars, pos, "null", Value::Null),
            c if c == '-' || c.is_ascii_digit() => parse_number(chars, pos),
            c => Err(format!("unexpected character '{c}' at {}", *pos)),
        }
    }

    fn parse_literal(
        chars: &[char],
        pos: &mut usize,
        lit: &str,
        value: Value,
    ) -> Result<Value, String> {
        for expected in lit.chars() {
            let c = peek(chars, *pos)?;
            if c != expected {
                return Err(format!("expected literal '{lit}' at {}", *pos));
            }
            *pos += 1;
        }
        Ok(value)
    }

    fn parse_object(chars: &[char], pos: &mut usize) -> Result<Value, String> {
        *pos += 1; // consume '{'
        let mut entries = Vec::new();
        skip_ws(chars, pos);
        if peek(chars, *pos)? == '}' {
            *pos += 1;
            return Ok(Value::Object(entries));
        }
        loop {
            skip_ws(chars, pos);
            if peek(chars, *pos)? != '"' {
                return Err(format!("expected string key at {}", *pos));
            }
            let key = parse_string(chars, pos)?;
            skip_ws(chars, pos);
            if peek(chars, *pos)? != ':' {
                return Err(format!("expected ':' at {}", *pos));
            }
            *pos += 1;
            let value = parse_value(chars, pos)?;
            entries.push((key, value));
            skip_ws(chars, pos);
            match peek(chars, *pos)? {
                ',' => *pos += 1,
                '}' => {
                    *pos += 1;
                    break;
                }
                c => return Err(format!("expected ',' or '}}' at {} (got '{c}')", *pos)),
            }
        }
        Ok(Value::Object(entries))
    }

    fn parse_array(chars: &[char], pos: &mut usize) -> Result<Value, String> {
        *pos += 1; // consume '['
        let mut items = Vec::new();
        skip_ws(chars, pos);
        if peek(chars, *pos)? == ']' {
            *pos += 1;
            return Ok(Value::Array(items));
        }
        loop {
            let value = parse_value(chars, pos)?;
            items.push(value);
            skip_ws(chars, pos);
            match peek(chars, *pos)? {
                ',' => *pos += 1,
                ']' => {
                    *pos += 1;
                    break;
                }
                c => return Err(format!("expected ',' or ']' at {} (got '{c}')", *pos)),
            }
        }
        Ok(Value::Array(items))
    }

    fn parse_string(chars: &[char], pos: &mut usize) -> Result<String, String> {
        *pos += 1; // consume opening quote
        let mut out = String::new();
        loop {
            let c = peek(chars, *pos)?;
            *pos += 1;
            match c {
                '"' => break,
                '\\' => {
                    let esc = peek(chars, *pos)?;
                    *pos += 1;
                    match esc {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let cp = parse_hex4(chars, pos)?;
                            if (0xD800..=0xDBFF).contains(&cp) {
                                if peek(chars, *pos)? == '\\' && chars.get(*pos + 1) == Some(&'u') {
                                    *pos += 2;
                                    let low = parse_hex4(chars, pos)?;
                                    if (0xDC00..=0xDFFF).contains(&low) {
                                        let c32 = 0x10000 + ((cp - 0xD800) << 10) + (low - 0xDC00);
                                        out.push(
                                            char::from_u32(c32).ok_or("invalid surrogate pair")?,
                                        );
                                    } else {
                                        return Err("invalid low surrogate".to_string());
                                    }
                                } else {
                                    return Err("unpaired high surrogate".to_string());
                                }
                            } else {
                                out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                            }
                        }
                        other => return Err(format!("invalid escape '\\{other}'")),
                    }
                }
                c if (c as u32) < 0x20 => return Err("control character in string".to_string()),
                c => out.push(c),
            }
        }
        Ok(out)
    }

    fn parse_hex4(chars: &[char], pos: &mut usize) -> Result<u32, String> {
        let mut value = 0u32;
        for _ in 0..4 {
            let c = peek(chars, *pos)?;
            let digit = c
                .to_digit(16)
                .ok_or_else(|| format!("invalid hex digit '{c}'"))?;
            value = value * 16 + digit;
            *pos += 1;
        }
        Ok(value)
    }

    fn parse_number(chars: &[char], pos: &mut usize) -> Result<Value, String> {
        let start = *pos;
        if peek(chars, *pos)? == '-' {
            *pos += 1;
        }
        while matches!(chars.get(*pos), Some(c) if c.is_ascii_digit()) {
            *pos += 1;
        }
        if matches!(chars.get(*pos), Some('.')) {
            *pos += 1;
            while matches!(chars.get(*pos), Some(c) if c.is_ascii_digit()) {
                *pos += 1;
            }
        }
        if matches!(chars.get(*pos), Some('e' | 'E')) {
            *pos += 1;
            if matches!(chars.get(*pos), Some('+' | '-')) {
                *pos += 1;
            }
            while matches!(chars.get(*pos), Some(c) if c.is_ascii_digit()) {
                *pos += 1;
            }
        }
        let text: String = chars[start..*pos].iter().collect();
        text.parse::<f64>()
            .map(Value::Number)
            .map_err(|e| format!("invalid number '{text}': {e}"))
    }
}

// ---------------------------------------------------------------------------
// Registry read path. The write side (registering roots, writing the registry,
// listing, pruning dead entries) is not implemented here.
// ---------------------------------------------------------------------------

/// One entry from the registry's `roots` array. The registry stores entries
/// with no shape validation on read, so a missing key is not an error: this
/// defaults a missing or wrong-typed field to its zero value rather than
/// rejecting the whole registry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RegistryEntry {
    /// The root value.
    pub root: String,
    /// The kind value.
    pub kind: String,
    /// The label value.
    pub label: Option<String>,
    /// The scope value.
    pub scope: Vec<String>,
    /// The initialized value.
    pub initialized: String,
    /// The last seen value.
    pub last_seen: String,
}

/// The parsed registry. Only the `roots` array is carried: the registry file
/// only ever holds `{roots}`, and no read-side API uses any other top-level
/// field.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Registry {
    /// The roots value.
    pub roots: Vec<RegistryEntry>,
}

/// The two ways reading the registry can fail, both reported as `registry at
/// <path> ...`.
#[derive(Debug)]
pub enum RegistryError {
    /// The registry file could not be read or is not valid JSON. Read failures
    /// (permission, or the file vanishing in a race after the existence check)
    /// are reported here too, not as a separate IO variant.
    NotValidJson {
        /// The registry path.
        path: PathBuf,
        /// The read or parsing error text.
        detail: String,
    },
    /// The parsed value is not an object, or its `roots` field is not an array.
    NoRootsArray {
        /// The registry path.
        path: PathBuf,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryError::NotValidJson { path, detail } => {
                write!(
                    f,
                    "registry at {} is not valid JSON: {detail}",
                    path.display()
                )
            }
            RegistryError::NoRootsArray { path } => {
                write!(f, "registry at {} has no \"roots\" array", path.display())
            }
        }
    }
}

impl std::error::Error for RegistryError {}

/// The registry file path: the `SCOUT_REGISTRY` env var when set, otherwise
/// `default_registry_path()`. Only an entirely *unset* var falls back; an
/// explicitly empty value is used verbatim.
///
/// There is no reliable way to derive an install root from wherever this binary
/// lives at runtime, so `default_registry_path` uses a runtime substitute (see
/// its doc comment). Every caller that cares about determinism already sets
/// `SCOUT_REGISTRY` explicitly, as this module's own tests do.
pub fn registry_path() -> PathBuf {
    match env::var("SCOUT_REGISTRY") {
        Ok(v) => PathBuf::from(v),
        Err(_) => default_registry_path(),
    }
}

// Derived from the runtime `$HOME` env var -- the same test seam `initcmd.rs`'s
// `settings_path` establishes -- joined with `.claude/scout/repos.json`, the
// conventional install location for this tool. This assumes the tool is
// installed at `~/.claude/scout`; an install elsewhere (e.g. `cargo install`
// dropping the binary in `~/.cargo/bin` with no `~/.claude/scout` beside it)
// will not find a registry there.
//
// `HOME` unset is not a hard error here the way it is in `settings_path` (a
// write path): this is read-only and `read_registry` already treats a
// missing/unreadable file as an empty registry, so an unresolvable path
// degrades gracefully to a bare relative `repos.json` (cwd-relative).
fn default_registry_path() -> PathBuf {
    match env::var("HOME") {
        Ok(home) => PathBuf::from(home)
            .join(".claude")
            .join("scout")
            .join("repos.json"),
        Err(_) => PathBuf::from("repos.json"),
    }
}

/// Reads the registry. A missing file reads as an empty registry, not an error
/// (absence and corruption are deliberately distinguished: silently losing
/// every configured scope would look exactly like a repo that never had one).
/// A present-but-corrupt file returns `Err(RegistryError)`.
pub fn read_registry() -> Result<Registry, RegistryError> {
    let path = registry_path();
    if !path.exists() {
        return Ok(Registry::default());
    }
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            return Err(RegistryError::NotValidJson {
                path,
                detail: e.to_string(),
            })
        }
    };
    let value = json::parse(&text).map_err(|detail| RegistryError::NotValidJson {
        path: path.clone(),
        detail,
    })?;
    let roots_value = match &value {
        json::Value::Object(fields) => fields.iter().find(|(k, _)| k == "roots").map(|(_, v)| v),
        _ => None,
    };
    match roots_value {
        Some(json::Value::Array(items)) => Ok(Registry {
            roots: items.iter().map(entry_from_json).collect(),
        }),
        _ => Err(RegistryError::NoRootsArray { path }),
    }
}

/// The registered entry for `root` (resolved to absolute first), or `Ok(None)`
/// if none is registered. Returns `Err(RegistryError)` if the registry itself
/// is corrupt (propagated straight from `read_registry`).
pub fn entry_for(root: &Path) -> Result<Option<RegistryEntry>, RegistryError> {
    let abs = resolve_path(root).to_string_lossy().into_owned();
    let registry = read_registry()?;
    Ok(registry.roots.into_iter().find(|entry| entry.root == abs))
}

fn entry_from_json(value: &json::Value) -> RegistryEntry {
    let fields: &[(String, json::Value)] = match value {
        json::Value::Object(f) => f.as_slice(),
        _ => &[],
    };
    let find = |key: &str| fields.iter().find(|(k, _)| k == key).map(|(_, v)| v);
    let as_string = |v: Option<&json::Value>| match v {
        Some(json::Value::String(s)) => Some(s.clone()),
        _ => None,
    };
    let scope = match find("scope") {
        Some(json::Value::Array(items)) => items
            .iter()
            .filter_map(|v| match v {
                json::Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    RegistryEntry {
        root: as_string(find("root")).unwrap_or_default(),
        kind: as_string(find("kind")).unwrap_or_default(),
        label: as_string(find("label")),
        scope,
        initialized: as_string(find("initialized")).unwrap_or_default(),
        last_seen: as_string(find("last_seen")).unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Unit tests -- pure-function coverage only (path lexical helpers, the JSON
// reader). Filesystem/git/worktree/registry-fixture coverage lives in the
// integration suite, which needs process-wide `SCOUT_REGISTRY` env mutation
// serialized behind a mutex -- kept out of this module so it never shares that
// mutex with an unrelated test binary.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as test_fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            env::temp_dir().join(format!("scout-repo-rs-{prefix}-{}-{n}", std::process::id()));
        test_fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    // -- resolve_path / normalize --------------------------------------

    #[test]
    fn resolve_path_collapses_dotdot_past_root() {
        assert_eq!(
            resolve_path(Path::new("/a/b/../../../c")),
            PathBuf::from("/c")
        );
    }

    #[test]
    fn resolve_path_is_idempotent_on_absolute_input() {
        let p = Path::new("/already/absolute/path");
        assert_eq!(resolve_path(p), PathBuf::from(p));
    }

    #[test]
    fn dirname_of_root_is_root() {
        assert_eq!(dirname(Path::new("/")), PathBuf::from("/"));
    }

    // -- climb ------------------------------------------------------------

    #[test]
    fn climb_finds_marker_in_ancestor() {
        let root = unique_temp_dir("climb-hit");
        test_fs::create_dir_all(root.join(".scout")).unwrap();
        test_fs::create_dir_all(root.join("src/deep")).unwrap();
        assert_eq!(climb(&root.join("src/deep"), ".scout"), Some(root));
    }

    #[test]
    fn climb_returns_none_without_a_match() {
        let root = unique_temp_dir("climb-miss");
        test_fs::create_dir_all(root.join("src/deep")).unwrap();
        assert_eq!(
            climb(&root.join("src/deep"), ".nonexistent-marker-xyz"),
            None
        );
    }

    // -- rel_path -----------------------------------------------------------

    #[test]
    fn rel_path_joins_with_forward_slash() {
        let root = unique_temp_dir("relpath");
        let abs = root.join("src").join("deep").join("f.ts");
        assert_eq!(rel_path(&root, &abs), "src/deep/f.ts");
    }

    #[test]
    fn rel_path_on_a_path_not_under_root_does_not_panic() {
        let root = unique_temp_dir("relpath-mismatch-a");
        let other = unique_temp_dir("relpath-mismatch-b");
        // Must not panic; content is documented as unhardened/unspecified.
        let _ = rel_path(&root, &other);
    }

    // -- git_dir_for / git_common_dir on a plain (non-git) dir --------------

    #[test]
    fn git_dir_for_none_without_dot_git() {
        let root = unique_temp_dir("nogit");
        assert_eq!(git_dir_for(&root), None);
        assert_eq!(git_common_dir(&root), None);
    }

    #[test]
    fn git_dir_for_a_plain_directory_dot_git() {
        let root = unique_temp_dir("plaingit");
        test_fs::create_dir_all(root.join(".git")).unwrap();
        assert_eq!(git_dir_for(&root), Some(root.join(".git")));
        assert_eq!(git_common_dir(&root), Some(root.join(".git")));
    }

    #[test]
    fn git_dir_for_parses_gitdir_pointer_file() {
        let root = unique_temp_dir("ptrgit");
        let target = unique_temp_dir("ptrgit-target");
        test_fs::write(root.join(".git"), format!("gitdir: {}\n", target.display())).unwrap();
        assert_eq!(git_dir_for(&root), Some(target));
    }

    #[test]
    fn git_common_dir_follows_commondir_file() {
        let root = unique_temp_dir("commondir");
        let admin = root.join(".git-admin");
        test_fs::create_dir_all(&admin).unwrap();
        test_fs::write(root.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();
        let shared = unique_temp_dir("commondir-shared");
        test_fs::write(admin.join("commondir"), format!("{}\n", shared.display())).unwrap();
        assert_eq!(git_common_dir(&root), Some(shared));
    }

    // -- JSON reader ----------------------------------------------------

    #[test]
    fn json_parses_object_array_string_number_bool_null() {
        let v = json::parse(r#"{"a": [1, -2.5, "s", true, false, null]}"#).unwrap();
        match v {
            json::Value::Object(fields) => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].0, "a");
                match &fields[0].1 {
                    json::Value::Array(items) => {
                        assert_eq!(items.len(), 6);
                        assert_eq!(items[0], json::Value::Number(1.0));
                        assert_eq!(items[1], json::Value::Number(-2.5));
                        assert_eq!(items[2], json::Value::String("s".to_string()));
                        assert_eq!(items[3], json::Value::Bool(true));
                        assert_eq!(items[4], json::Value::Bool(false));
                        assert_eq!(items[5], json::Value::Null);
                    }
                    other => panic!("expected array, got {other:?}"),
                }
            }
            other => panic!("expected object, got {other:?}"),
        }
    }

    #[test]
    fn json_parses_unicode_escape_and_surrogate_pair() {
        // é -> 'e' with acute accent; the astral emoji requires a
        // surrogate pair (😀 -> U+1F600).
        let v = json::parse(r#""café 😀""#).unwrap();
        assert_eq!(v, json::Value::String("caf\u{e9} \u{1F600}".to_string()));
    }

    #[test]
    fn json_rejects_malformed_input() {
        assert!(json::parse("{ not json").is_err());
        assert!(json::parse("").is_err());
        assert!(json::parse("{\"a\": }").is_err());
    }

    #[test]
    fn entry_from_json_defaults_missing_fields() {
        let v = json::parse(r#"{"root": "/x", "kind": "git"}"#).unwrap();
        let entry = entry_from_json(&v);
        assert_eq!(
            entry,
            RegistryEntry {
                root: "/x".to_string(),
                kind: "git".to_string(),
                label: None,
                scope: vec![],
                initialized: String::new(),
                last_seen: String::new(),
            }
        );
    }

    #[test]
    fn entry_from_json_reads_a_full_entry() {
        let v = json::parse(
            r#"{"root": "/x", "kind": "git", "label": "lbl", "scope": ["a", "b"], "initialized": "2026-01-01", "last_seen": "2026-01-02"}"#,
        )
        .unwrap();
        let entry = entry_from_json(&v);
        assert_eq!(entry.root, "/x");
        assert_eq!(entry.label, Some("lbl".to_string()));
        assert_eq!(entry.scope, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(entry.initialized, "2026-01-01");
        assert_eq!(entry.last_seen, "2026-01-02");
    }
}
