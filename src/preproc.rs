// C# conditional-compilation pre-pass.
//
// tree-sitter parses every arm of a #if/#else group in a C# file, so a file
// whose namespace or member declarations are chosen by a preprocessor symbol
// gets indexed twice -- once per arm. Worse, the grammar's error recovery
// mis-nests the arms: a dead namespace can parse as a file-scoped namespace
// while the live one swallows the closing #endif as an ERROR node. No cut at
// the syntax-tree level can separate live code from dead code here, so this
// module separates them before parsing even starts, the same way a C#
// compiler's lexer does: it walks every #if/#elif/#else/#endif group under
// the "no build symbols" model -- nothing predefined, #define/#undef inside
// the file are honored, every other symbol reads as false -- and overwrites
// every byte of an inactive line, and of every conditional-directive line
// itself (#if, #elif, #else, #endif, #define, #undef, active or not), with a
// space, leaving every \r and \n exactly where it was. Every other directive
// (#region, #pragma, #nullable, #line, #error, #warning) is left to the
// parser, which already treats it as an extra. Byte
// offsets and line numbers of the surviving text are therefore identical to
// the original source, and a parser fed the result sees at most one arm per
// group (none when a false `#if` has no `#else`).

use std::borrow::Cow;
use std::collections::HashSet;

/// Blanks inactive conditional-compilation arms with no predefined symbols.
pub fn strip_inactive(source: &str) -> Cow<'_, str> {
    strip_inactive_with(source, &[])
}

/// Blanks inactive conditional-compilation arms with `predefined` symbols
/// treated as defined on entry (a hook for a project-model or CLI supplied
/// symbol set).
pub fn strip_inactive_with<'a>(source: &'a str, predefined: &[&str]) -> Cow<'a, str> {
    let bytes = source.as_bytes();
    let spans = line_spans(bytes);

    let mut symbols: HashSet<String> = predefined.iter().map(|s| (*s).to_string()).collect();
    let mut stack: Vec<Frame> = Vec::new();
    let mut state = LexState::Code;
    let mut out: Option<Vec<u8>> = None;

    for (idx, &(start, end)) in spans.iter().enumerate() {
        let line = &bytes[start..end];
        let region_active = current_active(&stack);
        let is_first_line = idx == 0;

        let directive = if region_active {
            if state == LexState::Code {
                parse_directive(line, is_first_line)
            } else {
                None
            }
        } else {
            parse_directive(line, is_first_line)
        };

        match directive {
            Some(d) if is_conditional_keyword(d.keyword) => {
                process_conditional(d.keyword, d.rest, &mut stack, &mut symbols);
                blank(&mut out, bytes, start, end);
            }
            Some(_non_conditional) => {
                if !region_active {
                    blank(&mut out, bytes, start, end);
                }
            }
            None => {
                if region_active {
                    state = scan_line(state, line);
                } else {
                    blank(&mut out, bytes, start, end);
                }
            }
        }
    }

    match out {
        Some(buf) => Cow::Owned(String::from_utf8(buf).expect("only ASCII spaces were written")),
        None => Cow::Borrowed(source),
    }
}

// One `#if`/`#elif`/`#else` group on the conditional stack. `parent_active`
// is whether the enclosing group (or the top of the file) was active when
// this group's `#if` was seen; `taken` is whether some arm of this group has
// already been selected; `active` is whether the current arm is live.
struct Frame {
    parent_active: bool,
    taken: bool,
    active: bool,
}

fn current_active(stack: &[Frame]) -> bool {
    stack.last().is_none_or(|f| f.active)
}

// Overwrites `original[start..end]` with spaces in `out`, cloning
// `original` into `out` on the first call so unchanged input never pays for
// a copy.
fn blank(out: &mut Option<Vec<u8>>, original: &[u8], start: usize, end: usize) {
    let buf = out.get_or_insert_with(|| original.to_vec());
    for b in &mut buf[start..end] {
        *b = b' ';
    }
}

