use super::def_member_facts::member_facts;
use super::*;

// --- Foreach element type fact -----------------------------------

#[test]
fn ds0011_var_over_a_generic_collection_resolves_the_single_type_argument() {
    let e = extract_src(
        r#"
namespace App.ForEachVar;

public class Host
{
  private List<Widget> _field;

  public void Run(IEnumerable<Widget> param)
  {
    List<Widget> local = null;
    foreach (var a in _field) { a.Go(); }
    foreach (var b in param) { b.Go(); }
    foreach (var c in local) { c.Go(); }
  }
}
"#,
    );
    // A field, a parameter and a local: the same lookup the call
    // hop uses for its qualifier, reading whichever table vouches for the
    // collection's name.
    assert_eq!(
        member_facts(&e),
        vec![
            ("Go", Some("Widget")),
            ("Go", Some("Widget")),
            ("Go", Some("Widget"))
        ]
    );
}

#[test]
fn ds0011_a_var_foreach_stays_unknown_unless_the_collection_is_a_bare_identifier_with_one_type_argument(
) {
    let e = extract_src(
        r#"
namespace App.ForEachUnknown;

public class Host
{
  private Widget[] _array;
  private Dictionary<string, Widget> _pair;
  private Map map;

  public void Run<T>(List<T> generic)
  {
    foreach (var a in GetItems()) { a.Go(); }
    foreach (var b in map.Items) { b.Go(); }
    foreach (var c in _array) { c.Go(); }
    foreach (var d in _pair) { d.Go(); }
    foreach (var f in generic) { f.Go(); }
  }

  private List<Widget> GetItems() { return null; }
}
"#,
    );
    // A call, a dotted chain, an array (no top-level type-argument list
    // at all), a two-argument generic (not a SINGLE type argument), and a
    // one-argument generic whose argument is the method's own type
    // parameter (the wildcard descriptor, "*" -- nothing at this site
    // knows what it is bound to) all leave the loop variable exactly as
    // taken-but-unknown as every other unresolvable local. Filtered to
    // `Go` alone: `map.Items` is itself an ordinary tier-(e) member
    // access on the FIELD `map` (declared type `Map`), unrelated to this
    // ticket, so it earns its own unaffected receiver fact.
    let go_facts: Vec<_> = e
        .refs
        .iter()
        .filter(|r| r.member.as_deref() == Some("Go"))
        .map(|r| r.receiver_type.as_deref())
        .collect();
    assert_eq!(go_facts, vec![None, None, None, None, None]);
}

#[test]
fn ds0011_a_foreach_variable_used_as_another_foreachs_collection_is_never_a_fact() {
    let e = extract_src(
        r#"
namespace App.ForEachChain;

public class Host
{
  private List<Widget> _bag;

  public void Nested()
  {
    foreach (var outer in _bag)
    {
      outer.Go();
      foreach (var inner in outer) { inner.Go(); }
    }
  }
}
"#,
    );
    // `outer` resolves to a REAL fact (`_bag`'s single type argument), but
    // that derived fact never carries a type argument of its own (see
    // `collection_element_fact`), so `inner` -- one nested hop further --
    // finds no single argument to read and stays taken-but-unknown. One
    // hop, never a chain, structurally: a foreach element fact can be a
    // RECEIVER but never itself a collection another `var` derives from.
    assert_eq!(member_facts(&e), vec![("Go", Some("Widget")), ("Go", None)]);
}

#[test]
fn ds0011_a_foreach_variable_conflicting_with_another_declaration_stays_taken_but_unknown() {
    let e = extract_src(
        r#"
namespace App.ForEachConflict;

public class Host
{
  private List<Widget> made;

  public void Run(bool flag)
  {
    if (flag) { foreach (var made in Gadgets()) { made.Go(); } }
    else { Widget made = new Widget(); made.Go(); }
  }

  private List<Gadget> Gadgets() { return null; }
}
"#,
    );
    // `made` is declared twice with conflicting shapes in sibling blocks
    // of the SAME method (a foreach-derived local in one arm, an
    // explicit local in the other): the flat member table collapses both
    // to the same taken-but-unknown slot -- the ordinary `add_fact`
    // conflict rule, unchanged by this ticket -- with no fall-through to
    // the enclosing field of the same name.
    assert_eq!(member_facts(&e), vec![("Go", None), ("Go", None)]);
}

#[test]
fn ds0011_a_destructuring_foreach_variable_is_left_alone() {
    // `foreach (var (a, b) in pairs)` has no single name for the element
    // fact to name -- deliberately outside this ticket's scope, and its
    // absence must not disturb an ordinary sibling declaration's own fact.
    let e = extract_src(
        r#"
namespace App.ForEachTuple;

public class Host
{
  public void Run(List<(int, int)> pairs)
  {
    Widget w = new Widget();
    foreach (var (a, b) in pairs) { }
    w.Go();
  }
}
"#,
    );
    assert_eq!(member_facts(&e), vec![("Go", Some("Widget"))]);
}
