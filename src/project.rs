// A lightweight, hand-scanned model of the repo's `.csproj` projects: which
// directory each project owns, which other projects it references, and
// whether it is a test project. No MSBuild is invoked and no XML crate is
// used -- `scan_csproj`/`scan_props` below hand-scan just enough of the file
// to answer those three questions.
//
// ## Discovery
//
// `discover` walks each scope directory recursively (via
// `walk::list_files_with_ext`, so it inherits that walk's `SKIP_DIRS` and
// deterministic ordering) looking for `.csproj` and `.props` files, then
// additionally checks each scope directory's ancestors -- one directory at a
// time, NOT recursively -- up to and including `root`. That ancestor check
// exists because a repo-root `Directory.Build.props` commonly sits above
// every scope directory and would otherwise never be seen: the recursive
// walk only covers subtrees rooted AT a scope directory. `.props` files are
// filtered to the exact name `Directory.Build.props`; `.sln` files are never
// looked for (no extension in the scan list) and so are always ignored.
//
// ## Units and directory ownership
//
// Every discovered `.csproj` becomes a `Unit` keyed by its repo-relative path
// (`id`) and owning its containing directory (`dir`). `ProjectModel` maps a
// directory to its unit only when that directory holds EXACTLY one `.csproj`
// -- a directory holding two or more is deliberately left unmapped, so
// `unit_of_file` walks past it to the nearest single-project ancestor
// instead of guessing.
//
// ## Test detection
//
// `Unit.test` is decided by a fixed precedence, first source to answer wins:
// 1. an explicit `<IsTestProject>` element in the csproj itself;
// 2. an explicit `<IsTestProject>` in the nearest `Directory.Build.props`
//    (searched from the csproj's own directory upward to `root`; a props
//    file with no such element does not count as an answer and defers to the
//    next source, but a further-out props file is never consulted once a
//    nearer one has been found);
// 3. a `Microsoft.NET.Test.Sdk` `PackageReference`;
// 4. a project name ending in `.Tests` or `.Test`.
//
// ## Reference closure
//
// `Unit.refs` holds only DIRECT `ProjectReference` targets, normalized to
// repo-relative ids (backslashes to forward slashes, `..`/`.` resolved
// against the csproj's own directory). A reference that resolves above
// `root` is kept as a literal string starting with `..` rather than clamped
// -- clamping could accidentally alias it onto an unrelated real unit id, and
// a leading `..` can never match one (ids are always repo-relative paths
// with no `..` component), so it safely resolves to nothing everywhere a ref
// id is looked up. A reference naming an id no `Unit` actually has is simply
// ignored when the closure is built.
//
// `ProjectModel::from_units` computes each unit's full transitive closure by
// BFS over `refs` once, up front, so `reachable` is an O(1) set lookup.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::repo;
use crate::walk;

/// One `.csproj` project discovered under the repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// Repo-relative path to the `.csproj` file itself (`/`-joined), e.g.
    /// `"src/App/App.csproj"`. Doubles as this unit's identity: `refs`
    /// entries and cross-unit lookups are matched against this string.
    pub id: String,
    /// The project name -- the csproj file name with its `.csproj`
    /// extension stripped (e.g. `"App.Tests"` for `App.Tests.csproj`). Used
    /// only for the name-suffix fallback in test detection; not otherwise
    /// assumed to be unique.
    pub name: String,
    /// Repo-relative path to the directory containing the `.csproj` file
    /// (`/`-joined, no trailing slash; `""` for a `.csproj` directly at
    /// `root`).
    pub dir: String,
    /// The `id`s of this unit's DIRECT `ProjectReference` targets only (not
    /// transitively closed -- see `ProjectModel::reachable` for that). An
    /// entry that does not match any discovered unit's `id` (an out-of-root
    /// target, or a project this scan never found) is kept verbatim; it
    /// simply never resolves to anything.
    pub refs: Vec<String>,
    /// Whether this project is a test project, per the precedence documented
    /// on the module.
    pub test: bool,
}

