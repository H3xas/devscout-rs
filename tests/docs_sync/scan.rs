//! The `src/` source texts every page check reads.
//!
//! Two things are applied before anything is read out of a file, and both are
//! part of the check's own scope rather than an implementation detail:
//!
//! - **Non-test source.** `tests.rs`, `*_tests.rs` and any path with a `tests/`
//!   or `*_tests/` directory component are skipped outright, and every
//!   `#[cfg(test)] mod … { … }` body is removed from the files that stay: a
//!   fact only a test asserts is not a fact the pages describe.
//! - **Comments and literal bodies are blanked, not removed.** Every byte
//!   offset survives, so a hit is still reported at the line a reader sees, and
//!   a usage line quoted inside a comment cannot stand in for a usage constant.
//!
//! Everything here fails closed. An unterminated literal or block comment, a
//! string escape the scanner does not know, a brace it cannot balance, or a
//! walk that finds no file at all is an error rather than an empty result,
//! because an empty result is exactly what would let a completeness check pass
//! on a tree it never read.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// A scan that could not be completed, with the file and detail that stopped
/// it.
#[derive(Debug)]
pub struct ScanError {
    pub path: PathBuf,
    pub detail: String,
}

impl fmt::Display for ScanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path.display(), self.detail)
    }
}

/// One string literal's value and the byte offset it started at.
#[derive(Debug)]
pub struct Literal {
    pub offset: usize,
    pub value: String,
}

/// One scanned file.
#[derive(Debug)]
pub struct Scanned {
    /// Path relative to the scanned root.
    pub path: PathBuf,
    /// Source text with comments, literal bodies and `#[cfg(test)]` module
    /// bodies blanked -- every offset preserved.
    pub blanked: String,
    pub literals: Vec<Literal>,
}

impl Scanned {
    /// The one-based line an offset falls on.
    pub fn line_of(&self, offset: usize) -> usize {
        self.blanked[..offset].matches('\n').count() + 1
    }

    /// Every literal value in this file, in source order.
    pub fn values(&self) -> impl Iterator<Item = &str> {
        self.literals.iter().map(|literal| literal.value.as_str())
    }
}

/// Whether a source path is test-only, applied to the path relative to the
/// scanned root: `cli/tests/args.rs`, `cli/tests.rs` and `store_tests/mod.rs`
/// are test-only, `cli/args.rs` and `attest.rs` are not.
pub fn is_test_path(relative: &str) -> bool {
    let components: Vec<&str> = relative.split('/').collect();
    let (last, parents) = components.split_last().expect("a path has components");
    for component in parents {
        if *component == "tests" || component.ends_with("_tests") {
            return true;
        }
    }
    *last == "tests.rs" || last.ends_with("_tests.rs")
}

/// Every non-test `.rs` file under `root`, in path order.
pub fn scan_tree(root: &Path) -> Result<Vec<Scanned>, ScanError> {
    let mut files = Vec::new();
    collect(root, root, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    if files.is_empty() {
        return Err(ScanError {
            path: root.to_path_buf(),
            detail: "no non-test Rust source found under this root".to_string(),
        });
    }
    Ok(files)
}

fn collect(root: &Path, directory: &Path, into: &mut Vec<Scanned>) -> Result<(), ScanError> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let entries = fs::read_dir(directory).map_err(|error| ScanError {
        path: directory.to_path_buf(),
        detail: format!("cannot list directory: {error}"),
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| ScanError {
            path: directory.to_path_buf(),
            detail: format!("cannot read directory entry: {error}"),
        })?;
        paths.push(entry.path());
    }
    paths.sort();
    for path in paths {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if path.is_dir() {
            if name == "tests" || name.ends_with("_tests") {
                continue;
            }
            collect(root, &path, into)?;
            continue;
        }
        if !name.ends_with(".rs") || is_test_path(&relative) {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|error| ScanError {
            path: path.clone(),
            detail: format!("cannot read file: {error}"),
        })?;
        let (blanked, literals) = analyze(&path, &text)?;
        let (blanked, dropped) = drop_cfg_test_modules(&path, &blanked)?;
        let literals = literals
            .into_iter()
            .filter(|literal| {
                !dropped
                    .iter()
                    .any(|(start, end)| *start <= literal.offset && literal.offset < *end)
            })
            .collect();
        into.push(Scanned {
            path: PathBuf::from(relative),
            blanked,
            literals,
        });
    }
    Ok(())
}

