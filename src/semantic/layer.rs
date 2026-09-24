// The resolve-time consumption path's own artifact reader and compatibility
// join. Reads the ADMITTED artifact's raw bytes directly (`bytes` off
// `graph::compiler_facts::read_compiler_facts`, never re-parsing oracle
// output and never a second admission path) into the occurrence-site facts
// the resolver needs, keeping every field this crate has no other use for
// (candidates, diagnostics, symbols) opaque and unread.
//
// Two passes, deliberately kept apart: `parse_raw` (no `DefIndex`, pure byte
// parsing, callable before the resolver builds one) and `SemanticLayer::build`
// (translates each occurrence's own Roslyn-encoded identity into a
// `resolve::DefIndex` def id, using the index the resolver already built for
// this same run). The admitted artifact's `symbols[]`/`occurrences[].target`
// identity encoding is Roslyn's dotted display format with an inline
// generic-argument suffix at whichever segment declares it (`Ns.Outer.Nested<T>`),
// not devscout's own `+`-nested, arity-free def-id scheme
// (`Ns.Outer+Nested`); `translate_type_id` performs that translation, the
// same shortest-head-first walk `resolve::ladder`'s own nested-type step
// already performs for extractor-emitted references, reimplemented narrowly
// here rather than crossing a `pub(crate)` seam into a sibling module for a
// handful of lines.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use crate::graph::read_compiler_facts;
use crate::resolve::DefIndex;

use super::freshness::{self, Freshness};
use super::uncertainty::{self, Uncertainty};

/// One occurrence site, exactly as the admitted artifact wrote it -- every
/// field this module reads, and no others. `target_type`/`target_member` are
/// `None` for a site with no confirmed or candidate symbol at all
/// (`resolution == "unresolved"`).
///
/// `(file, name_line, target_member)` is the compatibility join key against
/// today's graph (`SemanticLayer::by_site`) -- it is never the fact's own
/// identity. The fields below it are that exact identity, carried alongside
/// the key rather than folded into it: `target_assembly`/
/// `target_generic_arity`/`target_overload_signature` are the target's
/// complete identity in the producer's own encoding (`occurrences.
/// identityEncoding`); `span_*` is the occurrence's own span under
/// `occurrences.spanEncoding`'s stated convention; `document_content_identity`/
/// `target_document_content_identities` are the caller's and target's own
/// document content identities. `target_overload_signature` doubles as
/// `lookup`'s own overload disambiguator (`overload_arg_count`): two
/// same-line occurrences of one member with different signatures are kept
/// as distinct facts instead of collapsing into one ambiguous outcome the
/// way the bare `(file, line, member)` key alone would.
#[derive(Debug, Clone)]
struct RawOccurrence {
    file: String,
    name_line: usize,
    resolution: String,
    target_type: Option<String>,
    target_member: Option<String>,
    compilation_identity: Value,
    #[allow(
        dead_code,
        reason = "the exact identity this consumer's own model carries alongside the compatibility key, parsed for a future consumer (an exact-identity export, a disagreement diagnostic naming the exact overload) -- target_overload_signature is the one field lookup reads today, for its own arity narrowing"
    )]
    target_assembly: Option<String>,
    #[allow(dead_code, reason = "see target_assembly's own allow above")]
    target_generic_arity: Option<u64>,
    target_overload_signature: Option<String>,
    #[allow(dead_code, reason = "see target_assembly's own allow above")]
    span_start_line: Option<u64>,
    #[allow(dead_code, reason = "see target_assembly's own allow above")]
    span_start_char: Option<u64>,
    #[allow(dead_code, reason = "see target_assembly's own allow above")]
    span_end_line: Option<u64>,
    #[allow(dead_code, reason = "see target_assembly's own allow above")]
    span_end_char: Option<u64>,
    #[allow(dead_code, reason = "see target_assembly's own allow above")]
    document_content_identity: Option<String>,
    #[allow(dead_code, reason = "see target_assembly's own allow above")]
    target_document_content_identities: Vec<String>,
}

