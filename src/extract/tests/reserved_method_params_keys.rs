use super::*;

#[test]
fn reserved_param_keys_ac_4_the_predicate_knows_exactly_the_reserved_spellings() {
    for key in [
        "@result:Index",
        "@result:",
        "@property:Depth",
        "@field:label",
        "@variant",
        "@implicit-conversion",
        "@interpolated-string-handler",
    ] {
        assert!(is_reserved_method_params_key(key), "{key} is reserved");
    }
    for key in [
        "Handle",
        "Index",
        "Invoke",
        "@event",
        "@class",
        "@result",
        "@property",
        "@field",
        "@variants",
        "@implicit-conversions",
        "@interpolated-string-handlers",
        "result:Index",
        "",
    ] {
        assert!(!is_reserved_method_params_key(key), "{key} is not reserved");
    }
}

#[test]
fn reserved_param_keys_ac_4_every_key_the_extractor_writes_is_classified_by_its_writer() {
    let e = extract_src(
        r#"
using System.Collections.Generic;
using System.Runtime.CompilerServices;

namespace App.Reserved;

public interface ISource<out T>
{
    T Next();
}

public class Gauge
{
    private string label;

    public int Depth { get; set; }

    public Dictionary<int, TKey> Index<TKey>() => null;

    public void Handle(Gauge other) { }

    public void @event(Gauge other) { }

    public static implicit operator Gauge(int depth) => new Gauge();
}

[InterpolatedStringHandler]
public struct GaugeHandler
{
    public void AppendLiteral(string text) { }
}

public delegate void Reading(Gauge gauge);
"#,
    );
    let mut reserved = Vec::new();
    let mut ordinary = Vec::new();
    for d in &e.defs {
        for (key, _) in d.method_params.iter() {
            if is_reserved_method_params_key(key) {
                reserved.push(key.clone());
            } else {
                ordinary.push(key.clone());
            }
        }
    }
    reserved.sort();
    ordinary.sort();
    assert_eq!(
        reserved,
        vec![
            "@field:label",
            "@implicit-conversion",
            "@interpolated-string-handler",
            "@property:Depth",
            "@result:Index",
            "@result:Next",
            "@variant",
        ]
    );
    assert_eq!(
        ordinary,
        vec![
            "@event",
            "AppendLiteral",
            "Handle",
            "Index",
            "Invoke",
            "Next"
        ]
    );
}
