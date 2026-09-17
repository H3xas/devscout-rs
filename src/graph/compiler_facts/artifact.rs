// The admitted-artifact header: the fixed identity/negotiation fields
// `admit` checks, parsed from a `serde_json::Value` view exactly as
// `graph::imports::parse_imported_edges` reads `format`/`schemaVersion`/
// `provenance` off a `Value` before typing sub-blocks. The bulk payload --
// diagnostics, symbol facts, and the embedded compilation-context
// envelope's own body -- is NEVER deserialized into an exhaustive Rust
// struct here and is not modeled by this type at all; `admit` keeps the
// original candidate byte slice unchanged and that is the one thing
// publication ever writes. A producer adding a new payload field can never
// be silently dropped by a re-serialization this module never performs,
// and reading a published artifact back always reproduces the exact bytes
// a valid run wrote.
//
// Wire shape (the one this module's `parse_header` accepts):
//
//   {
//     "format": "compiler-facts",
//     "contractVersion": 1,
//     "artifactSchemaVersion": 1,
//     "producer": {"name": "...", "engineRevision": "..."},
//     "profile": {"target": "...", "configuration": "...", "platform": "..."},
//     "dependencyFingerprint": "<hex>",
//     "context": {
//       "schemaVersion": 1,
//       "contextFingerprint": "<hex>",
//       "envelope": { ...the compilation-context envelope body, opaque... }
//     },
//     "sourceSnapshot": {"headSha": "<hex>"},
//     "capabilities": {"requested": [...], "provided": [...]},
//     "completion": {"terminal": true},
//     "units": {"processed": [...], "missing": [...]},
//     "coverage": {"state": "complete"} |
//                 {"state": "incomplete", "incompleteUnits": [{"unit":"...","reason":"..."}]},
//     "diagnostics": [ ...opaque... ],
//     "symbols": [ ...opaque... ]
//   }
//
// `context.contextFingerprint` is a header-level summary the engine copies
// out of its own embedded envelope body (`context.envelope.fingerprint`)
// so admission can check the two agree without parsing the envelope's
// internal shape -- see `RefusalReason::ContextFingerprintMismatch`'s doc
// comment for why this differs from `DependencyFingerprintMismatch`.

use serde_json::{Map, Value};

use super::reasons::RefusalReason;

/// The export format `compiler-facts` admission accepts.
pub const COMPILER_FACTS_FORMAT: &str = "compiler-facts";

/// The wire-contract version this admission path was built for. Also the
/// literal that names the artifact file (`compiler-facts-v<N>.json`).
pub const COMPILER_FACTS_CONTRACT_VERSION: u64 = 1;

/// This artifact's own schema version (distinct from `contractVersion`, the
/// producer's own wire-shape version, the same split
/// `graph::imports::IMPORTED_EDGES_SCHEMA_VERSION` /
/// `IMPORTED_EDGES_ARTIFACT_SCHEMA_VERSION` already draws).
pub const COMPILER_FACTS_ARTIFACT_SCHEMA_VERSION: u64 = 1;

/// The requested compilation profile: target, configuration and platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// The requested target framework moniker, e.g. `net9.0`.
    pub target: String,
    /// The requested build configuration, e.g. `Debug`.
    pub configuration: String,
    /// The requested platform, e.g. `AnyCPU`.
    pub platform: String,
}

/// One unit a structurally valid but declared-incomplete artifact could not
/// fully process, with the compiler's own per-unit reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncompleteUnit {
    /// The unit identifier, matching an entry in `units.processed`.
    pub unit: String,
    /// The compiler's own diagnostic reason this unit stayed incomplete.
    pub reason: String,
}

/// Whether an admitted artifact reports clean, complete coverage or a
/// qualified partial one. Never conflated with a [`RefusalReason`]: a
/// structurally valid, declared-incomplete artifact is admitted, not
/// refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Coverage {
    /// Every unit the candidate declared processed reports no diagnostic
    /// incompleteness.
    Complete,
    /// At least one processed unit carries a diagnostic and is reported
    /// incomplete; sibling units unrelated to it are still admitted.
    Incomplete {
        /// Every incomplete unit's id and reason, in declared order.
        units: Vec<IncompleteUnit>,
    },
}