// Byte ranges of each line in `bytes`, split on `\n`, `\r\n`, or a lone
// `\r`. A range excludes its terminator entirely, so it never ends in `\r`.
// A source that ends with a terminator gets no phantom empty line after it;
// a source with no trailing terminator still yields a final range for its
// last (possibly empty) line.
fn line_spans(bytes: &[u8]) -> Vec<(usize, usize)> {
    let len = bytes.len();
    let mut spans = Vec::new();
    let mut start = 0usize;
    loop {
        match bytes[start..]
            .iter()
            .position(|&b| b == b'\n' || b == b'\r')
        {
            Some(rel) => {
                let term = start + rel;
                spans.push((start, term));
                // `\r\n` is one terminator; a lone `\r` (a C# line terminator
                // in its own right) is another, so a CR-only file is still
                // split line by line instead of read as one directive.
                start = if bytes[term] == b'\r' && bytes.get(term + 1) == Some(&b'\n') {
                    term + 2
                } else {
                    term + 1
                };
                if start == len {
                    break;
                }
            }
            None => {
                spans.push((start, len));
                break;
            }
        }
    }
    spans
}

fn is_ascii_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ascii_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_conditional_keyword(keyword: &str) -> bool {
    matches!(
        keyword,
        "if" | "elif" | "else" | "endif" | "define" | "undef"
    )
}

// A recognized `#`-directive line: its keyword identifier (empty when `#`
// is followed by something other than an ASCII identifier) and the raw
// bytes after it, up to end of line content.
struct DirectiveLine<'a> {
    keyword: &'a str,
    rest: &'a [u8],
}

// Recognizes a directive line: its first non-blank byte -- blank meaning
// space, tab, or (on the file's first line only) a UTF-8 BOM -- is `#`.
// Callers are responsible for only calling this when the lexical state and
// region make a `#`-line count as a directive; this function only does the
// text-level recognition.
fn parse_directive(line: &[u8], is_first_line: bool) -> Option<DirectiveLine<'_>> {
    let mut i = 0;
    if is_first_line && line.starts_with(&[0xEF, 0xBB, 0xBF]) {
        i = 3;
    }
    while i < line.len() && (line[i] == b' ' || line[i] == b'\t') {
        i += 1;
    }
    if i >= line.len() || line[i] != b'#' {
        return None;
    }
    i += 1;
    while i < line.len() && (line[i] == b' ' || line[i] == b'\t') {
        i += 1;
    }
    let ident_start = i;
    if i < line.len() && is_ascii_ident_start(line[i]) {
        i += 1;
        while i < line.len() && is_ascii_ident_continue(line[i]) {
            i += 1;
        }
    }
    let keyword = std::str::from_utf8(&line[ident_start..i]).unwrap_or("");
    Some(DirectiveLine {
        keyword,
        rest: &line[i..],
    })
}

fn parse_symbol_ident(rest: &[u8]) -> Option<&str> {
    let mut i = 0;
    while i < rest.len() && (rest[i] == b' ' || rest[i] == b'\t') {
        i += 1;
    }
    let start = i;
    if i < rest.len() && is_ascii_ident_start(rest[i]) {
        i += 1;
        while i < rest.len() && is_ascii_ident_continue(rest[i]) {
            i += 1;
        }
    }
    if i == start {
        None
    } else {
        std::str::from_utf8(&rest[start..i]).ok()
    }
}

// Applies one conditional directive's effect on the group stack and, for
// `define`/`undef`, the symbol set. `rest` is the raw text after the
// keyword, not yet stripped of its trailing comment.
fn process_conditional(
    keyword: &str,
    rest: &[u8],
    stack: &mut Vec<Frame>,
    symbols: &mut HashSet<String>,
) {
    match keyword {
        "if" => {
            let parent = current_active(stack);
            let v = parent && eval(strip_comment(rest), symbols).unwrap_or(false);
            stack.push(Frame {
                parent_active: parent,
                taken: v,
                active: v,
            });
        }
        "elif" => {
            if let Some(top) = stack.last_mut() {
                if !top.parent_active || top.taken {
                    top.active = false;
                } else {
                    let v = eval(strip_comment(rest), symbols).unwrap_or(false);
                    top.active = v;
                    top.taken = v;
                }
            }
        }
        "else" => {
            if let Some(top) = stack.last_mut() {
                top.active = top.parent_active && !top.taken;
                top.taken = true;
            }
        }
        "endif" => {
            stack.pop();
        }
        "define" => {
            if current_active(stack) {
                if let Some(sym) = parse_symbol_ident(rest) {
                    symbols.insert(sym.to_string());
                }
            }
        }
        "undef" => {
            if current_active(stack) {
                if let Some(sym) = parse_symbol_ident(rest) {
                    symbols.remove(sym);
                }
            }
        }
        _ => unreachable!("caller only dispatches conditional keywords"),
    }
}

