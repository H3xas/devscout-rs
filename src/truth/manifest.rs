//! The case-manifest schema (`fixtures/csharp-truth/manifest.json`,
//! `contract: "semantic-truth-v1"`).
//!
//! Per case: id, language, exact compatibility profiles, source inputs,
//! expected context health, the fact-contract version, expected
//! present/absent/unresolved facts, expected diagnostics, and precise
//! occurrence sites.
//!
//! [`validate_case`] is the schema gate: it rejects a case object missing
//! any required field, naming the field, before any typed parse is
//! attempted -- a case is never silently accepted with a hole in it. It
//! also refuses a case whose expectation provenance names the producer
//! that case grades, so production output can never be used to create its
//! own truth.

use serde::Deserialize;
use serde_json::Value;

use super::identity::{OccurrenceSpan, SymbolIdentity};

/// The producer this harness grades. No case's expectation provenance may
/// name this tool -- that would be the producer authoring the truth it is
/// scored against.
pub const GRADED_PRODUCER: &str = "scout-semantic";

/// Where a case's expectation came from: a human review (the default) or a
/// named, separately selected compiler path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provenance {
    /// A human reviewed the expectation directly.
    Reviewed,
    /// A named compiler path, distinct from the graded producer, generated
    /// the expectation.
    SeparatelySelectedCompiler {
        /// The compiler tool and version that generated this expectation.
        tool: String,
    },
}

/// One fact a case expects to be present.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpectedPresentFact {
    /// The expected symbol identity.
    pub identity: SymbolIdentity,
    /// The expected occurrence span.
    pub occurrence: OccurrenceSpan,
    /// The expected confidence-state label.
    pub state: String,
}

/// One fact a case expects to never be asserted.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpectedAbsentFact {
    /// The identity that must never be asserted.
    pub identity: SymbolIdentity,
}

/// One fact a case expects to remain unresolved rather than be promoted to
/// confirmed.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpectedUnresolvedFact {
    /// The identity this obligation concerns.
    pub identity: SymbolIdentity,
    /// Where the fact occurs.
    pub occurrence: OccurrenceSpan,
    /// The expected non-confirmed state label.
    pub state: String,
    /// Why this fact cannot be confirmed.
    pub reason: String,
}

/// One diagnostic a case expects the analyzer to emit.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpectedDiagnostic {
    /// The expected diagnostic severity.
    pub severity: String,
    /// The expected diagnostic code.
    pub code: String,
    /// Where the diagnostic is expected.
    pub occurrence: OccurrenceSpan,
}

/// One manifest case: everything a reviewed case must declare so no case
/// is silently skipped -- id, scope, profiles, prerequisites, source,
/// expected context, and the four expectation lists.
#[derive(Debug, Clone)]
pub struct Case {
    /// The case's stable id.
    pub id: String,
    /// Which of the four scenario families this case belongs to.
    pub scenario_family: String,
    /// The case's source language.
    pub language: String,
    /// The exact compatibility profiles this case applies to.
    pub profiles: Vec<String>,
    /// Feature prerequisites that must hold before this case runs.
    pub prerequisites: Vec<String>,
    /// The fact-contract version this case's expectations were authored
    /// against.
    pub fact_contract: String,
    /// The case's source input files.
    pub source: Vec<String>,
    /// The expected context-health label.
    pub context: String,
    /// Facts this case expects to be present.
    pub present: Vec<ExpectedPresentFact>,
    /// Facts this case expects to never be asserted.
    pub absent: Vec<ExpectedAbsentFact>,
    /// Facts this case expects to remain unresolved.
    pub unresolved: Vec<ExpectedUnresolvedFact>,
    /// Diagnostics this case expects the analyzer to emit.
    pub diagnostics: Vec<ExpectedDiagnostic>,
    /// Where this case's expectation came from.
    pub provenance: Provenance,
}

/// A parsed, validated case manifest.
#[derive(Debug, Clone)]
pub struct CaseManifest {
    /// The manifest's schema version.
    pub schema_version: u32,
    /// The manifest's fact-contract version.
    pub contract: String,
    /// Every case the manifest declares.
    pub cases: Vec<Case>,
}

/// A schema or provenance violation, always naming the offending case and
/// field so a rejection is actionable rather than a bare parse failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// The manifest root, or a case, is not a JSON object.
    NotAnObject,
    /// A field that must be a JSON array was not.
    NotAnArray {
        /// The offending field's path.
        field: String,
    },
    /// A required field was absent.
    MissingField {
        /// Which case is missing the field.
        case_id: String,
        /// The missing field's path.
        field: String,
    },
    /// A case's expectation provenance names the producer that case
    /// grades.
    ProducerGradesOwnExpectation {
        /// The offending case.
        case_id: String,
    },
    /// A field present but not shaped as this schema requires.
    Malformed {
        /// Which case is malformed.
        case_id: String,
        /// What is wrong with it.
        detail: String,
    },
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestError::NotAnObject => write!(f, "manifest root is not a JSON object"),
            ManifestError::NotAnArray { field } => write!(f, "'{field}' must be a JSON array"),
            ManifestError::MissingField { case_id, field } => {
                write!(f, "case '{case_id}': missing required field '{field}'")
            }
            ManifestError::ProducerGradesOwnExpectation { case_id } => write!(
                f,
                "case '{case_id}': expectation provenance names '{GRADED_PRODUCER}', the producer this case grades"
            ),
            ManifestError::Malformed { case_id, detail } => {
                write!(f, "case '{case_id}': {detail}")
            }
        }
    }
}

