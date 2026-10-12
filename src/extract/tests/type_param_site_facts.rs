use super::*;

fn sites<'a>(d: &'a DefRecord, name: &str) -> &'a Vec<Vec<Vec<(usize, i64)>>> {
    &d.method_type_param_sites
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("no sites for {name}"))
        .1
}

fn params<'a>(d: &'a DefRecord, name: &str) -> &'a Vec<Vec<String>> {
    &d.method_params
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("no params for {name}"))
        .1
}

#[test]
fn a_methods_own_type_parameter_is_told_apart_from_its_types() {
    let e = extract_src(
        r#"
namespace App.Lists;

public class L<TElement>
{
    public Task<bool> Any(FilterDelegate<TElement> f, CancellationToken ct = default) => null;
    public Task<bool> Any<T>(FilterDelegate<IX<T>> f, CancellationToken ct = default) => null;
    public Task<bool> Any<T>(CancellationToken ct = default) => null;
}
"#,
    );
    let d = find_def(&e, "App.Lists.L").expect("L");
    assert_eq!(
        params(d, "Any"),
        &vec![
            vec![
                "FilterDelegate<*>".to_string(),
                "CancellationToken".to_string()
            ],
            vec![
                "FilterDelegate<IX<*>>".to_string(),
                "CancellationToken".to_string()
            ],
            vec!["CancellationToken".to_string()],
        ],
        "the descriptors alone write both type parameters as the same wildcard"
    );
    assert_eq!(
        sites(d, "Any"),
        &vec![
            vec![vec![], vec![]],
            vec![vec![(0, 0)], vec![]],
            vec![vec![]]
        ]
    );
    assert_eq!(
        d.method_type_arities,
        vec![("Any".to_string(), vec![0, 1, 1])],
        "aligned overload for overload with the type-parameter counts"
    );
}

#[test]
fn each_site_names_the_top_level_type_argument_it_sits_in() {
    let e = extract_src(
        r#"
namespace App.Shapes;

public class S
{
    public void M<T>(Func<T, bool> f) { }
    public void M<T>(Func<bool, T> f) { }
    public void M<T>(T seed, Action<T> a) { }
    public void N<T, U>(Action<T> a) { }
    public void P<T>(Expression<Func<Box<T>, bool>> e, T[] items, params T[] rest) { }
    public static void Q<T>(this Box<T> b, System.Action<T>? a) { }
}
"#,
    );
    let d = find_def(&e, "App.Shapes.S").expect("S");
    assert_eq!(
        sites(d, "M"),
        &vec![
            vec![vec![(0, 0)]],
            vec![vec![(0, 1)]],
            vec![vec![(0, -1)], vec![(0, 0)]],
        ]
    );
    assert_eq!(
        sites(d, "N"),
        &vec![vec![vec![(0, 0)]]],
        "U occurs nowhere and records no site"
    );
    assert_eq!(
        sites(d, "P"),
        &vec![vec![vec![(0, 0)], vec![(0, -1)], vec![(0, -1)]]],
        "one expression-tree wrapper is looked through, and an array is outside every type argument"
    );
    assert_eq!(sites(d, "Q"), &vec![vec![vec![(0, 0)], vec![(0, 0)]]]);
    assert_eq!(
        params(d, "P")[0].len(),
        sites(d, "P")[0].len(),
        "one site list per parameter descriptor, the flattened params array included"
    );
}

#[test]
fn a_partial_types_parts_record_their_own_overloads_in_order() {
    let e = extract_src(
        r#"
namespace App.Parts;

public partial class R
{
    public void M<T>(Action<T> a) { }
}

public partial class R
{
    public void M(int x) { }
    public void M<T, U>(Func<T, U> f) { }
}
"#,
    );
    let parts: Vec<&DefRecord> = e.defs.iter().filter(|d| d.id == "App.Parts.R").collect();
    assert_eq!(parts.len(), 2);
    assert_eq!(sites(parts[0], "M"), &vec![vec![vec![(0, 0)]]]);
    assert_eq!(
        sites(parts[1], "M"),
        &vec![vec![vec![]], vec![vec![(0, 0), (1, 1)]]]
    );
}

#[test]
fn a_delegate_records_its_return_and_where_each_type_parameter_occurs() {
    let e = extract_src(
        r#"
namespace App.Delegates;

public delegate bool D<in T>(T x);
public delegate void V(int x);
public delegate T R<T>();
public delegate T Both<T, U>(T seed, Box<U> b);
public delegate void None<T>();
"#,
    );
    let sig = |id: &str| {
        let d = find_def(&e, id).unwrap_or_else(|| panic!("no {id}"));
        let s = d.delegate_signature.as_ref().expect("delegate signature");
        (s.returns.clone(), s.type_params.clone())
    };
    assert_eq!(
        sig("App.Delegates.D"),
        ("bool".to_string(), vec!["params".to_string()])
    );
    assert_eq!(sig("App.Delegates.V"), ("void".to_string(), vec![]));
    assert_eq!(
        sig("App.Delegates.R"),
        ("*".to_string(), vec!["return".to_string()])
    );
    assert_eq!(
        sig("App.Delegates.Both"),
        (
            "*".to_string(),
            vec!["both".to_string(), "params".to_string()]
        )
    );
    assert_eq!(
        sig("App.Delegates.None"),
        ("void".to_string(), vec!["none".to_string()])
    );
}

#[test]
fn only_a_delegate_carries_a_signature() {
    let e = extract_src(
        r#"
namespace App.Delegates;

public class C { public void M<T>(Action<T> a) { } }
public interface I { void M(); }
"#,
    );
    assert!(e.defs.iter().all(|d| d.delegate_signature.is_none()));
    let json = crate::extract::extraction_to_json(&extract_src(
        "namespace App.Delegates;\npublic delegate bool D<in T>(T x);\n",
    ));
    assert!(json.contains("\"delegateSignature\": {"), "{json}");
    assert!(json.contains("\"returns\": \"bool\""), "{json}");
}
