// The markup half of the template scan: one pass over the text that turns it
// into element, interpolation and gap events with 1-based lines. It reads the
// subset an Angular template needs -- tags, attributes, text, comments, and
// the raw-text and void elements -- and nothing that a template cannot hold.
// It never backtracks, so a crafted template costs one linear pass.

use super::GapReason;

/// One attribute as written: its name, and its value with the line the value
/// starts on. A bare attribute (`disabled`) has no value.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Attr {
    pub(super) name: String,
    pub(super) line: usize,
    pub(super) value: Option<(String, usize)>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Event {
    /// A start tag. `closes` is true for a self-closing or void element,
    /// which has no children and no end tag.
    Open {
        tag: String,
        attrs: Vec<Attr>,
        closes: bool,
    },
    Close {
        tag: String,
    },
    /// A `{{ }}` in text, with the line its expression starts on.
    Interp {
        expr: String,
        line: usize,
    },
    Gap {
        line: usize,
        reason: GapReason,
    },
}

const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

// Their content is not markup: a `{{` inside a script is JavaScript.
const RAW_TEXT_ELEMENTS: &[&str] = &["script", "style"];

// The keywords that open a built-in control flow block after an `@`.
const BLOCK_KEYWORDS: &[&str] = &[
    "if",
    "else",
    "for",
    "empty",
    "switch",
    "case",
    "default",
    "defer",
    "placeholder",
    "loading",
    "error",
    "let",
];

fn is_name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b':' | b'.')
}

fn newlines(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&c| c == b'\n').count()
}

// The index of `needle` at or after `from`, or the end of the text.
fn find(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle))
        .map(|p| from + p)
}

struct Scanner<'a> {
    text: &'a str,
    bytes: &'a [u8],
    i: usize,
    line: usize,
    events: Vec<Event>,
}

/// The template's events in source order. An attribute value that never
/// closes ends the scan with one gap, because the rest of the text is inside
/// that value.
pub(super) fn events(text: &str, first_line: usize) -> Vec<Event> {
    let mut s = Scanner {
        text,
        bytes: text.as_bytes(),
        i: 0,
        line: first_line,
        events: Vec::new(),
    };
    s.run();
    s.events
}

