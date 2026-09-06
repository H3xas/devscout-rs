// The TS/TSX resolver: turns many files' reference fragments (see
// `extract::extract_ts_fragment`) into resolved edges.
//
// The split mirrors the C# side exactly: the extractor records what one file
// SAYS, this module decides what those words point at once every file is known.
// What it does NOT do is deliberately bounded -- no node_modules-internal
// resolution, no `.d.ts` / type-level analysis, and no resolution through a
// chained method call, which is the same open gap the C# side still carries.
//
// Everything here is pure over the fragment map except one read: a tsconfig
// chain, which is where a bare specifier's meaning is written down. A
// monorepo writes that meaning down more than once -- an app folder owns its
// own aliases below the root's -- so the read is scoped PER DIRECTORY: a file
// resolves through the nearest ancestor `tsconfig.json` above it, falling
// back to the repo root's own chain when no ancestor owns one, or when the
// one that does declares neither `paths` nor `baseUrl` of its own (see
// `read_ts_alias_scopes`).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::extract::TsFragment;
use crate::graph::{Def, Edge, OrderedMap};

const DEF_SEPARATOR: char = '#';
// Extension probe order for a specifier that names no extension. TypeScript
// sources lead so a repo carrying both `x.ts` and a built `x.js` resolves to
// the source, which is the file an agent asking "who calls this" wants.
const PROBE_EXTENSIONS: &[&str] = &[".ts", ".tsx", ".js", ".jsx"];
// ESM spells a relative import of a TypeScript module with the extension of
// what it COMPILES to. The mapping is the TypeScript compiler's own, applied
// only after the literal path has already failed to match a mapped file.
const OUTPUT_EXTENSION_SOURCES: &[(&str, &[&str])] = &[
    (".js", &[".ts", ".tsx"]),
    (".jsx", &[".tsx"]),
    (".mjs", &[".mts", ".ts"]),
    (".cjs", &[".cts", ".ts"]),
];
const TSCONFIG_CHAIN_LIMIT: usize = 4;
// Where a repo writes down what its bare specifiers mean. `tsconfig.json` is
// the default entry point; a workspace whose root config is generated per-app
// keeps the shared aliases in `tsconfig.base.json` instead, with nothing at
// the default name at all -- so both are tried, and the one that actually
// declares `paths` wins rather than whichever happens to exist.
const TSCONFIG_ROOTS: &[&str] = &["tsconfig.json", "tsconfig.base.json"];
// A directory below the root can own its OWN `tsconfig.json`, scoping the
// files beneath it to that config's chain instead of the root's -- the shape
// a monorepo's per-app aliases actually take. Only the default name is
// recognised as a nested owner (never `tsconfig.base.json`): a nested config
// is written by the app itself and names itself the ordinary way, unlike the
// root, which sometimes has nothing at the default name at all.
// Feature folders nest their index files two to four barrels deep, so one hop
// of following was not enough: a name a file re-exports is looked up in the
// module it re-exports FROM, which may itself re-export it again, up to this
// many times before the walk gives up. The bound is what keeps a cyclic
// `export *` pair (`a` re-exporting `b`, `b` re-exporting `a`) from recursing
// forever even before the visited-pair guard in `lookup_export` catches the
// cycle outright.
const BARREL_HOPS: usize = 8;

/// Builds a TypeScript definition identifier from a file and symbol name.
pub fn ts_def_id(file: &str, name: &str) -> String {
    format!("{file}{DEF_SEPARATOR}{name}")
}

fn dir_of(rel: &str) -> &str {
    match rel.rfind('/') {
        None => "",
        Some(i) => &rel[..i],
    }
}

// `/`-joined, repo-relative, no filesystem access: a `..` that would climb
// above the repo root returns `None` rather than an absolute escape.
fn join_rel(base: &str, rest: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    let joined = format!("{base}/{rest}");
    for seg in joined.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            if parts.is_empty() {
                return None;
            }
            parts.pop();
            continue;
        }
        parts.push(seg);
    }
    Some(parts.join("/"))
}

fn extension_of(rel: &str) -> &str {
    let slash = rel.rfind('/').map(|i| i as i64).unwrap_or(-1);
    let dot = rel.rfind('.').map(|i| i as i64).unwrap_or(-1);
    if dot > slash {
        &rel[dot as usize..]
    } else {
        ""
    }
}

// A resolved specifier is a file THIS INDEX MAPPED, never merely a path that
// exists: an edge to a file the graph does not carry would point a caller at a
// row that is not there. Probe order is fixed so the choice among several
// candidate spellings is deterministic.
fn probe_file(base: Option<&str>, file_set: &HashSet<&str>) -> Option<String> {
    let base = base?;
    if file_set.contains(base) {
        return Some(base.to_string());
    }
    let ext = extension_of(base);
    if let Some((_, sources)) = OUTPUT_EXTENSION_SOURCES.iter().find(|(e, _)| *e == ext) {
        let stem = &base[..base.len() - ext.len()];
        for s in *sources {
            let candidate = format!("{stem}{s}");
            if file_set.contains(candidate.as_str()) {
                return Some(candidate);
            }
        }
    }
    for e in PROBE_EXTENSIONS {
        let candidate = format!("{base}{e}");
        if file_set.contains(candidate.as_str()) {
            return Some(candidate);
        }
    }
    for e in PROBE_EXTENSIONS {
        let candidate = format!("{base}/index{e}");
        if file_set.contains(candidate.as_str()) {
            return Some(candidate);
        }
    }
    None
}

