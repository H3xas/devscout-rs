//! The facts the four pages are generated from, read out of non-test `src/`
//! source.
//!
//! Every function here takes already-scanned files, so a check and a renderer
//! read the same text, and a fact that cannot be established is reported with
//! the file and line a reader has to open rather than dropped.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::scan::Scanned;

/// The prefix every verb's usage line starts with.
pub const USAGE_PREFIX: &str = "usage: devscout";

/// The environment names the pages document beyond the scanned ones.
pub const HOME: &str = "HOME";

/// A fact found at a place in the source.
#[derive(Debug)]
pub struct Located {
    pub file: PathBuf,
    pub line: usize,
    pub detail: String,
}

impl Located {
    fn new(scanned: &Scanned, offset: usize, detail: String) -> Self {
        Self {
            file: scanned.path.clone(),
            line: scanned.line_of(offset),
            detail,
        }
    }

    pub fn describe(&self) -> String {
        format!("{}:{}: {}", self.file.display(), self.line, self.detail)
    }
}

/// Every non-test string literal that starts with the usage prefix.
pub fn usage_literals(files: &[Scanned]) -> Vec<Located> {
    let mut found = Vec::new();
    for scanned in files {
        for literal in &scanned.literals {
            if literal.value.starts_with(USAGE_PREFIX) {
                found.push(Located::new(scanned, literal.offset, literal.value.clone()));
            }
        }
    }
    found
}

/// Every environment name the source reads: `SCOUT_`-prefixed names carried by
/// a string literal, plus `HOME`.
pub fn environment_names(files: &[Scanned]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for scanned in files {
        for literal in &scanned.literals {
            for name in scout_names(&literal.value) {
                names.insert(name);
            }
            if literal.value == HOME {
                names.insert(HOME.to_string());
            }
        }
    }
    names
}

// A `SCOUT_` name is matched whole, so a literal that merely mentions the
// prefix (`"scout/..."`) contributes nothing and a literal naming two
// variables contributes both.
fn scout_names(value: &str) -> Vec<String> {
    let bytes = value.as_bytes();
    let mut names = Vec::new();
    let mut index = 0usize;
    while let Some(found) = value[index..].find("SCOUT_") {
        let start = index + found;
        let mut end = start;
        while end < bytes.len() {
            let byte = bytes[end];
            if byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_' {
                end += 1;
            } else {
                break;
            }
        }
        if end > start {
            names.push(value[start..end].to_string());
        }
        index = end.max(start + 1);
    }
    names
}

/// The exit-status constants `src/exit.rs` declares, by name, with the value
/// each initializes to. A constant the scanner cannot read is left out and
/// reported by the caller's own comparison, not guessed at.
pub fn exit_constants(files: &[Scanned]) -> BTreeMap<String, i64> {
    let mut constants = BTreeMap::new();
    for scanned in files {
        if scanned.path != PathBuf::from("exit.rs") {
            continue;
        }
        for line in scanned.blanked.lines() {
            let trimmed = line.trim();
            let Some(rest) = trimmed.strip_prefix("pub const ") else {
                continue;
            };
            let Some((name, tail)) = rest.split_once(':') else {
                continue;
            };
            let Some((kind, value)) = tail.split_once('=') else {
                continue;
            };
            if kind.trim() != "i32" {
                continue;
            }
            if let Some(number) = value.trim().strip_suffix(';') {
                if let Ok(parsed) = number.trim().parse::<i64>() {
                    constants.insert(name.trim().to_string(), parsed);
                }
            }
        }
    }
    constants
}

/// A bare integer used as a status: the first element of a tuple a status
/// function returns, an integer a status function returns or yields from a match
/// arm, an error value that is a bare integer, or an argument to `process::exit`.
pub fn numeric_status_sites(files: &[Scanned]) -> Vec<Located> {
    let mut found = Vec::new();
    for scanned in files {
        for (start, end) in status_functions(&scanned.blanked) {
            let body = &scanned.blanked[start..end];
            let mut shapes = tuple_status_literals(body);
            shapes.extend(bare_status_literals(body));
            shapes.sort_unstable();
            for offset in shapes {
                let line = scanned.line_of(start + offset);
                found.push(Located {
                    file: scanned.path.clone(),
                    line,
                    detail: format!(
                        "integer literal used as a status: {}",
                        excerpt(body, offset)
                    ),
                });
            }
        }
        for offset in exit_call_literals(&scanned.blanked) {
            found.push(Located::new(
                scanned,
                offset,
                "integer literal passed to a process exit".to_string(),
            ));
        }
    }
    found.sort_by(|left, right| (&left.file, left.line).cmp(&(&right.file, right.line)));
    found
}

