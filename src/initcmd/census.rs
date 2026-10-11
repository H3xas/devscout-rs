// The language census `init` prints: every file `walk::list_source_files`
// enumerates, counted by extension and grouped by support tier. Kept in its
// own sibling file (not inline in `initcmd.rs`) because that file sits at its
// `tools/size-ratchet.toml` ceiling.

use std::path::Path;

use crate::walk;

// C# gets the complete extraction path. TypeScript-family files are indexed
// and resolved into the graph, but with narrower edge coverage. Rust files
// are indexed and graphed through path resolution: edges a path names,
// never a method call typed by its receiver. The remaining
// extensions enumerated by `walk::SOURCE_EXT` are counted but not indexed.
const FULLY_SUPPORTED_EXT: &[&str] = &[".cs"];
const INDEXED_AND_GRAPHED_EXT: &[&str] =
    &[".ts", ".tsx", ".js", ".jsx", ".mts", ".cts", ".mjs", ".cjs"];
const PATH_RESOLVED_EXT: &[&str] = &[".rs"];

// ---------------------------------------------------------------------------
// Language census. Rather than a second, independent recursive walker, this
// counts extensions over exactly the file set `walk::list_source_files` already
// enumerates (its `SKIP_DIRS`/`SOURCE_EXT` contract). Bounded consequence, noted
// rather than solved: a real-but-unsupported language outside `walk::SOURCE_EXT`
// entirely (Python, Go, ...) is invisible to this census, same as it is
// invisible to `devscout map` itself.
// ---------------------------------------------------------------------------

pub(super) fn census_line(root: &Path) -> String {
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
    let (indexed_and_graphed, other): (Vec<_>, Vec<_>) = other
        .into_iter()
        .partition(|(e, _)| INDEXED_AND_GRAPHED_EXT.contains(&e.as_str()));
    let (path_resolved, not_indexed): (Vec<_>, Vec<_>) = other
        .into_iter()
        .partition(|(e, _)| PATH_RESOLVED_EXT.contains(&e.as_str()));
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
    if !path_resolved.is_empty() {
        groups.push(format!(
            "{} (indexed and graphed, path-resolved edges)",
            fmt_group(&path_resolved)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(prefix: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "scout-initcmd-census-{prefix}-{}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
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

    // -- language tier sanity ----------------------------------------------

    #[test]
    fn census_language_tiers_are_explicit() {
        assert_eq!(FULLY_SUPPORTED_EXT, &[".cs"]);
        assert_eq!(
            INDEXED_AND_GRAPHED_EXT,
            &[".ts", ".tsx", ".js", ".jsx", ".mts", ".cts", ".mjs", ".cjs"]
        );
    }

    #[test]
    fn census_line_counts_every_module_suffix_in_the_indexed_tier() {
        let dir = temp_dir("census-module-suffixes");
        fs::write(dir.join("a.mjs"), "export const a = 1;").unwrap();
        fs::write(dir.join("b.cjs"), "module.exports = {};").unwrap();
        fs::write(dir.join("c.mts"), "export const c = 1;").unwrap();
        fs::write(dir.join("d.cts"), "export const d = 1;").unwrap();
        fs::write(dir.join("e.MJS"), "export const e = 1;").unwrap();
        assert_eq!(
            census_line(&dir),
            "languages: 1 .cjs, 1 .cts, 1 .mjs, 1 .mts (indexed and graphed, narrower edge coverage)"
        );
    }

    #[test]
    fn census_line_puts_rust_between_the_typescript_tier_and_the_unindexed_files() {
        let dir = temp_dir("census-rust");
        fs::write(dir.join("A.cs"), "namespace X { class A {} }").unwrap();
        fs::write(dir.join("b.ts"), "export const x = 1;").unwrap();
        fs::write(dir.join("c.rs"), "fn c() {}").unwrap();
        fs::write(dir.join("d.rs"), "fn d() {}").unwrap();
        fs::write(dir.join("e.md"), "# doc").unwrap();
        assert_eq!(
            census_line(&dir),
            "languages: 1 .cs (fully supported); 1 .ts (indexed and graphed, narrower edge coverage); 2 .rs (indexed and graphed, path-resolved edges); 1 .md (present, not indexed)"
        );
    }

    #[test]
    fn census_line_rust_only_prints_only_the_rust_tier() {
        let dir = temp_dir("census-rust-only");
        fs::write(dir.join("lib.rs"), "fn f() {}").unwrap();
        assert_eq!(
            census_line(&dir),
            "languages: 1 .rs (indexed and graphed, path-resolved edges)"
        );
    }

    #[test]
    fn census_rust_tier_is_explicit() {
        assert_eq!(PATH_RESOLVED_EXT, &[".rs"]);
    }
}
