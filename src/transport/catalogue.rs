//! The closed catalogue of package types and members that transport
//! recognition reads.
//!
//! This is the only production file that spells a catalogue name. Every
//! entry is written by its fully qualified name, generic types with their
//! arity after a backtick, and every simple name the walker matches on is
//! derived from those names at run time.

/// What a catalogue entry is for, in the closed word list the export
/// documentation names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    BusRoot,
    Transport,
    Host,
    ReceiveEndpoint,
    EndpointConfiguration,
    Registration,
    NameFormatter,
    SendAddress,
    RequestClient,
    EndpointConvention,
    EntityName,
    OptionsBinding,
    Outbox,
    TestTransport,
}

/// Every role word, in declaration order.
pub(crate) const ROLE_WORDS: [&str; 14] = [
    "bus-root",
    "transport",
    "host",
    "receive-endpoint",
    "endpoint-configuration",
    "registration",
    "name-formatter",
    "send-address",
    "request-client",
    "endpoint-convention",
    "entity-name",
    "options-binding",
    "outbox",
    "test-transport",
];

impl Role {
    pub(crate) fn word(self) -> &'static str {
        ROLE_WORDS[self as usize]
    }
}

/// What the assembly stage does with a kept site of this member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    /// A bus fact.
    Bus,
    /// Sets the transport word of its bus.
    Transport,
    /// A host fact, and the scope of its bus.
    Host,
    /// A receive-endpoint fact.
    Receive,
    /// A type configured on the receive endpoint above it.
    Configured,
    /// A registration fact.
    Registration,
    /// Sets the endpoint name formatter of its bus.
    Formatter,
    /// A configure-endpoints fact.
    ConfigureEndpoints,
    /// A send-address fact.
    Send,
    /// A request-address fact.
    Request,
    /// An endpoint-convention fact.
    Convention,
    /// An entity-name fact.
    Entity,
    /// States the message type of the entity names below it.
    Topology,
    /// An options-binding fact.
    Options,
    /// States the configuration key of the options binding above it.
    OptionsKey,
    /// An outbox fact.
    Outbox,
    /// An endpoint-name fact.
    EndpointName,
    /// Only a link to the sites below it.
    Pass,
}

/// The kind of value an argument position or a property carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Val {
    /// A queue or endpoint name.
    Name,
    /// An entity name, which can also be an AWS resource name.
    EntityName,
    /// An address URI.
    Address,
    /// A broker host name, or a host URI written as a string.
    HostName,
    VirtualHost,
    Region,
    /// A host URI.
    HostUri,
    /// A configuration key.
    Key,
    /// An argument of an endpoint name formatter constructor.
    FormatterArg,
    /// A position that states nothing the facts record.
    Ignore,
}

impl Val {
    pub(crate) fn word(self) -> &'static str {
        match self {
            Val::Name => "name",
            Val::EntityName => "entity-name",
            Val::Address => "address",
            Val::HostName => "host",
            Val::VirtualHost => "virtual-host",
            Val::Region => "region",
            Val::HostUri => "host-uri",
            Val::Key => "key",
            Val::FormatterArg => "formatter-argument",
            Val::Ignore => "ignore",
        }
    }
}

/// The role a written type argument plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ty {
    Message,
    Consumer,
    Saga,
    StateMachine,
    State,
    Handler,
    Definition,
    Anchor,
    Options,
    DbContext,
    Bus,
    /// A type argument the facts do not record.
    Unused,
}

/// Every type role word the facts can carry.
pub(crate) const TYPE_ROLE_WORDS: [&str; 11] = [
    "message",
    "consumer",
    "saga",
    "state-machine",
    "state",
    "handler",
    "definition",
    "anchor",
    "options",
    "db-context",
    "bus",
];

impl Ty {
    pub(crate) fn word(self) -> &'static str {
        match self {
            Ty::Message => "message",
            Ty::Consumer => "consumer",
            Ty::Saga => "saga",
            Ty::StateMachine => "state-machine",
            Ty::State => "state",
            Ty::Handler => "handler",
            Ty::Definition => "definition",
            Ty::Anchor => "anchor",
            Ty::Options => "options",
            Ty::DbContext => "db-context",
            Ty::Bus => "bus",
            Ty::Unused => "",
        }
    }
}