const REQUIRED_CASE_FIELDS: &[&str] = &[
    "id",
    "scenarioFamily",
    "language",
    "profiles",
    "prerequisites",
    "factContract",
    "source",
    "expect",
    "provenance",
];

const REQUIRED_EXPECT_FIELDS: &[&str] =
    &["context", "present", "absent", "unresolved", "diagnostics"];

fn case_label(case: &Value) -> String {
    case.get("id")
        .and_then(Value::as_str)
        .unwrap_or("<no id>")
        .to_string()
}

/// Rejects a case object missing any required field, or whose expectation
/// provenance names the producer that case grades.
///
/// Runs before any typed parse, so a hole in a case is never accepted
/// merely because the fields it does carry happen to parse.
pub fn validate_case(case: &Value) -> Result<(), ManifestError> {
    let case_id = case_label(case);
    let obj = case.as_object().ok_or(ManifestError::NotAnObject)?;
    for field in REQUIRED_CASE_FIELDS {
        if !obj.contains_key(*field) {
            return Err(ManifestError::MissingField {
                case_id: case_id.clone(),
                field: (*field).to_string(),
            });
        }
    }
    let expect = case
        .get("expect")
        .and_then(Value::as_object)
        .ok_or_else(|| ManifestError::MissingField {
            case_id: case_id.clone(),
            field: "expect".to_string(),
        })?;
    for field in REQUIRED_EXPECT_FIELDS {
        if !expect.contains_key(*field) {
            return Err(ManifestError::MissingField {
                case_id: case_id.clone(),
                field: format!("expect.{field}"),
            });
        }
    }
    let provenance = case
        .get("provenance")
        .and_then(Value::as_object)
        .ok_or_else(|| ManifestError::MissingField {
            case_id: case_id.clone(),
            field: "provenance".to_string(),
        })?;
    let kind = provenance
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| ManifestError::MissingField {
            case_id: case_id.clone(),
            field: "provenance.kind".to_string(),
        })?;
    if kind == "separately-selected-compiler" {
        let tool = provenance
            .get("tool")
            .and_then(Value::as_str)
            .ok_or_else(|| ManifestError::MissingField {
                case_id: case_id.clone(),
                field: "provenance.tool".to_string(),
            })?;
        let tool_name = tool.split('@').next().unwrap_or(tool);
        if tool_name.eq_ignore_ascii_case(GRADED_PRODUCER) {
            return Err(ManifestError::ProducerGradesOwnExpectation { case_id });
        }
    } else if kind != "reviewed" {
        return Err(ManifestError::Malformed {
            case_id,
            detail: format!("unknown provenance.kind '{kind}'"),
        });
    }
    Ok(())
}

fn parse_provenance(case_id: &str, value: &Value) -> Result<Provenance, ManifestError> {
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match kind {
        "reviewed" => Ok(Provenance::Reviewed),
        "separately-selected-compiler" => {
            let tool = value
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Ok(Provenance::SeparatelySelectedCompiler { tool })
        }
        other => Err(ManifestError::Malformed {
            case_id: case_id.to_string(),
            detail: format!("unknown provenance.kind '{other}'"),
        }),
    }
}

fn parse_case(value: &Value) -> Result<Case, ManifestError> {
    validate_case(value)?;
    let case_id = case_label(value);
    let malformed = |detail: String| ManifestError::Malformed {
        case_id: case_id.clone(),
        detail,
    };

    let strings = |field: &str| -> Result<Vec<String>, ManifestError> {
        value
            .get(field)
            .and_then(Value::as_array)
            .ok_or_else(|| malformed(format!("'{field}' must be an array")))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| malformed(format!("'{field}' must contain only strings")))
            })
            .collect()
    };

    let expect = value.get("expect").expect("checked by validate_case");
    let present: Vec<ExpectedPresentFact> = serde_json::from_value(expect["present"].clone())
        .map_err(|e| malformed(format!("expect.present: {e}")))?;
    let absent: Vec<ExpectedAbsentFact> = serde_json::from_value(expect["absent"].clone())
        .map_err(|e| malformed(format!("expect.absent: {e}")))?;
    let unresolved: Vec<ExpectedUnresolvedFact> =
        serde_json::from_value(expect["unresolved"].clone())
            .map_err(|e| malformed(format!("expect.unresolved: {e}")))?;
    let diagnostics: Vec<ExpectedDiagnostic> =
        serde_json::from_value(expect["diagnostics"].clone())
            .map_err(|e| malformed(format!("expect.diagnostics: {e}")))?;

    Ok(Case {
        id: case_id.clone(),
        scenario_family: value["scenarioFamily"]
            .as_str()
            .ok_or_else(|| malformed("'scenarioFamily' must be a string".to_string()))?
            .to_string(),
        language: value["language"]
            .as_str()
            .ok_or_else(|| malformed("'language' must be a string".to_string()))?
            .to_string(),
        profiles: strings("profiles")?,
        prerequisites: strings("prerequisites")?,
        fact_contract: value["factContract"]
            .as_str()
            .ok_or_else(|| malformed("'factContract' must be a string".to_string()))?
            .to_string(),
        source: strings("source")?,
        context: expect["context"]
            .as_str()
            .ok_or_else(|| malformed("expect.context must be a string".to_string()))?
            .to_string(),
        present,
        absent,
        unresolved,
        diagnostics,
        provenance: parse_provenance(&case_id, &value["provenance"])?,
    })
}