// One pass over the text. Comments, literal bodies and `#[cfg(test)]` bodies
// are blanked in place -- newlines kept, every other byte replaced by a space
// -- so an offset in the result is still an offset in the file on disk.
fn analyze(path: &Path, text: &str) -> Result<(String, Vec<Literal>), ScanError> {
    let bytes = text.as_bytes();
    let mut blanked = String::with_capacity(text.len());
    let mut literals = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            let end = text[index..]
                .find('\n')
                .map(|at| index + at)
                .unwrap_or(bytes.len());
            blank(&mut blanked, &text[index..end]);
            index = end;
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            let end = block_comment_end(path, text, index)?;
            blank(&mut blanked, &text[index..end]);
            index = end;
            continue;
        }
        if let Some((opening, hashes)) = raw_string_start(text, index) {
            let end = raw_string_end(path, text, opening, hashes)?;
            literals.push(Literal {
                offset: index,
                value: text[opening + 1..end - 1 - hashes].to_string(),
            });
            blank(&mut blanked, &text[index..end]);
            index = end;
            continue;
        }
        if byte == b'"' {
            let (end, raw) = string_end(path, text, index)?;
            literals.push(Literal {
                offset: index,
                value: unescape(path, raw)?,
            });
            blank(&mut blanked, &text[index..end]);
            index = end;
            continue;
        }
        if byte == b'\'' {
            if let Some(end) = char_literal_end(text, index) {
                blank(&mut blanked, &text[index..end]);
                index = end;
                continue;
            }
        }
        // Every other byte, the `b` of `b"…"` and `br#"…"#` included, is code.
        let character = text[index..].chars().next().unwrap_or('\0');
        blanked.push(character);
        index += character.len_utf8();
    }
    Ok((blanked, literals))
}

fn blank(into: &mut String, slice: &str) {
    for character in slice.chars() {
        into.push(if character == '\n' { '\n' } else { ' ' });
    }
}

fn block_comment_end(path: &Path, text: &str, start: usize) -> Result<usize, ScanError> {
    let bytes = text.as_bytes();
    let mut depth = 1usize;
    let mut cursor = start + 2;
    while cursor < bytes.len() {
        if bytes[cursor] == b'/' && bytes.get(cursor + 1) == Some(&b'*') {
            depth += 1;
            cursor += 2;
        } else if bytes[cursor] == b'*' && bytes.get(cursor + 1) == Some(&b'/') {
            depth -= 1;
            cursor += 2;
            if depth == 0 {
                return Ok(cursor);
            }
        } else {
            cursor += 1;
        }
    }
    Err(ScanError {
        path: path.to_path_buf(),
        detail: format!(
            "unterminated block comment opened at line {}",
            text[..start].matches('\n').count() + 1
        ),
    })
}

// `r"…"`, `r#"…"#`, `br#"…"#`: the hash count is the delimiter, so the literal
// ends at the first `"` followed by exactly that many `#`.
fn raw_string_start(text: &str, index: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut cursor = index;
    if bytes.get(cursor) == Some(&b'b') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'r') {
        return None;
    }
    cursor += 1;
    let mut hashes = 0usize;
    while bytes.get(cursor) == Some(&b'#') {
        hashes += 1;
        cursor += 1;
    }
    if bytes.get(cursor) == Some(&b'"') {
        Some((cursor, hashes))
    } else {
        None
    }
}

fn raw_string_end(
    path: &Path,
    text: &str,
    opening: usize,
    hashes: usize,
) -> Result<usize, ScanError> {
    let terminator = format!("\"{}", "#".repeat(hashes));
    match text[opening + 1..].find(&terminator) {
        Some(at) => Ok(opening + 1 + at + terminator.len()),
        None => Err(ScanError {
            path: path.to_path_buf(),
            detail: format!(
                "unterminated raw string opened at line {}",
                text[..opening].matches('\n').count() + 1
            ),
        }),
    }
}

// The offset just past the closing quote, and the raw body between the quotes.
// A backslash skips the byte after it, so `\"` never ends the literal and a
// trailing backslash-newline line continuation survives to `unescape`.
fn string_end<'a>(path: &Path, text: &'a str, start: usize) -> Result<(usize, &'a str), ScanError> {
    let bytes = text.as_bytes();
    let mut cursor = start + 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 2,
            b'"' => return Ok((cursor + 1, &text[start + 1..cursor])),
            _ => cursor += 1,
        }
    }
    Err(ScanError {
        path: path.to_path_buf(),
        detail: format!(
            "unterminated string opened at line {}",
            text[..start].matches('\n').count() + 1
        ),
    })
}