/// One parameter kind of an overload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum P {
    /// A string value of the given kind.
    Val(Val),
    /// A `System.Uri` that wraps a value of the given kind.
    Uri(Val),
    /// A delegate whose parameter types are listed. An empty string is a
    /// type outside the catalogue, and `$endpoint` is the endpoint
    /// configurator type of the receiver.
    Lambda(&'static [&'static str]),
    /// A registration filter delegate.
    Filter,
    /// An endpoint name formatter instance.
    Formatter,
    /// A value the facts do not read, other than a string.
    Opaque,
    /// A string the facts do not read.
    Text,
    /// A `typeof` expression that states a type of the given role.
    TypeOf(Ty),
    /// A configuration section whose key is read.
    Section,
    /// A `params` array: every remaining argument.
    Rest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Param {
    pub(crate) kind: P,
    pub(crate) optional: bool,
}

const fn r(kind: P) -> Param {
    Param {
        kind,
        optional: false,
    }
}

const fn o(kind: P) -> Param {
    Param {
        kind,
        optional: true,
    }
}

/// One overload: where it is declared, the type it is called on, and its
/// explicit type arguments and parameters.
#[derive(Debug)]
pub(crate) struct Overload {
    /// The declaring type.
    pub(crate) decl: &'static str,
    /// The receiver type: the declaring type of an instance or static
    /// member, the `this` type of an extension method.
    pub(crate) on: &'static str,
    pub(crate) extension: bool,
    pub(crate) types: &'static [Ty],
    pub(crate) params: &'static [Param],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Form {
    Call,
    Assign,
    Property,
}

/// One member name, with every overload the catalogue reads.
#[derive(Debug)]
pub(crate) struct Member {
    pub(crate) name: &'static str,
    pub(crate) form: Form,
    pub(crate) role: Role,
    pub(crate) act: Act,
    /// A chain start: an extension method on a framework type that any
    /// receiver expression can carry.
    pub(crate) start: bool,
    /// The transport, formatter or registration word the member states.
    pub(crate) word: Option<&'static str>,
    pub(crate) returns: Option<&'static str>,
    pub(crate) overloads: &'static [Overload],
}

/// One catalogue type.
#[derive(Debug)]
pub(crate) struct TypeEntry {
    pub(crate) name: &'static str,
    pub(crate) role: Role,
    /// Direct base types inside the catalogue.
    pub(crate) bases: &'static [&'static str],
    /// The endpoint configurator type of a bus factory configurator.
    pub(crate) endpoint: Option<&'static str>,
}