/// The repo's discovered `.csproj` projects, their direct references, and
/// the transitive closure over those references.
#[derive(Debug, Clone)]
pub struct ProjectModel {
    /// All discovered units, sorted by `id`. A unit's position in this
    /// vector is its index everywhere else in this type (`dir_to_unit`
    /// values, `closure` entries, `reachable`'s arguments).
    pub units: Vec<Unit>,
    // Directory -> unit index, present only for a directory that owns
    // EXACTLY one unit. A directory with zero units has no entry (nothing to
    // map); a directory with two or more also has no entry, on purpose --
    // see `unit_of_file`.
    dir_to_unit: HashMap<String, usize>,
    // closure[i] is the set of unit indices transitively reachable from unit
    // i by following `refs`, NOT including i itself (even if a reference
    // cycle loops back to it -- `reachable` handles the `from == to` case
    // separately).
    closure: Vec<HashSet<usize>>,
}

/// Discovers the repo's `.csproj` project model under `scope`.
///
/// `scope` holds directory names relative to `root`, `"."` meaning `root`
/// itself -- same convention as `walk::list_source_files`. `Ok(None)` when
/// zero `.csproj` files are found anywhere in scope or above it; discovery
/// never fails just because nothing was there to discover.
///
/// See the module docs for the full discovery, ownership, test-detection and
/// closure rules.
pub fn discover(root: &Path, scope: &[String]) -> io::Result<Option<ProjectModel>> {
    let mut csproj_files: HashSet<PathBuf> = HashSet::new();
    let mut props_by_dir: HashMap<PathBuf, String> = HashMap::new();

    // Recursive walk of each scope directory.
    let found = walk::list_files_with_ext(root, scope, &[".csproj", ".props"])?;
    for rel in found {
        let abs = root.join(&rel);
        if rel.ends_with(".csproj") {
            csproj_files.insert(abs);
        } else if is_directory_build_props(&abs) {
            if let (Some(dir), Some(text)) = (abs.parent(), read_lossy(&abs)) {
                props_by_dir.insert(dir.to_path_buf(), text);
            }
        }
    }

    // Each scope dir's ancestors up to root, checked non-recursively (one
    // `read_dir` per ancestor, no descent into siblings).
    for d in scope {
        let scope_abs = if d == "." {
            root.to_path_buf()
        } else {
            root.join(d)
        };
        for ancestor in ancestors_up_to(root, &scope_abs) {
            scan_ancestor_dir(&ancestor, &mut csproj_files, &mut props_by_dir);
        }
    }

    if csproj_files.is_empty() {
        return Ok(None);
    }

    let mut units = Vec::with_capacity(csproj_files.len());
    for csproj_path in &csproj_files {
        units.push(build_unit(root, csproj_path, &props_by_dir)?);
    }

    Ok(Some(ProjectModel::from_units(units)))
}

// The ancestor directories of `dir`, starting at its parent and continuing
// up to and including `root`. Empty when `dir == root` (no ancestors left to
// check within the repo) or when `dir` is not under `root` at all.
fn ancestors_up_to(root: &Path, dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut cur = dir.to_path_buf();
    while cur != root {
        let Some(parent) = cur.parent() else {
            break;
        };
        out.push(parent.to_path_buf());
        if parent == root {
            break;
        }
        cur = parent.to_path_buf();
    }
    out
}

// Non-recursive `.csproj`/`Directory.Build.props` scan of a single
// directory's direct entries. A missing or unreadable directory (e.g. an
// ancestor computed lexically from a scope element that does not exist on
// disk) is silently skipped, matching `walk`'s "nonexistent scope element"
// behavior rather than failing the whole discovery.
fn scan_ancestor_dir(
    dir: &Path,
    csproj_files: &mut HashSet<PathBuf>,
    props_by_dir: &mut HashMap<PathBuf, String>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".csproj"))
        {
            csproj_files.insert(path);
        } else if is_directory_build_props(&path) {
            if let Some(text) = read_lossy(&path) {
                props_by_dir.insert(dir.to_path_buf(), text);
            }
        }
    }
}

fn is_directory_build_props(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()) == Some("Directory.Build.props")
}

