use serde::{Deserialize, Serialize};

// The fragment-cache shape of a written call argument's static type. Kept
// beside `fragment_types` rather than inside it, which is at the flat
// file-size limit.

/// The argument's known descriptor, or a member type to read using the
/// declaring file's context after all fragments have been indexed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FragArgType {
    /// A type descriptor known before resolution.
    Known(String),
    /// A member whose declared type will be read from the declaration index.
    Member {
        /// The receiver type name.
        owner: String,
        /// The member name.
        member: String,
        #[serde(
            default,
            rename = "receiverArgs",
            skip_serializing_if = "Option::is_none"
        )]
        /// Explicit type arguments on the receiver, when available.
        receiver_args: Option<Vec<String>>,
        /// Whether the member is invoked as a method.
        call: bool,
    },
}