const fn t(name: &'static str, role: Role, bases: &'static [&'static str]) -> TypeEntry {
    TypeEntry {
        name,
        role,
        bases,
        endpoint: None,
    }
}

const fn bus_cfg(name: &'static str, endpoint: &'static str) -> TypeEntry {
    TypeEntry {
        name,
        role: Role::Transport,
        bases: &["MassTransit.IBusFactoryConfigurator`1"],
        endpoint: Some(endpoint),
    }
}

/// The address wrapper: its string constructor carries the address.
pub(crate) const URI_TYPE: &str = "System.Uri";

/// The endpoint configurator a lambda parameter reads when the receiver
/// states none.
pub(crate) const DEFAULT_ENDPOINT: &str = "MassTransit.IReceiveEndpointConfigurator";

pub(crate) static TYPES: &[TypeEntry] = &[
    t(
        "Microsoft.Extensions.DependencyInjection.IServiceCollection",
        Role::BusRoot,
        &[],
    ),
    t(
        "MassTransit.IRegistrationConfigurator",
        Role::Registration,
        &["Microsoft.Extensions.DependencyInjection.IServiceCollection"],
    ),
    t(
        "MassTransit.IBusRegistrationConfigurator",
        Role::Registration,
        &["MassTransit.IRegistrationConfigurator"],
    ),
    t(
        "MassTransit.IBusRegistrationConfigurator`1",
        Role::Registration,
        &["MassTransit.IBusRegistrationConfigurator"],
    ),
    t("MassTransit.IRegistrationContext", Role::Registration, &[]),
    t(
        "MassTransit.IBusRegistrationContext",
        Role::Registration,
        &["MassTransit.IRegistrationContext"],
    ),
    t("MassTransit.Bus", Role::BusRoot, &[]),
    t("MassTransit.IBusFactorySelector", Role::BusRoot, &[]),
    t(
        "MassTransit.IReceiveConfigurator",
        Role::ReceiveEndpoint,
        &[],
    ),
    t(
        "MassTransit.IReceiveConfigurator`1",
        Role::ReceiveEndpoint,
        &["MassTransit.IReceiveConfigurator"],
    ),
    t(
        "MassTransit.IBusFactoryConfigurator",
        Role::Transport,
        &["MassTransit.IReceiveConfigurator"],
    ),
    t(
        "MassTransit.IBusFactoryConfigurator`1",
        Role::Transport,
        &[
            "MassTransit.IBusFactoryConfigurator",
            "MassTransit.IReceiveConfigurator`1",
        ],
    ),
    bus_cfg(
        "MassTransit.IRabbitMqBusFactoryConfigurator",
        "MassTransit.IRabbitMqReceiveEndpointConfigurator",
    ),
    bus_cfg(
        "MassTransit.IAmazonSqsBusFactoryConfigurator",
        "MassTransit.IAmazonSqsReceiveEndpointConfigurator",
    ),
    bus_cfg(
        "MassTransit.IInMemoryBusFactoryConfigurator",
        "MassTransit.IInMemoryReceiveEndpointConfigurator",
    ),
    t(
        "MassTransit.IReceiveEndpointConfigurator",
        Role::EndpointConfiguration,
        &[],
    ),
    t(
        "MassTransit.IRabbitMqReceiveEndpointConfigurator",
        Role::EndpointConfiguration,
        &["MassTransit.IReceiveEndpointConfigurator"],
    ),
    t(
        "MassTransit.IAmazonSqsReceiveEndpointConfigurator",
        Role::EndpointConfiguration,
        &["MassTransit.IReceiveEndpointConfigurator"],
    ),
    t(
        "MassTransit.IInMemoryReceiveEndpointConfigurator",
        Role::EndpointConfiguration,
        &["MassTransit.IReceiveEndpointConfigurator"],
    ),
    t("MassTransit.ISendEndpointProvider", Role::SendAddress, &[]),
    t(
        "MassTransit.IBus",
        Role::SendAddress,
        &["MassTransit.ISendEndpointProvider"],
    ),
    t(
        "MassTransit.IBusControl",
        Role::SendAddress,
        &["MassTransit.IBus"],
    ),
    t(
        "MassTransit.ConsumeContext",
        Role::SendAddress,
        &["MassTransit.ISendEndpointProvider"],
    ),
    t(
        "MassTransit.ConsumeContext`1",
        Role::SendAddress,
        &["MassTransit.ConsumeContext"],
    ),
    t(
        "MassTransit.EndpointConvention",
        Role::EndpointConvention,
        &[],
    ),
    t(
        "MassTransit.Configuration.IMessageTopologyConfigurator`1",
        Role::EntityName,
        &[],
    ),
    t("MassTransit.EntityNameAttribute", Role::EntityName, &[]),
    t(
        "MassTransit.AmazonSqsTransportOptions",
        Role::OptionsBinding,
        &[],
    ),
    t(
        "MassTransit.RabbitMqTransportOptions",
        Role::OptionsBinding,
        &[],
    ),
    t(
        "Microsoft.Extensions.Options.OptionsBuilder`1",
        Role::OptionsBinding,
        &[],
    ),
    t(
        "MassTransit.IEntityFrameworkOutboxConfigurator",
        Role::Outbox,
        &[],
    ),
    t(
        "MassTransit.ConsumerDefinition`1",
        Role::EndpointConfiguration,
        &[],
    ),
    t(
        "MassTransit.SagaDefinition`1",
        Role::EndpointConfiguration,
        &[],
    ),
    t(
        "MassTransit.IEndpointRegistrationConfigurator",
        Role::EndpointConfiguration,
        &[],
    ),
    t(
        "MassTransit.DefaultEndpointNameFormatter",
        Role::NameFormatter,
        &[],
    ),
    t(
        "MassTransit.SnakeCaseEndpointNameFormatter",
        Role::NameFormatter,
        &["MassTransit.DefaultEndpointNameFormatter"],
    ),
    t(
        "MassTransit.KebabCaseEndpointNameFormatter",
        Role::NameFormatter,
        &["MassTransit.SnakeCaseEndpointNameFormatter"],
    ),
    t("System.Uri", Role::SendAddress, &[]),
];

/// The endpoint name formatter types and the word each one states.
pub(crate) static FORMATTERS: &[(&str, &str)] = &[
    ("MassTransit.KebabCaseEndpointNameFormatter", "kebab-case"),
    ("MassTransit.SnakeCaseEndpointNameFormatter", "snake-case"),
    ("MassTransit.DefaultEndpointNameFormatter", "default"),
];

/// The bus factory configurators and the transport word each one states.
pub(crate) static TRANSPORTS: &[(&str, &str)] = &[
    ("MassTransit.IRabbitMqBusFactoryConfigurator", "rabbitmq"),
    ("MassTransit.IAmazonSqsBusFactoryConfigurator", "amazon-sqs"),
    ("MassTransit.IInMemoryBusFactoryConfigurator", "in-memory"),
];

/// The definition base types and the role of their type argument.
pub(crate) static DEFINITIONS: &[(&str, Ty)] = &[
    ("MassTransit.ConsumerDefinition`1", Ty::Consumer),
    ("MassTransit.SagaDefinition`1", Ty::Saga),
];

/// The attribute types, the value their first argument carries, and the
/// role of the type they decorate.
pub(crate) static ATTRIBUTES: &[(&str, Val, Ty)] = &[(
    "MassTransit.EntityNameAttribute",
    Val::EntityName,
    Ty::Message,
)];

/// The options types and their properties that hold an address or a scope
/// value. No credential property is listed.
pub(crate) static OPTIONS: &[(&str, &[(&str, Val)])] = &[
    (
        "MassTransit.AmazonSqsTransportOptions",
        &[("Region", Val::Region)],
    ),
    (
        "MassTransit.RabbitMqTransportOptions",
        &[("Host", Val::HostName), ("VHost", Val::VirtualHost)],
    ),
];

mod members;
mod registrations;

/// Every catalogue member, bus members first.
pub(crate) fn members() -> impl Iterator<Item = &'static Member> {
    members::BUS_MEMBERS
        .iter()
        .chain(registrations::CONFIG_MEMBERS.iter())
}

