// Template locals and what they hide. A `#ref` names the whole template, so it
// is collected before any read is judged. A structural directive's locals and
// an `ng-template`'s `let-x` cover the host element -- every attribute on it,
// the directive's own value included -- and its children. `$event` covers the
// one event binding it appears in.

use std::collections::HashSet;

use super::expr::{parse_action, parse_binding, Read};
use super::markup::{interpolations, Attr, Event};
use super::micro::{parse_micro, Micro};
use super::{GapReason, TemplateGap, TemplateRead, TemplateScan};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Binding {
    Structural,
    Event,
    TwoWay,
    Property,
    Ref,
    LetLocal,
    Plain,
}

fn binding_of(tag: &str, name: &str) -> Binding {
    let wrapped = |open: &str, close: &str| name.starts_with(open) && name.ends_with(close);
    if name.starts_with('*') {
        Binding::Structural
    } else if name.starts_with('#') || name.starts_with("ref-") {
        Binding::Ref
    } else if wrapped("[(", ")]") || name.starts_with("bindon-") {
        Binding::TwoWay
    } else if wrapped("[", "]") || name.starts_with("bind-") {
        Binding::Property
    } else if wrapped("(", ")") || name.starts_with("on-") {
        Binding::Event
    } else if name.starts_with("let-") && tag.eq_ignore_ascii_case("ng-template") {
        Binding::LetLocal
    } else {
        Binding::Plain
    }
}

fn ref_name(name: &str) -> &str {
    name.strip_prefix('#')
        .or_else(|| name.strip_prefix("ref-"))
        .unwrap_or(name)
}

struct Walk {
    refs: HashSet<String>,
    // One frame per open element, so an end tag pops exactly its own locals.
    frames: Vec<(String, Vec<String>)>,
    out: TemplateScan,
}

impl Walk {
    fn hidden(&self, name: &str) -> bool {
        self.refs.contains(name)
            || self
                .frames
                .iter()
                .any(|(_, locals)| locals.iter().any(|l| l == name))
    }

    fn keep(&mut self, reads: Vec<Read>, event_local: bool) {
        for (name, line) in reads {
            if (event_local && name == "$event") || self.hidden(&name) {
                continue;
            }
            self.out.reads.push(TemplateRead { name, line });
        }
    }

    fn gap(&mut self, line: usize) {
        self.out.gaps.push(TemplateGap {
            line,
            reason: GapReason::UnparsedExpression,
        });
    }

    fn open(&mut self, tag: String, attrs: &[Attr], closes: bool) {
        let mut locals = Vec::new();
        let mut micros: Vec<Option<Result<Micro, ()>>> = Vec::with_capacity(attrs.len());
        for a in attrs {
            let (text, line) = a.value.clone().unwrap_or((String::new(), a.line));
            micros.push(match binding_of(&tag, &a.name) {
                Binding::Structural => {
                    let m = parse_micro(&text, line);
                    if let Ok(m) = &m {
                        locals.extend(m.locals.iter().cloned());
                    }
                    Some(m)
                }
                Binding::LetLocal => {
                    locals.push(a.name["let-".len()..].to_string());
                    None
                }
                _ => None,
            });
        }
        self.frames.push((tag.clone(), locals));
        for (a, micro) in attrs.iter().zip(micros) {
            self.attr(&tag, a, micro);
        }
        if closes {
            self.frames.pop();
        }
    }

    fn attr(&mut self, tag: &str, a: &Attr, micro: Option<Result<Micro, ()>>) {
        let (text, line) = a.value.clone().unwrap_or((String::new(), a.line));
        let parsed = match binding_of(tag, &a.name) {
            Binding::Structural => micro.map_or(Ok(Vec::new()), |m| m.map(|m| m.reads)),
            Binding::Event => parse_action(&text, line),
            Binding::TwoWay | Binding::Property => parse_binding(&text, line),
            Binding::Ref | Binding::LetLocal => Ok(Vec::new()),
            Binding::Plain => {
                for piece in interpolations(&text, line) {
                    match piece.and_then(|(expr, at)| parse_binding(&expr, at).map_err(|()| at)) {
                        Ok(reads) => self.keep(reads, false),
                        Err(at) => self.gap(at),
                    }
                }
                return;
            }
        };
        let event_local = matches!(binding_of(tag, &a.name), Binding::Event | Binding::TwoWay);
        match parsed {
            Ok(reads) => self.keep(reads, event_local),
            Err(()) => self.gap(line),
        }
    }

    // A stray end tag that matches no open element changes nothing.
    fn close(&mut self, tag: &str) {
        if let Some(at) = self
            .frames
            .iter()
            .rposition(|(open, _)| open.eq_ignore_ascii_case(tag))
        {
            self.frames.truncate(at);
        }
    }
}

/// Judges every event's reads against the locals in scope.
pub(super) fn walk(events: Vec<Event>) -> TemplateScan {
    let refs = events
        .iter()
        .filter_map(|e| match e {
            Event::Open { attrs, .. } => Some(attrs),
            _ => None,
        })
        .flatten()
        .filter(|a| a.name.starts_with('#') || a.name.starts_with("ref-"))
        .map(|a| ref_name(&a.name).to_string())
        .collect();
    let mut w = Walk {
        refs,
        frames: Vec::new(),
        out: TemplateScan::default(),
    };
    for event in events {
        match event {
            Event::Open { tag, attrs, closes } => w.open(tag, &attrs, closes),
            Event::Close { tag } => w.close(&tag),
            Event::Interp { expr, line } => match parse_binding(&expr, line) {
                Ok(reads) => w.keep(reads, false),
                Err(()) => w.gap(line),
            },
            Event::Gap { line, reason } => w.out.gaps.push(TemplateGap { line, reason }),
        }
    }
    w.out
}
