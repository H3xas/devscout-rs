//! The messaging fact records of a C# fragment: dependency registrations,
//! publish sites and handler registrations.

use serde::{Deserialize, Serialize};

use super::fragment_types::is_zero;

/// One two-type-argument DI service registration a file's invocations
/// record -- see `extract::RegistrationRecord`.
///
/// `service` is the first type argument (the interface), `implementation`
/// the second (the concrete type). Field order (`service`,
/// `implementation`, `namespace`, `line`) is significant; neither type name
/// carries its enclosing-namespace qualification here (that is the
/// resolver's job, against the registration site's OWN using/alias context,
/// read from `namespace`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragRegistration {
    /// The service (first type argument) name.
    pub service: String,
    /// The implementation (second type argument) name.
    pub implementation: String,
    /// The registration call's enclosing namespace.
    pub namespace: String,
    /// The registration call's 1-based line.
    pub line: usize,
}

/// One message-bus publish-site fact a file's invocations record -- see
/// `extract::PublishRecord`. Field order (`verb`, `message`, `namespace`,
/// `line`, `outerTypes`, `enclosingMethod`, `argCount`, `enclosingCall`) is
/// significant; `outerTypes` is omitted when empty, `enclosingMethod` when
/// absent, `argCount` when zero and `enclosingCall` when absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragPublish {
    /// The invoked method name.
    pub verb: String,
    /// The resolved message-type descriptor.
    pub message: String,
    /// The call's enclosing namespace.
    pub namespace: String,
    /// The call's 1-based line.
    pub line: usize,
    /// Enclosing-type stack (see `FragRef::outer_types`).
    #[serde(default, rename = "outerTypes", skip_serializing_if = "Vec::is_empty")]
    pub outer_types: Vec<String>,
    /// The enclosing method's name, set only when this publish hands on its
    /// own caller's message -- see `extract::PublishRecord::enclosing_method`.
    /// Appended LAST, omitted when absent. Joined the schema with the v21
    /// cache bump.
    #[serde(
        default,
        rename = "enclosingMethod",
        skip_serializing_if = "Option::is_none"
    )]
    pub enclosing_method: Option<String>,
    /// The call's own argument count -- see `extract::PublishRecord::arg_count`.
    /// Appended LAST, after `enclosingMethod`, omitted when zero. Stays
    /// within the v22 generation (no accepted or released build has ever
    /// written v22).
    #[serde(default, rename = "argCount", skip_serializing_if = "is_zero")]
    pub arg_count: usize,
    /// The enclosing invocation the call's own lambda sits inside, when it
    /// sits inside one -- see `extract::PublishRecord::enclosing_call`.
    /// Appended LAST, after `argCount`, omitted when absent.
    #[serde(
        default,
        rename = "enclosingCall",
        skip_serializing_if = "Option::is_none"
    )]
    pub enclosing_call: Option<FragEnclosingCall>,
}

/// One publish call's own enclosing-lambda-argument fact -- see
/// `extract::EnclosingCallFact`. Field order (`verb`, `argPosition`,
/// `argCount`) is significant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragEnclosingCall {
    /// The enclosing invocation's own callee bare name.
    pub verb: String,
    /// The 0-based position the lambda occupies among the enclosing
    /// invocation's arguments.
    #[serde(rename = "argPosition")]
    pub arg_position: usize,
    /// The enclosing invocation's own total argument count.
    #[serde(rename = "argCount")]
    pub arg_count: usize,
}

/// One one-type-argument handler registration a file's invocations record
/// -- see `extract::HandlerRegistrationRecord`. Field order (`handler`,
/// `namespace`, `line`) is significant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragHandlerRegistration {
    /// The registered type's descriptor, as written.
    pub handler: String,
    /// The registration call's enclosing namespace.
    pub namespace: String,
    /// The registration call's 1-based line.
    pub line: usize,
}
