// Angular template expressions: a lexer and a recursive-descent parser for the
// subset the index reads. The parser builds no tree. Operator precedence
// cannot change which identifier is the root of an operand, so a binary chain
// is read flat; what the parser keeps is every root identifier with its line.
// An expression outside the subset is refused whole, so the caller records one
// gap instead of a partial read set.

/// One root identifier an expression reads, with the 1-based line it sits on.
pub(super) type Read = (String, usize);

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Tok {
    Ident(String),
    Literal,
    Punct(&'static str),
}

#[derive(Debug, Clone)]
pub(super) struct Token {
    pub(super) tok: Tok,
    pub(super) line: usize,
}

// Longest first, so `===` is never lexed as `==` followed by `=`.
const PUNCTS: &[&str] = &[
    "===", "!==", "**=", "&&=", "||=", "??=", "?.", "??", "==", "!=", "<=", ">=", "&&", "||", "**",
    "+=", "-=", "*=", "/=", "%=", "+", "-", "*", "/", "%", "<", ">", "!", "=", "?", ":", ".", ",",
    ";", "|", "(", ")", "[", "]", "{", "}",
];

const BINARY_OPS: &[&str] = &[
    "===", "!==", "??", "==", "!=", "<=", ">=", "&&", "||", "**", "+", "-", "*", "/", "%", "<", ">",
];

const ASSIGN_OPS: &[&str] = &[
    "=", "+=", "-=", "*=", "/=", "%=", "**=", "&&=", "||=", "??=",
];

// Names that are never a component member, whatever the component declares.
const NOT_READS: &[&str] = &["true", "false", "null", "undefined", "this", "$any"];

// Every nesting level passes through `pipe` or `unary`, so this one counter
// bounds the recursion: a crafted template cannot exhaust the stack.
const MAX_DEPTH: usize = 64;

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$'
}

fn is_ident_part(c: u8) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

// The index of the byte after the closing quote, counting the newlines the
// string spans, or `None` when the string never closes.
fn skip_string(bytes: &[u8], open: usize, line: &mut usize) -> Option<usize> {
    let quote = bytes[open];
    let mut i = open + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'\n' => *line += 1,
            c if c == quote => return Some(i + 1),
            _ => {}
        }
        i += 1;
    }
    None
}

/// Splits `text` into tokens. A character outside the subset (a backtick, an
/// entity's `&`, a non-ASCII identifier) or an unterminated string refuses the
/// whole text.
pub(super) fn lex(text: &str, first_line: usize) -> Result<Vec<Token>, ()> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut line = first_line;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let at = line;
        if c == b'\n' {
            line += 1;
            i += 1;
        } else if c.is_ascii_whitespace() {
            i += 1;
        } else if is_ident_start(c) {
            let start = i;
            while i < bytes.len() && is_ident_part(bytes[i]) {
                i += 1;
            }
            out.push(Token {
                tok: Tok::Ident(text[start..i].to_string()),
                line: at,
            });
        } else if c.is_ascii_digit() {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
                i += 1;
            }
            out.push(Token {
                tok: Tok::Literal,
                line: at,
            });
        } else if c == b'\'' || c == b'"' {
            i = skip_string(bytes, i, &mut line).ok_or(())?;
            out.push(Token {
                tok: Tok::Literal,
                line: at,
            });
        } else {
            let p = PUNCTS
                .iter()
                .find(|p| bytes[i..].starts_with(p.as_bytes()))
                .ok_or(())?;
            i += p.len();
            out.push(Token {
                tok: Tok::Punct(p),
                line: at,
            });
        }
    }
    Ok(out)
}

/// The parser over one token list. `micro.rs` drives it item by item; the
/// two entry points below drive it over a whole binding.
pub(super) struct Parser<'t> {
    toks: &'t [Token],
    pos: usize,
    depth: usize,
    allow_assign: bool,
    pub(super) reads: Vec<Read>,
}

type Step = Result<(), ()>;

impl<'t> Parser<'t> {
    pub(super) fn new(toks: &'t [Token], allow_assign: bool) -> Self {
        Parser {
            toks,
            pos: 0,
            depth: 0,
            allow_assign,
            reads: Vec::new(),
        }
    }

