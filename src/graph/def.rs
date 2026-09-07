use serde::{Deserialize, Serialize};

use super::fragment_types::is_zero;

// ---------------------------------------------------------------------------
// graph.json schema.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Represents `AlsoIn`.
pub struct AlsoIn {
    /// The file value.
    pub file: String,
    /// The line value.
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Represents `Def`.
pub struct Def {
    /// The id value.
    pub id: String,
    /// The name value.
    pub name: String,
    /// The namespace value.
    pub namespace: String,
    /// The kind value.
    pub kind: String,
    /// The file value.
    pub file: String,
    /// The line value.
    pub line: usize,
    /// The methods value.
    pub methods: Vec<String>,
    /// Test-coverage stage -- the one member fact that SURVIVES onto disk
    /// (`properties`/`fields`/`extensionMethods`/`bases` are resolution inputs
    /// only, stripped before the row is written): `devscout tests` reads it back
    /// out of graph.json. Positioned after `methods` and before `also_in`, and
    /// omitted when empty, so a def declaring no tests keeps its exact
    /// pre-stage bytes.
    #[serde(default, rename = "testMethods", skip_serializing_if = "Vec::is_empty")]
    pub test_methods: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// The also in value.
    pub also_in: Vec<AlsoIn>,
    #[serde(default, rename = "endLine", skip_serializing_if = "is_zero")]
    /// The end line value.
    pub end_line: usize,
}
