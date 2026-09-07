use super::*;

// --- Extension-method def facts ---------------------------------------------

#[test]
fn stage3_extension_methods_recorded_in_source_order_deduped_by_name_this_type_and_arity() {
    let e = extract_src(
        r#"
namespace App.Ext;

public static class WidgetExtensions
{
  public static Widget Copy(this Widget w) => w;
  public static void Render(this Widget w) { }
  public static void Render(this Widget w, int depth) { }
  public static void Render(this Widget w, string label) { }
  public static string Trim(this string s) => s;
  public static void Each(this System.Collections.Generic.List<Widget> items) { }
  public static void Plain(Widget w) { }
  public static void Late(Widget w, this Gadget g) { }
  internal static void Hidden(this Gadget g) { }
}
"#,
    );
    let d = find_def(&e, "App.Ext.WidgetExtensions").expect("WidgetExtensions def present");
    let pairs: Vec<(&str, &str, usize, i64)> = d
        .extension_methods
        .iter()
        .map(|x| {
            (
                x.name.as_str(),
                x.this_type.as_str(),
                x.arity_min,
                x.arity_max,
            )
        })
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("Copy", "Widget", 0, 0),
            // The two 1-parameter Render overloads collapse into ONE entry
            // -- the dedup key is the (name, thisType, arityMin, arityMax)
            // QUADRUPLE, and both name the same call shape. The 0-parameter
            // overload is a DIFFERENT entry; before the first tighten
            // amendment all three were one arity-blind entry any Render(...)
            // call could match.
            ("Render", "Widget", 0, 0),
            ("Render", "Widget", 1, 1),
            // predefined this-types are KEPT, unlike stage-2 receiver facts
            ("Trim", "string", 0, 0),
            // generic args stripped to the base identifier, qualified name
            // reduced to its last segment
            ("Each", "List", 0, 0),
            // no accessibility filter: internal extensions count
            ("Hidden", "Gadget", 0, 0),
        ]
    );
    // ...and the generic amendment records the stripped arguments alongside
    // the base identifier rather than discarding them.
    assert_eq!(
        d.extension_methods
            .iter()
            .map(|x| x.this_args.clone())
            .collect::<Vec<_>>(),
        vec![
            None,
            None,
            None,
            None,
            Some(vec!["Widget".to_string()]),
            None
        ],
        "thisArgs is present ONLY on the generic this-parameter"
    );
    assert!(
        !pairs.iter().any(|(n, ..)| *n == "Plain"),
        "a static method with no this-parameter is not an extension method"
    );
    assert!(
        !pairs.iter().any(|(n, ..)| *n == "Late"),
        "`this` on a NON-first parameter is not an extension method"
    );
    // The accessibility asymmetry is deliberate and worth pinning:
    // `methods` stays public-only, so `Hidden` is reachable ONLY through
    // the extension tier.
    assert!(!d.methods.contains(&"Hidden".to_string()));
}
