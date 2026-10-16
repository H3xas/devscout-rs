use super::*;
use crate::graph::FragHandlerRegistration;

// A registered carrier whose only `methodParams` descriptor naming its own
// base argument sits under `key`. The descriptor is planted even under the
// mark keys, which the extractor writes with empty parameter lists, so the
// skip is proven by the key alone and not by an empty list.
fn carrier_files(key: &str) -> Vec<(String, Fragment)> {
    let mut carrier = FragDef {
        bases: vec!["ShoalBase".to_string()],
        ..def(
            "App.Shoal.ShoalIndexer",
            "ShoalIndexer",
            "App.Shoal",
            "class",
        )
    };
    carrier
        .base_generic_args
        .insert("ShoalBase".to_string(), vec!["ShoalNotice".to_string()]);
    carrier
        .method_params
        .insert("Fetch".to_string(), vec![Vec::new()]);
    carrier
        .method_params
        .insert(key.to_string(), vec![vec!["ShoalNotice".to_string()]]);

    let mut install = frag(
        vec![def(
            "App.Shoal.Installation",
            "Installation",
            "App.Shoal",
            "class",
        )],
        vec![],
        vec![],
    );
    install.handler_registrations = vec![FragHandlerRegistration {
        handler: "ShoalIndexer".to_string(),
        namespace: "App.Shoal".to_string(),
        line: 5,
    }];

    let mut publisher = serde_json::to_value(frag(
        vec![def(
            "App.Shoal.Publisher",
            "Publisher",
            "App.Shoal",
            "class",
        )],
        vec![],
        vec![],
    ))
    .unwrap();
    publisher["publishes"] = serde_json::json!([{
        "verb": "Publish",
        "message": "ShoalNotice",
        "namespace": "App.Shoal",
        "line": 9,
    }]);

    vec![
        (
            "Shoal/Messages.cs".to_string(),
            frag(
                vec![def(
                    "App.Shoal.ShoalNotice",
                    "ShoalNotice",
                    "App.Shoal",
                    "class",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Shoal/Carriers.cs".to_string(),
            frag(vec![carrier], vec![], vec![]),
        ),
        ("Shoal/Installation.cs".to_string(), install),
        (
            "Shoal/Publisher.cs".to_string(),
            serde_json::from_value(publisher).unwrap(),
        ),
    ]
}

// The carrier's second partial declaration, in a file of its own, declaring
// one method `name` with the given parameter descriptors.
fn second_part(name: &str, params: &[&str]) -> (String, Fragment) {
    let mut part = def(
        "App.Shoal.ShoalIndexer",
        "ShoalIndexer",
        "App.Shoal",
        "class",
    );
    part.method_params.insert(
        name.to_string(),
        vec![params.iter().map(|p| p.to_string()).collect()],
    );
    (
        "Shoal/CarriersMore.cs".to_string(),
        frag(vec![part], vec![], vec![]),
    )
}

fn carrier_hops(key: &str) -> Vec<(String, String, String)> {
    hops_of(&carrier_files(key))
}

fn hops_of(files: &[(String, Fragment)]) -> Vec<(String, String, String)> {
    let g = resolve_graph(&no_git_root(), files);
    g.edges
        .iter()
        .filter_map(|e| match e {
            Edge::BusHop {
                message,
                to,
                evidence,
                ..
            } => Some((to.clone(), message.clone(), evidence.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn reserved_param_keys_ac_2_a_descriptor_under_a_reserved_key_admits_no_base() {
    for key in [
        "@result:Index",
        "@property:Index",
        "@field:Index",
        "@variant",
        "@implicit-conversion",
        "@interpolated-string-handler",
    ] {
        assert_eq!(
            carrier_hops(key),
            Vec::<(String, String, String)>::new(),
            "a descriptor under `{key}` is no declared parameter"
        );
    }
}

#[test]
fn reserved_param_keys_ac_2_the_same_descriptor_under_a_method_name_admits_the_base() {
    assert_eq!(
        carrier_hops("Index"),
        vec![(
            "App.Shoal.ShoalIndexer".to_string(),
            "App.Shoal.ShoalNotice".to_string(),
            "base-arg".to_string()
        )]
    );
}

#[test]
fn reserved_param_keys_ac_2_a_partial_declaration_merges_no_reserved_entry_into_a_parameter() {
    let mut files = carrier_files("@result:Index");
    files.push(second_part("Index", &[]));
    assert_eq!(
        hops_of(&files),
        Vec::<(String, String, String)>::new(),
        "the other part's parameterless Index does not turn the result into a parameter"
    );

    let mut files = carrier_files("@result:Index");
    files.push(second_part("Watch", &["ShoalNotice"]));
    assert_eq!(
        hops_of(&files),
        vec![(
            "App.Shoal.ShoalIndexer".to_string(),
            "App.Shoal.ShoalNotice".to_string(),
            "base-arg".to_string()
        )],
        "a real parameter in another part still admits"
    );
}
