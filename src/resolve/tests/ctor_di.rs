use super::*;

#[test]
fn ctor_di_a_closed_generic_ctor_param_resolves_to_the_open_generic_implementation_that_passes_its_type_argument_through(
) {
    let files = vec![
        (
            "Di/IRepository.cs".to_string(),
            frag(
                vec![def(
                    "App.Di.IRepository",
                    "IRepository",
                    "App.Di",
                    "interface",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Di/MongoRepository.cs".to_string(),
            frag(
                vec![def_with_bases_and_generics(
                    "App.Di.MongoRepository",
                    "MongoRepository",
                    "App.Di",
                    &["IRepository"],
                    &["T"],
                    &[("IRepository", &["*"])],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Di/User.cs".to_string(),
            frag(
                vec![def("App.Di.User", "User", "App.Di", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "Di/Controller.cs".to_string(),
            frag(
                vec![def("App.Di.Controller", "Controller", "App.Di", "class")],
                vec![],
                vec![ctor_param_ref(
                    "IRepository",
                    "App.Di",
                    Some(vec!["User".to_string()]),
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let edges = ctor_di_edges(&g, "IRepository");
    assert_eq!(edges.len(), 1);
    match edges[0] {
        Edge::CtorDi {
            resolution,
            args,
            to,
            candidates,
            ..
        } => {
            assert_eq!(resolution, "open-generic");
            assert_eq!(args.as_deref(), Some(&["User".to_string()][..]));
            assert_eq!(to.as_deref(), Some("App.Di.MongoRepository"));
            assert!(candidates.is_empty());
        }
        _ => unreachable!(),
    }
}

#[test]
fn ctor_di_a_plain_non_generic_ctor_param_resolves_to_its_sole_implementor() {
    let files = vec![
        (
            "Di/IFooService.cs".to_string(),
            frag(
                vec![def(
                    "App.Di.IFooService",
                    "IFooService",
                    "App.Di",
                    "interface",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Di/FooService.cs".to_string(),
            frag(
                vec![def_with_bases_and_generics(
                    "App.Di.FooService",
                    "FooService",
                    "App.Di",
                    &["IFooService"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Di/Controller.cs".to_string(),
            frag(
                vec![def("App.Di.Controller", "Controller", "App.Di", "class")],
                vec![],
                vec![ctor_param_ref("IFooService", "App.Di", None)],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let edges = ctor_di_edges(&g, "IFooService");
    assert_eq!(edges.len(), 1);
    match edges[0] {
        Edge::CtorDi {
            resolution,
            args,
            to,
            ..
        } => {
            assert_eq!(resolution, "plain");
            assert_eq!(
                *args, None,
                "a non-generic ctor param carries no args field at all"
            );
            assert_eq!(to.as_deref(), Some("App.Di.FooService"));
        }
        _ => unreachable!(),
    }
}

#[test]
fn ctor_di_a_ctor_param_type_absent_from_the_corpus_is_classified_infra_when_the_file_imports_a_bcl_namespace(
) {
    let files = vec![(
        "Di/Controller.cs".to_string(),
        frag(
            vec![def("App.Di.Controller", "Controller", "App.Di", "class")],
            vec![FragUsing::Plain {
                text: "Microsoft.Extensions.Logging".into(),
                global: false,
            }],
            vec![ctor_param_ref(
                "ILogger",
                "App.Di",
                Some(vec!["Controller".to_string()]),
            )],
        ),
    )];
    let g = resolve_graph(&no_git_root(), &files);
    let edges = ctor_di_edges(&g, "ILogger");
    assert_eq!(edges.len(), 1);
    match edges[0] {
        Edge::CtorDi {
            resolution,
            args,
            to,
            ..
        } => {
            assert_eq!(resolution, "infra");
            assert_eq!(args.as_deref(), Some(&["Controller".to_string()][..]));
            assert_eq!(*to, None);
        }
        _ => unreachable!(),
    }
}

#[test]
fn ctor_di_an_unresolvable_ctor_param_with_no_bcl_using_in_scope_is_unresolved_not_dropped() {
    let files = vec![(
        "Di/Controller.cs".to_string(),
        frag(
            vec![def("App.Di.Controller", "Controller", "App.Di", "class")],
            vec![],
            vec![ctor_param_ref("ISomeThirdPartyThing", "App.Di", None)],
        ),
    )];
    let g = resolve_graph(&no_git_root(), &files);
    let edges = ctor_di_edges(&g, "ISomeThirdPartyThing");
    assert_eq!(edges.len(), 1);
    match edges[0] {
        Edge::CtorDi { resolution, .. } => assert_eq!(resolution, "unresolved"),
        _ => unreachable!(),
    }
}

#[test]
fn ctor_di_two_implementors_tied_at_the_same_precedence_tier_are_ambiguous_never_guessed() {
    let files = vec![
        (
            "Di/IFooService.cs".to_string(),
            frag(
                vec![def(
                    "App.Di.IFooService",
                    "IFooService",
                    "App.Di",
                    "interface",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Di/FooServiceA.cs".to_string(),
            frag(
                vec![def_with_bases_and_generics(
                    "App.Di.FooServiceA",
                    "FooServiceA",
                    "App.Di",
                    &["IFooService"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Di/FooServiceB.cs".to_string(),
            frag(
                vec![def_with_bases_and_generics(
                    "App.Di.FooServiceB",
                    "FooServiceB",
                    "App.Di",
                    &["IFooService"],
                    &[],
                    &[],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Di/Controller.cs".to_string(),
            frag(
                vec![def("App.Di.Controller", "Controller", "App.Di", "class")],
                vec![],
                vec![ctor_param_ref("IFooService", "App.Di", None)],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let edges = ctor_di_edges(&g, "IFooService");
    assert_eq!(edges.len(), 1);
    match edges[0] {
        Edge::CtorDi {
            resolution,
            candidates,
            ..
        } => {
            assert_eq!(resolution, "ambiguous");
            assert_eq!(
                candidates.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
                vec!["App.Di.FooServiceA", "App.Di.FooServiceB"]
            );
        }
        _ => unreachable!(),
    }
}

#[test]
fn ctor_di_a_closed_implementor_is_preferred_over_an_open_generic_one_when_both_exist() {
    let files = vec![
        (
            "Di/IRepository.cs".to_string(),
            frag(
                vec![def(
                    "App.Di.IRepository",
                    "IRepository",
                    "App.Di",
                    "interface",
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Di/MongoRepository.cs".to_string(),
            frag(
                vec![def_with_bases_and_generics(
                    "App.Di.MongoRepository",
                    "MongoRepository",
                    "App.Di",
                    &["IRepository"],
                    &["T"],
                    &[("IRepository", &["*"])],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Di/SpecificUserRepository.cs".to_string(),
            frag(
                vec![def_with_bases_and_generics(
                    "App.Di.SpecificUserRepository",
                    "SpecificUserRepository",
                    "App.Di",
                    &["IRepository"],
                    &[],
                    &[("IRepository", &["User"])],
                )],
                vec![],
                vec![],
            ),
        ),
        (
            "Di/User.cs".to_string(),
            frag(
                vec![def("App.Di.User", "User", "App.Di", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "Di/Controller.cs".to_string(),
            frag(
                vec![def("App.Di.Controller", "Controller", "App.Di", "class")],
                vec![],
                vec![ctor_param_ref(
                    "IRepository",
                    "App.Di",
                    Some(vec!["User".to_string()]),
                )],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let edges = ctor_di_edges(&g, "IRepository");
    assert_eq!(edges.len(), 1);
    match edges[0] {
        Edge::CtorDi { resolution, to, .. } => {
            assert_eq!(resolution, "closed", "the non-generic, exactly-matching implementor wins over the open-generic passthrough");
            assert_eq!(to.as_deref(), Some("App.Di.SpecificUserRepository"));
        }
        _ => unreachable!(),
    }
}

// --- the property hop and the call hop ---