/// The namespaces an SDK adds as global usings when `ImplicitUsings` is
/// `enable` or `true`, from the SDK's own targets files.
pub(crate) static IMPLICIT_USINGS: &[(&str, &[&str])] = &[
    ("Microsoft.NET.Sdk", BASE_USINGS),
    ("Microsoft.NET.Sdk.Web", WEB_USINGS),
    ("Microsoft.NET.Sdk.Worker", WORKER_USINGS),
];

const BASE_USINGS: &[&str] = &[
    "System",
    "System.Collections.Generic",
    "System.IO",
    "System.Linq",
    "System.Net.Http",
    "System.Threading",
    "System.Threading.Tasks",
];

const WEB_USINGS: &[&str] = &[
    "System",
    "System.Collections.Generic",
    "System.IO",
    "System.Linq",
    "System.Net.Http",
    "System.Net.Http.Json",
    "System.Threading",
    "System.Threading.Tasks",
    "Microsoft.AspNetCore.Builder",
    "Microsoft.AspNetCore.Hosting",
    "Microsoft.AspNetCore.Http",
    "Microsoft.AspNetCore.Routing",
    "Microsoft.Extensions.Configuration",
    "Microsoft.Extensions.DependencyInjection",
    "Microsoft.Extensions.Hosting",
    "Microsoft.Extensions.Logging",
];