// JSON with comments and trailing commas -- the spelling every real tsconfig
// uses. Returns `None` on anything it cannot parse; a tsconfig this module
// cannot read means bare specifiers stay external, never that they get guessed.
// Two deliberate crudenesses: the trailing-comma pass is NOT string-aware (a
// plain scan over the comment-stripped text), and it does not rescan its own
// output.
fn strip_jsonc(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    let mut in_string = false;
    while i < chars.len() {
        let ch = chars[i];
        if in_string {
            out.push(ch);
            if ch == '\\' {
                if let Some(next) = chars.get(i + 1) {
                    out.push(*next);
                }
                i += 2;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if ch == '"' {
            in_string = true;
            out.push(ch);
            i += 1;
            continue;
        }
        if ch == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if ch == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    drop_trailing_commas(&out)
}

// Drops a comma that precedes a `}` or `]` (with optional whitespace between):
// one left-to-right pass, each match consuming the comma AND the
// whitespace-plus-closer that followed it, so the scan resumes past the closer
// and never rescans its own replacement.
fn drop_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != ',' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        if j < chars.len() && (chars[j] == '}' || chars[j] == ']') {
            for &c in &chars[i + 1..=j] {
                out.push(c);
            }
            i = j + 1;
            continue;
        }
        out.push(',');
        i += 1;
    }
    out
}

// `paths` is read through `OrderedMap` so the aliases are built in the
// document's own key order (serde_json's default `Map` sorts, which would
// reorder a length tie); the untagged fallback treats a non-object `paths` as
// "no paths" rather than failing the whole read of the tsconfig.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PathsField {
    Map(OrderedMap<Value>),
    Other(#[allow(dead_code)] Value),
}

#[derive(Debug, Default, Deserialize)]
struct CompilerOptions {
    #[serde(default, rename = "baseUrl")]
    base_url: Option<Value>,
    #[serde(default)]
    paths: Option<PathsField>,
}

#[derive(Debug, Default, Deserialize)]
struct TsConfigDoc {
    #[serde(default)]
    extends: Option<Value>,
    #[serde(default, rename = "compilerOptions")]
    compiler_options: Option<CompilerOptions>,
}

impl TsConfigDoc {
    fn base_url(&self) -> Option<&str> {
        self.compiler_options.as_ref()?.base_url.as_ref()?.as_str()
    }

    // The guard used when picking which config OWNS the aliases: a `paths` that
    // is present and an object.
    fn paths(&self) -> Option<&OrderedMap<Value>> {
        match self.compiler_options.as_ref()?.paths.as_ref()? {
            PathsField::Map(m) => Some(m),
            PathsField::Other(_) => None,
        }
    }

    // A bare TRUTHINESS test on `compilerOptions.paths` -- the weaker guard the
    // chain-selection loop uses, deliberately kept distinct from `paths()` above
    // so a config declaring a non-object `paths` still wins the chain and still
    // contributes no alias.
    fn declares_paths(&self) -> bool {
        match self
            .compiler_options
            .as_ref()
            .and_then(|o| o.paths.as_ref())
        {
            None => false,
            Some(PathsField::Map(_)) => true,
            Some(PathsField::Other(v)) => match v {
                Value::Bool(b) => *b,
                Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
                Value::String(str_value) => !str_value.is_empty(),
                Value::Null => false,
                _ => true,
            },
        }
    }
}

struct ChainLink {
    dir: String,
    cfg: TsConfigDoc,
}

fn read_tsconfig_chain(root: &Path, start: &str) -> Vec<ChainLink> {
    let mut chain: Vec<ChainLink> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut rel: Option<String> = Some(start.to_string());
    for _ in 0..TSCONFIG_CHAIN_LIMIT {
        let Some(current) = rel.clone() else { break };
        if !seen.insert(current.clone()) {
            break;
        }
        let path = root.join(&current);
        if !path.exists() {
            break;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            break;
        };
        let stripped = strip_jsonc(&text);
        // A parse failure means "cannot read this config" and stops the chain. A
        // config that parses but is not an object contributes no option at all,
        // which is what `TsConfigDoc::default()` is.
        let Ok(value) = serde_json::from_str::<Value>(&stripped) else {
            break;
        };
        let cfg = if value.is_object() {
            serde_json::from_str::<TsConfigDoc>(&stripped).unwrap_or_default()
        } else {
            TsConfigDoc::default()
        };
        let dir = dir_of(&current).to_string();
        // Only a RELATIVE extends is followed: a bare one names a package inside
        // node_modules, which this resolver deliberately does not resolve into.
        rel = match cfg.extends.as_ref().and_then(Value::as_str) {
            Some(ext) if ext.starts_with('.') => {
                let spelled = if ext.ends_with(".json") {
                    ext.to_string()
                } else {
                    format!("{ext}.json")
                };
                join_rel(&dir, &spelled)
            }
            _ => None,
        };
        chain.push(ChainLink { dir, cfg });
    }
    chain
}

#[derive(Debug, Clone)]
struct AliasTarget {
    prefix: Option<String>,
    suffix: String,
}

#[derive(Debug, Clone)]
struct Alias {
    exact: Option<String>,
    prefix: String,
    suffix: String,
    targets: Vec<AliasTarget>,
    wildcard: bool,
}

#[derive(Debug, Clone, Default)]
/// Represents `TsPathAliases`.
pub struct TsPathAliases {
    aliases: Vec<Alias>,
    base_url_dir: Option<String>,
}

#[derive(Debug, Clone, Default)]
/// A repo's tsconfig alias reading, scoped per directory.
///
/// `root` is the repo-root chain (see `read_ts_path_aliases`), and `nested`
/// is every directory below it that owns a `tsconfig.json` declaring `paths`
/// or `baseUrl` of its own, longest directory first so the nearest ancestor
/// is always checked before a shallower one. Built by `read_ts_alias_scopes`.
pub struct TsAliasScopes {
    root: TsPathAliases,
    nested: Vec<(String, TsPathAliases)>,
}

/// Reads the repo-root tsconfig chain into its aliases.
///
/// `paths` and `baseUrl` are whole-property overrides in TypeScript's own
/// inheritance rule, not merged key by key -- the nearest config that declares
/// one wins it outright. `paths` entries resolve against that config's own
/// baseUrl when it has one, else against the directory the config sits in
/// (TypeScript 4.1's rule).
pub fn read_ts_path_aliases(root: &Path) -> TsPathAliases {
    let mut chain: Vec<ChainLink> = Vec::new();
    for start in TSCONFIG_ROOTS {
        let candidate = read_tsconfig_chain(root, start);
        if candidate.is_empty() {
            continue;
        }
        let declares_paths = candidate.iter().any(|c| c.cfg.declares_paths());
        if chain.is_empty() || declares_paths {
            chain = candidate;
        }
        if declares_paths {
            break;
        }
    }
    aliases_from_chain(&chain)
}

// The part of alias reading shared by the root chain and every nested one:
// once a chain is picked, decide which link in it owns `paths` and which owns
// `baseUrl` (independently -- each is its own whole-property override) and
// resolve every pattern's targets against that.
fn aliases_from_chain(chain: &[ChainLink]) -> TsPathAliases {
    let paths_from = chain.iter().find(|c| c.cfg.paths().is_some());
    let base_url_from = chain.iter().find(|c| c.cfg.base_url().is_some());
    let base_url_dir = base_url_from.and_then(|c| join_rel(&c.dir, c.cfg.base_url().unwrap_or("")));
    let mut aliases: Vec<Alias> = Vec::new();
    if let Some(pf) = paths_from {
        let base: Option<String> = match pf.cfg.base_url() {
            Some(own) => join_rel(&pf.dir, own),
            None => base_url_dir.clone().or_else(|| Some(pf.dir.clone())),
        };
        for (pattern, targets) in pf.cfg.paths().expect("paths_from declares paths").iter() {
            let Some(targets) = targets.as_array() else {
                continue;
            };
            let resolved: Vec<AliasTarget> = targets
                .iter()
                .filter_map(Value::as_str)
                .map(|t| match t.find('*') {
                    None => AliasTarget {
                        prefix: join_rel(base.as_deref().unwrap_or(""), t),
                        suffix: String::new(),
                    },
                    Some(star) => AliasTarget {
                        prefix: Some(
                            join_rel(base.as_deref().unwrap_or(""), &t[..star]).unwrap_or_default(),
                        ),
                        suffix: t[star + 1..].to_string(),
                    },
                })
                .collect();
            aliases.push(match pattern.find('*') {
                None => Alias {
                    exact: Some(pattern.clone()),
                    prefix: pattern.clone(),
                    suffix: String::new(),
                    targets: resolved,
                    wildcard: false,
                },
                Some(star) => Alias {
                    exact: None,
                    prefix: pattern[..star].to_string(),
                    suffix: pattern[star + 1..].to_string(),
                    targets: resolved,
                    wildcard: true,
                },
            });
        }
    }
    // Longest prefix wins, TypeScript's own tie-break; the name breaks a length
    // tie so the order is total and identical on every run. `sort_by` is stable,
    // matching V8's own stable `Array.prototype.sort`.
    aliases.sort_by(|a, b| {
        b.prefix
            .len()
            .cmp(&a.prefix.len())
            .then_with(|| a.prefix.cmp(&b.prefix))
    });
    TsPathAliases {
        aliases,
        base_url_dir,
    }
}

// Walks a directory's own ancestors toward the repo root -- itself first, then
// its parent, and so on, stopping before the empty root directory -- for the
// nearest one that owns a `tsconfig.json` DECLARING `paths` or `baseUrl`. A
// config that declares neither is stepped over, never stopped at: the walk
// continues to the next owner above it, so the answer for a directory is a
// function of its own ancestors alone, never of which other files the repo
// happens to carry. Memoized per directory (the owner) and per config (its
// aliases) so two files under the same app cost one filesystem check between
// them rather than one each.
fn owning_tsconfig_dir(
    root: &Path,
    dir: &str,
    owner_cache: &mut HashMap<String, Option<String>>,
    alias_cache: &mut HashMap<String, Option<TsPathAliases>>,
) -> Option<String> {
    if dir.is_empty() {
        return None;
    }
    if let Some(hit) = owner_cache.get(dir) {
        return hit.clone();
    }
    let own = alias_cache
        .entry(dir.to_string())
        .or_insert_with(|| {
            let config = root.join(dir).join("tsconfig.json");
            if !config.is_file() {
                return None;
            }
            let chain = read_tsconfig_chain(root, &format!("{dir}/tsconfig.json"));
            let aliases = aliases_from_chain(&chain);
            let owns_something = !aliases.aliases.is_empty() || aliases.base_url_dir.is_some();
            owns_something.then_some(aliases)
        })
        .is_some();
    let answer = if own {
        Some(dir.to_string())
    } else {
        owning_tsconfig_dir(root, dir_of(dir), owner_cache, alias_cache)
    };
    owner_cache.insert(dir.to_string(), answer.clone());
    answer
}

/// Reads the root chain once, then finds each distinct directory `files`
/// names an owner for: the nearest ancestor `tsconfig.json` above it that
/// declares `paths` or `baseUrl` of its own.
///
/// A nested config that declares neither is stepped over -- a silent config
/// must not switch the workspace's own aliases off for the files beneath it --
/// so those files resolve through the next owner above it, and through `root`
/// when there is none, exactly as if the silent config did not exist.
pub fn read_ts_alias_scopes<'a>(
    root: &Path,
    files: impl IntoIterator<Item = &'a str>,
) -> TsAliasScopes {
    let root_aliases = read_ts_path_aliases(root);
    let mut owner_cache: HashMap<String, Option<String>> = HashMap::new();
    let mut alias_cache: HashMap<String, Option<TsPathAliases>> = HashMap::new();
    let mut owners: HashSet<String> = HashSet::new();
    for file in files {
        if let Some(owner) =
            owning_tsconfig_dir(root, dir_of(file), &mut owner_cache, &mut alias_cache)
        {
            owners.insert(owner);
        }
    }
    let mut nested: Vec<(String, TsPathAliases)> = owners
        .into_iter()
        .filter_map(|dir| {
            let aliases = alias_cache.remove(&dir).flatten()?;
            Some((dir, aliases))
        })
        .collect();
    // Longest directory first, ties broken by name for a total, deterministic
    // order: `scope_for` below returns the first match, so the nearest
    // ancestor must be checked before a shallower one that also prefixes the
    // file.
    nested.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
    TsAliasScopes {
        root: root_aliases,
        nested,
    }
}