/// One embedded envelope compilation's own state, keyed by its `identity`
/// for a same-value match against an occurrence's own `compilation.identity`
/// -- the same match the admission path's own consistency check performs.
/// `imports` is this compilation's own external-file list (`freshness::
/// ImportRef`), flattened across every compilation by `parse_raw` into the
/// whole-artifact list `SemanticLayer::load` passes to `freshness::evaluate`.
#[derive(Debug, Clone)]
struct RawCompilation {
    identity: Value,
    state: String,
    reason: String,
    imports: Vec<freshness::ImportRef>,
}

fn parse_imports(entry: &serde_json::Map<String, Value>) -> Vec<freshness::ImportRef> {
    let Some(raw) = entry.get("imports").and_then(Value::as_array) else {
        return Vec::new();
    };
    raw.iter()
        .filter_map(|item| {
            let obj = as_obj(item)?;
            Some(freshness::ImportRef {
                identity: obj.get("identity").and_then(Value::as_str)?.to_string(),
                hash: obj.get("hash").and_then(Value::as_str)?.to_string(),
            })
        })
        .collect()
}

/// The admitted artifact's bytes, parsed into exactly what this consumer
/// needs and nothing else. Building one performs no file I/O of its own --
/// see `load`, the one place this module reads a file.
struct RawFacts {
    source_head_sha: Option<String>,
    source_dirty: bool,
    compilations: Vec<RawCompilation>,
    occurrences: Vec<RawOccurrence>,
}

fn as_obj(v: &Value) -> Option<&serde_json::Map<String, Value>> {
    v.as_object()
}

fn parse_compilations(root: &Value) -> Vec<RawCompilation> {
    let Some(entries) = root
        .get("context")
        .and_then(as_obj)
        .and_then(|c| c.get("envelope"))
        .and_then(as_obj)
        .and_then(|e| e.get("compilations"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let obj = as_obj(entry)?;
            Some(RawCompilation {
                identity: obj.get("identity")?.clone(),
                state: obj.get("state").and_then(Value::as_str)?.to_string(),
                reason: obj
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                imports: parse_imports(obj),
            })
        })
        .collect()
}

