// Source enumeration: skip set, extension set, and a deterministic ordering.
// The order feeds artifact determinism, so it is load-bearing.
//
// ## Ordering semantics (load-bearing)
//
// `fs::read_dir` yields directory entries in an unspecified, platform-dependent
// order, so this walk sorts each directory's entries by `file_name()` before
// recursing/emitting. `OsString`/`str` `Ord` is byte-wise UTF-8 comparison,
// which orders mixed case the ASCII way ("Banana" before "apple" -- 'B' 0x42 <
// 'a' 0x61). The result is a stable, sorted depth-first pre-order that does not
// depend on the underlying filesystem's native order.
//
// Directories and files are NOT grouped separately -- a single sorted pass over
// one `read_dir` call interleaves them by name, and a directory's full subtree
// is walked (recursion) before the parent moves to the next sibling, i.e. plain
// sorted depth-first pre-order.
//
// ## Edge cases
//
// - **Symlinks are neither followed nor emitted.**
//   `std::fs::DirEntry::file_type()` reflects the directory entry's own type
//   without following it, so a symlink entry has `is_symlink() == true` and
//   matches neither `is_dir()` nor `is_file()` below -- a symlinked dir is
//   never recursed into and a symlinked file is never emitted, by construction.
//   Caveat: this is about symlink ENTRIES encountered *while listing a
//   directory*. A symlink passed as a `dirs` scope element behaves differently
//   -- see next point.
// - **A `dirs` scope element that is itself a symlink IS followed**, because
//   `Path::exists()` and `fs::read_dir()` resolve the given path the normal OS
//   way (`stat`, not `lstat`, on the path itself). Noted so the asymmetry with
//   the in-listing case above isn't mistaken for a bug.
// - **Unreadable directory fails the whole call.** `read_dir` on a
//   permission-denied dir returns an `io::Error`, which `?` propagates -- one
//   unreadable directory fails the entire `list_source_files` call with no
//   partial result.
// - **A `dirs` element that is a file, not a directory, also fails**
//   (`NotADirectory`-flavored `io::Error` from `read_dir`, propagated by `?`).
// - **Hidden dotfiles/dirs are walked normally.** Only the exact names in
//   `SKIP_DIRS` are skipped (notably `.git` and `.scout`); any other dotdir
//   (e.g. `.dotdir/`) is recursed into and dotfiles are subject to the same
//   extension check as anything else -- a bare dotfile with no extension after
//   the leading dot, e.g. `.gitignore`, has its whole name treated as the
//   "extension" (last `.` at index 0) and never matches `SOURCE_EXT`.
// - **Nonexistent `dirs` scope elements are silently skipped**
//   (`Path::exists()` is false -> no error).

use std::ffi::OsStr;
use std::fs::{self, DirEntry};
use std::io;
use std::path::Path;

use crate::repo;

/// Directory names skipped outright (and never recursed into), regardless
/// of depth. Exact-name match against a single path component -- not a
/// glob, not a suffix/prefix match.
pub const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    ".scout",
    "bin",
    "obj",
    "dist",
    "coverage",
    ".next",
    "target",
];

/// File extensions (including the leading dot) eligible for inclusion.
/// Case-sensitive, exact match on the substring from the *last* `.` in the
/// file name onward.
pub const SOURCE_EXT: &[&str] = &[".ts", ".tsx", ".js", ".jsx", ".cs", ".json", ".md", ".xaml", ".resw", ".resx"];

/// Enumerates source files under each scope directory. `root` must already be
/// an absolute, normalized path (the caller's responsibility; root discovery
/// always hands back an absolute path).
///
/// `dirs` elements are directory-scope names relative to `root`; `"."` means
/// `root` itself. Returns repo-relative (`/`-joined) paths in the deterministic
/// sorted depth-first order described in the module docs.
///
/// Returns `Err` and abandons the whole call on the first unreadable directory
/// or non-directory scope element (see module docs).
pub fn list_source_files(root: &Path, dirs: &[String]) -> io::Result<Vec<String>> {
    let mut out = Vec::new();
    for d in dirs {
        let abs = if d == "." {
            root.to_path_buf()
        } else {
            root.join(d)
        };
        if abs.exists() {
            walk(root, &abs, &mut out)?;
        }
    }
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> io::Result<()> {
    let mut entries: Vec<DirEntry> = fs::read_dir(dir)?.collect::<io::Result<Vec<_>>>()?;
    // Sort each directory's entries by name for a deterministic order (see
    // module docs). `OsStr`'s `Ord` is byte-wise UTF-8 comparison.
    entries.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    for entry in entries {
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        if file_type.is_dir() {
            if is_skip_dir(&name) {
                continue;
            }
            walk(root, &entry.path(), out)?;
        } else if file_type.is_file() {
            let name_str = name.to_string_lossy();
            if SOURCE_EXT.contains(&extension_of(&name_str)) {
                out.push(repo::rel_path(root, &entry.path()));
            }
        }
        // Symlinks (file_type.is_symlink()) match neither arm above and
        // are silently skipped -- see module docs.
    }
    Ok(())
}

fn is_skip_dir(name: &OsStr) -> bool {
    match name.to_str() {
        Some(s) => SKIP_DIRS.contains(&s),
        // A non-UTF-8 directory name can never equal any (UTF-8, ASCII)
        // SKIP_DIRS entry.
        None => false,
    }
}

// The substring from the last `.` in `name` to the end, or `""` if `name` has
// no `.`.
fn extension_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(idx) => &name[idx..],
        None => "",
    }
}