// The scope a specifier in `from_file` resolves against: its nearest owning
// nested config, or `root` when none owns it. `dir` prefixes `from_file` only
// when the next character is a path separator, so `apps/web2` never matches
// an owner directory `apps/web`.
fn scope_for<'a>(from_file: &str, scopes: &'a TsAliasScopes) -> &'a TsPathAliases {
    for (dir, aliases) in &scopes.nested {
        if from_file.len() > dir.len()
            && from_file.starts_with(dir.as_str())
            && from_file.as_bytes()[dir.len()] == b'/'
        {
            return aliases;
        }
    }
    &scopes.root
}

fn alias_targets(spec: &str, aliases: &[Alias]) -> Vec<Option<String>> {
    for a in aliases {
        if !a.wildcard {
            if a.exact.as_deref() == Some(spec) {
                return a.targets.iter().map(|t| t.prefix.clone()).collect();
            }
            continue;
        }
        // A length guard (`spec.len() < prefix.len() + suffix.len()`), then a
        // prefix and suffix match. Stripping the prefix first and the suffix off
        // what REMAINS expresses the same three conditions in one pass -- an
        // overlapping prefix/suffix pair fails the second strip exactly where the
        // length guard would reject it.
        let Some(rest) = spec.strip_prefix(a.prefix.as_str()) else {
            continue;
        };
        let Some(mid) = rest.strip_suffix(a.suffix.as_str()) else {
            continue;
        };
        return a
            .targets
            .iter()
            .map(|t| {
                let rest = if t.suffix.is_empty() {
                    mid.to_string()
                } else {
                    format!("{mid}{}", t.suffix)
                };
                t.prefix.as_deref().and_then(|p| join_rel(p, &rest))
            })
            .collect();
    }
    Vec::new()
}