// Truncates at the first `//`; the condition grammar has no operator built
// from a single `/`, so any occurrence starts a trailing comment.
fn strip_comment(bytes: &[u8]) -> &[u8] {
    match bytes.windows(2).position(|w| w == b"//") {
        Some(pos) => &bytes[..pos],
        None => bytes,
    }
}

// ---- Condition evaluation ----

#[derive(Clone, Copy)]
enum Tok<'a> {
    Ident(&'a str),
    True,
    False,
    Not,
    AndAnd,
    OrOr,
    EqEq,
    NotEq,
    LParen,
    RParen,
}

fn is_cond_ident_start(b: u8) -> bool {
    is_ascii_ident_start(b) || b >= 0x80
}

fn is_cond_ident_continue(b: u8) -> bool {
    is_ascii_ident_continue(b) || b >= 0x80
}

fn tokenize(input: &[u8]) -> Option<Vec<Tok<'_>>> {
    let mut toks = Vec::new();
    let mut i = 0;
    let n = input.len();
    while i < n {
        let b = input[i];
        match b {
            b' ' | b'\t' => i += 1,
            b'(' => {
                toks.push(Tok::LParen);
                i += 1;
            }
            b')' => {
                toks.push(Tok::RParen);
                i += 1;
            }
            b'!' => {
                if input.get(i + 1) == Some(&b'=') {
                    toks.push(Tok::NotEq);
                    i += 2;
                } else {
                    toks.push(Tok::Not);
                    i += 1;
                }
            }
            b'&' if input.get(i + 1) == Some(&b'&') => {
                toks.push(Tok::AndAnd);
                i += 2;
            }
            b'|' if input.get(i + 1) == Some(&b'|') => {
                toks.push(Tok::OrOr);
                i += 2;
            }
            b'=' if input.get(i + 1) == Some(&b'=') => {
                toks.push(Tok::EqEq);
                i += 2;
            }
            _ if is_cond_ident_start(b) => {
                let start = i;
                i += 1;
                while i < n && is_cond_ident_continue(input[i]) {
                    i += 1;
                }
                let text = std::str::from_utf8(&input[start..i]).ok()?;
                toks.push(match text {
                    "true" => Tok::True,
                    "false" => Tok::False,
                    _ => Tok::Ident(text),
                });
            }
            _ => return None,
        }
    }
    Some(toks)
}

struct CondParser<'a> {
    toks: Vec<Tok<'a>>,
    pos: usize,
}

impl<'a> CondParser<'a> {
    fn peek(&self) -> Option<Tok<'a>> {
        self.toks.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<Tok<'a>> {
        let t = self.peek();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn parse_or(&mut self, symbols: &HashSet<String>) -> Option<bool> {
        let mut v = self.parse_and(symbols)?;
        while matches!(self.peek(), Some(Tok::OrOr)) {
            self.bump();
            v = self.parse_and(symbols)? || v;
        }
        Some(v)
    }

    fn parse_and(&mut self, symbols: &HashSet<String>) -> Option<bool> {
        let mut v = self.parse_eq(symbols)?;
        while matches!(self.peek(), Some(Tok::AndAnd)) {
            self.bump();
            v = self.parse_eq(symbols)? && v;
        }
        Some(v)
    }

