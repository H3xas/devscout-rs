use super::modes::Mode;
use super::packages::OrderedValue;

pub(super) enum MapMatch {
    NotMatched,
    NoCondition,
    Targets(Vec<String>),
}

pub(super) fn exports(
    map: &OrderedValue,
    subpath: &str,
    usage: &str,
    mode: Mode,
    custom: &[String],
) -> MapMatch {
    if let Some(object) = map.as_object() {
        if object.iter().any(|(key, _)| key.starts_with('.')) {
            return select_key(map, subpath, usage, mode, custom);
        }
    }
    if subpath != "." {
        return MapMatch::NotMatched;
    }
    target(map, "", usage, mode, custom)
}

pub(super) fn imports(
    map: &OrderedValue,
    spec: &str,
    usage: &str,
    mode: Mode,
    custom: &[String],
) -> MapMatch {
    select_key(map, spec, usage, mode, custom)
}

fn select_key(
    map: &OrderedValue,
    spec: &str,
    usage: &str,
    mode: Mode,
    custom: &[String],
) -> MapMatch {
    let Some(object) = map.as_object() else {
        return MapMatch::NotMatched;
    };
    if let Some(value) = object.get(spec) {
        return target(value, "", usage, mode, custom);
    }
    let mut candidates = Vec::new();
    for (key, value) in object.iter() {
        if let Some(star) = key.find('*') {
            let (prefix, suffix) = (&key[..star], &key[star + 1..]);
            if spec.starts_with(prefix)
                && spec.ends_with(suffix)
                && spec.len() >= prefix.len() + suffix.len()
            {
                candidates.push((
                    prefix.len(),
                    suffix.len(),
                    &spec[prefix.len()..spec.len() - suffix.len()],
                    value,
                ));
            }
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    if let Some((_, _, capture, value)) = candidates.first() {
        target(value, capture, usage, mode, custom)
    } else {
        MapMatch::NotMatched
    }
}

fn target(
    value: &OrderedValue,
    capture: &str,
    usage: &str,
    mode: Mode,
    custom: &[String],
) -> MapMatch {
    match value {
        OrderedValue::Null => MapMatch::NoCondition,
        OrderedValue::String(path) => {
            let substituted = if path.contains('*') {
                path.replace('*', capture)
            } else if !capture.is_empty() && path.ends_with('/') {
                format!("{path}{capture}")
            } else {
                path.clone()
            };
            MapMatch::Targets(vec![substituted])
        }
        OrderedValue::Array(values) => {
            let mut targets = Vec::new();
            for value in values {
                if let MapMatch::Targets(more) = target(value, capture, usage, mode, custom) {
                    targets.extend(more);
                }
            }
            if targets.is_empty() {
                MapMatch::NoCondition
            } else {
                MapMatch::Targets(targets)
            }
        }
        OrderedValue::Object(object) => {
            for (condition, value) in object.iter() {
                let matches = condition != "types"
                    && (condition == "default"
                        || condition == usage
                        || condition == "node" && mode != Mode::Bundler
                        || custom.iter().any(|extra| extra == condition));
                if matches {
                    return target(value, capture, usage, mode, custom);
                }
            }
            MapMatch::NoCondition
        }
        OrderedValue::Bool(_) | OrderedValue::Number(_) => MapMatch::NoCondition,
    }
}

#[cfg(test)]
mod tests;