fn read_lossy(path: &Path) -> Option<String> {
    fs::read(path)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

// Reads and scans one discovered `.csproj` into its `Unit`. `props_by_dir`
// is the full set of `Directory.Build.props` files discovery already found
// (recursive-walk plus ancestor scan combined), keyed by their own
// directory, so the nearest-ancestor search below never touches the disk
// again.
fn build_unit(
    root: &Path,
    csproj_path: &Path,
    props_by_dir: &HashMap<PathBuf, String>,
) -> io::Result<Unit> {
    let id = repo::rel_path(root, csproj_path);
    let dir_abs = csproj_path.parent().unwrap_or(root).to_path_buf();
    let dir = repo::rel_path(root, &dir_abs);
    let name = csproj_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let bytes = fs::read(csproj_path)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let facts = scan_csproj(&text);

    let refs = facts
        .project_refs
        .iter()
        .map(|raw| resolve_ref(&dir, raw))
        .collect();

    // Precedence: explicit csproj value, then the nearest Directory.Build.props
    // that has one, then the test-SDK package, then the name suffix.
    let test = facts
        .is_test_project
        .or_else(|| nearest_props_is_test_project(root, &dir_abs, props_by_dir))
        .unwrap_or_else(|| {
            facts.has_test_sdk || name.ends_with(".Tests") || name.ends_with(".Test")
        });

    Ok(Unit {
        id,
        name,
        dir,
        refs,
        test,
    })
}

// Walks from `start_dir` (the unit's own directory) upward to and including
// `root`, returning the `<IsTestProject>` value from the FIRST
// `Directory.Build.props` found -- `None` if that nearest file has no such
// element (a further-out props file is not consulted) or if none exists on
// the path at all.
fn nearest_props_is_test_project(
    root: &Path,
    start_dir: &Path,
    props_by_dir: &HashMap<PathBuf, String>,
) -> Option<bool> {
    let mut cur = start_dir.to_path_buf();
    loop {
        if let Some(text) = props_by_dir.get(&cur) {
            return scan_props(text);
        }
        if cur == root {
            return None;
        }
        cur = cur.parent()?.to_path_buf();
    }
}

impl ProjectModel {
    /// Builds a `ProjectModel` from an already-scanned unit list: sorts
    /// `units` by `id`, maps each directory that owns exactly one unit to
    /// it, and computes each unit's transitive reference closure by BFS.
    /// This is the constructor `discover` itself uses once scanning is done,
    /// and the one a persistence layer would use to rebuild a `ProjectModel`
    /// from a stored unit list without re-scanning any files.
    pub fn from_units(mut units: Vec<Unit>) -> Self {
        units.sort_by(|a, b| a.id.cmp(&b.id));

        let id_to_index: HashMap<&str, usize> = units
            .iter()
            .enumerate()
            .map(|(i, u)| (u.id.as_str(), i))
            .collect();

        let mut dir_counts: HashMap<&str, usize> = HashMap::new();
        for u in &units {
            *dir_counts.entry(u.dir.as_str()).or_insert(0) += 1;
        }
        let mut dir_to_unit = HashMap::new();
        for (i, u) in units.iter().enumerate() {
            if dir_counts.get(u.dir.as_str()) == Some(&1) {
                dir_to_unit.insert(u.dir.clone(), i);
            }
        }

        let closure = build_closure(&units, &id_to_index);

        ProjectModel {
            units,
            dir_to_unit,
            closure,
        }
    }

    /// The unit owning `rel` (a repo-relative file path, `/`-joined): the
    /// nearest ancestor directory of `rel` that owns EXACTLY one unit.
    /// A directory owning zero units is walked past because nothing maps
    /// it; a directory owning two or more is ALSO walked past, on purpose
    /// (`ProjectModel::from_units` never maps an ambiguous directory) --
    /// climbing to the next single-project ancestor instead of guessing
    /// which of the colliding projects owns the file. `None` when no
    /// ancestor, up to and including the repo root, owns exactly one unit.
    pub fn unit_of_file(&self, rel: &str) -> Option<usize> {
        let mut dir = match rel.rfind('/') {
            Some(i) => &rel[..i],
            None => "",
        };
        loop {
            if let Some(&idx) = self.dir_to_unit.get(dir) {
                return Some(idx);
            }
            if dir.is_empty() {
                return None;
            }
            dir = match dir.rfind('/') {
                Some(i) => &dir[..i],
                None => "",
            };
        }
    }

    /// Whether `to` is reachable from `from`: trivially true when they are
    /// the same unit, otherwise true iff `to` is in `from`'s precomputed
    /// transitive `ProjectReference` closure.
    pub fn reachable(&self, from: usize, to: usize) -> bool {
        from == to || self.closure.get(from).is_some_and(|c| c.contains(&to))
    }
}

// BFS per unit over `refs` (resolved through `id_to_index`; an unresolved
// ref id is simply skipped) to produce each unit's full transitive closure
// once, up front.
fn build_closure(units: &[Unit], id_to_index: &HashMap<&str, usize>) -> Vec<HashSet<usize>> {
    units
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let mut seen = HashSet::new();
            seen.insert(i);
            let mut queue: VecDeque<usize> = VecDeque::new();
            queue.push_back(i);
            let mut result = HashSet::new();
            while let Some(cur) = queue.pop_front() {
                for r in &units[cur].refs {
                    if let Some(&idx) = id_to_index.get(r.as_str()) {
                        if seen.insert(idx) {
                            result.insert(idx);
                            queue.push_back(idx);
                        }
                    }
                }
            }
            result
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Hand-written XML scanning -- no XML crate. Just enough tag/attribute
// recognition to answer `scan_csproj`/`scan_props`'s three questions; nothing
// here builds a DOM or validates well-formedness.
// ---------------------------------------------------------------------------

// The facts `scan_csproj` extracts from a `.csproj` file's raw text.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct CsprojFacts {
    // Raw `ProjectReference` `Include` values, backslashes already turned to
    // forward slashes, but NOT yet resolved against any directory (the caller
    // does that with `resolve_ref`, since this function has no directory
    // context).
    project_refs: Vec<String>,
    // Whether any `PackageReference Include="Microsoft.NET.Test.Sdk"` tag
    // was seen.
    has_test_sdk: bool,
    // The parsed content of an `<IsTestProject>` element, if one was seen
    // with a recognizable `true`/`false` value (case-insensitive).
    is_test_project: Option<bool>,
}

// Scans `text` (a `.csproj` file's contents) for `ProjectReference`/
// `PackageReference` `Include` attributes and an `<IsTestProject>` element.
// XML comments (`<!-- ... -->`) are stripped first, so a fake reference
// living inside a comment is never seen.
fn scan_csproj(text: &str) -> CsprojFacts {
    let clean = strip_xml_comments(text);
    let mut facts = CsprojFacts::default();
    let mut pos = 0usize;
    let mut awaiting_close: Option<&str> = None;

    while let Some((gap, tag)) = next_tag(&clean, &mut pos) {
        if let Some(open_name) = awaiting_close {
            if tag.is_closing && tag.name == open_name {
                let value = gap.trim();
                if value.eq_ignore_ascii_case("true") {
                    facts.is_test_project = Some(true);
                } else if value.eq_ignore_ascii_case("false") {
                    facts.is_test_project = Some(false);
                }
            }
            awaiting_close = None;
        }

        if tag.is_closing {
            continue;
        }

        match tag.name {
            "ProjectReference" => {
                if let Some(include) = find_attr(tag.attrs, "Include") {
                    facts.project_refs.push(include.replace('\\', "/"));
                }
            }
            "PackageReference" => {
                if let Some(include) = find_attr(tag.attrs, "Include") {
                    if include == "Microsoft.NET.Test.Sdk" {
                        facts.has_test_sdk = true;
                    }
                }
            }
            "IsTestProject" if !tag.self_closing => {
                awaiting_close = Some("IsTestProject");
            }
            _ => {}
        }
    }

    facts
}

// Scans a `Directory.Build.props` file's contents for an `<IsTestProject>`
// element. Reuses `scan_csproj`'s scan (`Directory.Build.props` uses the same
// MSBuild element syntax); the `ProjectReference`/`PackageReference` facts it
// also computes are simply unused here.
fn scan_props(text: &str) -> Option<bool> {
    scan_csproj(text).is_test_project
}

// Resolves a raw (already backslash-normalized) `Include` path against
// `csproj_dir` (the referencing csproj's own repo-relative directory,
// `/`-joined, `""` for root) to a repo-relative id: `.` segments drop out,
// `..` pops the last real segment. A `..` with nothing left to pop (the
// reference climbs above `root`) is kept as a literal leading `..` segment
// instead of being clamped -- see the module docs for why that is safe.
fn resolve_ref(csproj_dir: &str, raw: &str) -> String {
    let mut stack: Vec<&str> = if csproj_dir.is_empty() {
        Vec::new()
    } else {
        csproj_dir.split('/').collect()
    };
    for seg in raw.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if matches!(stack.last(), None | Some(&"..")) {
                    stack.push("..");
                } else {
                    stack.pop();
                }
            }
            other => stack.push(other),
        }
    }
    stack.join("/")
}

