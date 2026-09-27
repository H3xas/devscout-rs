use serde::{Deserialize, Serialize};

// The fragment-cache shapes of the facts that decide which overload a lambda
// argument can convert to: the lambda's own shape on the calling ref, and a
// delegate's signature on its def. Kept beside `fragment_types` rather than
// inside it, which is close to the flat file-size limit.

/// One overload's own-type-parameter sites: per parameter, `[k, position]`
/// for the k-th own type parameter and the top-level type argument it sits
/// in, -1 outside every type argument.
pub type FragOverloadSites = Vec<Vec<(usize, i64)>>;

/// One positional lambda-literal argument (see extract.rs's `LambdaShape`).
/// Field order (`valueOnly`, `implicit`) is significant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FragLambdaShape {
    /// The body can only produce a value.
    #[serde(rename = "valueOnly")]
    pub value_only: bool,
    /// No lambda parameter carries a written type.
    #[serde(rename = "implicit")]
    pub implicitly_typed: bool,
}

/// A `delegate` def's return descriptor and where each of its own type
/// parameters occurs (see extract.rs's `DelegateSignature`). Field order
/// (`returns`, `typeParams`) is significant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FragDelegateSignature {
    /// The `Invoke` return descriptor, `void` written out.
    pub returns: String,
    /// Per own type parameter in declaration order: `params`, `return`,
    /// `both` or `none`.
    #[serde(rename = "typeParams")]
    pub type_params: Vec<String>,
}