fn parse_occurrences(root: &Value) -> Vec<RawOccurrence> {
    let Some(sites) = root
        .get("occurrences")
        .and_then(as_obj)
        .and_then(|o| o.get("sites"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    sites
        .iter()
        .filter_map(|site| {
            let obj = as_obj(site)?;
            let file = obj.get("file").and_then(Value::as_str)?.to_string();
            let name_line = obj
                .get("name")
                .and_then(as_obj)
                .and_then(|n| n.get("line"))
                .and_then(Value::as_u64)? as usize;
            let resolution = obj.get("resolution").and_then(Value::as_str)?.to_string();
            let target = obj.get("target").and_then(as_obj);
            let target_type = target
                .and_then(|t| t.get("type"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let target_member = target
                .and_then(|t| t.get("member"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let compilation_identity = obj
                .get("compilation")
                .and_then(as_obj)
                .and_then(|c| c.get("identity"))
                .cloned()
                .unwrap_or(Value::Null);
            let target_assembly = target
                .and_then(|t| t.get("assembly"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let target_generic_arity = target
                .and_then(|t| t.get("genericArity"))
                .and_then(Value::as_u64);
            let target_overload_signature = target
                .and_then(|t| t.get("overloadSignature"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let span = obj.get("span").and_then(as_obj);
            let span_start_line = span
                .and_then(|s| s.get("startLine"))
                .and_then(Value::as_u64);
            let span_start_char = span
                .and_then(|s| s.get("startChar"))
                .and_then(Value::as_u64);
            let span_end_line = span.and_then(|s| s.get("endLine")).and_then(Value::as_u64);
            let span_end_char = span.and_then(|s| s.get("endChar")).and_then(Value::as_u64);
            let document_content_identity = obj
                .get("documentContentIdentity")
                .and_then(Value::as_str)
                .map(str::to_string);
            let target_document_content_identities = obj
                .get("targetDocumentContentIdentities")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            Some(RawOccurrence {
                file,
                name_line,
                resolution,
                target_type,
                target_member,
                compilation_identity,
                target_assembly,
                target_generic_arity,
                target_overload_signature,
                span_start_line,
                span_start_char,
                span_end_line,
                span_end_char,
                document_content_identity,
                target_document_content_identities,
            })
        })
        .collect()
}

/// Parses the parameter count out of an overload signature like
/// `"()->void"` or `"(bool, string)->void"` -- the producer's own
/// `overloadSignature` encoding. Depth-aware: a comma inside a generic
/// argument list (`"(Dictionary<string, int>)->void"`) never splits, the
/// same discipline `split_qualified_type` already applies to a qualified
/// type name. `None` when the string does not have the `"(...)->..."` shape
/// at all -- treated as "cannot disambiguate by arity", never as zero.
fn overload_arg_count(signature: &str) -> Option<usize> {
    let after_open = signature.strip_prefix('(')?;
    let close = after_open.find(")->")?;
    let params = &after_open[..close];
    if params.trim().is_empty() {
        return Some(0);
    }
    let mut count = 1;
    let mut depth = 0i32;
    for ch in params.chars() {
        match ch {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => count += 1,
            _ => {}
        }
    }
    Some(count)
}

fn parse_raw(bytes: &[u8]) -> Option<RawFacts> {
    let root: Value = serde_json::from_slice(bytes).ok()?;
    let source_head_sha = root
        .get("sourceSnapshot")
        .and_then(as_obj)
        .and_then(|s| s.get("headSha"))
        .and_then(Value::as_str)
        .map(str::to_string);
    // Absent `dirty` cannot be trusted to mean clean -- see freshness.rs's
    // own header comment.
    let source_dirty = root
        .get("sourceSnapshot")
        .and_then(as_obj)
        .and_then(|s| s.get("dirty"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    Some(RawFacts {
        source_head_sha,
        source_dirty,
        compilations: parse_compilations(&root),
        occurrences: parse_occurrences(&root),
    })
}

/// Splits a Roslyn fully-qualified type display string into its dotted
/// segments, stripping each segment's own trailing balanced `<...>`
/// generic-argument suffix (present only on a segment whose own type
/// declares generic parameters) and never splitting on a `.` that sits
/// inside one -- a generic argument can itself be a qualified type name.
fn split_qualified_type(raw: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    for ch in raw.chars() {
        match ch {
            '<' => {
                depth += 1;
                current.push(ch);
            }
            '>' => {
                depth -= 1;
                current.push(ch);
            }
            '.' if depth == 0 => {
                segments.push(current.clone());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
        .into_iter()
        .map(|seg| match seg.find('<') {
            Some(i) => seg[..i].to_string(),
            None => seg,
        })
        .collect()
}

/// Translates one admitted-artifact type identity into a devscout def index,
/// trying the shortest possible outer-type head first and walking the
/// remaining segments as nested-type `+` links -- the resolver's own ladder
/// performs the equivalent walk for extractor-emitted references; this
/// reapplies the same shape to a fully-qualified string that carries no
/// using or alias ambiguity to resolve. `None` when no split point's walk
/// consumes every segment, which the caller treats as "no confirmed answer"
/// rather than an error.
fn translate_type_id(raw: &str, index: &DefIndex) -> Option<usize> {
    let segments = split_qualified_type(raw);
    for k in 1..=segments.len() {
        let head = segments[..k].join(".");
        let Some(&start) = index.qualified_name_to_def.get(&head) else {
            continue;
        };
        let mut cur = start;
        let mut complete = true;
        for seg in &segments[k..] {
            let candidate = format!("{}+{}", index.defs[cur].id, seg);
            match index.qualified_name_to_def.get(&candidate) {
                Some(&next) => cur = next,
                None => {
                    complete = false;
                    break;
                }
            }
        }
        if complete {
            return Some(cur);
        }
    }
    None
}

/// One translated, override-ready target: the devscout def id/file the
/// compiler's own confirmed occurrence names, resolved to the specific enum
/// member def when the containing type is an enum, the same way the
/// resolver's own enum branch does for an extractor-emitted reference.
/// `overload_signature` carries the admitted occurrence's own signature
/// alongside `to_def_id`/`to_file` -- never folded into the compatibility
/// key, but part of THIS target's own exact identity, so `dedup` (both here
/// and in `lookup`'s own `resolved_targets.dedup()`) keys on the full fact
/// rather than only on which TYPE it names: two same-line overloads of one
/// member on the SAME type carry the SAME `to_def_id`/`to_file` and must
/// stay two distinct targets, while two admitted records of the genuinely
/// SAME occurrence (same type, same overload) must still collapse to one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticTarget {
    /// The devscout def id the compiler's target translates to.
    pub to_def_id: String,
    /// That def's own declaring file.
    pub to_file: String,
    /// The admitted occurrence's own overload signature, when it named one.
    pub overload_signature: Option<String>,
}

fn resolve_target(
    type_idx: usize,
    member: &str,
    overload_signature: Option<&str>,
    index: &DefIndex,
) -> SemanticTarget {
    let def = &index.defs[type_idx];
    let overload_signature = overload_signature.map(str::to_string);
    if def.kind == "enum" {
        let key = format!("{}.{member}", def.id);
        if let Some(&member_idx) = index.qualified_name_to_def.get(&key) {
            let member_def = &index.defs[member_idx];
            return SemanticTarget {
                to_def_id: member_def.id.clone(),
                to_file: member_def.file.clone(),
                overload_signature,
            };
        }
    }
    SemanticTarget {
        to_def_id: def.id.clone(),
        to_file: def.file.clone(),
        overload_signature,
    }
}

/// What a same-context lookup at one reference's `(file, name_line, member)`
/// key found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupOutcome {
    /// No admitted occurrence site at this key -- the ordinary absent case,
    /// never treated as a negative fact.
    NoFact,
    /// Exactly one same-context confirmed target, ready to override the
    /// ladder.
    Confirmed(SemanticTarget),
    /// A fact exists but is not confirmed-and-translatable; every other
    /// consumer-visible state.
    Other(Uncertainty),
    /// More than one distinct confirmed target survived (no caller-side
    /// arity narrowed the set to one, or there was none to narrow with in
    /// the first place). Never itself an ambiguity: it is the caller's own
    /// choice whether "more than one confirmed target, nothing left to pick
    /// between them" projects every one of them or degrades to
    /// `Other(Uncertainty::Ambiguous)` -- see `discovered.rs` (which
    /// projects same-type overloads, since it carries no caller argument
    /// count to disambiguate with at all) and `precedence.rs` (which
    /// degrades, since an extractor-emitted reference already has its own
    /// single-target slot to fill and a real caller-side tie there is
    /// genuine ambiguity, not a projection opportunity).
    ConfirmedMany(Vec<SemanticTarget>),
}

/// The resolve-time consumption path's built-once layer: freshness, plus a
/// `(file, name_line, member)` compatibility join index over every admitted
/// occurrence, with identity translation applied lazily per lookup. Built by
/// `load`.
pub struct SemanticLayer {
    freshness: Freshness,
    by_site: HashMap<(String, usize, String), Vec<usize>>,
    occurrences: Vec<RawOccurrence>,
    compilations: Vec<RawCompilation>,
}

impl SemanticLayer {
    /// Reads this checkout's admitted compiler-facts artifact and builds the
    /// layer. `None` when no artifact is admitted (the ordinary "no engine
    /// ever ran here" state) or its bytes do not parse as this module
    /// expects; both are fail-open, not errors -- the whole run proceeds as
    /// a syntax-only build.
    pub fn load(root: &Path) -> Option<SemanticLayer> {
        let admitted = read_compiler_facts(root)?;
        let raw = parse_raw(&admitted.bytes)?;
        let imports: Vec<freshness::ImportRef> = raw
            .compilations
            .iter()
            .flat_map(|c| c.imports.iter().cloned())
            .collect();
        let freshness = freshness::evaluate(
            root,
            raw.source_head_sha.as_deref(),
            raw.source_dirty,
            &imports,
        );

        let mut by_site: HashMap<(String, usize, String), Vec<usize>> = HashMap::new();
        for (i, occ) in raw.occurrences.iter().enumerate() {
            if let Some(member) = &occ.target_member {
                by_site
                    .entry((occ.file.clone(), occ.name_line, member.clone()))
                    .or_default()
                    .push(i);
            }
        }

        Some(SemanticLayer {
            freshness,
            by_site,
            occurrences: raw.occurrences,
            compilations: raw.compilations,
        })
    }

    /// Whether the whole artifact is fresh -- consulted once per `map` run,
    /// not per reference.
    pub fn is_fresh(&self) -> bool {
        self.freshness.is_fresh()
    }

    /// Every distinct `(file, name_line, member)` compatibility key this
    /// layer indexes -- `discovered.rs`'s own iteration surface, walked once
    /// per admitted artifact rather than once per extractor reference.
    pub fn site_keys(&self) -> impl Iterator<Item = &(String, usize, String)> {
        self.by_site.keys()
    }

    fn compilation_state(&self, identity: &Value) -> Option<(&str, &str)> {
        self.compilations
            .iter()
            .find(|c| &c.identity == identity)
            .map(|c| (c.state.as_str(), c.reason.as_str()))
    }

    /// Looks up the compatibility join key `(file, name_line, member)`.
    /// Freshness is checked first (a stale artifact never confirms anything,
    /// regardless of what any individual site says). More than one matching
    /// site is not automatically an ambiguity: `caller_arg_count`, the
    /// reference's own argument count when the call syntax carries one
    /// (`None` for a property read, which never disambiguates), is compared
    /// against each candidate's `target_overload_signature` -- the exact
    /// identity carried alongside this compatibility key, not folded into
    /// it. When exactly one candidate's own arity matches, that candidate
    /// alone is resolved, the same way a single hit would be; two same-line
    /// overloads of one member survive as distinct facts through this path
    /// rather than collapsing together. Only when arity cannot narrow the
    /// set to one (no caller arg count, no signature, more than one
    /// survivor, or a genuine tie) do disagreeing targets collapse to an
    /// ambiguous outcome rather than picking one arbitrarily -- the key is
    /// a compatibility join, not the fact's own identity, so more than one
    /// unresolved hit at it is a real ambiguity this consumer introduces,
    /// not one the compiler itself reported.
    pub fn lookup(
        &self,
        index: &DefIndex,
        file: &str,
        line: usize,
        member: &str,
        caller_arg_count: Option<usize>,
    ) -> LookupOutcome {
        if !self.freshness.is_fresh() {
            let Freshness::Stale { reason } = &self.freshness else {
                unreachable!("is_fresh() already returned false");
            };
            return LookupOutcome::Other(Uncertainty::Stale {
                reason: reason.clone(),
            });
        }
        let Some(indices) = self
            .by_site
            .get(&(file.to_string(), line, member.to_string()))
        else {
            return LookupOutcome::NoFact;
        };

        // Arity narrowing: only ever REMOVES candidates, and only when it
        // lands on exactly one -- see this function's own doc comment.
        let narrowed: Vec<usize>;
        let indices: &[usize] = if indices.len() > 1 {
            if let Some(want) = caller_arg_count {
                let matching: Vec<usize> = indices
                    .iter()
                    .copied()
                    .filter(|&i| {
                        self.occurrences[i]
                            .target_overload_signature
                            .as_deref()
                            .and_then(overload_arg_count)
                            == Some(want)
                    })
                    .collect();
                if matching.len() == 1 {
                    narrowed = matching;
                    &narrowed
                } else {
                    indices.as_slice()
                }
            } else {
                indices.as_slice()
            }
        } else {
            indices.as_slice()
        };

        let mut resolved_targets: Vec<SemanticTarget> = Vec::new();
        let mut other: Option<Uncertainty> = None;
        for &i in indices {
            let occ = &self.occurrences[i];
            let state = self.compilation_state(&occ.compilation_identity);
            let uncertainty = match state {
                Some((cstate, creason)) => uncertainty::from_compilation_state(cstate, creason)
                    .unwrap_or_else(|| uncertainty::from_resolution(&occ.resolution)),
                None => uncertainty::from_resolution(&occ.resolution),
            };
            if uncertainty.is_confirmed() {
                if let Some(type_id) = occ.target_type.as_deref() {
                    if let Some(type_idx) = translate_type_id(type_id, index) {
                        resolved_targets.push(resolve_target(
                            type_idx,
                            member,
                            occ.target_overload_signature.as_deref(),
                            index,
                        ));
                        continue;
                    }
                }
                // A confirmed fact this consumer's identity translation
                // cannot place in the graph at all: never silently dropped,
                // never treated as a negative fact either -- surfaced as its
                // own explicit state.
                other.get_or_insert(Uncertainty::Candidate);
            } else {
                other.get_or_insert(uncertainty);
            }
        }

        resolved_targets.dedup();
        match resolved_targets.len() {
            0 => other
                .map(LookupOutcome::Other)
                .unwrap_or(LookupOutcome::NoFact),
            1 => LookupOutcome::Confirmed(resolved_targets.remove(0)),
            _ => LookupOutcome::ConfirmedMany(resolved_targets),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overload_arg_count_reads_a_zero_arg_signature() {
        assert_eq!(overload_arg_count("()->void"), Some(0));
    }

    #[test]
    fn overload_arg_count_counts_top_level_commas() {
        assert_eq!(overload_arg_count("(bool, string)->void"), Some(2));
    }

    #[test]
    fn overload_arg_count_never_splits_inside_a_generic_argument_list() {
        assert_eq!(
            overload_arg_count("(System.Collections.Generic.Dictionary<string, int>)->void"),
            Some(1)
        );
    }

    #[test]
    fn overload_arg_count_is_none_for_a_string_with_no_parenthesized_shape() {
        assert_eq!(overload_arg_count("void"), None);
        assert_eq!(overload_arg_count(""), None);
    }

    #[test]
    fn splits_plain_dotted_segments() {
        assert_eq!(
            split_qualified_type("Ns.Outer.Widget"),
            vec!["Ns", "Outer", "Widget"]
        );
    }

    #[test]
    fn strips_a_trailing_generic_suffix_per_segment() {
        assert_eq!(
            split_qualified_type("Ns.Outer.Nested<T>"),
            vec!["Ns", "Outer", "Nested"]
        );
    }

    #[test]
    fn never_splits_inside_a_generic_argument_list() {
        assert_eq!(
            split_qualified_type("Ns.Holder<System.String>.Leaf"),
            vec!["Ns", "Holder", "Leaf"]
        );
    }

    #[test]
    fn a_doubly_nested_generic_strips_every_level() {
        assert_eq!(
            split_qualified_type("Ns.Outer<K>.Middle<V>.Leaf"),
            vec!["Ns", "Outer", "Middle", "Leaf"]
        );
    }
}
