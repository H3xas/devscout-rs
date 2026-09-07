use super::def_member_facts::member_facts;
use super::*;

// --- Property types and var-from-invocation ----------

#[test]
fn ds0012_def_records_property_types_in_source_order_with_generic_args() {
    let e = extract_src(
        r#"
namespace App.PropTypes;

public class Widget
{
  public Settings Config { get; set; }
  public string Label { get; set; }
  public Box<Gadget> Slots { get; set; }
  public Settings Config { get; set; }
}
"#,
    );
    let d = find_def(&e, "App.PropTypes.Widget").expect("Widget def present");
    assert_eq!(d.properties, vec!["Config", "Label", "Slots"]);
    let recorded: Vec<_> = d
        .property_types
        .iter()
        .map(|(n, f)| (n.as_str(), f.type_name.as_str(), f.args.as_ref()))
        .collect();
    // A predefined type vouches for nothing, exactly as it does for a local
    // or a method return, so `Label` has no entry -- the map is parallel to
    // `properties` but not equal in length.
    assert_eq!(
        recorded,
        vec![
            ("Config", "Settings", None),
            ("Slots", "Box", Some(&vec!["Gadget".to_string()]))
        ]
    );
}

#[test]
fn ds0012_a_property_vouches_for_its_own_name_like_a_field() {
    let e = extract_src(
        r#"
namespace App.PropFacts;

public class Host
{
  private Widget _field;
  public Widget Current { get; set; }
  public string Name { get; set; }

  public void Run()
  {
    _field.Render();
    Current.Render();
    Name.Trim();
  }
}
"#,
    );
    // A predefined property type is still no fact, and the name stays
    // TAKEN: `Name` can never be read back as a TYPE named Name.
    assert_eq!(
        member_facts(&e),
        vec![
            ("Render", Some("Widget")),
            ("Render", Some("Widget")),
            ("Trim", None)
        ]
    );
}

#[test]
fn ds0012_receiver_property_owner_is_recorded_only_for_a_typed_two_segment_chain() {
    let e = extract_src(
        r#"
namespace App.ChainHeads;

public class Host
{
  private Widget _widget;
  private string _text;

  public void Run(Widget param)
  {
    _widget.Config.Reload();
    param.Config.Reload();
    _text.Config.Reload();
    unknown.Config.Reload();
    _widget.Config.Inner.Reload();
    App.Other.Thing.Reload();
  }
}
"#,
    );
    let owners: Vec<_> = e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-member")
        .map(|r| {
            (
                r.qualified.as_deref().unwrap_or(r.name.as_str()),
                r.receiver_property_owner.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        owners,
        vec![
            ("_widget.Config", Some("Widget")),
            // The head window itself is a BARE qualifier: a receiverType,
            // never an owner.
            ("_widget", None),
            ("param.Config", Some("Widget")),
            ("param", None),
            // Head typed `string`: no fact, so no owner.
            ("_text.Config", None),
            ("_text", None),
            // Head nothing in scope declares: no fact, so no owner.
            ("unknown.Config", None),
            ("unknown", None),
            // Three segments: only the innermost pair is a hop this fact
            // can start.
            ("_widget.Config.Inner", None),
            ("_widget.Config", Some("Widget")),
            ("_widget", None),
            // A namespace path is not a typed head.
            ("App.Other.Thing", None),
            ("App.Other", None),
            ("App", None),
        ]
    );
}

#[test]
fn ds0010_var_from_a_qualified_invocation_records_the_callee_owner_and_member() {
    let e = extract_src(
        r#"
namespace App.CallFacts;

public class Host
{
  private Factory _factory;

  public void Run()
  {
    var made = _factory.Make();
    made.Render();
    var stat = Factory.Create();
    stat.Render();
    var bare = Compute();
    bare.Render();
    var chained = made.Wrap();
    chained.Render();
  }
}
"#,
    );
    let facts: Vec<_> = e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Render"))
        .map(|r| {
            (
                r.name.as_str(),
                r.receiver_type.as_deref(),
                r.receiver_call_owner.as_deref(),
                r.receiver_call_member.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        facts,
        vec![
            // An instance callee: the owner is what the QUALIFIER's own
            // fact says.
            ("made", None, Some("Factory"), Some("Make")),
            // A static callee: nothing in scope claims the name, so the
            // qualifier text is itself the type name candidate.
            ("stat", None, Some("Factory"), Some("Create")),
            // A bare call has no qualifier to put through the ladder.
            ("bare", None, None, None),
            // The qualifier is itself one of these locals: one hop, never
            // a chain.
            ("chained", None, None, None),
        ]
    );
    assert!(
        e.refs
            .iter()
            .filter(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Render"))
            .all(|r| !r.receiver_awaited),
        "none of these calls are awaited, so none of their facts unwrap a Task later"
    );
}

#[test]
fn ds0010_a_conflicting_second_declaration_cancels_the_call_fact() {
    let e = extract_src(
        r#"
namespace App.CallConflict;

public class Host
{
  private Widget made;

  public void Run(bool flag)
  {
    if (flag) { var made = Factory.Make(); made.Render(); }
    else { Gadget made = new Gadget(); made.Render(); }
  }
}
"#,
    );
    let facts: Vec<_> = e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Render"))
        .map(|r| (r.receiver_type.as_deref(), r.receiver_call_owner.as_deref()))
        .collect();
    // Both windows read the same conflicted slot: no type fact, no call
    // fact, and no fall-through to the same-named FIELD of a different
    // type.
    assert_eq!(facts, vec![(None, None), (None, None)]);
}
