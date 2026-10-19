use std::collections::BTreeSet;

use super::*;

const NAMESPACE_ROOTS: [&str; 3] = ["MassTransit", "Microsoft.Extensions", "System"];

fn qualified(name: &str) -> bool {
    name.contains('.')
        && NAMESPACE_ROOTS
            .iter()
            .any(|root| name == *root || name.starts_with(&format!("{root}.")))
}

fn is_type(name: &str) -> bool {
    type_entry(name).is_some()
}

#[test]
fn transport_facts_ac_1_no_entry_is_listed_twice() {
    let mut types = BTreeSet::new();
    for entry in TYPES {
        assert!(types.insert(entry.name), "{} twice", entry.name);
    }
    let mut simple_names = BTreeSet::new();
    for entry in TYPES {
        assert!(
            simple_names.insert((simple(entry.name), arity(entry.name))),
            "{} shares its simple name and arity",
            entry.name
        );
    }
    let mut overloads = BTreeSet::new();
    for member in members() {
        for overload in member.overloads {
            let signature = format!(
                "{}.{}{:?}{:?}{:?}",
                overload.decl, member.name, overload.on, overload.types, overload.params
            );
            assert!(overloads.insert(signature.clone()), "{signature} twice");
        }
    }
    let mut keys = std::collections::BTreeMap::new();
    for member in members() {
        for overload in member.overloads {
            let key = format!("{}.{}", overload.decl, member.name);
            let previous = keys.insert(key.clone(), member as *const Member);
            assert!(
                previous.is_none_or(|p| std::ptr::eq(p, member)),
                "{key} names two members"
            );
            let (found, _) = member_by_key(&key).expect("every key resolves");
            assert!(std::ptr::eq(found, member), "{key}");
        }
    }
}

#[test]
fn transport_facts_ac_1_every_name_is_fully_qualified() {
    for entry in TYPES {
        assert!(qualified(entry.name), "{}", entry.name);
        for base in entry.bases {
            assert!(is_type(base), "{}: base {base}", entry.name);
        }
        if let Some(endpoint) = entry.endpoint {
            assert!(is_type(endpoint), "{}: {endpoint}", entry.name);
        }
    }
    for member in members() {
        if let Some(returns) = member.returns {
            assert!(is_type(returns), "{}: {returns}", member.name);
        }
        for overload in member.overloads {
            assert!(
                qualified(overload.decl),
                "{}: {}",
                member.name,
                overload.decl
            );
            assert!(is_type(overload.on), "{}: on {}", member.name, overload.on);
            for param in overload.params {
                if let P::Lambda(types) = param.kind {
                    for ty in types {
                        assert!(
                            ty.is_empty() || *ty == "$endpoint" || is_type(ty),
                            "{}: lambda type {ty}",
                            member.name
                        );
                    }
                }
            }
        }
    }
    let tables = FORMATTERS
        .iter()
        .map(|(n, _)| *n)
        .chain(DEFINITIONS.iter().map(|(n, _)| *n))
        .chain(ATTRIBUTES.iter().map(|(n, _, _)| *n))
        .chain(OPTIONS.iter().map(|(n, _)| *n))
        .chain(TRANSPORTS.iter().map(|(n, _)| *n))
        .chain([URI_TYPE, DEFAULT_ENDPOINT]);
    for name in tables {
        assert!(is_type(name), "{name}");
    }
    for (sdk, namespaces) in IMPLICIT_USINGS {
        assert!(sdk.starts_with("Microsoft.NET.Sdk"), "{sdk}");
        for namespace in *namespaces {
            assert!(
                *namespace == "System"
                    || namespace.starts_with("System.")
                    || namespace.starts_with("Microsoft."),
                "{sdk}: {namespace}"
            );
        }
    }
}

#[test]
fn transport_facts_ac_1_every_role_is_known() {
    let roles: BTreeSet<&str> = ROLE_WORDS.iter().copied().collect();
    assert_eq!(roles.len(), ROLE_WORDS.len(), "the role words are distinct");
    for entry in TYPES {
        assert!(roles.contains(entry.role.word()), "{}", entry.name);
    }
    for member in members() {
        assert!(roles.contains(member.role.word()), "{}", member.name);
        for overload in member.overloads {
            for ty in overload.types {
                assert!(
                    *ty == Ty::Unused || TYPE_ROLE_WORDS.contains(&ty.word()),
                    "{}: {ty:?}",
                    member.name
                );
            }
        }
    }
    for (_, role) in DEFINITIONS {
        assert!(TYPE_ROLE_WORDS.contains(&role.word()));
    }
}

#[test]
fn transport_facts_ac_1_receivers_derive_through_the_catalogue_bases() {
    assert!(derives(
        "MassTransit.IRabbitMqBusFactoryConfigurator",
        "MassTransit.IReceiveConfigurator"
    ));
    assert!(derives(
        "MassTransit.ConsumeContext`1",
        "MassTransit.ISendEndpointProvider"
    ));
    assert!(!derives(
        "MassTransit.IReceiveEndpointConfigurator",
        "MassTransit.IRabbitMqBusFactoryConfigurator"
    ));
    assert_eq!(
        endpoint_of("MassTransit.IAmazonSqsBusFactoryConfigurator"),
        "MassTransit.IAmazonSqsReceiveEndpointConfigurator"
    );
    assert_eq!(
        endpoint_of("MassTransit.IReceiveConfigurator"),
        DEFAULT_ENDPOINT
    );
}
