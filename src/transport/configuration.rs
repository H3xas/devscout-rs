//! Reading the values a project's settings files state for a configuration
//! key, the way the .NET JSON configuration provider reads them: comments
//! and trailing commas are allowed, and key parts match without regard to
//! ASCII case.

use std::fs;
use std::path::Path;

use super::catalogue::Val;
use super::fact_types::{ConfigValue, ValueObj};
use super::site_types::Parsed;
use super::values::classify;

const UNREADABLE: &str = "unreadable-configuration";

#[derive(Debug, PartialEq)]
enum Json {
    Str(String),
    Object(Vec<(String, Json)>),
    Other,
}

enum Found {
    Missing,
    Text(String),
    Unreadable,
}

struct Reader<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl Reader<'_> {
    fn skip(&mut self) -> Option<()> {
        loop {
            match self.chars.peek() {
                Some(c) if c.is_whitespace() => {
                    self.chars.next();
                }
                Some('/') => {
                    self.chars.next();
                    match self.chars.next()? {
                        '/' => while self.chars.next().is_some_and(|c| c != '\n') {},
                        '*' => self.block_comment()?,
                        _ => return None,
                    }
                }
                _ => return Some(()),
            }
        }
    }

    fn block_comment(&mut self) -> Option<()> {
        let mut star = false;
        loop {
            let c = self.chars.next()?;
            if star && c == '/' {
                return Some(());
            }
            star = c == '*';
        }
    }

    fn value(&mut self) -> Option<Json> {
        self.skip()?;
        match *self.chars.peek()? {
            '{' => self.object(),
            '[' => self.array(),
            '"' => self.string().map(Json::Str),
            _ => self.scalar(),
        }
    }

    fn object(&mut self) -> Option<Json> {
        self.chars.next();
        let mut entries = Vec::new();
        loop {
            self.skip()?;
            if self.chars.peek() == Some(&'}') {
                self.chars.next();
                return Some(Json::Object(entries));
            }
            let key = self.string()?;
            self.skip()?;
            if self.chars.next()? != ':' {
                return None;
            }
            entries.push((key, self.value()?));
            self.skip()?;
            match self.chars.next()? {
                ',' => {}
                '}' => return Some(Json::Object(entries)),
                _ => return None,
            }
        }
    }

    fn array(&mut self) -> Option<Json> {
        self.chars.next();
        loop {
            self.skip()?;
            if self.chars.peek() == Some(&']') {
                self.chars.next();
                return Some(Json::Other);
            }
            self.value()?;
            self.skip()?;
            match self.chars.next()? {
                ',' => {}
                ']' => return Some(Json::Other),
                _ => return None,
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        if self.chars.next()? != '"' {
            return None;
        }
        let mut out = String::new();
        loop {
            match self.chars.next()? {
                '"' => return Some(out),
                '\\' => out.push(self.escape()?),
                c => out.push(c),
            }
        }
    }

    fn escape(&mut self) -> Option<char> {
        Some(match self.chars.next()? {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            'b' => '\u{8}',
            'f' => '\u{c}',
            'u' => {
                let hex: String = (0..4).filter_map(|_| self.chars.next()).collect();
                char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?
            }
            c => c,
        })
    }

    fn scalar(&mut self) -> Option<Json> {
        let mut any = false;
        while self
            .chars
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || "+-.".contains(*c))
        {
            self.chars.next();
            any = true;
        }
        any.then_some(Json::Other)
    }
}

fn parse(text: &str) -> Option<Json> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = Reader {
        chars: text.chars().peekable(),
    };
    let value = reader.value()?;
    reader.skip()?;
    reader.chars.peek().is_none().then_some(value)
}

fn lookup(json: &Json, key: &str) -> Found {
    let mut node = json;
    for part in key.split(':') {
        let Json::Object(entries) = node else {
            return Found::Missing;
        };
        let mut matching = entries.iter().filter(|(k, _)| k.eq_ignore_ascii_case(part));
        match (matching.next(), matching.next()) {
            (None, _) => return Found::Missing,
            (Some(_), Some(_)) => return Found::Unreadable,
            (Some((_, value)), None) => node = value,
        }
    }
    match node {
        Json::Str(s) => Found::Text(s.clone()),
        _ => Found::Unreadable,
    }
}

// The one part of a parsed value a property of kind `val` holds.
fn part(parsed: Parsed, val: Val) -> Option<String> {
    match val {
        Val::HostName => parsed.host,
        Val::VirtualHost => parsed.virtual_host,
        Val::Region => parsed.region,
        _ => parsed.value,
    }
}

fn unknown(reason: &str, key: &str) -> ValueObj {
    ValueObj::Unknown {
        reason: reason.to_string(),
        key: Some(key.to_string()),
    }
}

/// The value `files` state for `key`, read as kind `val`. A file that does
/// not parse, a key two entries match, a value that is no string, and a
/// value outside its kind's grammar make the whole value unknown.
pub(crate) fn configured_value(root: &Path, files: &[String], key: &str, val: Val) -> ValueObj {
    let mut values = Vec::new();
    for file in files {
        let Some(json) = fs::read_to_string(root.join(file))
            .ok()
            .as_deref()
            .and_then(parse)
        else {
            return unknown(UNREADABLE, key);
        };
        let text = match lookup(&json, key) {
            Found::Missing => continue,
            Found::Unreadable => return unknown(UNREADABLE, key),
            Found::Text(text) => text,
        };
        match classify(&text, val).map(|p| part(p, val)) {
            Ok(Some(value)) => values.push(ConfigValue {
                file: file.clone(),
                value,
            }),
            Ok(None) => return unknown(super::values::UNREAD, key),
            Err(reason) => return unknown(reason, key),
        }
    }
    ValueObj::Configuration {
        key: key.to_string(),
        values,
    }
}

#[cfg(test)]
#[path = "configuration_tests.rs"]
mod tests;
