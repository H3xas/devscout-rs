use super::*;

// --- Def member facts -------------------------------------------------------

pub(super) fn member_facts(e: &Extraction) -> Vec<(&str, Option<&str>)> {
    e.refs
        .iter()
        .filter(|r| r.kind == "uses-member")
        .map(|r| {
            (
                r.member.as_deref().unwrap_or(""),
                r.receiver_type.as_deref(),
            )
        })
        .collect()
}

#[test]
fn stage2a_def_records_properties_fields_and_method_returns() {
    let e = extract_src(
        r#"
namespace App.Facts;

public class Widget
{
  private readonly ILogger _log;
  private int a, b;
  public const int Max = 3;
  public static string Prefix { get; } = "p";
  public string Name => "n";
  public int this[int i] => i;
  public event System.EventHandler Changed;

  public Task<Foo> GetAsync() => null;
  public void Nothing() { }
  public Some.Ns.Thing Qualified() => null;
  public int Num() => 1;
  private Hidden Secret() => null;
}
"#,
    );
    let d = find_def(&e, "App.Facts.Widget").expect("Widget def present");
    assert_eq!(
        d.properties,
        vec!["Prefix", "Name"],
        "source order; the indexer is not a property"
    );
    assert_eq!(
        d.fields,
        vec!["_log", "a", "b", "Max"],
        "every declarator of every field, incl. const; the event is not a field"
    );
    assert_eq!(
            d.method_returns,
            vec![("GetAsync".to_string(), "Task".to_string()), ("Qualified".to_string(), "Thing".to_string())],
            "generic args stripped to the base identifier, a qualified return reduced to its last segment, void/int omitted, FIRST-declaration order"
        );
    assert!(
        !d.methods.contains(&"Secret".to_string()),
        "methodReturns stays parallel to methods: a private method is in neither"
    );
    assert!(d.method_returns.iter().all(|(n, _)| n != "Secret"));
}

#[test]
fn stage2a_type_declaring_none_of_the_new_members_records_all_three_empty() {
    let e = extract_src("namespace App.Bare { public class Empty { public void Go() { } } }");
    let d = find_def(&e, "App.Bare.Empty").expect("Empty def present");
    assert_eq!(d.methods, vec!["Go"]);
    assert!(d.properties.is_empty());
    assert!(d.fields.is_empty());
    assert!(
        d.method_returns.is_empty(),
        "empty means OMITTED at serialization -- pre-stage-2 bytes preserved"
    );
}

#[test]
fn stage2a_method_returns_keeps_the_first_overload_and_never_backfills_a_void_first_declaration() {
    let e = extract_src(
        r#"
namespace App.Overloads;

public class Api
{
  public Foo Get(int id) => null;
  public Bar Get(string key) => null;
  public void Send(int id) { }
  public Receipt Send(string key) => null;
}
"#,
    );
    let d = find_def(&e, "App.Overloads.Api").expect("Api def present");
    assert_eq!(
        d.method_returns,
        vec![("Get".to_string(), "Foo".to_string())],
        "Get keeps the first overload; Send is blocked by its void first declaration"
    );
}

#[test]
fn stage2a_interface_records_properties_and_returns_without_any_public_modifier() {
    let e = extract_src(
        r#"
namespace App.Contracts;

public interface IRepo
{
  string Name { get; }
  Widget Find(int id);
}
"#,
    );
    let d = find_def(&e, "App.Contracts.IRepo").expect("IRepo def present");
    assert_eq!(d.properties, vec!["Name"]);
    assert_eq!(
        d.method_returns,
        vec![("Find".to_string(), "Widget".to_string())]
    );
    assert!(d.fields.is_empty());
}