fn excerpt(body: &str, offset: usize) -> String {
    let rest = &body[offset..];
    let stop = rest.find('\n').unwrap_or(rest.len());
    rest[..stop].trim().to_string()
}

// (start, end) of every function body whose return type mentions `i32`: the
// verbs that hand a status back, and the shape a bare status literal would
// appear in.
fn status_functions(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut search_from = 0usize;
    while let Some(at) = text[search_from..].find("fn ") {
        let start = search_from + at;
        let Some(open) = text[start..].find('(').map(|offset| start + offset) else {
            break;
        };
        let Some(params_end) = matching(text, open, '(', ')') else {
            break;
        };
        let Some(body_open) = text[params_end..]
            .find('{')
            .map(|offset| params_end + offset)
        else {
            search_from = params_end + 1;
            continue;
        };
        let signature = &text[start..body_open];
        if !mentions_i32(&signature) {
            search_from = body_open + 1;
            continue;
        }
        let Some(body_end) = matching(text, body_open, '{', '}') else {
            break;
        };
        ranges.push((body_open + 1, body_end));
        search_from = body_end + 1;
    }
    ranges
}

fn mentions_i32(signature: &str) -> bool {
    let Some(arrow) = signature.find("->") else {
        return false;
    };
    signature[arrow..].contains("i32")
}

fn matching(text: &str, open: usize, opener: char, closer: char) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, character) in text[open..].char_indices() {
        if character == opener {
            depth += 1;
        } else if character == closer {
            depth -= 1;
            if depth == 0 {
                return Some(open + offset);
            }
        }
    }
    None
}

// The offset of every integer literal that opens a tuple in this body. A `(`
// preceded by an identifier, `!`, `]` or `)` calls or indexes instead, unless
// the token before it is `return` or `break`, which return a tuple.
fn tuple_status_literals(body: &str) -> Vec<usize> {
    let bytes = body.as_bytes();
    let mut found = Vec::new();
    for (offset, character) in body.char_indices() {
        if character != '(' {
            continue;
        }
        let before = body[..offset].trim_end();
        let opens_tuple = match before.chars().last() {
            None => true,
            Some(previous) => {
                !(previous.is_alphanumeric()
                    || previous == '_'
                    || previous == '!'
                    || previous == ']'
                    || previous == ')')
                    || before.ends_with("return")
                    || before.ends_with("break")
            }
        };
        if !opens_tuple {
            continue;
        }
        let mut cursor = offset + 1;
        while matches!(bytes.get(cursor), Some(b' ') | Some(b'\n')) {
            cursor += 1;
        }
        let digits_start = cursor;
        while matches!(bytes.get(cursor), Some(byte) if byte.is_ascii_digit()) {
            cursor += 1;
        }
        if cursor == digits_start {
            continue;
        }
        let mut after = cursor;
        while matches!(bytes.get(after), Some(b' ') | Some(b'\n')) {
            after += 1;
        }
        if bytes.get(after) == Some(&b',') {
            found.push(offset + 1 + (digits_start - offset - 1));
        }
    }
    found
}

// `exit(0)`: any call whose argument is nothing but digits.
fn exit_call_literals(text: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut search_from = 0usize;
    while let Some(at) = text[search_from..].find("exit(") {
        let start = search_from + at + "exit(".len();
        let Some(close) = text[start..].find(')').map(|offset| start + offset) else {
            break;
        };
        let argument = text[start..close].trim();
        if !argument.is_empty() && argument.bytes().all(|byte| byte.is_ascii_digit()) {
            found.push(start + text[start..close].find(argument).unwrap_or(0));
        }
        search_from = close + 1;
    }
    found
}