/// Detailed variant of `default_purpose`, exposing whether the returned line
/// came from the comment-marker branch (`match_comment`). `mapcmd.rs`'s TS/JS
/// dispatch needs `is_comment` to decide whether an AST purpose gets a leading
/// comment-text prefix.
pub struct DefaultPurposeDetail {
    pub text: String,
    pub is_comment: bool,
}

/// Best-effort heuristic purpose line from a source file's first 15 non-blank
/// lines, plus whether that line was comment-derived. Returns `("", false)` for
/// an unreadable file, or when no qualifying line is found (the latter is
/// actually unreachable in practice, see below).
///
/// Decode is lossy so a strict UTF-8 read cannot drop real files carrying stray
/// non-UTF-8 bytes (seen in the wild in comments).
pub fn default_purpose_detailed(root: &Path, rel: &str) -> DefaultPurposeDetail {
    let text = match fs::read(root.join(rel)) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(_) => return DefaultPurposeDetail { text: String::new(), is_comment: false },
    };
    for raw in text.split('\n').take(15) {
        // `char::is_whitespace` does not include U+FEFF (ZWNBSP/BOM), so a
        // BOM-prefixed first line would keep the BOM in the purpose text
        // without stripping it explicitly here.
        let line = raw.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}');
        if line.is_empty() {
            continue;
        }
        if let Some(m) = match_comment(line) {
            return DefaultPurposeDetail { text: truncate(m), is_comment: true };
        }
        if starts_with_namespace_ws(line) {
            return DefaultPurposeDetail { text: truncate(line), is_comment: false };
        }
        if starts_with_any(
            line,
            &["export", "public", "class", "interface", "def ", "function "],
        ) {
            return DefaultPurposeDetail { text: truncate(line), is_comment: false };
        }
        // This unconditional fall-through returns the first non-blank line
        // regardless of whether any pattern above matched -- so the loop never
        // advances past line 1 in practice, and the final `return ""` after the
        // loop is dead code for any file with at least one non-blank line among
        // the first 15. Kept as-is deliberately.
        return DefaultPurposeDetail { text: truncate(line), is_comment: false };
    }
    DefaultPurposeDetail { text: String::new(), is_comment: false }
}

/// The heuristic purpose line for a source file -- `default_purpose_detailed(..).text`.
pub fn default_purpose(root: &Path, rel: &str) -> String {
    default_purpose_detailed(root, rel).text
}

// Matches a leading `//`, `///`, `*`, or `#` (no other leading whitespace,
// since `line` is already trimmed), then optional whitespace, then a required
// non-empty remainder.
fn match_comment(line: &str) -> Option<&str> {
    let rest = if let Some(r) = line.strip_prefix("///") {
        r
    } else if let Some(r) = line.strip_prefix("//") {
        r
    } else if let Some(r) = line.strip_prefix('*') {
        r
    } else if let Some(r) = line.strip_prefix('#') {
        r
    } else {
        return None;
    };
    let trimmed = rest.trim_start();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

// Matches exactly the literal keyword "namespace" followed by at least one
// whitespace character.
fn starts_with_namespace_ws(line: &str) -> bool {
    line.strip_prefix("namespace")
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_whitespace()))
}

fn starts_with_any(line: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| line.starts_with(p))
}