impl Coverage {
    /// Whether this coverage state is [`Coverage::Complete`].
    pub fn is_complete(&self) -> bool {
        matches!(self, Coverage::Complete)
    }
}

/// The admitted-artifact header: every fixed identity/negotiation field
/// `admit` checks, parsed from the candidate's own JSON. See this module's
/// header comment for the full wire shape and for why the bulk payload
/// (diagnostics, symbols, the embedded context-envelope body) is not
/// modeled here at all.
#[derive(Debug, Clone)]
pub struct CandidateHeader {
    /// The candidate's own declared wire-contract version.
    pub contract_version: u64,
    /// The producer name, e.g. `scout-semantic`.
    pub producer_name: String,
    /// The producer's own engine-protocol revision.
    pub engine_revision: String,
    /// The requested compilation profile.
    pub profile: Profile,
    /// The engine build's own dependency-lock digest.
    pub dependency_fingerprint: String,
    /// The embedded compilation-context envelope's own version literal.
    pub context_schema_version: u64,
    /// The header-level context-fingerprint summary, when the producer
    /// wrote one.
    pub context_fingerprint_header: Option<String>,
    /// The same fingerprint as carried inside the embedded envelope body,
    /// when both the envelope and its `fingerprint` field are present.
    pub context_fingerprint_envelope: Option<String>,
    /// The declared source-snapshot identity (`sourceSnapshot.headSha`),
    /// when the producer wrote one.
    pub source_head_sha: Option<String>,
    /// Whether the candidate carries an explicit terminal completion
    /// record. A killed or truncated run never reaches `true`.
    pub completion_terminal: bool,
    /// Every unit id the candidate declares processed.
    pub units_processed: Vec<String>,
    /// Every unit id the candidate declares missing.
    pub units_missing: Vec<String>,
    /// The candidate's own declared coverage state.
    pub coverage: Coverage,
}

fn obj_str<'a>(obj: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(Value::as_str)
}

fn obj_u64(obj: &Map<String, Value>, key: &str) -> Option<u64> {
    obj.get(key).and_then(Value::as_u64)
}

fn obj_bool(obj: &Map<String, Value>, key: &str) -> Option<bool> {
    obj.get(key).and_then(Value::as_bool)
}

fn obj_obj<'a>(obj: &'a Map<String, Value>, key: &str) -> Option<&'a Map<String, Value>> {
    obj.get(key).and_then(Value::as_object)
}

fn str_array(obj: &Map<String, Value>, key: &str) -> Option<Vec<String>> {
    obj.get(key)?
        .as_array()?
        .iter()
        .map(|v| v.as_str().map(str::to_string))
        .collect()
}

fn parse_coverage(obj: &Map<String, Value>) -> Result<Coverage, RefusalReason> {
    let coverage_obj = obj_obj(obj, "coverage").ok_or(RefusalReason::MalformedEncoding)?;
    match obj_str(coverage_obj, "state").ok_or(RefusalReason::MalformedEncoding)? {
        "complete" => Ok(Coverage::Complete),
        "incomplete" => {
            let raw = coverage_obj
                .get("incompleteUnits")
                .and_then(Value::as_array)
                .ok_or(RefusalReason::MalformedEncoding)?;
            let mut units = Vec::with_capacity(raw.len());
            for item in raw {
                let item_obj = item.as_object().ok_or(RefusalReason::MalformedEncoding)?;
                units.push(IncompleteUnit {
                    unit: obj_str(item_obj, "unit")
                        .ok_or(RefusalReason::MalformedEncoding)?
                        .to_string(),
                    reason: obj_str(item_obj, "reason")
                        .ok_or(RefusalReason::MalformedEncoding)?
                        .to_string(),
                });
            }
            if units.is_empty() {
                return Err(RefusalReason::MalformedEncoding);
            }
            Ok(Coverage::Incomplete { units })
        }
        _ => Err(RefusalReason::MalformedEncoding),
    }
}