/// Relative specifiers resolve directly; a bare one resolves only as far as
/// the repo's own tsconfig paths/baseUrl already take it -- the nearest
/// owning scope to `from_file`, root when none owns it.
///
/// Anything else is `external` -- the TS-side counterpart of the C# graph's
/// cross-project handling, and never a guess at a package's internals.
pub fn resolve_specifier(
    from_file: &str,
    spec: &str,
    file_set: &HashSet<&str>,
    alias: &TsAliasScopes,
) -> Option<String> {
    if spec.starts_with('.') {
        return probe_file(join_rel(dir_of(from_file), spec).as_deref(), file_set);
    }
    let scope = scope_for(from_file, alias);
    for candidate in alias_targets(spec, &scope.aliases) {
        if let Some(hit) = probe_file(candidate.as_deref(), file_set) {
            return Some(hit);
        }
    }
    if let Some(base) = &scope.base_url_dir {
        if let Some(hit) = probe_file(join_rel(base, spec).as_deref(), file_set) {
            return Some(hit);
        }
    }
    None
}

/// The four TS edge kinds' counts, appended to `stats.edges_by_kind` in this
/// order and only when the repo carries a TS fragment at all.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TsEdgeCounts {
    /// The import value.
    pub import: usize,
    /// The call value.
    pub call: usize,
    /// The jsx use value.
    pub jsx_use: usize,
    /// The dispatch value.
    pub dispatch: usize,
}

/// `stats.ts` -- appended inside `stats` after `test_def_count`, and only when
/// the repo carries a TS fragment at all. `heuristic_by_tier` is the newer
/// fact and follows it (see `graph::Stats`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TsStats {
    /// The ts file count value.
    pub ts_file_count: usize,
    /// The ts def count value.
    pub ts_def_count: usize,
    /// The external import count value.
    pub external_import_count: usize,
    /// The unresolved ref count value.
    pub unresolved_ref_count: usize,
}

/// Represents `TsGraph`.
pub struct TsGraph {
    /// The defs value.
    pub defs: Vec<Def>,
    /// The edges value.
    pub edges: Vec<Edge>,
    /// The edges by kind value.
    pub edges_by_kind: TsEdgeCounts,
    /// The stats value.
    pub stats: TsStats,
}

struct ResolvedReexport {
    spec: String,
    line: usize,
    star: bool,
    names: Vec<(String, String)>,
    to: Option<String>,
}