fn truncate(s: &str) -> String {
    // Cap at 100 characters, replacing the tail past 97 with "...". Length is
    // counted in `char`s (Unicode scalar values).
    let char_count = s.chars().count();
    if char_count > 100 {
        let truncated: String = s.chars().take(97).collect();
        format!("{truncated}...")
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "scout-walk-test-{}-{}-{}",
            label,
            std::process::id(),
            nanos
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

    #[test]
    fn skips_configured_dirs_and_filters_by_extension() {
        let root = scratch_dir("basic");
        write_file(&root.join("src/a.ts"), "export const x = 1;\n");
        write_file(&root.join("src/sub/b.cs"), "namespace Foo.Bar;\nclass B {}\n");
        write_file(&root.join("src/node_modules/pkg/c.js"), "ignored");
        write_file(&root.join("src/bin/d.ts"), "ignored");

        let mut files = list_source_files(&root, &["src".to_string()]).unwrap();
        files.sort();
        assert_eq!(files, vec!["src/a.ts".to_string(), "src/sub/b.cs".to_string()]);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn default_purpose_extracts_leading_comment_and_namespace() {
        let root = scratch_dir("purpose");
        write_file(
            &root.join("src/a.ts"),
            "// Handles group creation\nexport const x = 1;\n",
        );
        write_file(&root.join("src/sub/b.cs"), "namespace Foo.Bar;\nclass B {}\n");

        let a = default_purpose(&root, "src/a.ts");
        assert!(a.to_lowercase().contains("group creation"), "got {a:?}");

        let b = default_purpose(&root, "src/sub/b.cs");
        assert!(b.contains("Foo.Bar"), "got {b:?}");

        fs::remove_dir_all(&root).ok();
    }

    // -- default_purpose_detailed's is_comment flag ------------------------

    #[test]
    fn default_purpose_detailed_flags_a_comment_derived_match() {
        let root = scratch_dir("purpose-detailed-comment");
        write_file(&root.join("src/a.ts"), "// Handles group creation\nexport const x = 1;\n");
        let d = default_purpose_detailed(&root, "src/a.ts");
        assert!(d.is_comment, "leading `//` line must be flagged comment-derived");
        assert!(d.text.to_lowercase().contains("group creation"), "got {:?}", d.text);
        assert_eq!(default_purpose(&root, "src/a.ts"), d.text);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn default_purpose_detailed_does_not_flag_a_code_first_line() {
        let root = scratch_dir("purpose-detailed-code");
        write_file(&root.join("src/a.ts"), "export const x = 1;\n// a comment on line 2, never reached\n");
        let d = default_purpose_detailed(&root, "src/a.ts");
        assert!(!d.is_comment, "a code first line must not be flagged comment-derived");
        assert_eq!(d.text, "export const x = 1;");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn default_purpose_detailed_on_unreadable_file_is_empty_and_not_comment() {
        let root = scratch_dir("purpose-detailed-missing");
        let d = default_purpose_detailed(&root, "src/does-not-exist.ts");
        assert_eq!(d.text, "");
        assert!(!d.is_comment);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn skip_dir_names_are_exact_match_only() {
        let root = scratch_dir("skip-exact");
        // "bin2" is not "bin" -- must be walked, not skipped.
        write_file(&root.join("bin2/kept.ts"), "kept");
        write_file(&root.join("bin/dropped.ts"), "dropped");

        let files = list_source_files(&root, &[".".to_string()]).unwrap();
        assert_eq!(files, vec!["bin2/kept.ts".to_string()]);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn nonexistent_scope_element_is_skipped_silently() {
        let root = scratch_dir("nonexistent");
        write_file(&root.join("src/a.ts"), "x");

        let files =
            list_source_files(&root, &["src".to_string(), "does-not-exist".to_string()]).unwrap();
        assert_eq!(files, vec!["src/a.ts".to_string()]);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn symlinked_entries_are_neither_followed_nor_emitted() {
        let root = scratch_dir("symlinks");
        write_file(&root.join("real/real.ts"), "real");
        write_file(&root.join("outside.ts"), "outside");

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(root.join("real"), root.join("linkdir")).unwrap();
            symlink(root.join("outside.ts"), root.join("linkfile.ts")).unwrap();
            symlink(root.join("does-not-exist"), root.join("broken.ts")).unwrap();

            let mut files = list_source_files(&root, &[".".to_string()]).unwrap();
            files.sort();
            assert_eq!(
                files,
                vec!["outside.ts".to_string(), "real/real.ts".to_string()]
            );
        }

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unreadable_directory_propagates_as_error() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let root = scratch_dir("unreadable");
            write_file(&root.join("readable/a.ts"), "x");
            write_file(&root.join("noperm/b.ts"), "x");
            fs::set_permissions(root.join("noperm"), fs::Permissions::from_mode(0o000)).unwrap();

            let result = list_source_files(&root, &[".".to_string()]);
            assert!(result.is_err(), "expected an error, got {result:?}");

            // Restore perms so the scratch dir can be cleaned up.
            fs::set_permissions(root.join("noperm"), fs::Permissions::from_mode(0o755)).unwrap();
            fs::remove_dir_all(&root).ok();
        }
    }
}
