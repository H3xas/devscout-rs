use super::*;

const DOMAIN: &str = "namespace App { public class Svc { public void Use(int v) {} } \
     public class Gadget { public void Ping() {} } public class Widget { public void Ping() {} } }";

// Every `uses-member` edge from the calling file for `member`, as
// (target, heuristic).
fn edges(run: &str, member: &str) -> Vec<(String, bool)> {
    let graph = resolve_graph(
        &no_git_root(),
        &fragments_for(&[("Domain/Types.cs", DOMAIN), ("Use/Run.cs", run)]),
    );
    graph
        .edges
        .iter()
        .filter_map(|edge| match edge {
            Edge::UsesMember {
                from_file,
                to,
                heuristic,
                member: m,
                ..
            } if from_file == "Use/Run.cs" && m.as_deref() == Some(member) => {
                Some((to.clone(), *heuristic))
            }
            _ => None,
        })
        .collect()
}

fn assert_binds_svc_precisely(run: &str) {
    let got = edges(run, "Use");
    assert!(
        !got.is_empty() && got.iter().all(|(to, h)| to == "App.Svc" && !h),
        "{run}: {got:?}"
    );
}

fn runner(body: &str) -> String {
    format!("namespace App {{ public class Runner {{ {body} }} }}")
}

#[test]
fn member_scope_ac_1_a_deconstructed_name_beside_a_field_binds_the_selected_overload() {
    assert_binds_svc_precisely(&runner(
        "string a; public void Run(Svc s, (int, int) pair) { var (a, b) = pair; s.Use(a); }",
    ));
    assert_binds_svc_precisely(&runner(
        "string k; public void Run(Svc s, (int, int)[] pairs) { foreach (var (k, v) in pairs) { s.Use(k); } }",
    ));
    assert_binds_svc_precisely(&runner(
        "string x; public void Run(Svc s, object o) { if (o is var (x, y)) { s.Use(x); } }",
    ));
}

#[test]
fn member_scope_ac_2_a_use_outside_every_declaring_construct_binds_the_selected_overload() {
    let bodies = [
        "int v; public void Run(Svc s) { s.Use(v); { string v = \"x\"; } }",
        "int v; public void Run(Svc s) { { string v = \"x\"; } { s.Use(v); } }",
        "int v; public void Run(Svc s, System.Action<string> f) { f = (string v) => { }; s.Use(v); }",
        "int v; public void Run(Svc s) { void Local(string v) { } s.Use(v); }",
        "int v; public void Run(Svc s, string[] vs) { foreach (string v in vs) { } s.Use(v); }",
        "int v; public void Run(Svc s) { try { } catch (Gadget v) { } s.Use(v); }",
        "int v; public void Run(Svc s, object o) { { if (o is string v) { } } s.Use(v); }",
    ];
    for body in bodies {
        assert_binds_svc_precisely(&runner(body));
    }
}

#[test]
fn member_scope_ac_2_an_out_of_scope_receiver_takes_no_local_type() {
    let got = edges(
        &runner("Gadget w; public void Run() { { Widget w = null; } w.Ping(); }"),
        "Ping",
    );
    assert!(
        !got.iter().any(|(to, h)| to == "App.Widget" && !h),
        "{got:?}"
    );
}

#[test]
fn member_scope_ac_3_in_construct_controls_stay_precise() {
    let bodies = [
        "string id; public void Run(Svc s, System.Collections.Generic.List<int> ids) { ids.ForEach(id => s.Use(id)); }",
        "public void Run(Svc s) { { string v = \"a\"; } { int v = 1; s.Use(v); } }",
        "string x; public void Run(Svc s, int[] xs) { foreach (var x in xs) { s.Use(x); } }",
        "string x; public void Run(Svc s, int[] xs) { var q = from x in xs select Touch(s, x); } int Touch(Svc s, int x) { s.Use(x); return x; }",
        "string v; public void Run(Svc s) { void Local(int v) { s.Use(v); } Local(1); }",
        "string v; public void Run(Svc s, object o) { switch (o) { case int v: s.Use(v); break; } }",
        "public void Run(Svc s, System.Collections.Generic.List<int> xs, System.Action<string> sink) { sink = (string v) => { }; xs.ForEach(v => s.Use(v)); }",
        "public void Run(Svc s, System.Func<string, int> f, System.Action<int> g) { f = (string v) => 0; g = (int v) => s.Use(v); }",
        "public void Run(Svc s, object a, object b) { if (a is string w) { } if (b is int w2) { s.Use(w2); } }",
        "string n; public void Run(Svc s) { try { } catch (System.Exception) { int n = 1; s.Use(n); } }",
        "string n; public void Run(Svc s) { int.TryParse(\"1\", out var n); s.Use(n); }",
        "string v; public void Run(Svc s, int v) { s.Use(v); { s.Use(v); } }",
        "string n; public void Run(Svc s, string t) { if (!int.TryParse(t, out var n)) return; s.Use(n); }",
    ];
    for body in bodies {
        assert_binds_svc_precisely(&runner(body));
    }
    assert_binds_svc_precisely(
        "namespace App { public class Runner(int v) { string v2; public void Run(Svc s) { s.Use(v); } } }",
    );
}