/// Literals that carry a listed artifact name somewhere other than the one
/// constant that defines it. `owner` is the file whose constant holds the
/// name; a name with no literal anywhere (a name composed at run time) is
/// reported for the caller to compare.
pub fn names_outside_their_constant(
    files: &[Scanned],
    owner: &str,
    listed: &[String],
) -> Vec<Located> {
    let mut found = Vec::new();
    for scanned in files {
        for literal in &scanned.literals {
            for name in listed {
                if !literal.value.contains(name) {
                    continue;
                }
                let defining = scanned.path == PathBuf::from(owner) && literal.value == *name;
                if !defining {
                    found.push(Located::new(
                        scanned,
                        literal.offset,
                        format!(
                            "{} names the artifact {name} outside its constant",
                            scanned.path.display()
                        ),
                    ));
                }
            }
        }
    }
    found
}

// The other ways a status function can hand back a bare number: `return 1;`, a
// match arm whose value is a literal (`Ok(()) => 0,`), and an error value that
// is one (`Err(1)`).
fn bare_status_literals(body: &str) -> Vec<usize> {
    let bytes = body.as_bytes();
    let mut found = Vec::new();
    for (offset, pattern) in [("return ", 0usize), ("=> ", 0usize)].into_iter() {
        let _ = pattern;
        let mut search_from = 0usize;
        while let Some(at) = body[search_from..].find(offset) {
            let start = search_from + at + offset.len();
            let mut cursor = start;
            while matches!(bytes.get(cursor), Some(b' ') | Some(b'\n')) {
                cursor += 1;
            }
            let digits_start = cursor;
            while matches!(bytes.get(cursor), Some(byte) if byte.is_ascii_digit()) {
                cursor += 1;
            }
            if cursor > digits_start {
                let next = bytes.get(cursor).copied();
                let ends_a_value = matches!(next, Some(b';') | Some(b',') | Some(b'}') | None);
                if ends_a_value {
                    found.push(digits_start);
                }
            }
            search_from = cursor.max(start + 1);
        }
    }
    // A block whose whole value is a literal: `if ok { 0 } else { 1 }`.
    let mut search_from = 0usize;
    while let Some(at) = body[search_from..].find('{') {
        let start = search_from + at + 1;
        let mut cursor = start;
        while matches!(bytes.get(cursor), Some(b' ') | Some(b'\n')) {
            cursor += 1;
        }
        let digits_start = cursor;
        while matches!(bytes.get(cursor), Some(byte) if byte.is_ascii_digit()) {
            cursor += 1;
        }
        if cursor > digits_start {
            let mut after = cursor;
            while matches!(bytes.get(after), Some(b' ') | Some(b'\n')) {
                after += 1;
            }
            if bytes.get(after) == Some(&b'}') {
                found.push(digits_start);
            }
        }
        search_from = cursor.max(start);
    }
    let mut search_from = 0usize;
    while let Some(at) = body[search_from..].find("Err(") {
        let start = search_from + at + "Err(".len();
        let mut cursor = start;
        while matches!(bytes.get(cursor), Some(b' ') | Some(b'\n')) {
            cursor += 1;
        }
        let digits_start = cursor;
        while matches!(bytes.get(cursor), Some(byte) if byte.is_ascii_digit()) {
            cursor += 1;
        }
        if cursor > digits_start {
            let mut after = cursor;
            while matches!(bytes.get(after), Some(b' ') | Some(b'\n')) {
                after += 1;
            }
            if bytes.get(after) == Some(&b')') {
                found.push(digits_start);
            }
        }
        search_from = cursor.max(start + 1);
    }
    found
}

/// The `pub const` declarations of one scanned module, by name: what the module
/// says its own constants are called.
pub fn usage_declarations(scanned: &Scanned) -> BTreeMap<String, String> {
    let mut declared = BTreeMap::new();
    for literal in &scanned.literals {
        let before = &scanned.blanked[..literal.offset];
        let Some(at) = before.rfind("pub const ") else {
            continue;
        };
        let rest = &before[at + "pub const ".len()..];
        let name: String = rest
            .chars()
            .take_while(|character| character.is_alphanumeric() || *character == '_')
            .collect();
        if !name.is_empty() {
            declared.insert(name, literal.value.clone());
        }
    }
    declared
}
