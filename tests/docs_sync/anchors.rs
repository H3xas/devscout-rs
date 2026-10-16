//! Heading anchors: the slug a Markdown heading answers to, and the links from
//! the repository's tracked Markdown into the generated pages.
//!
//! A page that drops a heading its readers link to is a broken link in the
//! published documentation, so the check resolves every inbound link against
//! the headings the generated page actually carries.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The GitHub-style slug of a heading: lowercase, spaces to hyphens, everything
/// that is neither alphanumeric nor a hyphen or underscore dropped.
pub fn slug(heading: &str) -> String {
    let mut slug = String::with_capacity(heading.len());
    for character in heading.trim().chars() {
        if character.is_alphanumeric() {
            slug.extend(character.to_lowercase());
        } else if character == '-' || character == '_' {
            slug.push(character);
        } else if character.is_whitespace() {
            slug.push('-');
        }
    }
    slug
}

/// A link from one tracked Markdown file into a generated page.
#[derive(Debug)]
pub struct InboundLink {
    pub from: PathBuf,
    pub target: String,
    pub anchor: String,
}

/// Every link from a tracked Markdown file to one of `pages`, by page file
/// name. `git ls-files` supplies the set, so a file the repository does not
/// track cannot make the check pass or fail.
pub fn inbound_links(root: &Path, pages: &[&str]) -> Result<Vec<InboundLink>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--", "*.md"])
        .output()
        .map_err(|error| format!("cannot run git ls-files: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git ls-files failed (exit {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let listing = String::from_utf8(output.stdout)
        .map_err(|error| format!("git ls-files returned a non-UTF-8 path: {error}"))?;
    let mut links = Vec::new();
    for relative in listing.split('\0').filter(|path| !path.is_empty()) {
        let path = root.join(relative);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let directory = Path::new(relative).parent().unwrap_or(Path::new(""));
        for target in markdown_links(&text) {
            let Some((file, fragment)) = target.split_once('#') else {
                continue;
            };
            let name = file.rsplit('/').next().unwrap_or(file).to_string();
            if !pages.contains(&name.as_str()) {
                continue;
            }
            let resolved = directory.join(file);
            if !resolved.exists() {
                return Err(format!(
                    "{} links {target}, which does not resolve next to it",
                    path.display()
                ));
            }
            links.push(InboundLink {
                from: PathBuf::from(relative),
                target: name,
                anchor: fragment.to_string(),
            });
        }
    }
    links.sort_by(|left, right| {
        (&left.target, &left.anchor, &left.from).cmp(&(&right.target, &right.anchor, &right.from))
    });
    Ok(links)
}

// `[text](target)`: the target of every inline link whose target carries a
// fragment. Images and reference-style links are not page links.
fn markdown_links(text: &str) -> Vec<String> {
    let mut links = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b']' || bytes.get(index + 1) != Some(&b'(') {
            index += 1;
            continue;
        }
        if index > 0 && bytes[index - 1] == b'!' {
            index += 1;
            continue;
        }
        let start = index + 2;
        let Some(close) = text[start..].find(')').map(|offset| start + offset) else {
            break;
        };
        let target = text[start..close].trim().to_string();
        if target.contains('#') && !target.starts_with("http") {
            links.push(target);
        }
        index = close + 1;
    }
    links
}

/// The slugs a page's headings answer to.
pub fn page_slugs(content: &str) -> BTreeMap<String, String> {
    let mut slugs = BTreeMap::new();
    for line in content.lines() {
        let heading = line
            .strip_prefix("### ")
            .or_else(|| line.strip_prefix("## "));
        if let Some(heading) = heading {
            let heading = heading.trim();
            slugs.insert(slug(heading), heading.to_string());
        }
    }
    slugs
}

/// The inbound links whose anchor no heading of the linked page answers to.
pub fn missing_anchors(pages: &[(&str, String)], links: &[InboundLink]) -> Vec<String> {
    let slugs: BTreeMap<&str, BTreeMap<String, String>> = pages
        .iter()
        .map(|(name, content)| (*name, page_slugs(content)))
        .collect();
    let mut missing = Vec::new();
    for link in links {
        let Some(page) = slugs.get(link.target.as_str()) else {
            missing.push(format!(
                "{} links {}, which is not one of the generated pages",
                link.from.display(),
                link.target
            ));
            continue;
        };
        if !page.contains_key(&link.anchor) {
            missing.push(format!(
                "{} links {}#{}, and no heading of that page answers to that anchor",
                link.from.display(),
                link.target,
                link.anchor
            ));
        }
    }
    missing
}