// Strips `<!-- ... -->` regions from `text`. An unterminated comment (no
// closing `-->`) drops everything from the `<!--` onward.
fn strip_xml_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        match rest.find("<!--") {
            Some(start) => {
                out.push_str(&rest[..start]);
                match rest[start..].find("-->") {
                    Some(end_rel) => {
                        rest = &rest[start + end_rel + 3..];
                    }
                    None => return out,
                }
            }
            None => {
                out.push_str(rest);
                return out;
            }
        }
    }
}

// One `<...>` tag, already split into its name and raw attribute text.
struct TagInfo<'a> {
    name: &'a str,
    is_closing: bool,
    self_closing: bool,
    // Everything after the name, unparsed -- `find_attr` reads out of this.
    // Empty for a closing tag.
    attrs: &'a str,
}

// Advances `*pos` past the next `<...>` tag in `text` found at or after
// `*pos`, returning it along with the plain text that preceded it (the gap
// between the previous tag's `>` and this tag's `<`) -- callers that need an
// element's text content (`IsTestProject`) read it out of the NEXT call's
// `gap`, since that is exactly the text between this tag and whatever tag
// follows it. `None` once no further tag is found.
//
// The search for the tag's closing `>` tracks single/double-quoted spans so
// a multi-line attribute value (or one that happens to be empty) does not
// end the tag early; nothing here assumes a tag fits on one line.
fn next_tag<'a>(text: &'a str, pos: &mut usize) -> Option<(&'a str, TagInfo<'a>)> {
    let search_from = *pos;
    let lt = text[search_from..].find('<')? + search_from;
    let gap = &text[search_from..lt];

    let bytes = text.as_bytes();
    let mut i = lt + 1;
    let mut quote: Option<u8> = None;
    let mut gt = None;
    while i < bytes.len() {
        let c = bytes[i];
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => {
                if c == b'"' || c == b'\'' {
                    quote = Some(c);
                } else if c == b'>' {
                    gt = Some(i);
                    break;
                }
            }
        }
        i += 1;
    }
    let gt = gt?;
    *pos = gt + 1;

    let content = text[lt + 1..gt].trim();
    if let Some(name) = content.strip_prefix('/') {
        return Some((
            gap,
            TagInfo {
                name: name.trim(),
                is_closing: true,
                self_closing: false,
                attrs: "",
            },
        ));
    }

    let self_closing = content.ends_with('/');
    let body = if self_closing {
        content[..content.len() - 1].trim_end()
    } else {
        content
    };
    let (name, attrs) = match body.find(|c: char| c.is_whitespace()) {
        Some(i2) => (&body[..i2], body[i2..].trim()),
        None => (body, ""),
    };

    Some((
        gap,
        TagInfo {
            name,
            is_closing: false,
            self_closing,
            attrs,
        },
    ))
}