// A char literal is a quote, one character or one escape, then a quote. A
// lifetime (`'a`, `'static`) has no closing quote and stays code.
fn char_literal_end(text: &str, start: usize) -> Option<usize> {
    let rest = &text[start + 1..];
    let mut characters = rest.char_indices();
    let (_, first) = characters.next()?;
    if first == '\\' {
        let (at, escape) = characters.next()?;
        if escape == 'u' || escape == 'x' {
            let close = rest.find('\'')?;
            return Some(start + 1 + close + 1);
        }
        let (close, quote) = characters.next()?;
        return (quote == '\'').then_some(start + 1 + close + len_of(quote, at));
    }
    if first == '\'' {
        return None;
    }
    let (at, character) = characters.next()?;
    (character == '\'').then_some(start + 1 + at + character.len_utf8())
}

fn len_of(character: char, fallback: usize) -> usize {
    let _ = fallback;
    character.len_utf8()
}

fn unescape(path: &Path, raw: &str) -> Result<String, ScanError> {
    let mut value = String::with_capacity(raw.len());
    let mut characters = raw.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            value.push(character);
            continue;
        }
        let Some(escape) = characters.next() else {
            return Err(ScanError {
                path: path.to_path_buf(),
                detail: "string ends in a lone backslash".to_string(),
            });
        };
        match escape {
            'n' => value.push('\n'),
            'r' => value.push('\r'),
            't' => value.push('\t'),
            '0' => value.push('\0'),
            '\\' => value.push('\\'),
            '\'' => value.push('\''),
            '"' => value.push('"'),
            // A backslash before a newline drops the newline and the leading
            // whitespace of the next line. No constant in this repository
            // depends on that whitespace, and the value has to stay comparable
            // with the bytes the constant holds.
            '\n' => {
                while matches!(characters.clone().next(), Some(' ') | Some('\t')) {
                    characters.next();
                }
            }
            // `\x41` takes two hex digits; `\u{1f600}` takes a braced run.
            'x' | 'u' => {
                let mut digits = String::new();
                if escape == 'x' {
                    for _ in 0..2 {
                        let Some(digit) = characters.next() else {
                            break;
                        };
                        digits.push(digit);
                    }
                } else {
                    if characters.next() != Some('{') {
                        return Err(ScanError {
                            path: path.to_path_buf(),
                            detail: "a unicode escape is not braced".to_string(),
                        });
                    }
                    loop {
                        match characters.next() {
                            Some('}') => break,
                            Some(digit) => digits.push(digit),
                            None => {
                                return Err(ScanError {
                                    path: path.to_path_buf(),
                                    detail: "a unicode escape is not closed".to_string(),
                                })
                            }
                        }
                    }
                }
                match u32::from_str_radix(&digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                {
                    Some(decoded) => value.push(decoded),
                    None => {
                        return Err(ScanError {
                            path: path.to_path_buf(),
                            detail: format!("cannot decode the escape \\{escape}{digits}"),
                        })
                    }
                }
            }
            other => {
                return Err(ScanError {
                    path: path.to_path_buf(),
                    detail: format!("unknown string escape \\{other}"),
                })
            }
        }
    }
    Ok(value)
}

// `#[cfg(test)] mod … { … }` bodies go after blanking, so no check reads a
// number or a name that only a test carries. An unbalanced brace is an error
// rather than a silently short body.
fn drop_cfg_test_modules(
    path: &Path,
    text: &str,
) -> Result<(String, Vec<(usize, usize)>), ScanError> {
    let mut blanked = text.to_string();
    let mut dropped = Vec::new();
    let mut search_from = 0usize;
    while let Some(attribute) = blanked[search_from..]
        .find("#[cfg(test)]")
        .map(|at| at + search_from)
    {
        let Some(brace) = blanked[attribute..].find('{').map(|at| at + attribute) else {
            break;
        };
        let Some(end) = matching_brace(&blanked, brace) else {
            return Err(ScanError {
                path: path.to_path_buf(),
                detail: format!(
                    "unbalanced brace after the test module at line {}",
                    text[..attribute].matches('\n').count() + 1
                ),
            });
        };
        let replacement: String = blanked[attribute..=end]
            .chars()
            .map(|character| if character == '\n' { '\n' } else { ' ' })
            .collect();
        blanked.replace_range(attribute..=end, &replacement);
        dropped.push((attribute, end + 1));
        search_from = end + 1;
    }
    Ok((blanked, dropped))
}

fn matching_brace(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, character) in text[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}