    fn parse_eq(&mut self, symbols: &HashSet<String>) -> Option<bool> {
        let mut v = self.parse_unary(symbols)?;
        loop {
            match self.peek() {
                Some(Tok::EqEq) => {
                    self.bump();
                    v = v == self.parse_unary(symbols)?;
                }
                Some(Tok::NotEq) => {
                    self.bump();
                    v = v != self.parse_unary(symbols)?;
                }
                _ => break,
            }
        }
        Some(v)
    }

    fn parse_unary(&mut self, symbols: &HashSet<String>) -> Option<bool> {
        if matches!(self.peek(), Some(Tok::Not)) {
            self.bump();
            return Some(!self.parse_unary(symbols)?);
        }
        self.parse_primary(symbols)
    }

    fn parse_primary(&mut self, symbols: &HashSet<String>) -> Option<bool> {
        match self.bump()? {
            Tok::LParen => {
                let v = self.parse_or(symbols)?;
                match self.bump() {
                    Some(Tok::RParen) => Some(v),
                    _ => None,
                }
            }
            Tok::True => Some(true),
            Tok::False => Some(false),
            Tok::Ident(name) => Some(symbols.contains(name)),
            _ => None,
        }
    }
}

// Evaluates a `#if`/`#elif` condition (comment already stripped). `None`
// covers every malformed shape -- a lex error, an empty condition, an
// unbalanced paren, or tokens left over after a full parse -- and the
// caller treats `None` as `false`.
fn eval(condition: &[u8], symbols: &HashSet<String>) -> Option<bool> {
    let toks = tokenize(condition)?;
    if toks.is_empty() {
        return None;
    }
    let mut parser = CondParser { toks, pos: 0 };
    let v = parser.parse_or(symbols)?;
    if parser.pos != parser.toks.len() {
        return None;
    }
    Some(v)
}

// ---- Lexical tracker ----

// Multi-line lexical state carried between active, non-directive lines so a
// `#`-looking line inside an unclosed comment or string is never mistaken
// for a directive. `RawString(n)` records the quote-run length (>= 3) that
// opened the raw string, since closing it takes a run of at least that many.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LexState {
    Code,
    BlockComment,
    VerbatimString,
    RawString(usize),
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

