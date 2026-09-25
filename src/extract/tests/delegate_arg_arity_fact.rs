use super::*;

// --- lambdaArgArity: a delegate-shaped argument's own parameter count -----

// The fact recorded on the `this.Take(...)` call on `line`.
fn take_arity(e: &Extraction, line: usize) -> Option<Vec<Option<usize>>> {
    e.refs
        .iter()
        .find(|r| r.member.as_deref() == Some("Take") && r.line == line)
        .unwrap_or_else(|| panic!("no Take ref on line {line}"))
        .lambda_arg_arity
        .clone()
}

#[test]
fn a_local_function_argument_records_its_own_parameter_count() {
    let e = extract_src(
        r#"
namespace App.Groups;

public class Host
{
  public void Run()
  {
    object One(int a) => a;
    object Two(int a, int b) => a;
    this.Take(One);
    this.Take(7, Two);
    {
      this.Take(x => x, One);
    }
  }
}
"#,
    );
    assert_eq!(take_arity(&e, 10), Some(vec![Some(1)]));
    assert_eq!(
        take_arity(&e, 11),
        Some(vec![None, Some(2)]),
        "positions line up with the call's own argument list"
    );
    assert_eq!(
        take_arity(&e, 13),
        Some(vec![Some(1), Some(1)]),
        "a local function declared in an enclosing block is in scope"
    );
}

#[test]
fn a_method_group_that_is_not_one_unshadowed_local_function_records_nothing() {
    let e = extract_src(
        r#"
namespace App.Groups;

public class Host
{
  object Member(int a) => a;

  public void Run(object[] items)
  {
    object Local(int a) => a;
    this.Take(Member);
    this.Take(make: Local);
    System.Action run = () =>
    {
      System.Func<int, int, object> Local = null;
      this.Take(Local);
    };
    System.Action<object> each = Local => this.Take(Local);
    System.Action loop = () => { foreach (var Local in items) { this.Take(Local); } };
  }

  public void Other()
  {
    this.Take(Local);
  }
}
"#,
    );
    let cases = [
        (11, "a member method group needs overload resolution"),
        (12, "a named argument can be reordered"),
        (
            16,
            "a nested function's local variable shadows the local function",
        ),
        (18, "a lambda parameter shadows the local function"),
        (19, "a foreach variable shadows the local function"),
        (24, "another member's local function is out of scope"),
    ];
    for (line, why) in cases {
        assert_eq!(take_arity(&e, line), None, "line {line}: {why}");
    }
}