const WORKER_USINGS: &[&str] = &[
    "System",
    "System.Collections.Generic",
    "System.IO",
    "System.Linq",
    "System.Net.Http",
    "System.Threading",
    "System.Threading.Tasks",
    "Microsoft.Extensions.Configuration",
    "Microsoft.Extensions.DependencyInjection",
    "Microsoft.Extensions.Hosting",
    "Microsoft.Extensions.Logging",
];

/// The simple name of a catalogue name: the last segment, without arity.
pub(crate) fn simple(name: &str) -> &str {
    let last = name.rsplit('.').next().unwrap_or(name);
    last.split('`').next().unwrap_or(last)
}

/// The generic arity a catalogue name carries.
pub(crate) fn arity(name: &str) -> usize {
    name.rsplit_once('`')
        .and_then(|(_, n)| n.parse().ok())
        .unwrap_or(0)
}

/// The namespace of a catalogue name.
pub(crate) fn namespace_of(name: &str) -> &str {
    name.rsplit_once('.').map_or("", |(ns, _)| ns)
}

pub(crate) fn type_entry(name: &str) -> Option<&'static TypeEntry> {
    TYPES.iter().find(|t| t.name == name)
}

/// The catalogue types with this simple name and arity.
pub(crate) fn types_named(simple_name: &str, type_arity: usize) -> Vec<&'static TypeEntry> {
    TYPES
        .iter()
        .filter(|t| simple(t.name) == simple_name && arity(t.name) == type_arity)
        .collect()
}

/// Whether `ty` is `target` or derives from it through catalogue bases.
pub(crate) fn derives(ty: &str, target: &str) -> bool {
    if ty == target {
        return true;
    }
    type_entry(ty).is_some_and(|t| t.bases.iter().any(|b| derives(b, target)))
}

/// The endpoint configurator a lambda parameter typed `$endpoint` reads on
/// a receiver of type `ty`.
pub(crate) fn endpoint_of(ty: &str) -> &'static str {
    type_entry(ty)
        .and_then(|t| t.endpoint)
        .unwrap_or(DEFAULT_ENDPOINT)
}

/// Every member with this name and form.
pub(crate) fn members_named(name: &str, form: Form) -> impl Iterator<Item = &'static Member> + '_ {
    members().filter(move |m| m.name == name && m.form == form)
}

/// The member and overload flag a recorded `member` key names.
pub(crate) fn member_by_key(key: &str) -> Option<(&'static Member, &'static Overload)> {
    let (decl, name) = key.rsplit_once('.')?;
    members()
        .filter(|m| m.name == name)
        .find_map(|m| m.overloads.iter().find(|o| o.decl == decl).map(|o| (m, o)))
}

/// Whether any member, at any form, has this simple name.
pub(crate) fn is_member_name(name: &str) -> bool {
    members().any(|m| m.name == name)
}

/// The words a file must contain for any site to be possible: every
/// catalogue type simple name and every bus root member name. The service
/// collection, the options builder and the address wrapper are left out,
/// because no chain starts at them alone.
pub(crate) fn prefilter_words() -> Vec<&'static str> {
    let mut words: Vec<&'static str> = TYPES
        .iter()
        .filter(|t| !UNFILTERED.contains(&t.name))
        .map(|t| simple(t.name))
        .collect();
    words.extend(
        members()
            .filter(|m| m.start && m.act == Act::Bus)
            .map(|m| m.name),
    );
    words.sort_unstable();
    words.dedup();
    words
}

const UNFILTERED: [&str; 3] = [
    members::ISC,
    "Microsoft.Extensions.Options.OptionsBuilder`1",
    URI_TYPE,
];

#[cfg(test)]
#[path = "catalogue_tests.rs"]
mod tests;
