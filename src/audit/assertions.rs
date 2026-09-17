// Threshold evaluation for `--assert <file>`, kept beside the report it
// reads: the file is scored against the exact JSON `--json` would print,
// so a caller that asserts and a caller that parses see one shape.

// ---------------------------------------------------------------------------
// `--assert <file>` -- a flat `{"dotted.metric.path": {"min": x} | {"max":
// y}}` object, evaluated against the SAME JSON this run would print with
// `--json` (parsed back through `serde_json::Value` so a dotted path walks
// it generically, one `.get(segment)` per `.`-separated piece) -- one
// violation line per failing or missing metric, `"assert: {path} = {actual}
// > max {y}"` / `"< min {x}"` / `"{path} missing"`.
// ---------------------------------------------------------------------------

pub(super) fn lookup_metric<'a>(
    v: &'a serde_json::Value,
    path: &str,
) -> Option<&'a serde_json::Value> {
    let mut cur = v;
    for seg in path.split('.') {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

/// `13`, not `13.0`; `0.381`, not `0.38100000000000001` -- a violation line
/// quotes the metric and its threshold the way the assert file spelled them.
fn fmt_num(x: f64) -> String {
    if x == x.trunc() && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

/// Evaluates `assert_text` (the `--assert` file's contents) against
/// `report_json` (this run's `--json` shape). `Ok(violations)`, empty when
/// every threshold holds; `Err` only for a malformed assert file or a
/// threshold entry that is not `{"min": _}`/`{"max": _}`.
pub(super) fn evaluate_assert(report_json: &str, assert_text: &str) -> Result<Vec<String>, String> {
    let report_value: serde_json::Value = serde_json::from_str(report_json)
        .map_err(|e| format!("internal: audit report is not valid JSON: {e}"))?;
    let assert_value: serde_json::Value = serde_json::from_str(assert_text)
        .map_err(|e| format!("assert file is not valid JSON: {e}"))?;
    let obj = assert_value.as_object().ok_or_else(|| {
        "assert file must be a JSON object of {\"path\": {\"min\"|\"max\": n}}".to_string()
    })?;

    let mut violations = Vec::new();
    for (path, spec) in obj {
        let spec_obj = spec.as_object().ok_or_else(|| {
            format!("assert entry '{path}' must be an object with a \"min\" or \"max\" key")
        })?;
        let min = spec_obj.get("min").and_then(serde_json::Value::as_f64);
        let max = spec_obj.get("max").and_then(serde_json::Value::as_f64);
        if min.is_none() && max.is_none() {
            return Err(format!(
                "assert entry '{path}' must carry a numeric \"min\" or \"max\""
            ));
        }

        let actual = lookup_metric(&report_value, path).and_then(serde_json::Value::as_f64);
        let Some(actual) = actual else {
            violations.push(format!("assert: {path} missing"));
            continue;
        };
        if let Some(min) = min {
            if actual < min {
                violations.push(format!(
                    "assert: {path} = {} < min {}",
                    fmt_num(actual),
                    fmt_num(min)
                ));
                continue;
            }
        }
        if let Some(max) = max {
            if actual > max {
                violations.push(format!(
                    "assert: {path} = {} > max {}",
                    fmt_num(actual),
                    fmt_num(max)
                ));
            }
        }
    }
    Ok(violations)
}
