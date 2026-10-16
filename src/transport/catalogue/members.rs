//! The catalogue members: every overload the walk matches a call against,
//! split from the type tables by the stage of the configuration it reads.
//! The bus roots, transports, hosts and endpoint configuration are here; the
//! registrations, formatters, addresses, options and outbox are in
//! `registrations`.

use super::{o, r, Act, Form, Member, Overload, Param, Role, Ty, Val, P};

pub(super) const RECEIVE_LAMBDA: P = P::Lambda(&["$endpoint"]);
pub(super) const CONTEXT_THEN_RABBIT: P = P::Lambda(&[
    "MassTransit.IBusRegistrationContext",
    "MassTransit.IRabbitMqBusFactoryConfigurator",
]);
pub(super) const CONTEXT_THEN_SQS: P = P::Lambda(&[
    "MassTransit.IBusRegistrationContext",
    "MassTransit.IAmazonSqsBusFactoryConfigurator",
]);
pub(super) const CONTEXT_THEN_MEMORY: P = P::Lambda(&[
    "MassTransit.IBusRegistrationContext",
    "MassTransit.IInMemoryBusFactoryConfigurator",
]);
pub(super) const REGISTRATION_LAMBDA: P = P::Lambda(&["MassTransit.IBusRegistrationConfigurator"]);
pub(super) const TYPED_REGISTRATION_LAMBDA: P =
    P::Lambda(&["MassTransit.IBusRegistrationConfigurator`1"]);
pub(super) const UNTYPED: P = P::Lambda(&[]);

pub(super) const IRC: &str = "MassTransit.IRegistrationConfigurator";
pub(super) const RCE: &str = "MassTransit.RegistrationConfiguratorExtensions";
pub(super) const RE: &str = "MassTransit.RegistrationExtensions";
pub(super) const ICTX: &str = "MassTransit.RegistrationContextExtensions";
pub(super) const ISC: &str = "Microsoft.Extensions.DependencyInjection.IServiceCollection";
pub(super) const IREC: &str = "MassTransit.IReceiveEndpointConfigurator";

pub(super) const fn inst(
    decl: &'static str,
    types: &'static [Ty],
    params: &'static [Param],
) -> Overload {
    Overload {
        decl,
        on: decl,
        extension: false,
        types,
        params,
    }
}

pub(super) const fn ext(
    decl: &'static str,
    on: &'static str,
    types: &'static [Ty],
    params: &'static [Param],
) -> Overload {
    Overload {
        decl,
        on,
        extension: true,
        types,
        params,
    }
}

pub(super) const fn call(
    name: &'static str,
    role: Role,
    act: Act,
    overloads: &'static [Overload],
) -> Member {
    Member {
        name,
        form: Form::Call,
        role,
        act,
        start: false,
        word: None,
        returns: None,
        overloads,
    }
}

pub(super) const fn worded(member: Member, word: &'static str) -> Member {
    Member {
        word: Some(word),
        ..member
    }
}

pub(super) const fn started(member: Member) -> Member {
    Member {
        start: true,
        ..member
    }
}

pub(super) const fn returning(member: Member, returns: &'static str) -> Member {
    Member {
        returns: Some(returns),
        ..member
    }
}

pub(super) const fn assign(name: &'static str, overloads: &'static [Overload]) -> Member {
    Member {
        name,
        form: Form::Assign,
        role: Role::EndpointConfiguration,
        act: Act::EndpointName,
        start: false,
        word: None,
        returns: None,
        overloads,
    }
}

