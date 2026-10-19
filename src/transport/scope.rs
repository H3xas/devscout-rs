//! Transport scopes: the broker host, virtual host, AWS region and AWS
//! account a bus or an address is in, each with its own evidence class.

use super::catalogue::Val;
use super::constants::Resolved;
use super::fact_types::{TransportScope, ValueObj};
use super::site_types::Parsed;

/// One scope part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    Host,
    VirtualHost,
    Region,
    Account,
}

const PARTS: [Part; 4] = [Part::Host, Part::VirtualHost, Part::Region, Part::Account];

pub(crate) fn not_stated() -> TransportScope {
    TransportScope {
        host: ValueObj::NotStated,
        virtual_host: ValueObj::NotStated,
        region: ValueObj::NotStated,
        account: ValueObj::NotStated,
    }
}

pub(crate) fn get(scope: &TransportScope, part: Part) -> &ValueObj {
    match part {
        Part::Host => &scope.host,
        Part::VirtualHost => &scope.virtual_host,
        Part::Region => &scope.region,
        Part::Account => &scope.account,
    }
}

pub(crate) fn set(scope: &mut TransportScope, part: Part, value: ValueObj) {
    match part {
        Part::Host => scope.host = value,
        Part::VirtualHost => scope.virtual_host = value,
        Part::Region => scope.region = value,
        Part::Account => scope.account = value,
    }
}

/// The parts a transport uses; `None` for no known transport, which keeps
/// every part a value states.
pub(crate) fn used_by(transport: Option<&str>) -> &'static [Part] {
    match transport {
        Some("rabbitmq") => &[Part::Host, Part::VirtualHost],
        Some("amazon-sqs") => &[Part::Region, Part::Account],
        Some(_) => &[],
        None => &PARTS,
    }
}

fn stated(parsed: &Parsed, part: Part) -> Option<&String> {
    match part {
        Part::Host => parsed.host.as_ref(),
        Part::VirtualHost => parsed.virtual_host.as_ref(),
        Part::Region => parsed.region.as_ref(),
        Part::Account => parsed.account.as_ref(),
    }
}

/// The parts a value position of kind `val` can state when its value is
/// unreadable.
fn could_state(val: Val) -> &'static [Part] {
    match val {
        Val::HostName => &[Part::Host],
        Val::VirtualHost => &[Part::VirtualHost],
        Val::Region => &[Part::Region],
        Val::HostUri | Val::Address => &PARTS,
        _ => &[],
    }
}

/// The scope parts one resolved value states, each with the value's own
/// class: `Some` per stated part.
pub(crate) fn parts_of(resolved: &Resolved, val: Val) -> Vec<(Part, ValueObj)> {
    let Some(parsed) = &resolved.parts else {
        let reason = match &resolved.obj {
            ValueObj::Unknown { reason, .. } => reason.clone(),
            _ => return Vec::new(),
        };
        return could_state(val)
            .iter()
            .map(|p| {
                (
                    *p,
                    ValueObj::Unknown {
                        reason: reason.clone(),
                        key: None,
                    },
                )
            })
            .collect();
    };
    PARTS
        .iter()
        .filter_map(|part| {
            let value = stated(parsed, *part)?.clone();
            let obj = if resolved.fields.is_empty() {
                ValueObj::Literal {
                    value,
                    scheme: None,
                }
            } else {
                ValueObj::Constant {
                    value,
                    scheme: None,
                    fields: resolved.fields.clone(),
                }
            };
            Some((*part, obj))
        })
        .collect()
}

/// A scope from the parts the values state, limited to the parts the
/// transport uses.
pub(crate) fn from_parts(parts: &[(Part, ValueObj)], transport: Option<&str>) -> TransportScope {
    let mut scope = not_stated();
    let used = used_by(transport);
    for (part, value) in parts {
        if used.contains(part) {
            set(&mut scope, *part, value.clone());
        }
    }
    scope
}

/// The scope of a bus from the scopes of its host calls: one host call's
/// scope, `not-stated` with none, and every used part unknown with two or
/// more.
pub(crate) fn bus_scope(hosts: &[TransportScope], transport: Option<&str>) -> TransportScope {
    match hosts {
        [] => not_stated(),
        [one] => {
            let mut scope = not_stated();
            for part in used_by(transport) {
                set(&mut scope, *part, get(one, *part).clone());
            }
            scope
        }
        _ => {
            let mut scope = not_stated();
            for part in used_by(transport) {
                set(
                    &mut scope,
                    *part,
                    ValueObj::Unknown {
                        reason: "multiple-hosts".to_string(),
                        key: None,
                    },
                );
            }
            scope
        }
    }
}

/// The scope of an address: each part its own value states first, then the
/// part of its bus, limited to the parts the transport uses.
pub(crate) fn address_scope(
    own: &[(Part, ValueObj)],
    bus: Option<&TransportScope>,
    transport: Option<&str>,
) -> TransportScope {
    let mut scope = not_stated();
    for part in used_by(transport) {
        let value = own
            .iter()
            .find(|(p, _)| p == part)
            .map(|(_, v)| v.clone())
            .or_else(|| bus.map(|b| get(b, *part).clone()))
            .unwrap_or(ValueObj::NotStated);
        set(&mut scope, *part, value);
    }
    scope
}