    pub(super) fn at_end(&self) -> bool {
        self.pos >= self.toks.len()
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.tok)
    }

    fn peek_at(&self, ahead: usize) -> Option<&Tok> {
        self.toks.get(self.pos + ahead).map(|t| &t.tok)
    }

    fn peek_punct(&self, p: &str) -> bool {
        matches!(self.peek(), Some(Tok::Punct(q)) if *q == p)
    }

    pub(super) fn eat_punct(&mut self, p: &str) -> bool {
        let hit = self.peek_punct(p);
        if hit {
            self.pos += 1;
        }
        hit
    }

    fn expect_punct(&mut self, p: &str) -> Step {
        if self.eat_punct(p) {
            Ok(())
        } else {
            Err(())
        }
    }

    /// The identifier at the cursor, consumed, or `None` (nothing consumed).
    pub(super) fn eat_ident(&mut self) -> Option<String> {
        match self.peek() {
            Some(Tok::Ident(name)) => {
                let name = name.clone();
                self.pos += 1;
                Some(name)
            }
            _ => None,
        }
    }

    pub(super) fn peek_ident(&self, word: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(name)) if name == word)
    }

    fn line(&self) -> usize {
        self.toks.get(self.pos).map_or(0, |t| t.line)
    }

    fn enter(&mut self) -> Step {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            Err(())
        } else {
            Ok(())
        }
    }

    fn read(&mut self, name: String, line: usize) {
        if !NOT_READS.contains(&name.as_str()) {
            self.reads.push((name, line));
        }
    }

    /// One expression with its pipes: `value | name:arg:arg`. A pipe name is
    /// not a read; each argument is.
    pub(super) fn pipe(&mut self) -> Step {
        self.enter()?;
        self.assign()?;
        while self.eat_punct("|") {
            self.eat_ident().ok_or(())?;
            while self.eat_punct(":") {
                self.assign()?;
            }
        }
        self.depth -= 1;
        Ok(())
    }

    // A loop rather than right recursion, so `a = b = c ...` costs no depth.
    fn assign(&mut self) -> Step {
        loop {
            self.conditional()?;
            let assigns = matches!(self.peek(), Some(Tok::Punct(p)) if ASSIGN_OPS.contains(p));
            if !(self.allow_assign && assigns) {
                return Ok(());
            }
            self.pos += 1;
        }
    }

    fn conditional(&mut self) -> Step {
        self.binary()?;
        if self.eat_punct("?") {
            self.pipe()?;
            self.expect_punct(":")?;
            self.pipe()?;
        }
        Ok(())
    }

    fn binary(&mut self) -> Step {
        self.unary()?;
        while matches!(self.peek(), Some(Tok::Punct(p)) if BINARY_OPS.contains(p)) {
            self.pos += 1;
            self.unary()?;
        }
        Ok(())
    }

    fn unary(&mut self) -> Step {
        self.enter()?;
        let prefix = matches!(self.peek(), Some(Tok::Punct("!" | "-" | "+")))
            || self.peek_ident("typeof")
            || self.peek_ident("void");
        if prefix {
            self.pos += 1;
            self.unary()?;
        } else {
            self.postfix()?;
        }
        self.depth -= 1;
        Ok(())
    }

    fn postfix(&mut self) -> Step {
        self.primary()?;
        loop {
            if self.eat_punct(".") {
                self.eat_ident().ok_or(())?;
            } else if self.eat_punct("?.") {
                if self.eat_ident().is_none() && !self.peek_punct("[") && !self.peek_punct("(") {
                    return Err(());
                }
            } else if self.eat_punct("[") {
                self.pipe()?;
                self.expect_punct("]")?;
            } else if self.eat_punct("(") {
                self.list(")")?;
            } else if !self.eat_punct("!") {
                return Ok(());
            }
        }
    }

    // Comma-separated expressions up to `close`, which is consumed.
    fn list(&mut self, close: &str) -> Step {
        if self.eat_punct(close) {
            return Ok(());
        }
        loop {
            self.pipe()?;
            if self.eat_punct(",") {
                continue;
            }
            return self.expect_punct(close);
        }
    }

    fn primary(&mut self) -> Step {
        let line = self.line();
        match self.peek().cloned() {
            Some(Tok::Ident(name)) => {
                self.pos += 1;
                if name == "this" {
                    self.this_member();
                } else {
                    self.read(name, line);
                }
                Ok(())
            }
            Some(Tok::Literal) => {
                self.pos += 1;
                Ok(())
            }
            Some(Tok::Punct("(")) => {
                self.pos += 1;
                self.pipe()?;
                self.expect_punct(")")
            }
            Some(Tok::Punct("[")) => {
                self.pos += 1;
                self.list("]")
            }
            Some(Tok::Punct("{")) => {
                self.pos += 1;
                self.object()
            }
            _ => Err(()),
        }
    }

    // `this.x` reads `x`: the member after `this.` is the root of the chain.
    fn this_member(&mut self) {
        let dotted = matches!(self.peek(), Some(Tok::Punct("." | "?.")));
        if let (true, Some(Tok::Ident(name))) = (dotted, self.peek_at(1).cloned()) {
            let line = self.toks[self.pos + 1].line;
            self.pos += 2;
            self.read(name, line);
        }
    }

    // `{key: value, shorthand}`: a key is not a read, a shorthand key is.
    fn object(&mut self) -> Step {
        if self.eat_punct("}") {
            return Ok(());
        }
        loop {
            let line = self.line();
            match self.peek().cloned() {
                Some(Tok::Ident(key)) => {
                    self.pos += 1;
                    if self.eat_punct(":") {
                        self.pipe()?;
                    } else {
                        self.read(key, line);
                    }
                }
                Some(Tok::Literal) => {
                    self.pos += 1;
                    self.expect_punct(":")?;
                    self.pipe()?;
                }
                _ => return Err(()),
            }
            if self.eat_punct(",") {
                if self.eat_punct("}") {
                    return Ok(());
                }
                continue;
            }
            return self.expect_punct("}");
        }
    }
}

/// The reads of a property binding or an interpolation: one expression that
/// must use every token.
pub(super) fn parse_binding(text: &str, first_line: usize) -> Result<Vec<Read>, ()> {
    let toks = lex(text, first_line)?;
    let mut p = Parser::new(&toks, false);
    if !p.at_end() {
        p.pipe()?;
    }
    if p.at_end() {
        Ok(p.reads)
    } else {
        Err(())
    }
}

/// The reads of an event binding: statements split by `;`, where an
/// assignment is allowed and its target is a read too.
pub(super) fn parse_action(text: &str, first_line: usize) -> Result<Vec<Read>, ()> {
    let toks = lex(text, first_line)?;
    let mut p = Parser::new(&toks, true);
    while !p.at_end() {
        if p.eat_punct(";") {
            continue;
        }
        p.pipe()?;
        if !p.at_end() && !p.eat_punct(";") {
            return Err(());
        }
    }
    Ok(p.reads)
}