// Scans one line's bytes (already excluding `\r`/`\n`) starting from
// `state`, returning the state in effect at the start of the next line.
fn scan_line(mut state: LexState, line: &[u8]) -> LexState {
    let n = line.len();
    let mut i = 0;
    while i < n {
        state = match state {
            LexState::Code => {
                let b = line[i];
                if b == b'/' && line.get(i + 1) == Some(&b'/') {
                    i = n;
                    LexState::Code
                } else if b == b'/' && line.get(i + 1) == Some(&b'*') {
                    i += 2;
                    LexState::BlockComment
                } else if b == b'"' {
                    // The `@` prefix decides before the quote run is counted:
                    // `@""""` is a verbatim string holding one quote, not a
                    // raw string literal opened by four.
                    let is_verbatim = (i >= 1 && line[i - 1] == b'@')
                        || (i >= 2
                            && ((line[i - 2] == b'$' && line[i - 1] == b'@')
                                || (line[i - 2] == b'@' && line[i - 1] == b'$')));
                    let run_start = i;
                    let mut j = i;
                    while j < n && line[j] == b'"' {
                        j += 1;
                    }
                    let run = j - run_start;
                    if is_verbatim {
                        i += 1;
                        LexState::VerbatimString
                    } else if run >= 3 {
                        i = j;
                        LexState::RawString(run)
                    } else {
                        i += 1;
                        while i < n {
                            if line[i] == b'\\' {
                                i += 2;
                            } else if line[i] == b'"' {
                                i += 1;
                                break;
                            } else {
                                i += 1;
                            }
                        }
                        LexState::Code
                    }
                } else if b == b'\'' {
                    i += 1;
                    while i < n {
                        if line[i] == b'\\' {
                            i += 2;
                        } else if line[i] == b'\'' {
                            i += 1;
                            break;
                        } else {
                            i += 1;
                        }
                    }
                    LexState::Code
                } else {
                    i += 1;
                    LexState::Code
                }
            }
            LexState::BlockComment => match find_subslice(&line[i..], b"*/") {
                Some(off) => {
                    i += off + 2;
                    LexState::Code
                }
                None => {
                    i = n;
                    LexState::BlockComment
                }
            },
            LexState::VerbatimString => {
                let mut j = i;
                let mut closed_at = None;
                while j < n {
                    if line[j] == b'"' {
                        if j + 1 < n && line[j + 1] == b'"' {
                            j += 2;
                        } else {
                            closed_at = Some(j + 1);
                            break;
                        }
                    } else {
                        j += 1;
                    }
                }
                match closed_at {
                    Some(pos) => {
                        i = pos;
                        LexState::Code
                    }
                    None => {
                        i = n;
                        LexState::VerbatimString
                    }
                }
            }
            LexState::RawString(m) => {
                let mut j = i;
                let mut closed_at = None;
                while j < n {
                    if line[j] == b'"' {
                        let run_start = j;
                        while j < n && line[j] == b'"' {
                            j += 1;
                        }
                        if j - run_start >= m {
                            closed_at = Some(j);
                            break;
                        }
                    } else {
                        j += 1;
                    }
                }
                match closed_at {
                    Some(pos) => {
                        i = pos;
                        LexState::Code
                    }
                    None => {
                        i = n;
                        LexState::RawString(m)
                    }
                }
            }
        };
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_of(s: &str) -> Vec<&str> {
        s.lines().collect()
    }

    fn assert_blank_line(input_line: &str, output_line: &str) {
        assert_eq!(
            output_line.len(),
            input_line.len(),
            "blanked line must keep byte length: {input_line:?}"
        );
        assert!(
            output_line.bytes().all(|b| b == b' '),
            "expected an all-space line, got {output_line:?}"
        );
    }

    fn assert_kept_line(input_line: &str, output_line: &str) {
        assert_eq!(output_line, input_line);
    }

    #[test]
    fn no_conditional_directives_borrows_input() {
        let src = "namespace Widgets;\n\nclass Gadget {}\n";
        let out = strip_inactive(src);
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(out, src);
    }

    #[test]
    fn region_and_pragma_only_borrows_input() {
        let src = "#region Setup\n#pragma warning disable 219\nclass Gadget {}\n#endregion\n";
        let out = strip_inactive(src);
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(out, src);
    }

    #[test]
    fn undefined_symbol_if_arm_blanked_else_arm_kept() {
        let src = "#if WIDGET_V2\nclass NewGadget {}\n#else\nclass OldGadget {}\n#endif\n";
        let out = strip_inactive(src);
        assert!(matches!(out, Cow::Owned(_)));
        assert_eq!(out.len(), src.len());
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_eq!(ins.len(), outs.len());
        assert_blank_line(ins[0], outs[0]); // #if
        assert_blank_line(ins[1], outs[1]); // dead arm
        assert_blank_line(ins[2], outs[2]); // #else
        assert_kept_line(ins[3], outs[3]); // live arm
        assert_blank_line(ins[4], outs[4]); // #endif
    }

    #[test]
    fn negated_undefined_symbol_if_arm_kept_else_blanked() {
        let src = "#if !WIDGET_V2\nclass OldGadget {}\n#else\nclass NewGadget {}\n#endif\n";
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_kept_line(ins[1], outs[1]);
        assert_blank_line(ins[3], outs[3]);
    }

    #[test]
    fn if_true_literal_kept() {
        let src = "#if true\nclass Gadget {}\n#endif\n";
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_kept_line(ins[1], outs[1]);
    }

    #[test]
    fn if_false_literal_blanked() {
        let src = "#if false\nclass Gadget {}\n#endif\n";
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_blank_line(ins[1], outs[1]);
    }

    #[test]
    fn elif_chain_selects_matching_arm() {
        let src = concat!(
            "#if A\n",
            "arm_a();\n",
            "#elif B\n",
            "arm_b();\n",
            "#elif !C\n",
            "arm_c();\n",
            "#else\n",
            "arm_else();\n",
            "#endif\n",
        );
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_blank_line(ins[1], outs[1]); // A false
        assert_blank_line(ins[3], outs[3]); // B false
        assert_kept_line(ins[5], outs[5]); // !C true
        assert_blank_line(ins[7], outs[7]); // else never reached
    }

    #[test]
    fn elif_chain_short_circuits_after_true_arm() {
        let src = concat!(
            "#if !A\n",
            "arm_a();\n",
            "#elif true\n",
            "arm_b();\n",
            "#else\n",
            "arm_else();\n",
            "#endif\n",
        );
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_kept_line(ins[1], outs[1]); // !A true, taken
        assert_blank_line(ins[3], outs[3]); // elif never evaluated live
        assert_blank_line(ins[5], outs[5]); // else never reached
    }

    #[test]
    fn nested_if_inside_inactive_outer_stays_blanked() {
        let src = concat!(
            "#if OUTER\n",
            "#if !INNER\n",
            "nested();\n",
            "#endif\n",
            "#endif\n",
            "after();\n",
        );
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_blank_line(ins[2], outs[2]); // nested() stays dead
        assert_kept_line(ins[5], outs[5]); // after the outer group
    }

    #[test]
    fn nested_if_inside_active_outer_kept_and_endif_pops() {
        let src = concat!(
            "#if !OUTER\n",
            "#if !INNER\n",
            "nested();\n",
            "#endif\n",
            "#endif\n",
            "after();\n",
        );
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_kept_line(ins[2], outs[2]);
        assert_kept_line(ins[5], outs[5]);
    }

    #[test]
    fn define_then_undef_toggles_symbol() {
        let src = concat!(
            "#define WIDGET_V2\n",
            "#if WIDGET_V2\n",
            "arm_defined();\n",
            "#endif\n",
            "#undef WIDGET_V2\n",
            "#if WIDGET_V2\n",
            "arm_undefined();\n",
            "#endif\n",
        );
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_kept_line(ins[2], outs[2]);
        assert_blank_line(ins[6], outs[6]);
    }

    #[test]
    fn define_inside_inactive_arm_has_no_effect() {
        let src = concat!(
            "#if false\n",
            "#define WIDGET_V2\n",
            "#endif\n",
            "#if WIDGET_V2\n",
            "arm();\n",
            "#endif\n",
        );
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_blank_line(ins[4], outs[4]);
    }

    #[test]
    fn predefined_symbol_activates_if() {
        let src = "#if WIDGET_V2\narm();\n#endif\n";
        let out = strip_inactive_with(src, &["WIDGET_V2"]);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_kept_line(ins[1], outs[1]);
    }

    #[test]
    fn operator_precedence_or_and_not() {
        // (!A) || (B && C), with B and C defined -> active regardless of A.
        let src = "#if !A || B && C\narm();\n#endif\n";
        let out = strip_inactive_with(src, &["B", "C"]);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_kept_line(ins[1], outs[1]);
    }

    #[test]
    fn equality_with_true_false_literal() {
        let src = "#if A == false\narm();\n#endif\n";
        let out = strip_inactive(src); // A undefined -> false == false -> true
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_kept_line(ins[1], outs[1]);
    }

    #[test]
    fn parenthesized_and_not_combo() {
        let src = "#if (A || B) && !C\narm();\n#endif\n";
        let out = strip_inactive_with(src, &["B"]);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_kept_line(ins[1], outs[1]);
    }

    #[test]
    fn not_equal_operator() {
        let src = "#if A != B\narm();\n#endif\n";
        let out = strip_inactive_with(src, &["B"]);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_kept_line(ins[1], outs[1]);
    }

    #[test]
    fn malformed_conditions_are_inactive() {
        for cond in ["#if (A", "#if A &&", "#if A B", "#if"] {
            let src = format!("{cond}\narm();\n#endif\n");
            let out = strip_inactive(&src);
            let ins = lines_of(&src);
            let outs = lines_of(&out);
            assert_blank_line(ins[1], outs[1]);
        }
    }

    #[test]
    fn stray_else_elif_endif_are_ignored_but_blanked() {
        let src = "#else\nkept_a();\n#elif X\nkept_b();\n#endif\nkept_c();\n";
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_blank_line(ins[0], outs[0]);
        assert_kept_line(ins[1], outs[1]);
        assert_blank_line(ins[2], outs[2]);
        assert_kept_line(ins[3], outs[3]);
        assert_blank_line(ins[4], outs[4]);
        assert_kept_line(ins[5], outs[5]);
    }

    #[test]
    fn unclosed_if_runs_to_eof() {
        let src = "#if WIDGET_V2\narm_a();\narm_b();\n";
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_blank_line(ins[1], outs[1]);
        assert_blank_line(ins[2], outs[2]);
    }

    #[test]
    fn region_pragma_nullable_line_directives_active_vs_inactive() {
        let src = concat!(
            "#region Setup\n",
            "#pragma warning disable 219\n",
            "#nullable enable\n",
            "#line 10\n",
            "#if false\n",
            "#endregion\n",
            "#endif\n",
        );
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        for i in 0..4 {
            assert_kept_line(ins[i], outs[i]);
        }
        assert_blank_line(ins[5], outs[5]); // #endregion inside the inactive arm
    }

    #[test]
    fn trailing_comment_and_whitespace_after_hash_recognized() {
        let src = "#  if WIDGET_V2 // note\narm_a();\n#\telse // fallback\narm_b();\n#endif // WIDGET_V2\n";
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_blank_line(ins[1], outs[1]);
        assert_kept_line(ins[3], outs[3]);
    }

    #[test]
    fn crlf_line_endings_preserved() {
        let src = "#if WIDGET_V2\r\narm_a();\r\n#else\r\narm_b();\r\n#endif\r\n";
        let out = strip_inactive(src);
        assert_eq!(out.len(), src.len());
        assert_eq!(out.matches('\r').count(), src.matches('\r').count());
        assert_eq!(out.matches('\n').count(), src.matches('\n').count());
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_eq!(ins.len(), outs.len());
        assert_blank_line(ins[1], outs[1]);
        assert_kept_line(ins[3], outs[3]);
    }

    #[test]
    fn block_comment_suppresses_directive_detection() {
        let src = "/*\n#if WIDGET_V2\n*/\nafter();\n";
        let out = strip_inactive(src);
        assert!(matches!(out, Cow::Borrowed(_)));
    }

    #[test]
    fn verbatim_string_suppresses_directive_detection() {
        let src = "var t = @\"\n#if WIDGET_V2\n#endif\";\nafter();\n";
        let out = strip_inactive(src);
        assert!(matches!(out, Cow::Borrowed(_)));
    }

    #[test]
    fn raw_string_suppresses_directive_detection() {
        let src = "var t = \"\"\"\n#endif\n\"\"\";\nafter();\n";
        let out = strip_inactive(src);
        assert!(matches!(out, Cow::Borrowed(_)));
    }

    #[test]
    fn string_containing_comment_marker_does_not_open_block_comment() {
        let src = "var t = \"/*\";\n#if WIDGET_V2\narm();\n#endif\n";
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_kept_line(ins[0], outs[0]);
        assert_blank_line(ins[2], outs[2]); // #if IS a directive, symbol undefined
    }

    #[test]
    fn char_literal_does_not_open_string() {
        let src = "var c = '\"';\n#if WIDGET_V2\narm();\n#endif\n";
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_kept_line(ins[0], outs[0]);
        assert_blank_line(ins[2], outs[2]);
    }

    #[test]
    fn line_comment_containing_block_comment_marker_ignored() {
        let src = "// looks like /* a comment start\n#if WIDGET_V2\narm();\n#endif\n";
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_kept_line(ins[0], outs[0]);
        assert_blank_line(ins[2], outs[2]);
    }

    #[test]
    fn doubled_quote_inside_verbatim_string_not_closing() {
        let src = "var t = @\"a\"\"b\n#if WIDGET_V2\n#endif\";\nafter();\n";
        let out = strip_inactive(src);
        assert!(matches!(out, Cow::Borrowed(_)));
    }

    #[test]
    fn multibyte_characters_on_blanked_line_preserve_byte_length() {
        let src = "#if false\n// caf\u{e9} \u{4e2d}\u{6587} \u{1f600}\n#endif\nafter();\n";
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_eq!(out.len(), src.len());
        assert_blank_line(ins[1], outs[1]);
        assert_kept_line(ins[3], outs[3]);
    }

    #[test]
    fn bom_prefixed_first_line_directive_recognized() {
        let src = "\u{feff}#if WIDGET_V2\narm();\n#endif\nafter();\n";
        let out = strip_inactive(src);
        assert_eq!(out.len(), src.len());
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_blank_line(ins[1], outs[1]);
        assert_kept_line(ins[3], outs[3]);
    }

    #[test]
    fn directive_inside_inactive_region_recognized_despite_lexical_state() {
        let src = concat!("#if false\n", "/*\n", "#endif\n", "*/\n", "after();\n",);
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        // The #endif two lines below the unterminated /* still closes the
        // group even though it sits inside what looks like a block comment.
        assert_kept_line(ins[4], outs[4]);
    }

    #[test]
    fn verbatim_string_opened_by_a_quote_run_does_not_latch_the_tracker() {
        // `@""""` is a verbatim string holding one quote character; the run
        // of four quotes must not be read as a raw string literal, or every
        // directive after it would sit "inside a string" and stay unstripped.
        let src = "const string Quote = @\"\"\"\";\n#if LIGHT_MODE\nclass Light {}\n#else\nclass Full {}\n#endif\n";
        let out = strip_inactive(src);
        assert!(matches!(out, Cow::Owned(_)));
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_kept_line(ins[0], outs[0]);
        assert_blank_line(ins[1], outs[1]);
        assert_blank_line(ins[2], outs[2]);
        assert_blank_line(ins[3], outs[3]);
        assert_kept_line(ins[4], outs[4]);
        assert_blank_line(ins[5], outs[5]);
    }

    #[test]
    fn verbatim_string_with_a_longer_quote_run_still_closes_on_its_own_line() {
        // `@"""a"""` is the verbatim string `"a"`: the escaped pairs and the
        // final closing quote are all consumed on one line, so the next
        // line's directive is a directive.
        let src =
            "var s = @\"\"\"a\"\"\";\n#if LIGHT_MODE\nclass Light {}\n#endif\nclass Full {}\n";
        let out = strip_inactive(src);
        let ins = lines_of(src);
        let outs = lines_of(&out);
        assert_kept_line(ins[0], outs[0]);
        assert_blank_line(ins[1], outs[1]);
        assert_blank_line(ins[2], outs[2]);
        assert_blank_line(ins[3], outs[3]);
        assert_kept_line(ins[4], outs[4]);
    }

    #[test]
    fn cr_only_line_endings_are_split_and_preserved() {
        // A lone `\r` is a C# line terminator; the file is four lines, not
        // one directive line, and every `\r` stays where it was.
        let src = "#if LIGHT_MODE\rclass Light {}\r#endif\rclass Full {}\r";
        let out = strip_inactive(src);
        assert_eq!(out.len(), src.len());
        let ins: Vec<&str> = src.split('\r').collect();
        let outs: Vec<&str> = out.split('\r').collect();
        assert_eq!(ins.len(), outs.len());
        assert_blank_line(ins[0], outs[0]);
        assert_blank_line(ins[1], outs[1]);
        assert_blank_line(ins[2], outs[2]);
        assert_kept_line(ins[3], outs[3]);
        assert_eq!(out.matches('\r').count(), src.matches('\r').count());
    }

    #[test]
    fn mixed_crlf_and_lone_cr_keep_every_terminator_byte() {
        let src = "#if !LIGHT_MODE\r\nclass Full {}\r#endif\r\n";
        let out = strip_inactive(src);
        assert_eq!(out.len(), src.len());
        assert_eq!(out.matches('\r').count(), src.matches('\r').count());
        assert_eq!(out.matches('\n').count(), src.matches('\n').count());
        assert!(out.contains("class Full {}\r"));
    }
}
