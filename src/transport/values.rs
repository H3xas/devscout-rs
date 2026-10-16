//! Reading one stated string as a value of a known kind.
//!
//! Every value is checked against the grammar of its kind before anything
//! keeps it, so a string that holds anything else, a credential included,
//! reads as unknown and is never stored.

use super::catalogue::Val;
use super::site_types::Parsed;

/// The closed list of reasons an unknown value can carry.
pub(crate) const REASONS: [&str; 16] = [
    "interpolated-string",
    "parameter",
    "local",
    "static-readonly",
    "field",
    "method-call",
    "nameof",
    "expression",
    "package-constant",
    "unresolved-constant",
    "uri-scheme",
    "malformed-uri",
    "unread-value",
    "unreadable-configuration",
    "overload",
    "multiple-hosts",
];

pub(crate) const UNREAD: &str = "unread-value";
const MALFORMED: &str = "malformed-uri";
const SCHEME: &str = "uri-scheme";

fn all(text: &str, ok: impl Fn(char) -> bool) -> bool {
    !text.is_empty() && text.chars().all(ok)
}

/// Queue, endpoint and entity names: `[A-Za-z0-9._:/-]+`.
pub(crate) fn is_name(text: &str) -> bool {
    all(text, |c| c.is_ascii_alphanumeric() || "._:/-".contains(c))
}

fn is_host(text: &str) -> bool {
    all(text, |c| c.is_ascii_alphanumeric() || ".-".contains(c))
}

fn is_virtual_host(text: &str) -> bool {
    all(text, |c| c.is_ascii_alphanumeric() || "._/-".contains(c))
}

fn is_key(text: &str) -> bool {
    all(text, |c| c.is_ascii_alphanumeric() || "_.:-".contains(c))
}

/// An AWS region: `[a-z]{2}(-[a-z]+)+-[0-9]+`.
pub(crate) fn is_region(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    let n = parts.len();
    n >= 3
        && parts[0].len() == 2
        && parts[..n - 1]
            .iter()
            .all(|p| all(p, |c| c.is_ascii_lowercase()))
        && all(parts[n - 1], |c| c.is_ascii_digit())
}

fn is_account(text: &str) -> bool {
    text.len() == 12 && all(text, |c| c.is_ascii_digit())
}

/// Reads `text` as a value of kind `val`, or names why it cannot.
pub(crate) fn classify(text: &str, val: Val) -> Result<Parsed, &'static str> {
    let named = |ok: bool, parsed: Parsed| if ok { Ok(parsed) } else { Err(UNREAD) };
    match val {
        Val::Name | Val::FormatterArg => named(is_name(text), value(text)),
        Val::Key => named(is_key(text), value(text)),
        Val::EntityName if text.starts_with("arn:") => arn(text),
        Val::EntityName => named(is_name(text), value(text)),
        Val::Address => address_uri(text),
        Val::HostName if text.contains("://") => host_uri(text),
        Val::HostName => named(
            is_host(text),
            Parsed {
                host: Some(text.to_string()),
                ..Parsed::default()
            },
        ),
        Val::HostUri => host_uri(text),
        Val::VirtualHost => named(
            is_virtual_host(text),
            Parsed {
                virtual_host: Some(text.to_string()),
                ..Parsed::default()
            },
        ),
        Val::Region => named(
            is_region(text),
            Parsed {
                region: Some(text.to_string()),
                ..Parsed::default()
            },
        ),
        Val::Ignore => Ok(Parsed::default()),
    }
}

fn value(text: &str) -> Parsed {
    Parsed {
        value: Some(text.to_string()),
        ..Parsed::default()
    }
}

// The scheme, lowercased, and what follows its colon.
fn split_scheme(text: &str) -> Option<(String, &str)> {
    let (scheme, rest) = text.split_once(':')?;
    let mut chars = scheme.chars();
    let first = chars.next()?;
    let valid = first.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c));
    valid.then(|| (scheme.to_ascii_lowercase(), rest))
}

fn strip_query(text: &str) -> &str {
    let end = text.find(['?', '#']).unwrap_or(text.len());
    &text[..end]
}

