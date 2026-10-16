//! The provisional transport sites one file records, as the fragment cache
//! stores them.
//!
//! A site is what the syntax walk alone can say: which catalogue member a
//! call names, through which receiver, with which written types and values.
//! Whether the site is kept is decided at graph rebuild, where the usings of
//! the whole project and the repository's own declarations are known.

use serde::{Deserialize, Serialize};

fn is_false(b: &bool) -> bool {
    !*b
}

/// One provisional transport site.
///
/// Field order is significant, and every field after `member` is omitted
/// when empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransportSite {
    /// The 1-based line of the call, assignment, base type or attribute.
    pub line: usize,
    /// The catalogue key: the declaring type and member name, or the type
    /// name of a definition base or an attribute.
    pub member: String,
    /// The index, in the same file's site list, of the site whose lambda
    /// parameter, return value or instance is this site's receiver.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<usize>,
    /// A catalogue type name as written, which must qualify to that
    /// catalogue type: a declared receiver, a static receiver, a base type
    /// or an attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub written: Option<String>,
    /// The declared type of a chain start's receiver, as written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
    /// The enclosing namespace.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub namespace: String,
    /// The enclosing type names, outermost first.
    #[serde(default, rename = "outerTypes", skip_serializing_if = "Vec::is_empty")]
    pub outer_types: Vec<String>,
    /// The written types the site states, with their roles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<SiteType>,
    /// The values at the argument positions the catalogue marks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<SiteValue>,
    /// The endpoint name formatter argument, when the call takes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formatter: Option<SiteFormatter>,
    /// Whether the call states a registration filter.
    #[serde(default, skip_serializing_if = "is_false")]
    pub filtered: bool,
}

/// One written type and its role.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteType {
    /// The role word.
    pub role: String,
    /// The type name as written.
    pub name: String,
}

/// One value at a marked argument position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteValue {
    /// The value kind word.
    pub role: String,
    /// The address wrapper type as written, for a value read out of a
    /// `new Uri(…)` argument.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrapper: Option<String>,
    /// The value form.
    pub value: RawValue,
}

/// A value as the source states it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "kebab-case")]
pub enum RawValue {
    /// A string literal, already split into its parts.
    Literal(Parsed),
    /// String literals and field references joined with `+`, or one field
    /// reference alone.
    Pieces {
        /// The pieces in reading order.
        pieces: Vec<Piece>,
    },
    /// A form the rules do not read.
    Unknown {
        /// The reason word.
        reason: String,
    },
}

/// One piece of a joined value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Piece {
    /// A string literal's text.
    Lit(String),
    /// A field reference path as written.
    Ref(String),
}

/// The parts one value states. A URI never keeps its user information,
/// port, query or fragment.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parsed {
    /// The name, key or entity name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// The URI scheme or `arn`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
    /// A host name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// A virtual host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_host: Option<String>,
    /// An AWS region.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// An AWS account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// An endpoint name formatter argument.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteFormatter {
    /// The formatter type as written, or none for a form the rules do not
    /// read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub written: Option<String>,
    /// The constructor arguments in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<RawValue>,
}