// Finds `name="value"` (or `name='value'`) inside a tag's raw attribute
// text, requiring `name` to be a whole attribute name -- preceded by the
// start of `attrs` or whitespace, and followed (after optional whitespace)
// directly by `=`, not by more identifier characters. Values may span
// multiple lines (the search does not treat `\n` specially).
fn find_attr(attrs: &str, name: &str) -> Option<String> {
    let mut i = 0usize;
    while i < attrs.len() {
        let found = attrs[i..].find(name)?;
        let abs = i + found;
        let boundary_before = abs == 0 || attrs.as_bytes()[abs - 1].is_ascii_whitespace();
        if boundary_before {
            let after = &attrs[abs + name.len()..];
            let after_trimmed = after.trim_start();
            if let Some(value_part) = after_trimmed.strip_prefix('=') {
                let value_part = value_part.trim_start();
                if let Some(quote) = value_part.chars().next() {
                    if quote == '"' || quote == '\'' {
                        if let Some(end) = value_part[quote.len_utf8()..].find(quote) {
                            return Some(
                                value_part[quote.len_utf8()..quote.len_utf8() + end].to_string(),
                            );
                        }
                    }
                }
            }
        }
        i = abs + name.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn scratch_dir(label: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "scout-project-rs-{label}-{}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(path, contents).expect("write file");
    }

    // -- scan_csproj: ProjectReference / PackageReference -------------------

    #[test]
    fn scan_csproj_reads_include_from_a_self_closing_project_reference() {
        let facts = scan_csproj(
            r#"<Project><ItemGroup><ProjectReference Include="../B/B.csproj" /></ItemGroup></Project>"#,
        );
        assert_eq!(facts.project_refs, vec!["../B/B.csproj".to_string()]);
    }

    #[test]
    fn scan_csproj_reads_include_from_an_open_close_project_reference() {
        let facts = scan_csproj(
            r#"<ItemGroup><ProjectReference Include="../B/B.csproj"><Private>false</Private></ProjectReference></ItemGroup>"#,
        );
        assert_eq!(facts.project_refs, vec!["../B/B.csproj".to_string()]);
    }

    #[test]
    fn scan_csproj_reads_a_multi_line_attribute() {
        let xml =
            "<ItemGroup>\n  <ProjectReference\n    Include=\"../B/B.csproj\"\n  />\n</ItemGroup>";
        let facts = scan_csproj(xml);
        assert_eq!(facts.project_refs, vec!["../B/B.csproj".to_string()]);
    }

    #[test]
    fn scan_csproj_reads_single_quoted_attributes() {
        let facts = scan_csproj(
            r#"<ItemGroup><PackageReference Include='Microsoft.NET.Test.Sdk' Version='17.0.0' /></ItemGroup>"#,
        );
        assert!(facts.has_test_sdk);
    }

    #[test]
    fn scan_csproj_skips_a_project_reference_inside_a_comment() {
        let xml = r#"
            <ItemGroup>
              <!-- <ProjectReference Include="../Fake/Fake.csproj" /> -->
              <ProjectReference Include="../Real/Real.csproj" />
            </ItemGroup>
        "#;
        let facts = scan_csproj(xml);
        assert_eq!(facts.project_refs, vec!["../Real/Real.csproj".to_string()]);
    }

    #[test]
    fn scan_csproj_detects_the_test_sdk_package_reference() {
        let facts = scan_csproj(
            r#"<ItemGroup><PackageReference Include="Microsoft.NET.Test.Sdk" Version="17.8.0" /></ItemGroup>"#,
        );
        assert!(facts.has_test_sdk);
    }

    #[test]
    fn scan_csproj_reads_explicit_is_test_project_true() {
        let facts =
            scan_csproj("<PropertyGroup><IsTestProject>true</IsTestProject></PropertyGroup>");
        assert_eq!(facts.is_test_project, Some(true));
    }

    #[test]
    fn scan_csproj_reads_explicit_is_test_project_false() {
        let facts =
            scan_csproj("<PropertyGroup><IsTestProject>False</IsTestProject></PropertyGroup>");
        assert_eq!(facts.is_test_project, Some(false));
    }

    #[test]
    fn scan_props_reads_is_test_project() {
        assert_eq!(
            scan_props("<Project><PropertyGroup><IsTestProject>true</IsTestProject></PropertyGroup></Project>"),
            Some(true)
        );
        assert_eq!(scan_props("<Project><PropertyGroup /></Project>"), None);
    }

    // -- resolve_ref ----------------------------------------------------------

    #[test]
    fn resolve_ref_resolves_dotdot_and_backslashes_against_the_csproj_directory() {
        let facts = scan_csproj(r#"<ProjectReference Include="..\..\Shared\Shared.csproj" />"#);
        assert_eq!(
            facts.project_refs,
            vec!["../../Shared/Shared.csproj".to_string()]
        );
        assert_eq!(
            resolve_ref("tests/T", "../../Shared/Shared.csproj"),
            "Shared/Shared.csproj"
        );
    }

    #[test]
    fn resolve_ref_keeps_an_out_of_root_target_as_a_normalised_string_that_matches_nothing() {
        let resolved = resolve_ref("src/A", "../../../Outside/Outside.csproj");
        assert_eq!(resolved, "../Outside/Outside.csproj");
        assert!(resolved.starts_with(".."));
    }

    // -- from_units: sorting, closure, reachability --------------------------

    fn unit(id: &str, dir: &str, refs: &[&str], test: bool) -> Unit {
        Unit {
            id: id.to_string(),
            name: id.trim_end_matches(".csproj").to_string(),
            dir: dir.to_string(),
            refs: refs.iter().map(|r| r.to_string()).collect(),
            test,
        }
    }

    #[test]
    fn from_units_sorts_by_id_and_reachable_is_reflexive_and_directional() {
        let model = ProjectModel::from_units(vec![
            unit("src/C/C.csproj", "src/C", &[], false),
            unit("src/A/A.csproj", "src/A", &["src/B/B.csproj"], false),
            unit("src/B/B.csproj", "src/B", &["src/C/C.csproj"], false),
        ]);

        let ids: Vec<&str> = model.units.iter().map(|u| u.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["src/A/A.csproj", "src/B/B.csproj", "src/C/C.csproj"]
        );

        let a = model
            .units
            .iter()
            .position(|u| u.id == "src/A/A.csproj")
            .unwrap();
        let b = model
            .units
            .iter()
            .position(|u| u.id == "src/B/B.csproj")
            .unwrap();
        let c = model
            .units
            .iter()
            .position(|u| u.id == "src/C/C.csproj")
            .unwrap();

        assert!(model.reachable(a, a), "a unit reaches itself");
        assert!(model.reachable(a, c), "A -> B -> C is transitive");
        assert!(!model.reachable(b, a), "B does not reference A");
        assert!(!model.reachable(c, b), "C references nothing");
        assert!(!model.reachable(c, a), "C cannot reach A");
    }

    #[test]
    fn from_units_ignores_an_unresolved_reference_in_the_closure() {
        let model = ProjectModel::from_units(vec![unit(
            "src/A/A.csproj",
            "src/A",
            &["src/Missing/Missing.csproj"],
            false,
        )]);
        let a = 0;
        assert!(model.closure[a].is_empty());
        assert!(!model.reachable(a, 99));
    }

    #[test]
    fn unit_of_file_climbs_to_a_root_level_unit_from_any_nested_file() {
        let model = ProjectModel::from_units(vec![unit("A.csproj", "", &[], false)]);
        assert_eq!(model.unit_of_file("Elsewhere/Deep/x.cs"), Some(0));
    }

    #[test]
    fn unit_of_file_returns_none_when_no_ancestor_directory_has_exactly_one_unit() {
        // The only unit owns "src/A"; a file outside that whole directory
        // chain (root itself owns no unit here) never resolves.
        let model = ProjectModel::from_units(vec![unit("src/A/A.csproj", "src/A", &[], false)]);
        assert_eq!(model.unit_of_file("other/y.cs"), None);
    }

    // -- discover: end to end over real temp directories ---------------------

    #[test]
    fn discover_returns_none_when_no_csproj_is_under_scope_or_above_it() {
        let root = scratch_dir("no-csproj");
        write_file(&root.join("src/a.cs"), "class A {}\n");

        let result = discover(&root, &["src".to_string()]).unwrap();
        assert!(result.is_none());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discover_builds_units_closure_and_test_flags_for_a_referenced_project_chain() {
        let root = scratch_dir("chain");
        write_file(
            &root.join("src/A/A.csproj"),
            r#"<Project><ItemGroup><ProjectReference Include="../B/B.csproj" /></ItemGroup></Project>"#,
        );
        write_file(
            &root.join("src/B/B.csproj"),
            r#"<Project><ItemGroup><ProjectReference Include="../C/C.csproj" /></ItemGroup></Project>"#,
        );
        write_file(&root.join("src/C/C.csproj"), "<Project></Project>");
        write_file(
            &root.join("tests/T/T.csproj"),
            r#"<Project>
                <ItemGroup>
                  <PackageReference Include="Microsoft.NET.Test.Sdk" Version="17.8.0" />
                  <ProjectReference Include="../../src/A/A.csproj" />
                </ItemGroup>
               </Project>"#,
        );
        write_file(&root.join("src/A/Sub/x.cs"), "class X {}\n");

        let model = discover(&root, &["src".to_string(), "tests".to_string()])
            .unwrap()
            .expect("csproj files were written under scope");

        let idx = |id: &str| model.units.iter().position(|u| u.id == id).unwrap();
        let a = idx("src/A/A.csproj");
        let b = idx("src/B/B.csproj");
        let c = idx("src/C/C.csproj");
        let t = idx("tests/T/T.csproj");

        assert!(model.reachable(a, c), "A -> B -> C must be transitive");
        assert!(!model.reachable(b, a), "B does not reference A");
        assert_eq!(model.unit_of_file("src/A/Sub/x.cs"), Some(a));
        assert!(model.units[t].test, "T carries the test SDK package");
        assert!(model.reachable(t, a), "T directly references A");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discover_directory_with_two_csproj_files_is_skipped_when_resolving_a_file_beneath_it() {
        let root = scratch_dir("two-csproj-dir");
        write_file(&root.join("src/Root.csproj"), "<Project></Project>");
        write_file(&root.join("src/shared/X.csproj"), "<Project></Project>");
        write_file(&root.join("src/shared/Y.csproj"), "<Project></Project>");
        write_file(&root.join("src/shared/deep/z.cs"), "class Z {}\n");

        let model = discover(&root, &["src".to_string()]).unwrap().unwrap();

        let root_unit = model
            .units
            .iter()
            .position(|u| u.id == "src/Root.csproj")
            .unwrap();
        // "src/shared" owns two csproj files, so it is never mapped; a file
        // under it (directly or nested) resolves to the nearest single-csproj
        // ancestor, "src" (owning Root.csproj).
        assert_eq!(model.unit_of_file("src/shared/deep/z.cs"), Some(root_unit));
        assert_eq!(model.unit_of_file("src/shared/w.cs"), Some(root_unit));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discover_props_precedence_beats_the_test_sdk_package() {
        let root = scratch_dir("props-precedence");
        write_file(
            &root.join("src/App/App.csproj"),
            r#"<Project><ItemGroup><PackageReference Include="Microsoft.NET.Test.Sdk" Version="17.8.0" /></ItemGroup></Project>"#,
        );
        write_file(
            &root.join("src/App/Directory.Build.props"),
            "<Project><PropertyGroup><IsTestProject>false</IsTestProject></PropertyGroup></Project>",
        );

        let model = discover(&root, &["src".to_string()]).unwrap().unwrap();
        let app = model
            .units
            .iter()
            .position(|u| u.id == "src/App/App.csproj")
            .unwrap();
        assert!(
            !model.units[app].test,
            "an explicit Directory.Build.props IsTestProject must beat the test-SDK package"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discover_name_suffix_fallback_when_nothing_else_says_test() {
        let root = scratch_dir("name-suffix");
        write_file(
            &root.join("src/App.Tests/App.Tests.csproj"),
            "<Project></Project>",
        );
        write_file(&root.join("src/App/App.csproj"), "<Project></Project>");

        let model = discover(&root, &["src".to_string()]).unwrap().unwrap();
        let tests_unit = model
            .units
            .iter()
            .position(|u| u.id == "src/App.Tests/App.Tests.csproj")
            .unwrap();
        let app_unit = model
            .units
            .iter()
            .position(|u| u.id == "src/App/App.csproj")
            .unwrap();

        assert!(model.units[tests_unit].test, "name ends with .Tests");
        assert!(!model.units[app_unit].test);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discover_finds_a_directory_build_props_above_the_scope_root_via_ancestor_scan() {
        let root = scratch_dir("ancestor-props");
        write_file(
            &root.join("Directory.Build.props"),
            "<Project><PropertyGroup><IsTestProject>true</IsTestProject></PropertyGroup></Project>",
        );
        write_file(&root.join("src/A/A.csproj"), "<Project></Project>");

        let model = discover(&root, &["src".to_string()]).unwrap().unwrap();
        let a = model
            .units
            .iter()
            .position(|u| u.id == "src/A/A.csproj")
            .unwrap();
        assert!(
            model.units[a].test,
            "the root Directory.Build.props sits above the scope dir and must still be found"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discover_ancestor_scan_does_not_recurse_into_sibling_directories() {
        let root = scratch_dir("ancestor-no-recurse");
        write_file(&root.join("src/A/A.csproj"), "<Project></Project>");
        // Sibling of "src", one level below the ancestor "root" -- must NOT
        // be found: the ancestor check on "root" is non-recursive.
        write_file(&root.join("other/Other.csproj"), "<Project></Project>");

        let model = discover(&root, &["src".to_string()]).unwrap().unwrap();
        assert_eq!(model.units.len(), 1);
        assert_eq!(model.units[0].id, "src/A/A.csproj");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discover_ignores_sln_files() {
        let root = scratch_dir("sln-ignored");
        write_file(
            &root.join("Repo.sln"),
            "Microsoft Visual Studio Solution File",
        );
        write_file(&root.join("src/A/A.csproj"), "<Project></Project>");

        let model = discover(&root, &["src".to_string()]).unwrap().unwrap();
        assert_eq!(model.units.len(), 1);
        assert_eq!(model.units[0].id, "src/A/A.csproj");

        fs::remove_dir_all(&root).ok();
    }
}