/// Parses a manifest document, validating every case before any typed
/// conversion.
///
/// The first invalid case aborts the parse -- a manifest is accepted
/// whole or not at all, never with a silently skipped case.
pub fn parse_manifest(text: &str) -> Result<CaseManifest, ManifestError> {
    let root: Value = serde_json::from_str(text).map_err(|e| ManifestError::Malformed {
        case_id: "<root>".to_string(),
        detail: e.to_string(),
    })?;
    let obj = root.as_object().ok_or(ManifestError::NotAnObject)?;
    let schema_version = obj
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .ok_or_else(|| ManifestError::MissingField {
            case_id: "<root>".to_string(),
            field: "schemaVersion".to_string(),
        })? as u32;
    let contract = obj
        .get("contract")
        .and_then(Value::as_str)
        .ok_or_else(|| ManifestError::MissingField {
            case_id: "<root>".to_string(),
            field: "contract".to_string(),
        })?
        .to_string();
    let raw_cases =
        obj.get("cases")
            .and_then(Value::as_array)
            .ok_or_else(|| ManifestError::NotAnArray {
                field: "cases".to_string(),
            })?;
    let cases = raw_cases
        .iter()
        .map(parse_case)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CaseManifest {
        schema_version,
        contract,
        cases,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_case() -> Value {
        json!({
            "id": "overload-arity-a",
            "scenarioFamily": "shared-language-semantics",
            "language": "csharp",
            "profiles": ["csharp-net8.0-sdk"],
            "prerequisites": [],
            "factContract": "semantic-truth-v1",
            "source": ["src/Overloads.cs"],
            "expect": {
                "context": "complete",
                "present": [],
                "absent": [],
                "unresolved": [],
                "diagnostics": []
            },
            "provenance": { "kind": "reviewed" }
        })
    }

    #[test]
    fn valid_case_passes_validation() {
        assert!(validate_case(&valid_case()).is_ok());
    }

    #[test]
    fn missing_required_field_is_named() {
        let mut case = valid_case();
        case.as_object_mut().unwrap().remove("profiles");
        let err = validate_case(&case).unwrap_err();
        assert_eq!(
            err,
            ManifestError::MissingField {
                case_id: "overload-arity-a".to_string(),
                field: "profiles".to_string(),
            }
        );
    }

    #[test]
    fn missing_expect_subfield_is_named() {
        let mut case = valid_case();
        case["expect"]
            .as_object_mut()
            .unwrap()
            .remove("diagnostics");
        let err = validate_case(&case).unwrap_err();
        assert_eq!(
            err,
            ManifestError::MissingField {
                case_id: "overload-arity-a".to_string(),
                field: "expect.diagnostics".to_string(),
            }
        );
    }

    #[test]
    fn provenance_naming_the_graded_producer_is_refused() {
        let mut case = valid_case();
        case["provenance"] =
            json!({ "kind": "separately-selected-compiler", "tool": "scout-semantic@1.0.0" });
        let err = validate_case(&case).unwrap_err();
        assert_eq!(
            err,
            ManifestError::ProducerGradesOwnExpectation {
                case_id: "overload-arity-a".to_string(),
            }
        );
    }

    #[test]
    fn a_different_compiler_path_is_accepted() {
        let mut case = valid_case();
        case["provenance"] =
            json!({ "kind": "separately-selected-compiler", "tool": "roslyn-cli@4.14.0" });
        assert!(validate_case(&case).is_ok());
    }

    #[test]
    fn whole_manifest_parses_and_rejects_the_first_bad_case() {
        let good = valid_case();
        let mut bad = valid_case();
        bad["id"] = json!("second-case");
        bad.as_object_mut().unwrap().remove("source");
        let doc = json!({
            "schemaVersion": 1,
            "contract": "semantic-truth-v1",
            "cases": [good, bad]
        });
        let err = parse_manifest(&doc.to_string()).unwrap_err();
        assert_eq!(
            err,
            ManifestError::MissingField {
                case_id: "second-case".to_string(),
                field: "source".to_string(),
            }
        );
    }
}