// `//authority/path` into the host (with user information and port
// dropped) and the non-empty path segments.
fn authority(rest: &str) -> Result<(&str, Vec<&str>), &'static str> {
    let rest = rest.strip_prefix("//").ok_or(MALFORMED)?;
    let rest = strip_query(rest);
    let (auth, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = auth.rsplit_once('@').map_or(auth, |(_, h)| h);
    let host = match host.rsplit_once(':') {
        Some((h, port)) if all(port, |c| c.is_ascii_digit()) => h,
        _ => host,
    };
    if !is_host(host) {
        return Err(if host.is_empty() { MALFORMED } else { UNREAD });
    }
    Ok((host, path.split('/').filter(|s| !s.is_empty()).collect()))
}

fn address_uri(text: &str) -> Result<Parsed, &'static str> {
    let (scheme, rest) = split_scheme(text).ok_or(MALFORMED)?;
    match scheme.as_str() {
        "queue" | "exchange" | "topic" => {
            let name = strip_query(rest);
            if name.starts_with("//") || name.is_empty() {
                return Err(MALFORMED);
            }
            if !is_name(name) {
                return Err(UNREAD);
            }
            Ok(Parsed {
                value: Some(name.to_string()),
                scheme: Some(scheme),
                ..Parsed::default()
            })
        }
        "rabbitmq" | "rabbitmqs" | "amazonsqs" => broker_address(&scheme, rest),
        _ => Err(SCHEME),
    }
}

fn broker_address(scheme: &str, rest: &str) -> Result<Parsed, &'static str> {
    let (host, segments) = authority(rest)?;
    let name = *segments.last().ok_or(MALFORMED)?;
    if !is_name(name) {
        return Err(UNREAD);
    }
    let mut parsed = Parsed {
        value: Some(name.to_string()),
        scheme: Some(scheme.to_string()),
        ..Parsed::default()
    };
    if scheme == "amazonsqs" {
        parsed.region = is_region(host).then(|| host.to_string());
        if parsed.region.is_none() {
            return Err(UNREAD);
        }
    } else {
        parsed.host = Some(host.to_string());
        if segments.len() >= 2 {
            if !is_virtual_host(segments[0]) {
                return Err(UNREAD);
            }
            parsed.virtual_host = Some(segments[0].to_string());
        }
    }
    Ok(parsed)
}

fn host_uri(text: &str) -> Result<Parsed, &'static str> {
    let (scheme, rest) = split_scheme(text).ok_or(MALFORMED)?;
    match scheme.as_str() {
        "rabbitmq" | "rabbitmqs" => {
            let (host, segments) = authority(rest)?;
            let virtual_host = match segments.first() {
                Some(v) if is_virtual_host(v) => Some((*v).to_string()),
                Some(_) => return Err(UNREAD),
                None => None,
            };
            Ok(Parsed {
                scheme: Some(scheme),
                host: Some(host.to_string()),
                virtual_host,
                ..Parsed::default()
            })
        }
        "amazonsqs" => {
            let (host, _) = authority(rest)?;
            if !is_region(host) {
                return Err(UNREAD);
            }
            Ok(Parsed {
                scheme: Some(scheme),
                region: Some(host.to_string()),
                ..Parsed::default()
            })
        }
        _ => Err(SCHEME),
    }
}

// `arn:<partition>:<sns|sqs>:<region>:<account>:<name>`.
fn arn(text: &str) -> Result<Parsed, &'static str> {
    let parts: Vec<&str> = text.split(':').collect();
    let readable = parts.len() == 6
        && parts[1].starts_with("aws")
        && matches!(parts[2], "sns" | "sqs")
        && is_region(parts[3])
        && is_account(parts[4])
        && is_name(parts[5]);
    if !readable {
        return Err(UNREAD);
    }
    Ok(Parsed {
        value: Some(parts[5].to_string()),
        scheme: Some("arn".to_string()),
        region: Some(parts[3].to_string()),
        account: Some(parts[4].to_string()),
        ..Parsed::default()
    })
}

#[cfg(test)]
#[path = "values_tests.rs"]
mod tests;
