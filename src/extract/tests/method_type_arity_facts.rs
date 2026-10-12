use super::*;

// (member, argCount, methodTypeArgCount) for every uses-member ref, in
// extraction order.
fn member_type_arg_counts(e: &Extraction) -> Vec<(String, Option<usize>, Option<usize>)> {
    e.refs
        .iter()
        .filter(|r| r.kind == "uses-member")
        .map(|r| {
            (
                r.member.clone().unwrap_or_default(),
                r.arg_count,
                r.method_type_arg_count,
            )
        })
        .collect()
}

#[test]
fn a_call_written_with_a_type_argument_list_records_its_length() {
    let e = extract_src(
        r#"
namespace App.Consumers;

public class Runner
{
  public void Run(IRegistrar x)
  {
    x.Add<Alpha, Beta>();
    x.Add<Dictionary<string, int>>(1);
    x.Add();
    App.Registry.Tables.Seed<Alpha>();
    x?.Add<Alpha, Beta, Gamma>();
    var p = x.Count;
  }
}
"#,
    );
    assert_eq!(
        member_type_arg_counts(&e),
        vec![
            ("Add".to_string(), Some(0), Some(2)),
            ("Add".to_string(), Some(1), Some(1)),
            ("Add".to_string(), Some(0), None),
            ("Seed".to_string(), Some(0), Some(1)),
            ("Tables".to_string(), None, None),
            ("Registry".to_string(), None, None),
            ("Add".to_string(), Some(0), Some(3)),
            ("Count".to_string(), None, None),
        ],
        "a nested argument's own commas never count, and a call or read written without a list records nothing"
    );
}

#[test]
fn the_qualifiers_own_type_argument_count_is_unchanged_beside_the_method_count() {
    let e = extract_src(
        r#"
namespace App.Consumers;

public class Runner
{
  public void Run() => Cache<Alpha>.Load<Beta, Gamma>();
}
"#,
    );
    let r = e
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Load"))
        .expect("Load ref present");
    assert_eq!(r.type_arg_count, Some(1));
    assert!(r.generic);
    assert_eq!(r.method_type_arg_count, Some(2));
}

#[test]
fn a_chain_tail_call_records_the_count_written_on_its_own_member() {
    let e = extract_src(
        r#"
namespace App.Consumers;

public class Runner
{
  public void Run(Factory f) => f.Make().Build<Alpha, Beta>();
}
"#,
    );
    let r = e
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Build"))
        .expect("Build ref present");
    assert_eq!(r.method_type_arg_count, Some(2));
}

#[test]
fn every_overload_records_its_type_parameter_count_aligned_with_its_value_range() {
    let e = extract_src(
        r#"
namespace App.Registry;

public class Registrar<TKey>
{
    public void M<T>(int a) { }
    public void M<T, U>() { }
    private void M() { }
    internal void Seed<T>(T item = default) { }
}
"#,
    );
    let d = find_def(&e, "App.Registry.Registrar").expect("Registrar def present");
    assert_eq!(
        d.method_arities,
        vec![
            ("M".to_string(), vec![(1, 1), (0, 0), (0, 0)]),
            ("Seed".to_string(), vec![(0, 1)]),
        ]
    );
    assert_eq!(
        d.method_type_arities,
        vec![
            ("M".to_string(), vec![1, 2, 0]),
            ("Seed".to_string(), vec![1]),
        ],
        "the enclosing type's own type parameter never counts toward a method's"
    );
}

#[test]
fn each_part_of_a_partial_type_records_its_own_aligned_lists() {
    let e = extract_src(
        r#"
namespace App.Registry;

public partial class Registrar
{
    public void M<T>(int a) { }
}

public partial class Registrar
{
    public void M<T, U>(int a) { }
    public void M() { }
}
"#,
    );
    let parts: Vec<&DefRecord> = e
        .defs
        .iter()
        .filter(|d| d.id == "App.Registry.Registrar")
        .collect();
    assert_eq!(parts.len(), 2);
    assert_eq!(
        parts[0].method_arities,
        vec![("M".to_string(), vec![(1, 1)])]
    );
    assert_eq!(
        parts[0].method_type_arities,
        vec![("M".to_string(), vec![1])]
    );
    assert_eq!(
        parts[1].method_arities,
        vec![("M".to_string(), vec![(1, 1), (0, 0)])]
    );
    assert_eq!(
        parts[1].method_type_arities,
        vec![("M".to_string(), vec![2, 0])]
    );
}

#[test]
fn an_extension_entry_records_its_methods_own_type_parameter_count() {
    let e = extract_src(
        r#"
namespace App.Ext;

public static class RegistrarExtensions
{
    public static void M<T, U>(this IRegistrar x) { }
    public static void M<T>(this IRegistrar x) { }
    public static void N(this IRegistrar x) { }
}
"#,
    );
    let d = find_def(&e, "App.Ext.RegistrarExtensions").expect("extension class present");
    let entries: Vec<(&str, usize, usize, i64)> = d
        .extension_methods
        .iter()
        .map(|x| {
            (
                x.name.as_str(),
                x.type_param_count,
                x.arity_min,
                x.arity_max,
            )
        })
        .collect();
    assert_eq!(
        entries,
        vec![("M", 2, 0, 0), ("M", 1, 0, 0), ("N", 0, 0, 0)],
        "two overloads sharing a value range but not a type-parameter count stay two entries"
    );
}

#[test]
fn extract_dump_carries_both_new_keys() {
    let e = extract_src(
        r#"
namespace App.Ext;

public static class RegistrarExtensions
{
    public static void M<T, U>(this IRegistrar x) { }
    public static void Run(IRegistrar x) => x.M<Alpha, Beta>();
}
"#,
    );
    let json = crate::extract::extraction_to_json(&e);
    assert!(json.contains("\"typeParamCount\": 2"), "{json}");
    assert!(json.contains("\"methodTypeArgCount\": 2"), "{json}");
}

#[test]
fn tuple_arguments_and_comments_inside_the_list_do_not_change_the_count() {
    let e = extract_src(
        r#"
namespace App.Consumers;

public class Runner
{
  public void Run(IRegistrar x)
  {
    x.Add<(int, string), Beta>();
    x.Add</* first */ Alpha, /* second, still one */ Beta>();
    var group = x.Add<Alpha>;
    var less = x.Count < limit;
  }
}
"#,
    );
    assert_eq!(
        member_type_arg_counts(&e),
        vec![
            ("Add".to_string(), Some(0), Some(2)),
            ("Add".to_string(), Some(0), Some(2)),
            ("Add".to_string(), None, Some(1)),
            ("Count".to_string(), None, None),
        ],
        "a method group keeps its written list but carries no call shape, and a comparison is no list"
    );
}
