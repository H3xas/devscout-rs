//! The transport facts `graph.json` records and the export publishes.
//!
//! Field declaration order is the serialized key order, which the export
//! line keeps after its leading `family` and `evidence` keys. Every key a
//! kind does not state is omitted.

use serde::{Deserialize, Serialize};

fn is_false(b: &bool) -> bool {
    !*b
}

/// One kept transport fact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportFact {
    /// The repository path of the site.
    pub file: String,
    /// The 1-based line of the site.
    pub line: usize,
    /// The fact kind word.
    pub kind: String,
    /// The fully qualified catalogue member or attribute type.
    pub member: String,
    /// The file of the bus fact this fact sits in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bus_file: Option<String>,
    /// The line of the bus fact this fact sits in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bus_line: Option<usize>,
    /// The transport word of a bus fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    /// The registration word of a registration fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registration: Option<String>,
    /// The types the fact states.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<FactType>,
    /// The endpoint name formatter in force.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formatter: Option<String>,
    /// Where the formatter in force comes from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formatter_source: Option<String>,
    /// The formatter's stated constructor arguments, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub formatter_arguments: Vec<ValueObj>,
    /// Whether a registration filter is stated.
    #[serde(default, skip_serializing_if = "is_false")]
    pub filtered: bool,
    /// The address or name the fact states.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<ValueObj>,
    /// The configuration key an options binding binds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<ValueObj>,
    /// The marked options properties of an options binding.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<FactProperty>,
    /// The transport scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<TransportScope>,
}

/// One type a fact states.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactType {
    /// The type role word.
    pub role: String,
    /// The type name as written.
    #[serde(rename = "type")]
    pub name: String,
    /// The one repository definition the name qualifies to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    /// The file of that definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_file: Option<String>,
    /// The line of the configuring call, for a type configured on a
    /// receive endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

/// One marked property of an options binding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactProperty {
    /// The property name.
    pub name: String,
    /// The scope part word the property holds.
    pub role: String,
    /// The property's value.
    pub value: ValueObj,
}

/// The broker scope of a bus or an address.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportScope {
    /// The broker host.
    pub host: ValueObj,
    /// The broker virtual host.
    pub virtual_host: ValueObj,
    /// The AWS region.
    pub region: ValueObj,
    /// The AWS account.
    pub account: ValueObj,
}

/// A value with its evidence class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "class", rename_all = "kebab-case")]
pub enum ValueObj {
    /// A string literal.
    Literal {
        /// The value.
        value: String,
        /// The URI scheme, or `arn`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scheme: Option<String>,
    },
    /// Repository `const string` fields, with literals around them.
    Constant {
        /// The value.
        value: String,
        /// The URI scheme, or `arn`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scheme: Option<String>,
        /// The fields read, in reading order.
        fields: Vec<FieldRef>,
    },
    /// A configuration key and the values the project's settings files
    /// state for it.
    Configuration {
        /// The key path, `:` between parts.
        key: String,
        /// The stated values, sorted by file.
        values: Vec<ConfigValue>,
    },
    /// A form the rules do not read.
    Unknown {
        /// The reason word.
        reason: String,
        /// The configuration key the source states, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
    },
    /// A scope part the source does not state.
    NotStated,
}

/// One constant field a constant value reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldRef {
    /// The field's declaring type id and name.
    pub field: String,
    /// The file that declares it.
    pub file: String,
    /// The line of its declarator.
    pub line: usize,
}

/// One value a settings file states.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigValue {
    /// The settings file.
    pub file: String,
    /// The value.
    pub value: String,
}
