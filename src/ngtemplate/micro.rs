// Structural directive microsyntax (`*ngFor="let item of items; index as i"`).
// The value is a list of items split by `;` or `,`:
//
//   let x          a local bound to the context's implicit value
//   let x = key    a local bound to a named context value (not a read)
//   expr as v      the first item only: an expression, optionally exported
//   key as v       a context value exported as a local (not a read)
//   key: expr      a directive input; the colon is optional
//
// The parser answers the locals the directive declares and the reads of its
// expressions; the caller decides what those locals hide.

use super::expr::{lex, Parser, Read};

/// What one structural directive's value declares and reads.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Micro {
    pub(super) locals: Vec<String>,
    pub(super) reads: Vec<Read>,
}

/// Parses a structural directive's value, or refuses it whole.
pub(super) fn parse_micro(text: &str, first_line: usize) -> Result<Micro, ()> {
    let toks = lex(text, first_line)?;
    let mut p = Parser::new(&toks, false);
    let mut locals = Vec::new();
    let mut first = true;
    loop {
        while p.eat_punct(";") || p.eat_punct(",") {}
        if p.at_end() {
            break;
        }
        if p.peek_ident("let") {
            p.eat_ident();
            locals.push(p.eat_ident().ok_or(())?);
            if p.eat_punct("=") {
                p.eat_ident().ok_or(())?;
            }
        } else if first {
            p.pipe()?;
            export_as(&mut p, &mut locals)?;
        } else {
            p.eat_ident().ok_or(())?;
            if !export_as(&mut p, &mut locals)? {
                p.eat_punct(":");
                p.pipe()?;
                export_as(&mut p, &mut locals)?;
            }
        }
        first = false;
    }
    Ok(Micro {
        locals,
        reads: p.reads,
    })
}

// An optional `as v` after an item, whose local is recorded. Answers whether
// one was there.
fn export_as(p: &mut Parser, locals: &mut Vec<String>) -> Result<bool, ()> {
    if !p.peek_ident("as") {
        return Ok(false);
    }
    p.eat_ident();
    locals.push(p.eat_ident().ok_or(())?);
    Ok(true)
}