impl Scanner<'_> {
    fn run(&mut self) {
        let mut text_start = 0;
        let mut text_line = self.line;
        while self.i < self.bytes.len() {
            if self.bytes[self.i] != b'<' || !self.starts_markup() {
                if self.bytes[self.i] == b'\n' {
                    self.line += 1;
                }
                self.i += 1;
                continue;
            }
            self.text_events(text_start, self.i, text_line);
            if !self.markup() {
                return;
            }
            text_start = self.i;
            text_line = self.line;
        }
        self.text_events(text_start, self.bytes.len(), text_line);
    }

    fn peek(&self, ahead: usize) -> u8 {
        self.bytes.get(self.i + ahead).copied().unwrap_or(0)
    }

    // A `<` that opens a tag, an end tag, a comment or a declaration. Any
    // other `<` is text.
    fn starts_markup(&self) -> bool {
        let next = self.peek(1);
        next.is_ascii_alphabetic()
            || matches!(next, b'!' | b'?')
            || (next == b'/' && self.peek(2).is_ascii_alphabetic())
    }

    // Moves the cursor to `to`, counting the newlines it passes.
    fn advance_to(&mut self, to: usize) {
        let to = to.min(self.bytes.len());
        self.line += newlines(&self.bytes[self.i..to]);
        self.i = to;
    }

    // One markup construct at the cursor. Answers false when the scan has to
    // stop.
    fn markup(&mut self) -> bool {
        if self.bytes[self.i..].starts_with(b"<!--") {
            let end = find(self.bytes, self.i + 4, b"-->").map_or(self.bytes.len(), |e| e + 3);
            self.advance_to(end);
            return true;
        }
        if matches!(self.peek(1), b'!' | b'?') {
            let end = find(self.bytes, self.i, b">").map_or(self.bytes.len(), |e| e + 1);
            self.advance_to(end);
            return true;
        }
        if self.peek(1) == b'/' {
            self.i += 2;
            let tag = self.name();
            let end = find(self.bytes, self.i, b">").map_or(self.bytes.len(), |e| e + 1);
            self.advance_to(end);
            self.events.push(Event::Close { tag });
            return true;
        }
        self.start_tag()
    }

    fn name(&mut self) -> String {
        let start = self.i;
        while self.i < self.bytes.len() && is_name_char(self.bytes[self.i]) {
            self.i += 1;
        }
        self.text[start..self.i].to_string()
    }

    fn skip_space(&mut self) {
        while self.i < self.bytes.len() && self.bytes[self.i].is_ascii_whitespace() {
            if self.bytes[self.i] == b'\n' {
                self.line += 1;
            }
            self.i += 1;
        }
    }

    fn start_tag(&mut self) -> bool {
        self.i += 1;
        let tag = self.name();
        let lower = tag.to_ascii_lowercase();
        let mut attrs = Vec::new();
        let mut closes = VOID_ELEMENTS.contains(&lower.as_str());
        loop {
            self.skip_space();
            match (self.peek(0), self.peek(1)) {
                (0, _) => break,
                (b'>', _) => {
                    self.i += 1;
                    break;
                }
                (b'/', b'>') => {
                    self.i += 2;
                    closes = true;
                    break;
                }
                (b'/', _) => self.i += 1,
                _ => match self.attr() {
                    Some(a) => attrs.push(a),
                    None => return false,
                },
            }
        }
        self.events.push(Event::Open { tag, attrs, closes });
        if !closes && RAW_TEXT_ELEMENTS.contains(&lower.as_str()) {
            let close = format!("</{lower}");
            let end = find(self.bytes, self.i, close.as_bytes()).unwrap_or(self.bytes.len());
            self.advance_to(end);
        }
        true
    }

    // One attribute at the cursor, or `None` after recording the gap for a
    // quoted value that never closes.
    fn attr(&mut self) -> Option<Attr> {
        let line = self.line;
        let start = self.i;
        while self.i < self.bytes.len() {
            let c = self.bytes[self.i];
            if c.is_ascii_whitespace()
                || c == b'='
                || c == b'>'
                || (c == b'/' && self.peek(1) == b'>')
            {
                break;
            }
            self.i += 1;
        }
        if self.i == start {
            self.i += 1;
        }
        let name = self.text[start..self.i].to_string();
        self.skip_space();
        if self.peek(0) != b'=' {
            return Some(Attr {
                name,
                line,
                value: None,
            });
        }
        self.i += 1;
        self.skip_space();
        let value_line = self.line;
        let quote = self.peek(0);
        let value = if quote == b'"' || quote == b'\'' {
            let Some(end) = find(self.bytes, self.i + 1, &[quote]) else {
                self.events.push(Event::Gap {
                    line: value_line,
                    reason: GapReason::UnparsedExpression,
                });
                return None;
            };
            let value = self.text[self.i + 1..end].to_string();
            self.advance_to(end + 1);
            value
        } else {
            let start = self.i;
            while self.i < self.bytes.len()
                && !self.bytes[self.i].is_ascii_whitespace()
                && self.bytes[self.i] != b'>'
            {
                self.i += 1;
            }
            self.text[start..self.i].to_string()
        };
        Some(Attr {
            name,
            line,
            value: Some((value, value_line)),
        })
    }

    // The interpolations and control flow lines of one text run.
    fn text_events(&mut self, start: usize, end: usize, line: usize) {
        if start >= end {
            return;
        }
        let run = &self.text[start..end];
        for line_no in block_lines(run, line) {
            self.events.push(Event::Gap {
                line: line_no,
                reason: GapReason::ControlFlowBlock,
            });
        }
        for piece in interpolations(run, line) {
            self.events.push(match piece {
                Ok((expr, line)) => Event::Interp { expr, line },
                Err(line) => Event::Gap {
                    line,
                    reason: GapReason::UnparsedExpression,
                },
            });
        }
    }
}

/// Every `{{ }}` in `text` with the line its expression starts on, or the
/// line of an opening `{{` that never closes; the scan stops there.
pub(super) fn interpolations(text: &str, first_line: usize) -> Vec<Result<(String, usize), usize>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = first_line;
    while let Some(open) = find(bytes, i, b"{{") {
        line += newlines(&bytes[i..open]);
        let Some(close) = find(bytes, open + 2, b"}}") else {
            out.push(Err(line));
            return out;
        };
        out.push(Ok((text[open + 2..close].to_string(), line)));
        line += newlines(&bytes[open..close]);
        i = close + 2;
    }
    out
}

// The lines on which an `@` and a block keyword open a control flow block.
// The `@` must not follow a name character, so an e-mail address in text is
// not a block.
fn block_lines(text: &str, first_line: usize) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut line = first_line;
    for (i, &c) in bytes.iter().enumerate() {
        if c == b'\n' {
            line += 1;
            continue;
        }
        if c != b'@' || (i > 0 && is_name_char(bytes[i - 1])) {
            continue;
        }
        let word_end = bytes[i + 1..]
            .iter()
            .position(|b| !b.is_ascii_alphabetic())
            .map_or(bytes.len(), |p| i + 1 + p);
        let opens = BLOCK_KEYWORDS.contains(&&text[i + 1..word_end])
            && bytes
                .get(word_end)
                .is_none_or(|b| b.is_ascii_whitespace() || matches!(b, b'(' | b'{' | b';'));
        if opens && out.last() != Some(&line) {
            out.push(line);
        }
    }
    out
}