/// Resolves the TS/TSX graph: every file's reference fragments into edges.
pub fn resolve_ts_graph(fragments: &[(String, TsFragment)], alias: &TsAliasScopes) -> TsGraph {
    let file_set: HashSet<&str> = fragments.iter().map(|(f, _)| f.as_str()).collect();

    let mut defs: Vec<Def> = Vec::new();
    let mut exports_by_file: HashMap<&str, HashMap<String, usize>> = HashMap::new();
    for (file, frag) in fragments {
        let mut by_name: HashMap<String, usize> = HashMap::new();
        for d in &frag.defs {
            if by_name.contains_key(&d.name) {
                continue;
            }
            by_name.insert(d.name.clone(), defs.len());
            defs.push(Def {
                id: ts_def_id(file, &d.name),
                name: d.name.clone(),
                namespace: dir_of(file).to_string(),
                kind: d.kind.clone(),
                file: file.clone(),
                line: d.line,
                methods: Vec::new(),
                test_methods: Vec::new(),
                also_in: Vec::new(),
                end_line: d.end_line,
            });
        }
        // `export default Foo` makes the SAME declaration importable under the
        // name `default` -- one def, two names, never a second row on one
        // declaration.
        if let Some(dflt) = &frag.default {
            if let Some(&i) = by_name.get(dflt) {
                by_name.insert("default".to_string(), i);
            }
        }
        exports_by_file.insert(file.as_str(), by_name);
    }

    let mut reexports_by_file: HashMap<&str, Vec<ResolvedReexport>> = HashMap::new();
    for (file, frag) in fragments {
        let rows = frag
            .reexports
            .iter()
            .map(|rx| ResolvedReexport {
                spec: rx.spec.clone(),
                line: rx.line,
                star: rx.star,
                names: rx
                    .names
                    .iter()
                    .map(|n| (n.exported.clone(), n.imported.clone()))
                    .collect(),
                to: resolve_specifier(file, &rx.spec, &file_set, alias),
            })
            .collect();
        reexports_by_file.insert(file.as_str(), rows);
    }

    // `visited` is one top-level lookup's own map, fresh at every call site
    // below (never shared across bindings or refs): a `(file, name)` pair
    // revisited mid-walk means a cyclic `export *` pair sent the walk back to
    // a state it is already computing, so it fails closed to `None` right
    // there rather than recursing again. That guard also keeps a single
    // `export *` fan-out linear in files times names. What the map REMEMBERS
    // is the shallowest hop count the pair was walked from: a pair first met
    // deep in the walk, where the cap cut its own re-exports short, is walked
    // again when a shorter path reaches it with more hops to spend -- and
    // never again from the same depth or deeper.
    fn lookup_export(
        file: &str,
        name: &str,
        hops: usize,
        exports_by_file: &HashMap<&str, HashMap<String, usize>>,
        reexports_by_file: &HashMap<&str, Vec<ResolvedReexport>>,
        visited: &mut HashMap<(String, String), usize>,
    ) -> Option<usize> {
        let key = (file.to_string(), name.to_string());
        if visited.get(&key).is_some_and(|&seen| seen <= hops) {
            return None;
        }
        visited.insert(key, hops);
        // Own declarations are checked before the hop cap is enforced, so a
        // chain of exactly `BARREL_HOPS` barrels followed by a declaring file
        // still resolves -- the cap bounds how many re-export hops are
        // FOLLOWED, not how many files may be visited in total.
        if let Some(own) = exports_by_file.get(file).and_then(|m| m.get(name)) {
            return Some(*own);
        }
        if hops >= BARREL_HOPS {
            return None;
        }
        for rx in reexports_by_file
            .get(file)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let Some(to) = &rx.to else { continue };
            if rx.star {
                if let Some(hit) = lookup_export(
                    to,
                    name,
                    hops + 1,
                    exports_by_file,
                    reexports_by_file,
                    visited,
                ) {
                    return Some(hit);
                }
                continue;
            }
            // Declaration order is the tie-break: the first re-export naming
            // this export wins, matching the first-occurrence rule the def
            // map itself uses.
            for (exported, imported) in &rx.names {
                if exported != name {
                    continue;
                }
                if let Some(hit) = lookup_export(
                    to,
                    imported,
                    hops + 1,
                    exports_by_file,
                    reexports_by_file,
                    visited,
                ) {
                    return Some(hit);
                }
            }
        }
        None
    }

    let mut edges: Vec<Edge> = Vec::new();
    let mut counts = TsEdgeCounts::default();
    let mut external_imports = 0usize;
    let mut unresolved_refs = 0usize;

    for (file, frag) in fragments {
        let mut bindings: HashMap<String, (String, String)> = HashMap::new();
        for imp in &frag.imports {
            let to = resolve_specifier(file, &imp.spec, &file_set, alias);
            let Some(to) = to else {
                external_imports += 1;
                continue;
            };
            edges.push(Edge::Import {
                from_file: file.clone(),
                from_line: imp.line,
                target: imp.spec.clone(),
                to_file: to.clone(),
                via: None,
            });
            counts.import += 1;
            // First binding of a local name wins, the same first-occurrence
            // rule the def map above uses: a file that binds one name twice is
            // already broken TypeScript, and picking the later one would make
            // the answer depend on statement order for no gain.
            for b in &imp.bindings {
                bindings
                    .entry(b.local.clone())
                    .or_insert((to.clone(), b.imported.clone()));
            }
            // A barrel is a routing table, not a dependency: `import
            // { useThing } from '../hooks'` names index.ts but DEPENDS on the
            // module index.ts re-exports it from. Without this edge, `impact`
            // on the declaring file misses every consumer that came through
            // the barrel. The edge to the named file stays (it is what the
            // source literally says); this one carries `via` so a reader can
            // tell the two apart, and one line contributes at most one edge per
            // distinct declaring file however many names it pulls through.
            let mut followed: HashSet<String> = HashSet::new();
            for b in &imp.bindings {
                if b.imported == "*" {
                    continue;
                }
                let mut visited: HashMap<(String, String), usize> = HashMap::new();
                let Some(target) = lookup_export(
                    &to,
                    &b.imported,
                    0,
                    &exports_by_file,
                    &reexports_by_file,
                    &mut visited,
                ) else {
                    continue;
                };
                let target_file = defs[target].file.clone();
                if target_file == to || followed.contains(&target_file) {
                    continue;
                }
                followed.insert(target_file.clone());
                edges.push(Edge::Import {
                    from_file: file.clone(),
                    from_line: imp.line,
                    target: imp.spec.clone(),
                    to_file: target_file,
                    via: Some(to.clone()),
                });
                counts.import += 1;
            }
        }
        for rx in reexports_by_file
            .get(file.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            match &rx.to {
                None => external_imports += 1,
                Some(to) => {
                    edges.push(Edge::Import {
                        from_file: file.clone(),
                        from_line: rx.line,
                        target: rx.spec.clone(),
                        to_file: to.clone(),
                        via: None,
                    });
                    counts.import += 1;
                }
            }
        }

        for r in &frag.refs {
            let target = match bindings.get(&r.name) {
                // A namespace binding (`* as ns`, or a whole-module `require`)
                // carries no name of its own -- only `ns.member` says which
                // export is meant.
                Some((to, imported)) if imported == "*" => match &r.member {
                    Some(m) => {
                        let mut visited: HashMap<(String, String), usize> = HashMap::new();
                        lookup_export(to, m, 0, &exports_by_file, &reexports_by_file, &mut visited)
                    }
                    None => None,
                },
                Some((to, imported)) => {
                    let mut visited: HashMap<(String, String), usize> = HashMap::new();
                    lookup_export(
                        to,
                        imported,
                        0,
                        &exports_by_file,
                        &reexports_by_file,
                        &mut visited,
                    )
                }
                None if r.member.is_none() => exports_by_file
                    .get(file.as_str())
                    .and_then(|m| m.get(&r.name))
                    .copied(),
                None => None,
            };
            let Some(target) = target else {
                unresolved_refs += 1;
                continue;
            };
            let to = defs[target].id.clone();
            let to_file = defs[target].file.clone();
            match r.kind.as_str() {
                "call" => {
                    edges.push(Edge::Call {
                        from_file: file.clone(),
                        from_line: r.line,
                        to,
                        to_file,
                    });
                    counts.call += 1;
                }
                "jsx-use" => {
                    edges.push(Edge::JsxUse {
                        from_file: file.clone(),
                        from_line: r.line,
                        to,
                        to_file,
                    });
                    counts.jsx_use += 1;
                }
                "dispatch" => {
                    edges.push(Edge::Dispatch {
                        from_file: file.clone(),
                        from_line: r.line,
                        to,
                        to_file,
                    });
                    counts.dispatch += 1;
                }
                // Unreachable over a fragment this crate produced: the
                // extractor emits exactly the three ref kinds above. A cached
                // fragment carrying anything else is dropped rather than
                // guessed at, the same stance every other unknown fact gets.
                _ => unresolved_refs += 1,
            }
        }
    }

    let stats = TsStats {
        ts_file_count: fragments.len(),
        ts_def_count: defs.len(),
        external_import_count: external_imports,
        unresolved_ref_count: unresolved_refs,
    };
    TsGraph {
        defs,
        edges,
        edges_by_kind: counts,
        stats,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::{TsBinding, TsFragmentDef, TsImport, TsReexport, TsReexportName, TsRef};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn frag(
        defs: Vec<(&str, &str, usize)>,
        imports: Vec<TsImport>,
        reexports: Vec<TsReexport>,
        refs: Vec<TsRef>,
    ) -> TsFragment {
        TsFragment {
            ts: 1,
            defs: defs
                .into_iter()
                .map(|(n, k, l)| TsFragmentDef {
                    name: n.into(),
                    kind: k.into(),
                    line: l,
                    end_line: l,
                })
                .collect(),
            imports,
            reexports,
            refs,
            default: None,
        }
    }

    fn import(spec: &str, line: usize, bindings: &[(&str, &str)]) -> TsImport {
        TsImport {
            spec: spec.into(),
            line,
            bindings: bindings
                .iter()
                .map(|(l, i)| TsBinding {
                    local: (*l).into(),
                    imported: (*i).into(),
                })
                .collect(),
        }
    }

    fn call_ref(name: &str, line: usize) -> TsRef {
        TsRef {
            kind: "call".into(),
            name: name.into(),
            member: None,
            line,
        }
    }

    fn named_barrel(spec: &str, exported: &str) -> TsReexport {
        TsReexport {
            spec: spec.into(),
            line: 1,
            star: false,
            names: vec![TsReexportName {
                exported: exported.into(),
                imported: exported.into(),
            }],
        }
    }

    fn star_barrel(spec: &str) -> TsReexport {
        TsReexport {
            spec: spec.into(),
            line: 1,
            star: true,
            names: vec![],
        }
    }

    fn no_alias() -> TsAliasScopes {
        TsAliasScopes::default()
    }

    // A throwaway directory tree for the alias-scope tests below: nothing
    // else in this module touches disk. Named and counted the same way every
    // other `src/*.rs` test's temp-dir helper is (see e.g. `mapcmd.rs`'s),
    // and removed on drop so a panicking assertion still cleans up.
    struct TmpDir(std::path::PathBuf);

    static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    impl TmpDir {
        fn new(label: &str) -> Self {
            let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!(
                "scout-tsgraph-test-{label}-{}-{n}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).expect("create temp dir");
            TmpDir(dir)
        }

        fn write(&self, rel: &str, contents: &str) {
            let path = self.0.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent dirs");
            }
            fs::write(&path, contents).expect("write fixture file");
        }
    }

    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_root_tsconfig_alias_resolves_a_bare_specifier() {
        let tmp = TmpDir::new("root-alias");
        tmp.write(
            "tsconfig.base.json",
            r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": { "@core/*": ["packages/core/src/*"] }
  }
}"#,
        );
        let files = ["apps/api/src/handler.ts"];
        let scopes = read_ts_alias_scopes(&tmp.0, files.iter().copied());
        let file_set: HashSet<&str> = [
            "apps/api/src/handler.ts",
            "packages/core/src/formatLabel.ts",
        ]
        .into_iter()
        .collect();
        assert_eq!(
            resolve_specifier(
                "apps/api/src/handler.ts",
                "@core/formatLabel",
                &file_set,
                &scopes
            ),
            Some("packages/core/src/formatLabel.ts".to_string())
        );
    }

    #[test]
    fn a_nested_tsconfig_owns_the_files_beneath_it() {
        let tmp = TmpDir::new("nested-owns");
        tmp.write(
            "tsconfig.base.json",
            r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": { "@core/*": ["packages/core/src/*"] }
  }
}"#,
        );
        tmp.write(
            "apps/web/tsconfig.json",
            r#"{
  "extends": "../../tsconfig.base.json",
  "compilerOptions": {
    "baseUrl": ".",
    "paths": { "@/*": ["./src/*"] }
  }
}"#,
        );
        let files = ["apps/web/src/pages/Home.tsx", "apps/api/src/handler.ts"];
        let scopes = read_ts_alias_scopes(&tmp.0, files.iter().copied());
        let file_set: HashSet<&str> = [
            "apps/web/src/pages/Home.tsx",
            "apps/api/src/handler.ts",
            "packages/core/src/formatLabel.ts",
            "apps/web/src/components/Button.tsx",
        ]
        .into_iter()
        .collect();

        assert_eq!(
            resolve_specifier(
                "apps/web/src/pages/Home.tsx",
                "@/components/Button",
                &file_set,
                &scopes
            ),
            Some("apps/web/src/components/Button.tsx".to_string()),
            "the nested config's own @/ alias resolves"
        );
        assert_eq!(
            resolve_specifier(
                "apps/web/src/pages/Home.tsx",
                "@core/formatLabel",
                &file_set,
                &scopes
            ),
            None,
            "the nested paths override is whole-property: it turns off the root's @core alias too"
        );
        assert_eq!(
            resolve_specifier(
                "apps/api/src/handler.ts",
                "@core/formatLabel",
                &file_set,
                &scopes
            ),
            Some("packages/core/src/formatLabel.ts".to_string()),
            "a file with no nested owner still resolves through the root chain"
        );
    }

    #[test]
    fn a_nested_paths_entry_shadows_the_root_entry_with_the_same_pattern() {
        let tmp = TmpDir::new("shadow");
        tmp.write(
            "tsconfig.base.json",
            r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": { "@shared/*": ["packages/shared/src/*"] }
  }
}"#,
        );
        tmp.write(
            "apps/web/tsconfig.json",
            r#"{
  "extends": "../../tsconfig.base.json",
  "compilerOptions": {
    "baseUrl": ".",
    "paths": { "@shared/*": ["./vendor/shared/*"] }
  }
}"#,
        );
        let files = ["apps/web/src/index.ts", "apps/other/src/index.ts"];
        let scopes = read_ts_alias_scopes(&tmp.0, files.iter().copied());
        let file_set: HashSet<&str> = [
            "apps/web/src/index.ts",
            "apps/other/src/index.ts",
            "packages/shared/src/thing.ts",
            "apps/web/vendor/shared/thing.ts",
        ]
        .into_iter()
        .collect();

        assert_eq!(
            resolve_specifier("apps/web/src/index.ts", "@shared/thing", &file_set, &scopes),
            Some("apps/web/vendor/shared/thing.ts".to_string()),
            "the file under the nested config resolves to the nested target"
        );
        assert_eq!(
            resolve_specifier(
                "apps/other/src/index.ts",
                "@shared/thing",
                &file_set,
                &scopes
            ),
            Some("packages/shared/src/thing.ts".to_string()),
            "a file outside the nested config still resolves to the root target"
        );
    }

    #[test]
    fn a_nested_tsconfig_without_paths_or_base_url_falls_back_to_the_root_scope() {
        let tmp = TmpDir::new("silent-nested");
        tmp.write(
            "tsconfig.base.json",
            r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": { "@core/*": ["packages/core/src/*"] }
  }
}"#,
        );
        tmp.write(
            "apps/web/tsconfig.json",
            r#"{
  "compilerOptions": { "strict": true }
}"#,
        );
        let files = ["apps/web/src/index.ts"];
        let scopes = read_ts_alias_scopes(&tmp.0, files.iter().copied());
        let file_set: HashSet<&str> = ["apps/web/src/index.ts", "packages/core/src/formatLabel.ts"]
            .into_iter()
            .collect();

        assert_eq!(
            resolve_specifier(
                "apps/web/src/index.ts",
                "@core/formatLabel",
                &file_set,
                &scopes
            ),
            Some("packages/core/src/formatLabel.ts".to_string()),
            "a nested config declaring neither paths nor baseUrl does not switch the root alias off"
        );
    }

    #[test]
    fn a_silent_nested_tsconfig_is_stepped_over_to_the_next_owner_above_it() {
        // `apps/` owns an alias; `apps/web/` carries a config that declares
        // nothing. A file under `apps/web/` resolves through `apps/` -- and
        // gets the same answer whether or not some sibling file elsewhere
        // under `apps/` happens to exist to register that owner.
        let tmp = TmpDir::new("stepped-over");
        tmp.write(
            "apps/tsconfig.json",
            r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": { "@app/*": ["shared/*"] }
  }
}"#,
        );
        tmp.write(
            "apps/web/tsconfig.json",
            r#"{
  "compilerOptions": { "strict": true }
}"#,
        );
        let file_set: HashSet<&str> = [
            "apps/web/src/x.ts",
            "apps/tool/y.ts",
            "apps/shared/label.ts",
        ]
        .into_iter()
        .collect();
        let alone = read_ts_alias_scopes(&tmp.0, ["apps/web/src/x.ts"]);
        let with_sibling = read_ts_alias_scopes(&tmp.0, ["apps/web/src/x.ts", "apps/tool/y.ts"]);
        for scopes in [&alone, &with_sibling] {
            assert_eq!(
                resolve_specifier("apps/web/src/x.ts", "@app/label", &file_set, scopes),
                Some("apps/shared/label.ts".to_string()),
                "the next owner above a silent config answers, sibling or no sibling"
            );
        }
    }

    #[test]
    fn nested_scopes_are_ordered_longest_directory_first() {
        let tmp = TmpDir::new("nested-order");
        tmp.write(
            "apps/web/tsconfig.json",
            r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": { "@/*": ["./from-web/*"] }
  }
}"#,
        );
        tmp.write(
            "apps/web/admin/tsconfig.json",
            r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": { "@/*": ["./from-admin/*"] }
  }
}"#,
        );
        let files = ["apps/web/other/file.ts", "apps/web/admin/src/page.ts"];
        let scopes = read_ts_alias_scopes(&tmp.0, files.iter().copied());
        let file_set: HashSet<&str> = [
            "apps/web/other/file.ts",
            "apps/web/admin/src/page.ts",
            "apps/web/from-web/thing.ts",
            "apps/web/admin/from-admin/thing.ts",
        ]
        .into_iter()
        .collect();

        assert_eq!(
            resolve_specifier("apps/web/admin/src/page.ts", "@/thing", &file_set, &scopes),
            Some("apps/web/admin/from-admin/thing.ts".to_string()),
            "the deeper nested config owns files beneath it, not the shallower one"
        );
        assert_eq!(
            resolve_specifier("apps/web/other/file.ts", "@/thing", &file_set, &scopes),
            Some("apps/web/from-web/thing.ts".to_string()),
            "a file outside the deeper config still resolves through the shallower one"
        );
    }

    #[test]
    fn join_rel_refuses_to_climb_above_the_repo_root() {
        assert_eq!(
            join_rel("src", "../util/format"),
            Some("util/format".to_string())
        );
        assert_eq!(join_rel("", "../escape"), None);
    }

    #[test]
    fn probe_order_prefers_a_typescript_source_over_a_built_js_sibling() {
        let files: HashSet<&str> = ["src/x.ts", "src/x.js"].into_iter().collect();
        assert_eq!(
            probe_file(Some("src/x"), &files),
            Some("src/x.ts".to_string())
        );
    }

    #[test]
    fn an_esm_js_specifier_resolves_to_the_ts_source_it_compiles_from() {
        let files: HashSet<&str> = ["src/x.ts"].into_iter().collect();
        assert_eq!(
            probe_file(Some("src/x.js"), &files),
            Some("src/x.ts".to_string())
        );
    }

    #[test]
    fn a_directory_specifier_resolves_through_its_index_file() {
        let files: HashSet<&str> = ["src/hooks/index.ts"].into_iter().collect();
        assert_eq!(
            probe_file(Some("src/hooks"), &files),
            Some("src/hooks/index.ts".to_string())
        );
    }

    #[test]
    fn strip_jsonc_drops_comments_and_one_trailing_comma_per_closer() {
        let text = "{\n // a\n \"a\": 1, /* b */\n \"b\": [1, 2,],\n}";
        let value: Value = serde_json::from_str(&strip_jsonc(text)).expect("parses");
        assert_eq!(value["a"], serde_json::json!(1));
        assert_eq!(value["b"], serde_json::json!([1, 2]));
    }

    #[test]
    fn a_relative_import_becomes_one_import_edge_and_a_resolved_call() {
        let fragments = vec![
            (
                "src/a.ts".to_string(),
                frag(
                    vec![],
                    vec![import("./b", 1, &[("go", "go")])],
                    vec![],
                    vec![call_ref("go", 3)],
                ),
            ),
            (
                "src/b.ts".to_string(),
                frag(vec![("go", "function", 1)], vec![], vec![], vec![]),
            ),
        ];
        let g = resolve_ts_graph(&fragments, &no_alias());
        assert_eq!(
            g.edges_by_kind,
            TsEdgeCounts {
                import: 1,
                call: 1,
                jsx_use: 0,
                dispatch: 0
            }
        );
        assert_eq!(g.stats.ts_def_count, 1);
        assert_eq!(g.stats.unresolved_ref_count, 0);
    }

    #[test]
    fn an_unresolvable_specifier_is_external_and_never_an_edge() {
        let fragments = vec![(
            "src/a.ts".to_string(),
            frag(
                vec![],
                vec![import("./nowhere", 1, &[("go", "go")])],
                vec![],
                vec![call_ref("go", 3)],
            ),
        )];
        let g = resolve_ts_graph(&fragments, &no_alias());
        assert_eq!(g.edges.len(), 0);
        assert_eq!(g.stats.external_import_count, 1);
        assert_eq!(g.stats.unresolved_ref_count, 1);
    }

    #[test]
    fn chained_barrels_resolve_to_the_declaring_file_within_the_hop_cap() {
        let fragments = vec![
            (
                "src/a.ts".to_string(),
                frag(
                    vec![],
                    vec![import("./b", 1, &[("go", "go")])],
                    vec![],
                    vec![call_ref("go", 3)],
                ),
            ),
            (
                "src/b.ts".to_string(),
                frag(vec![], vec![], vec![named_barrel("./c", "go")], vec![]),
            ),
            (
                "src/c.ts".to_string(),
                frag(vec![], vec![], vec![star_barrel("./d")], vec![]),
            ),
            (
                "src/d.ts".to_string(),
                frag(vec![], vec![], vec![named_barrel("./e", "go")], vec![]),
            ),
            (
                "src/e.ts".to_string(),
                frag(vec![("go", "function", 1)], vec![], vec![], vec![]),
            ),
        ];
        let g = resolve_ts_graph(&fragments, &no_alias());
        assert!(matches!(
            g.edges.iter().find(|e| matches!(
                e,
                Edge::Import { via: Some(v), .. } if v == "src/b.ts"
            )),
            Some(Edge::Import { to_file, .. }) if to_file == "src/e.ts"
        ));
        assert!(matches!(
            g.edges.iter().find(|e| matches!(e, Edge::Call { .. })),
            Some(Edge::Call { to_file, .. }) if to_file == "src/e.ts"
        ));

        // One barrel more than the cap allows: the walk gives up one hop
        // short of the file that finally declares the name, so it resolves
        // to nothing.
        let mut deep = vec![(
            "src/deep_a.ts".to_string(),
            frag(
                vec![],
                vec![import("./deep0", 1, &[("go", "go")])],
                vec![],
                vec![],
            ),
        )];
        for i in 0..=BARREL_HOPS {
            let next = if i == BARREL_HOPS {
                "deep_decl".to_string()
            } else {
                format!("deep{}", i + 1)
            };
            deep.push((
                format!("src/deep{i}.ts"),
                frag(
                    vec![],
                    vec![],
                    vec![named_barrel(&format!("./{next}"), "go")],
                    vec![],
                ),
            ));
        }
        deep.push((
            "src/deep_decl.ts".to_string(),
            frag(vec![("go", "function", 1)], vec![], vec![], vec![]),
        ));
        let g2 = resolve_ts_graph(&deep, &no_alias());
        assert!(
            !g2.edges
                .iter()
                .any(|e| matches!(e, Edge::Import { via: Some(_), .. })),
            "a chain one barrel past the hop cap resolves to nothing"
        );
    }

    #[test]
    fn a_cyclic_star_reexport_pair_terminates_and_resolves_nothing_for_an_absent_name() {
        let fragments = vec![
            (
                "src/a.ts".to_string(),
                frag(
                    vec![],
                    vec![import("./b", 1, &[("missing", "missing")])],
                    vec![],
                    vec![call_ref("missing", 3)],
                ),
            ),
            (
                "src/b.ts".to_string(),
                frag(vec![], vec![], vec![star_barrel("./c")], vec![]),
            ),
            (
                "src/c.ts".to_string(),
                frag(vec![], vec![], vec![star_barrel("./b")], vec![]),
            ),
        ];
        // The assertion below is the point of the test, but so is simply
        // reaching it: a cyclic `export *` pair with no bound would recurse
        // forever over a name neither file ever declares.
        let g = resolve_ts_graph(&fragments, &no_alias());
        assert!(
            !g.edges
                .iter()
                .any(|e| matches!(e, Edge::Import { via: Some(_), .. })),
            "a name absent from a cyclic star-reexport pair carries no via edge"
        );
        assert!(
            !g.edges.iter().any(|e| matches!(e, Edge::Call { .. })),
            "a name absent from a cyclic star-reexport pair resolves no call"
        );
    }

    #[test]
    fn a_cycle_still_resolves_a_name_one_of_its_members_declares() {
        let fragments = vec![
            (
                "src/a.ts".to_string(),
                frag(
                    vec![],
                    vec![import("./b", 1, &[("go", "go")])],
                    vec![],
                    vec![call_ref("go", 3)],
                ),
            ),
            (
                "src/b.ts".to_string(),
                frag(vec![], vec![], vec![star_barrel("./c")], vec![]),
            ),
            (
                "src/c.ts".to_string(),
                frag(
                    vec![("go", "function", 1)],
                    vec![],
                    vec![star_barrel("./b")],
                    vec![],
                ),
            ),
        ];
        let g = resolve_ts_graph(&fragments, &no_alias());
        assert!(matches!(
            g.edges.iter().find(|e| matches!(e, Edge::Import { via: Some(_), .. })),
            Some(Edge::Import { to_file, .. }) if to_file == "src/c.ts"
        ));
        assert!(matches!(
            g.edges.iter().find(|e| matches!(e, Edge::Call { .. })),
            Some(Edge::Call { to_file, .. }) if to_file == "src/c.ts"
        ));
    }

    #[test]
    fn a_pair_first_cut_by_the_cap_is_walked_again_from_a_shorter_path() {
        // `b` fans out to a long chain first and a short one second, both
        // reaching `deep`, which alone re-exports the declaring file. Down
        // the long chain `deep` is entered exactly at the cap, so its own
        // re-export is never followed; the short chain then reaches it with
        // hops to spare and must be allowed to walk it again.
        let long_links = BARREL_HOPS - 1;
        let mut fragments = vec![
            (
                "src/a.ts".to_string(),
                frag(
                    vec![],
                    vec![import("./b", 1, &[("go", "go")])],
                    vec![],
                    vec![call_ref("go", 3)],
                ),
            ),
            (
                "src/b.ts".to_string(),
                frag(
                    vec![],
                    vec![],
                    vec![star_barrel("./long1"), star_barrel("./short")],
                    vec![],
                ),
            ),
            (
                "src/short.ts".to_string(),
                frag(vec![], vec![], vec![star_barrel("./deep")], vec![]),
            ),
            (
                "src/deep.ts".to_string(),
                frag(vec![], vec![], vec![star_barrel("./x")], vec![]),
            ),
            (
                "src/x.ts".to_string(),
                frag(vec![("go", "function", 1)], vec![], vec![], vec![]),
            ),
        ];
        for i in 1..=long_links {
            let next = if i == long_links {
                "./deep".to_string()
            } else {
                format!("./long{}", i + 1)
            };
            fragments.push((
                format!("src/long{i}.ts"),
                frag(vec![], vec![], vec![star_barrel(&next)], vec![]),
            ));
        }
        let g = resolve_ts_graph(&fragments, &no_alias());
        assert!(
            matches!(
                g.edges.iter().find(|e| matches!(e, Edge::Call { .. })),
                Some(Edge::Call { to_file, .. }) if to_file == "src/x.ts"
            ),
            "the short chain must still reach src/x.ts: {:?}",
            g.edges
        );
    }

    #[test]
    fn a_namespace_binding_resolves_only_through_a_member() {
        let fragments = vec![
            (
                "src/a.ts".to_string(),
                frag(
                    vec![],
                    vec![import("./b", 1, &[("ns", "*")])],
                    vec![],
                    vec![
                        TsRef {
                            kind: "call".into(),
                            name: "ns".into(),
                            member: Some("go".into()),
                            line: 3,
                        },
                        call_ref("ns", 4),
                    ],
                ),
            ),
            (
                "src/b.ts".to_string(),
                frag(vec![("go", "function", 1)], vec![], vec![], vec![]),
            ),
        ];
        let g = resolve_ts_graph(&fragments, &no_alias());
        assert_eq!(g.edges_by_kind.call, 1);
        assert_eq!(g.stats.unresolved_ref_count, 1);
    }
}