/// The bus roots, transports, hosts, receive endpoints and the types
/// configured on them.
pub(crate) static BUS_MEMBERS: &[Member] = &[
    started(call(
        "AddMassTransit",
        Role::BusRoot,
        Act::Bus,
        &[
            ext(
                "MassTransit.DependencyInjectionRegistrationExtensions",
                ISC,
                &[],
                &[o(REGISTRATION_LAMBDA)],
            ),
            ext(
                "MassTransit.DependencyInjectionRegistrationExtensions",
                ISC,
                &[Ty::Bus],
                &[r(TYPED_REGISTRATION_LAMBDA)],
            ),
            ext(
                "MassTransit.DependencyInjectionRegistrationExtensions",
                ISC,
                &[Ty::Bus, Ty::Unused],
                &[r(TYPED_REGISTRATION_LAMBDA)],
            ),
        ],
    )),
    worded(
        started(call(
            "AddMassTransitTestHarness",
            Role::TestTransport,
            Act::Bus,
            &[
                ext(
                    "MassTransit.DependencyInjectionTestingExtensions",
                    ISC,
                    &[],
                    &[o(REGISTRATION_LAMBDA)],
                ),
                ext(
                    "MassTransit.DependencyInjectionTestingExtensions",
                    ISC,
                    &[],
                    &[r(P::Opaque), o(REGISTRATION_LAMBDA)],
                ),
            ],
        )),
        "test-harness",
    ),
    returning(
        Member {
            form: Form::Property,
            ..call(
                "Factory",
                Role::BusRoot,
                Act::Pass,
                &[inst("MassTransit.Bus", &[], &[])],
            )
        },
        "MassTransit.IBusFactorySelector",
    ),
    worded(
        call(
            "CreateUsingRabbitMq",
            Role::BusRoot,
            Act::Bus,
            &[ext(
                "MassTransit.RabbitMqBusFactoryConfiguratorExtensions",
                "MassTransit.IBusFactorySelector",
                &[],
                &[o(P::Lambda(&[
                    "MassTransit.IRabbitMqBusFactoryConfigurator",
                ]))],
            )],
        ),
        "rabbitmq",
    ),
    worded(
        call(
            "CreateUsingAmazonSqs",
            Role::BusRoot,
            Act::Bus,
            &[ext(
                "MassTransit.AmazonSqsBusFactoryConfiguratorExtensions",
                "MassTransit.IBusFactorySelector",
                &[],
                &[r(P::Lambda(&[
                    "MassTransit.IAmazonSqsBusFactoryConfigurator",
                ]))],
            )],
        ),
        "amazon-sqs",
    ),
    worded(
        call(
            "CreateUsingInMemory",
            Role::BusRoot,
            Act::Bus,
            &[
                ext(
                    "MassTransit.InMemoryConfigurationExtensions",
                    "MassTransit.IBusFactorySelector",
                    &[],
                    &[r(P::Lambda(&[
                        "MassTransit.IInMemoryBusFactoryConfigurator",
                    ]))],
                ),
                ext(
                    "MassTransit.InMemoryConfigurationExtensions",
                    "MassTransit.IBusFactorySelector",
                    &[],
                    &[
                        r(P::Uri(Val::Ignore)),
                        r(P::Lambda(&["MassTransit.IInMemoryBusFactoryConfigurator"])),
                    ],
                ),
            ],
        ),
        "in-memory",
    ),
    worded(
        call(
            "UsingRabbitMq",
            Role::Transport,
            Act::Transport,
            &[ext(
                "MassTransit.RabbitMqBusFactoryConfiguratorExtensions",
                "MassTransit.IBusRegistrationConfigurator",
                &[],
                &[o(CONTEXT_THEN_RABBIT)],
            )],
        ),
        "rabbitmq",
    ),
    worded(
        call(
            "UsingAmazonSqs",
            Role::Transport,
            Act::Transport,
            &[ext(
                "MassTransit.AmazonSqsBusFactoryConfiguratorExtensions",
                "MassTransit.IBusRegistrationConfigurator",
                &[],
                &[o(CONTEXT_THEN_SQS)],
            )],
        ),
        "amazon-sqs",
    ),
    worded(
        call(
            "UsingInMemory",
            Role::Transport,
            Act::Transport,
            &[
                ext(
                    "MassTransit.InMemoryConfigurationExtensions",
                    "MassTransit.IBusRegistrationConfigurator",
                    &[],
                    &[o(CONTEXT_THEN_MEMORY)],
                ),
                ext(
                    "MassTransit.InMemoryConfigurationExtensions",
                    "MassTransit.IBusRegistrationConfigurator",
                    &[],
                    &[r(P::Uri(Val::Ignore)), o(CONTEXT_THEN_MEMORY)],
                ),
            ],
        ),
        "in-memory",
    ),
    call(
        "Host",
        Role::Host,
        Act::Host,
        &[
            ext(
                "MassTransit.RabbitMqHostConfigurationExtensions",
                "MassTransit.IRabbitMqBusFactoryConfigurator",
                &[],
                &[r(P::Uri(Val::HostUri)), o(UNTYPED)],
            ),
            ext(
                "MassTransit.RabbitMqHostConfigurationExtensions",
                "MassTransit.IRabbitMqBusFactoryConfigurator",
                &[],
                &[r(P::Val(Val::HostName)), o(UNTYPED)],
            ),
            ext(
                "MassTransit.RabbitMqHostConfigurationExtensions",
                "MassTransit.IRabbitMqBusFactoryConfigurator",
                &[],
                &[r(P::Uri(Val::HostUri)), r(P::Text), o(UNTYPED)],
            ),
            ext(
                "MassTransit.RabbitMqHostConfigurationExtensions",
                "MassTransit.IRabbitMqBusFactoryConfigurator",
                &[],
                &[
                    r(P::Val(Val::HostName)),
                    r(P::Val(Val::VirtualHost)),
                    o(UNTYPED),
                ],
            ),
            ext(
                "MassTransit.RabbitMqHostConfigurationExtensions",
                "MassTransit.IRabbitMqBusFactoryConfigurator",
                &[],
                &[
                    r(P::Val(Val::HostName)),
                    r(P::Val(Val::VirtualHost)),
                    r(P::Text),
                    o(UNTYPED),
                ],
            ),
            ext(
                "MassTransit.RabbitMqHostConfigurationExtensions",
                "MassTransit.IRabbitMqBusFactoryConfigurator",
                &[],
                &[
                    r(P::Val(Val::HostName)),
                    r(P::Opaque),
                    r(P::Val(Val::VirtualHost)),
                    r(UNTYPED),
                ],
            ),
            ext(
                "MassTransit.RabbitMqHostConfigurationExtensions",
                "MassTransit.IRabbitMqBusFactoryConfigurator",
                &[],
                &[
                    r(P::Val(Val::HostName)),
                    r(P::Opaque),
                    r(P::Val(Val::VirtualHost)),
                    r(P::Text),
                    o(UNTYPED),
                ],
            ),
            ext(
                "MassTransit.AmazonSqsHostConfigurationExtensions",
                "MassTransit.IAmazonSqsBusFactoryConfigurator",
                &[],
                &[r(P::Uri(Val::HostUri)), r(UNTYPED)],
            ),
            ext(
                "MassTransit.AmazonSqsHostConfigurationExtensions",
                "MassTransit.IAmazonSqsBusFactoryConfigurator",
                &[],
                &[r(P::Val(Val::Region)), r(UNTYPED)],
            ),
        ],
    ),
    call(
        "ReceiveEndpoint",
        Role::ReceiveEndpoint,
        Act::Receive,
        &[
            inst(
                "MassTransit.IReceiveConfigurator",
                &[],
                &[r(P::Val(Val::Name)), r(RECEIVE_LAMBDA)],
            ),
            inst(
                "MassTransit.IReceiveConfigurator",
                &[],
                &[r(P::Opaque), o(P::Formatter), o(RECEIVE_LAMBDA)],
            ),
        ],
    ),
    call(
        "ConfigureConsumer",
        Role::EndpointConfiguration,
        Act::Configured,
        &[
            ext(ICTX, IREC, &[Ty::Consumer], &[r(P::Opaque), o(UNTYPED)]),
            ext(ICTX, IREC, &[], &[r(P::Opaque), r(P::TypeOf(Ty::Consumer))]),
        ],
    ),
    call(
        "ConfigureSaga",
        Role::EndpointConfiguration,
        Act::Configured,
        &[
            ext(ICTX, IREC, &[Ty::Saga], &[r(P::Opaque), o(UNTYPED)]),
            ext(ICTX, IREC, &[], &[r(P::Opaque), r(P::TypeOf(Ty::Saga))]),
        ],
    ),
    call(
        "Consumer",
        Role::EndpointConfiguration,
        Act::Configured,
        &[
            ext(
                "MassTransit.DependencyInjectionReceiveEndpointExtensions",
                IREC,
                &[Ty::Consumer],
                &[r(P::Opaque), o(UNTYPED)],
            ),
            ext(
                "MassTransit.ConsumerExtensions",
                IREC,
                &[Ty::Consumer],
                &[o(UNTYPED)],
            ),
        ],
    ),
    call(
        "Handler",
        Role::EndpointConfiguration,
        Act::Configured,
        &[ext(
            "MassTransit.HandlerExtensions",
            IREC,
            &[Ty::Handler],
            &[r(UNTYPED), o(UNTYPED)],
        )],
    ),
];