/// Parses and structurally validates a candidate artifact's header.
///
/// `Err(RefusalReason::MalformedEncoding)` is the only failure mode: invalid
/// JSON, a non-object root, an unrecognised `format`, or any required
/// header field missing or of the wrong type. Every other refusal class is
/// decided by `super::admit::admit` once this header is in hand -- this
/// function performs no identity comparison of its own.
pub fn parse_header(bytes: &[u8]) -> Result<CandidateHeader, RefusalReason> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| RefusalReason::MalformedEncoding)?;
    let obj = value.as_object().ok_or(RefusalReason::MalformedEncoding)?;

    if obj_str(obj, "format") != Some(COMPILER_FACTS_FORMAT) {
        return Err(RefusalReason::MalformedEncoding);
    }
    let contract_version =
        obj_u64(obj, "contractVersion").ok_or(RefusalReason::MalformedEncoding)?;
    // `artifactSchemaVersion` is read for presence/shape only; nothing in
    // this admission path branches on it yet (there is only one artifact
    // schema version so far).
    obj_u64(obj, "artifactSchemaVersion").ok_or(RefusalReason::MalformedEncoding)?;

    let producer = obj_obj(obj, "producer").ok_or(RefusalReason::MalformedEncoding)?;
    let producer_name = obj_str(producer, "name")
        .ok_or(RefusalReason::MalformedEncoding)?
        .to_string();
    let engine_revision = obj_str(producer, "engineRevision")
        .ok_or(RefusalReason::MalformedEncoding)?
        .to_string();

    let profile_obj = obj_obj(obj, "profile").ok_or(RefusalReason::MalformedEncoding)?;
    let profile = Profile {
        target: obj_str(profile_obj, "target")
            .ok_or(RefusalReason::MalformedEncoding)?
            .to_string(),
        configuration: obj_str(profile_obj, "configuration")
            .ok_or(RefusalReason::MalformedEncoding)?
            .to_string(),
        platform: obj_str(profile_obj, "platform")
            .ok_or(RefusalReason::MalformedEncoding)?
            .to_string(),
    };

    let dependency_fingerprint = obj_str(obj, "dependencyFingerprint")
        .ok_or(RefusalReason::MalformedEncoding)?
        .to_string();

    let context_obj = obj_obj(obj, "context").ok_or(RefusalReason::MalformedEncoding)?;
    let context_schema_version =
        obj_u64(context_obj, "schemaVersion").ok_or(RefusalReason::MalformedEncoding)?;
    let context_fingerprint_header = obj_str(context_obj, "contextFingerprint").map(str::to_string);
    let context_fingerprint_envelope = obj_obj(context_obj, "envelope")
        .and_then(|env| obj_str(env, "fingerprint"))
        .map(str::to_string);

    let source_head_sha = obj_obj(obj, "sourceSnapshot")
        .and_then(|s| obj_str(s, "headSha"))
        .map(str::to_string);

    let completion_obj = obj_obj(obj, "completion").ok_or(RefusalReason::MalformedEncoding)?;
    let completion_terminal =
        obj_bool(completion_obj, "terminal").ok_or(RefusalReason::MalformedEncoding)?;

    let units_obj = obj_obj(obj, "units").ok_or(RefusalReason::MalformedEncoding)?;
    let units_processed =
        str_array(units_obj, "processed").ok_or(RefusalReason::MalformedEncoding)?;
    let units_missing = str_array(units_obj, "missing").ok_or(RefusalReason::MalformedEncoding)?;

    let coverage = parse_coverage(obj)?;

    Ok(CandidateHeader {
        contract_version,
        producer_name,
        engine_revision,
        profile,
        dependency_fingerprint,
        context_schema_version,
        context_fingerprint_header,
        context_fingerprint_envelope,
        source_head_sha,
        completion_terminal,
        units_processed,
        units_missing,
        coverage,
    })
}
