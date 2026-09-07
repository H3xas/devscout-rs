use super::*;

// --- method_params: per-overload parameter descriptors -----------------

#[test]
fn stage4_method_params_record_each_overload_with_type_parameters_as_wildcards() {
    let e = extract_src(
        r#"
namespace App.MethodParams;

public class Registrar<TKey>
{
    public void Register(Action<Options> configure) { }
    public void Register(string name, Func<Options, bool> pick) { }
    public void Register<T>(Action<T> configure, TKey key) { }
    public void Ping() { }
    private void Seed(List<Options>? items, Options[] more, Expression<Func<Options, object>> selector) { }
}
"#,
    );
    let d = find_def(&e, "App.MethodParams.Registrar").expect("Registrar def present");
    let params: Vec<(&str, &[Vec<String>])> = d
        .method_params
        .iter()
        .map(|(n, o)| (n.as_str(), o.as_slice()))
        .collect();
    assert_eq!(
        params,
        vec![
            (
                "Register",
                &[
                    vec!["Action<Options>".to_string()],
                    vec!["string".to_string(), "Func<Options,bool>".to_string()],
                    vec!["Action<*>".to_string(), "*".to_string()],
                ][..]
            ),
            ("Ping", &[vec![]][..]),
            (
                "Seed",
                &[vec![
                    "List<Options>".to_string(),
                    "Options[]".to_string(),
                    "Expression<Func<Options,object>>".to_string(),
                ]][..]
            ),
        ]
    );
}

#[test]
fn stage4_delegate_declaration_records_its_parameters_under_invoke() {
    let e = extract_src(
        r#"
namespace App.Delegates;

public delegate void Configure(Options options, int depth);
"#,
    );
    let d = find_def(&e, "App.Delegates.Configure").expect("Configure delegate present");
    assert_eq!(
        d.method_params,
        vec![(
            "Invoke".to_string(),
            vec![vec!["Options".to_string(), "int".to_string()]]
        )]
    );
}

#[test]
fn stage4_extension_method_params_mark_the_this_parameter() {
    let e = extract_src(
        r#"
namespace App.Wiring;

public static class Ext
{
    public static void Wire(this Host host, Action<Options> configure) { }
}
"#,
    );
    let d = find_def(&e, "App.Wiring.Ext").expect("Ext def present");
    assert_eq!(
        d.method_params,
        vec![(
            "Wire".to_string(),
            vec![vec!["this Host".to_string(), "Action<Options>".to_string()]]
        )]
    );
}
