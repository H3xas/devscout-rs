//! The registration, formatter, address, entity name, options, outbox and
//! definition members of the catalogue.

use super::members::{
    assign, call, ext, inst, returning, started, worded, ICTX, IRC, IREC, ISC, RCE, RE, UNTYPED,
};
use super::{o, r, Act, Member, Role, Ty, Val, P};

/// The registrations, formatters, addresses, entity names, options
/// bindings, outbox registrations and definitions.
pub(crate) static CONFIG_MEMBERS: &[Member] = &[
    worded(
        call(
            "AddConsumer",
            Role::Registration,
            Act::Registration,
            &[
                inst(IRC, &[Ty::Consumer], &[o(UNTYPED)]),
                inst(
                    IRC,
                    &[Ty::Consumer],
                    &[r(P::TypeOf(Ty::Definition)), o(UNTYPED)],
                ),
                ext(RE, IRC, &[Ty::Consumer, Ty::Definition], &[o(UNTYPED)]),
                ext(
                    RCE,
                    IRC,
                    &[],
                    &[r(P::TypeOf(Ty::Consumer)), o(P::TypeOf(Ty::Definition))],
                ),
            ],
        ),
        "consumer",
    ),
    worded(
        call(
            "AddSaga",
            Role::Registration,
            Act::Registration,
            &[
                inst(IRC, &[Ty::Saga], &[o(UNTYPED)]),
                inst(
                    IRC,
                    &[Ty::Saga],
                    &[r(P::TypeOf(Ty::Definition)), o(UNTYPED)],
                ),
                ext(RE, IRC, &[Ty::Saga, Ty::Definition], &[o(UNTYPED)]),
                ext(
                    RCE,
                    IRC,
                    &[],
                    &[r(P::TypeOf(Ty::Saga)), o(P::TypeOf(Ty::Definition))],
                ),
            ],
        ),
        "saga",
    ),
    worded(
        call(
            "AddSagaStateMachine",
            Role::Registration,
            Act::Registration,
            &[
                inst(IRC, &[Ty::StateMachine, Ty::State], &[o(UNTYPED)]),
                inst(
                    IRC,
                    &[Ty::StateMachine, Ty::State],
                    &[r(P::TypeOf(Ty::Definition)), o(UNTYPED)],
                ),
                ext(
                    RE,
                    IRC,
                    &[Ty::StateMachine, Ty::State, Ty::Definition],
                    &[o(UNTYPED)],
                ),
                ext(
                    RCE,
                    IRC,
                    &[],
                    &[
                        r(P::TypeOf(Ty::StateMachine)),
                        o(P::TypeOf(Ty::Definition)),
                    ],
                ),
            ],
        ),
        "state-machine",
    ),
    worded(
        call(
            "AddConsumersFromNamespaceContaining",
            Role::Registration,
            Act::Registration,
            &[
                ext(RE, IRC, &[Ty::Anchor], &[o(UNTYPED)]),
                ext(RE, IRC, &[], &[r(P::TypeOf(Ty::Anchor)), o(UNTYPED)]),
            ],
        ),
        "consumer-namespace",
    ),
    worded(
        call(
            "AddSagasFromNamespaceContaining",
            Role::Registration,
            Act::Registration,
            &[
                ext(RE, IRC, &[Ty::Anchor], &[o(UNTYPED)]),
                ext(RE, IRC, &[], &[r(P::TypeOf(Ty::Anchor)), o(UNTYPED)]),
            ],
        ),
        "saga-namespace",
    ),
    worded(
        call(
            "AddSagaStateMachinesFromNamespaceContaining",
            Role::Registration,
            Act::Registration,
            &[
                ext(RE, IRC, &[Ty::Anchor], &[o(UNTYPED)]),
                ext(RE, IRC, &[], &[r(P::TypeOf(Ty::Anchor)), o(UNTYPED)]),
            ],
        ),
        "state-machine-namespace",
    ),
    worded(
        call(
            "AddConsumers",
            Role::Registration,
            Act::Registration,
            &[ext(RE, IRC, &[], &[r(P::Rest)])],
        ),
        "assembly-scan",
    ),
    worded(
        call(
            "AddSagas",
            Role::Registration,
            Act::Registration,
            &[ext(RE, IRC, &[], &[r(P::Rest)])],
        ),
        "assembly-scan",
    ),
    worded(
        call(
            "AddSagaStateMachines",
            Role::Registration,
            Act::Registration,
            &[ext(RE, IRC, &[], &[r(P::Rest)])],
        ),
        "assembly-scan",
    ),
    worded(
        call(
            "SetKebabCaseEndpointNameFormatter",
            Role::NameFormatter,
            Act::Formatter,
            &[ext(RE, IRC, &[], &[])],
        ),
        "kebab-case",
    ),
    worded(
        call(
            "SetSnakeCaseEndpointNameFormatter",
            Role::NameFormatter,
            Act::Formatter,
            &[ext(RE, IRC, &[], &[])],
        ),
        "snake-case",
    ),
    worded(
        call(
            "SetDefaultEndpointNameFormatter",
            Role::NameFormatter,
            Act::Formatter,
            &[ext(RE, IRC, &[], &[])],
        ),
        "default",
    ),
    call(
        "SetEndpointNameFormatter",
        Role::NameFormatter,
        Act::Formatter,
        &[inst(IRC, &[], &[r(P::Formatter)])],
    ),
    call(
        "ConfigureEndpoints",
        Role::EndpointConfiguration,
        Act::ConfigureEndpoints,
        &[
            ext(
                ICTX,
                "MassTransit.IBusFactoryConfigurator`1",
                &[],
                &[r(P::Opaque), o(P::Formatter)],
            ),
            ext(
                ICTX,
                "MassTransit.IBusFactoryConfigurator`1",
                &[],
                &[r(P::Opaque), r(P::Filter), o(P::Formatter)],
            ),
            inst(
                "MassTransit.IBusRegistrationContext",
                &[],
                &[r(P::Opaque), r(P::Formatter)],
            ),
            inst(
                "MassTransit.IBusRegistrationContext",
                &[],
                &[r(P::Opaque), r(P::Formatter), r(P::Filter)],
            ),
        ],
    ),
    call(
        "GetSendEndpoint",
        Role::SendAddress,
        Act::Send,
        &[inst(
            "MassTransit.ISendEndpointProvider",
            &[],
            &[r(P::Uri(Val::Address))],
        )],
    ),
    call(
        "AddRequestClient",
        Role::RequestClient,
        Act::Request,
        &[
            inst(IRC, &[Ty::Message], &[o(P::Opaque)]),
            inst(
                IRC,
                &[Ty::Message],
                &[r(P::Uri(Val::Address)), o(P::Opaque)],
            ),
            inst(IRC, &[], &[r(P::TypeOf(Ty::Message)), o(P::Opaque)]),
            inst(
                IRC,
                &[],
                &[
                    r(P::TypeOf(Ty::Message)),
                    r(P::Uri(Val::Address)),
                    o(P::Opaque),
                ],
            ),
        ],
    ),
    call(
        "Map",
        Role::EndpointConvention,
        Act::Convention,
        &[
            inst(
                "MassTransit.EndpointConvention",
                &[Ty::Message],
                &[r(P::Uri(Val::Address))],
            ),
            inst(
                "MassTransit.EndpointConvention",
                &[Ty::Message],
                &[r(UNTYPED)],
            ),
        ],
    ),
    call(
        "Message",
        Role::EntityName,
        Act::Topology,
        &[inst(
            "MassTransit.IBusFactoryConfigurator",
            &[Ty::Message],
            &[r(P::Lambda(&[
                "MassTransit.Configuration.IMessageTopologyConfigurator`1",
            ]))],
        )],
    ),
    call(
        "SetEntityName",
        Role::EntityName,
        Act::Entity,
        &[inst(
            "MassTransit.Configuration.IMessageTopologyConfigurator`1",
            &[],
            &[r(P::Val(Val::EntityName))],
        )],
    ),
    started(returning(
        call(
            "AddOptions",
            Role::OptionsBinding,
            Act::Options,
            &[ext(
                "Microsoft.Extensions.DependencyInjection.OptionsServiceCollectionExtensions",
                ISC,
                &[Ty::Options],
                &[],
            )],
        ),
        "Microsoft.Extensions.Options.OptionsBuilder`1",
    )),
    call(
        "BindConfiguration",
        Role::OptionsBinding,
        Act::OptionsKey,
        &[ext(
            "Microsoft.Extensions.DependencyInjection.OptionsBuilderConfigurationExtensions",
            "Microsoft.Extensions.Options.OptionsBuilder`1",
            &[],
            &[r(P::Val(Val::Key)), o(UNTYPED)],
        )],
    ),
    started(call(
        "Configure",
        Role::OptionsBinding,
        Act::Options,
        &[
            ext(
                "Microsoft.Extensions.DependencyInjection.OptionsConfigurationServiceCollectionExtensions",
                ISC,
                &[Ty::Options],
                &[r(P::Section)],
            ),
            ext(
                "Microsoft.Extensions.DependencyInjection.OptionsServiceCollectionExtensions",
                ISC,
                &[Ty::Options],
                &[r(UNTYPED)],
            ),
        ],
    )),
    call(
        "AddEntityFrameworkOutbox",
        Role::Outbox,
        Act::Outbox,
        &[ext(
            "MassTransit.EntityFrameworkOutboxConfigurationExtensions",
            "MassTransit.IBusRegistrationConfigurator",
            &[Ty::DbContext],
            &[o(P::Lambda(&["MassTransit.IEntityFrameworkOutboxConfigurator"]))],
        )],
    ),
    call(
        "UseBusOutbox",
        Role::Outbox,
        Act::Outbox,
        &[inst(
            "MassTransit.IEntityFrameworkOutboxConfigurator",
            &[],
            &[o(UNTYPED)],
        )],
    ),
    call(
        "UseEntityFrameworkOutbox",
        Role::Outbox,
        Act::Outbox,
        &[ext(
            "MassTransit.EntityFrameworkOutboxConfigurationExtensions",
            IREC,
            &[Ty::DbContext],
            &[r(P::Opaque), o(UNTYPED)],
        )],
    ),
    call(
        "Endpoint",
        Role::EndpointConfiguration,
        Act::Pass,
        &[
            inst(
                "MassTransit.ConsumerDefinition`1",
                &[],
                &[o(P::Lambda(&["MassTransit.IEndpointRegistrationConfigurator"]))],
            ),
            inst(
                "MassTransit.SagaDefinition`1",
                &[],
                &[o(P::Lambda(&["MassTransit.IEndpointRegistrationConfigurator"]))],
            ),
        ],
    ),
    assign(
        "EndpointName",
        &[
            inst(
                "MassTransit.ConsumerDefinition`1",
                &[],
                &[r(P::Val(Val::Name))],
            ),
            inst("MassTransit.SagaDefinition`1", &[], &[r(P::Val(Val::Name))]),
        ],
    ),
    assign(
        "Name",
        &[inst(
            "MassTransit.IEndpointRegistrationConfigurator",
            &[],
            &[r(P::Val(Val::Name))],
        )],
    ),
];
