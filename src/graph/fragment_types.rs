use serde::{Deserialize, Serialize};

use crate::extract;

use super::ordered::OrderedMap;

// ---------------------------------------------------------------------------
// Fragments cache schema -- the RAW (unresolved) per-file extraction output,
// keyed by repo-relative path. Mirrors extract.rs's DefRecord/UsingRecord/
// RefRecord field-for-field; the serde impls live here rather than on the
// extractor's own types.
// ---------------------------------------------------------------------------

/// Field order is significant: id, name, namespace, kind, line, methods, then
/// the member-fact additions in that exact order -- properties, fields,
/// methodReturns -- and after those extensionMethods, appended LAST. Each is
/// omitted entirely when empty, so a type declaring none of them serializes
/// exactly as it did before those additions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragDef {
    /// The id value.
    pub id: String,
    /// The name value.
    pub name: String,
    /// The namespace value.
    pub namespace: String,
    /// The kind value.
    pub kind: String,
    /// The line value.
    pub line: usize,
    /// The methods value.
    pub methods: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// The properties value.
    pub properties: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// The fields value.
    pub fields: Vec<String>,
    /// Member name -> declared return type NAME, in FIRST-DECLARATION SOURCE
    /// ORDER -- deliberately `OrderedMap`, never a `BTreeMap`: this is an
    /// ordered pair list, and sorting the keys here would break the
    /// serialized bytes.
    #[serde(
        default,
        rename = "methodReturns",
        skip_serializing_if = "OrderedMap::is_empty"
    )]
    pub method_returns: OrderedMap<String>,
    /// The extension methods this type declares, in source
    /// order, deduped by (name, thisType, arityMin, arityMax). Appended after
    /// `method_returns`, and omitted entirely when empty, so a type declaring
    /// none keeps its exact prior bytes.
    #[serde(
        default,
        rename = "extensionMethods",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub extension_methods: Vec<FragExtensionMethod>,
    /// DIRECT base-type identifiers, source
    /// order, deduped. Appended LAST, after `extension_methods`, omitted when
    /// empty. A resolution input only: `resolve_graph` strips it (along with
    /// properties/fields/extensionMethods) before graph.json's def rows, so
    /// the on-disk def bytes are unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bases: Vec<String>,
    /// The declaring type's own type-parameter names, empty for
    /// every non-generic declaration. Appended after `bases`, omitted when
    /// empty. A resolution input, like `bases`: the ctor-DI resolver's
    /// "is this def itself an open-generic implementation" signal.
    #[serde(default, rename = "typeParams", skip_serializing_if = "Vec::is_empty")]
    pub type_params: Vec<String>,
    /// Per base name that carried a type-argument list, that
    /// list's generic-arg descriptors relative to `type_params` (`OrderedMap`
    /// for the same reason `method_returns` is one: the serialized key order
    /// is significant). Appended after `type_params`, omitted when
    /// empty.
    #[serde(
        default,
        rename = "baseGenericArgs",
        skip_serializing_if = "OrderedMap::is_empty"
    )]
    pub base_generic_args: OrderedMap<Vec<String>>,
    /// The methods a test framework would DISCOVER as
    /// tests, source order, deduped. Appended LAST, after `baseGenericArgs`,
    /// omitted when empty. Unlike its neighbours this one is NOT stripped by
    /// `resolve_graph`: it is what `devscout tests` answers from.
    #[serde(default, rename = "testMethods", skip_serializing_if = "Vec::is_empty")]
    pub test_methods: Vec<String>,
    /// Property name -> declared type fact, in source order and
    /// under the same dedup as `properties`. An `OrderedMap` for the same
    /// reason `method_returns` is one: the serialized key order is significant.
    /// Appended LAST, after `test_methods`, omitted when empty. A
    /// resolution input only, like `properties`/`fields`.
    #[serde(
        default,
        rename = "propertyTypes",
        skip_serializing_if = "OrderedMap::is_empty"
    )]
    pub property_types: OrderedMap<FragFact>,
    /// Field name -> declared type fact, in source order and
    /// under the same dedup as `fields`. Mirrors `propertyTypes` field for
    /// field: an `OrderedMap` for the same reason (the serialized key order
    /// is significant), a resolution input only, like `properties`/`fields`,
    /// and this is a purely additive field -- an absent key reads back as
    /// "no fields typed", the safe default for every fragment cached before
    /// this field existed, so it joins the schema with no cache-version
    /// bump. Appended right after `propertyTypes`, omitted when empty.
    #[serde(
        default,
        rename = "fieldTypes",
        skip_serializing_if = "OrderedMap::is_empty"
    )]
    pub field_types: OrderedMap<FragFact>,
    /// Per method name `methodReturns` also carries an entry for,
    /// that return type's top-level generic-arg descriptors -- the same
    /// capture `baseGenericArgs` keeps beside `bases` (see extract.rs's
    /// `DefRecord`). `methodReturns` itself is left exactly as it always
    /// was (the bare return-type identifier, "Task" for
    /// `Task<Order> GetAsync()`) so an UNAWAITED use of the same callee
    /// keeps reading "Task" unchanged; this is a purely additive sibling
    /// field, not a reshaping of `methodReturns`, which is what lets it
    /// join the schema with no cache-version bump -- an absent key reads
    /// back as "no generic args", the safe default for every fragment
    /// cached before this field existed. Appended LAST of all, after
    /// `fieldTypes`, omitted when empty.
    #[serde(
        default,
        rename = "methodReturnArgs",
        skip_serializing_if = "OrderedMap::is_empty"
    )]
    pub method_return_args: OrderedMap<Vec<String>>,
    /// Declared method names `methods` does not carry because
    /// `is_recorded_method` gates that list on a literal `public` modifier
    /// (or `kind == "interface"`, where this list is always empty). Source
    /// order, no dedup, same as `methods`. A resolution input only, like
    /// `properties`/`fields`/`bases`: `resolve_graph` strips it before
    /// graph.json's def rows. Consulted only for hierarchy-internal
    /// receivers (`base.` and the `this.` shape's own base walk); the
    /// scored tier keeps reading `methods` alone. Appended LAST of all,
    /// after `methodReturnArgs`, omitted when empty -- purely additive, so
    /// an absent key reads back as "no non-public methods", the safe
    /// default for every fragment cached before this field existed, which
    /// is what lets it join the schema with no cache-version bump.
    #[serde(
        default,
        rename = "nonPublicMethods",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub non_public_methods: Vec<String>,
    /// Method name -> the (min, max) argument-count range every overload
    /// sharing that name accepts, one tuple per overload, in declaration
    /// order -- covers every method, public and non-public alike (see
    /// `extract::DefRecord::method_arities`). `max` is -1 for an unbounded
    /// `params` overload, the same sentinel `FragExtensionMethod::arity_max`
    /// already uses. An `OrderedMap` for the same reason `methodReturns` is
    /// one: the serialized key order is significant. Appended LAST of all,
    /// after `nonPublicMethods`, omitted when empty -- purely additive, so
    /// an absent key reads back as "no arity facts", the safe default for
    /// every fragment cached before this field existed, which is what lets
    /// it join the schema with no cache-version bump.
    #[serde(
        default,
        rename = "methodArities",
        skip_serializing_if = "OrderedMap::is_empty"
    )]
    pub method_arities: OrderedMap<Vec<(usize, i64)>>,
    /// Method name -> per-overload parameter type descriptors, one
    /// `Vec<String>` per overload in declaration order -- covers every
    /// method, public and non-public alike, same method set as
    /// `methodArities` (see `extract::DefRecord::method_params`). A
    /// position naming a type parameter of the enclosing method or class is
    /// recorded as `"*"`; a parameter carrying the `this` modifier is
    /// written `"this <descriptor>"`. A `delegate` def carries exactly one
    /// entry here, keyed `"Invoke"`. An `OrderedMap` for the same reason
    /// `methodArities` is one: the serialized key order is significant.
    /// Appended LAST of all, after `methodArities`, omitted when empty.
    /// Joined the schema with the v18 cache bump: a v17 fragment read back
    /// carries none, and every overload-shape/`this`-marker/callee-slot
    /// lookup this field backs would silently see no candidates.
    #[serde(
        default,
        rename = "methodParams",
        skip_serializing_if = "OrderedMap::is_empty"
    )]
    pub method_params: OrderedMap<Vec<Vec<String>>>,
    /// Declared method names carrying the literal `override` modifier,
    /// source order, deduped. Appended LAST of all, after `method_params`,
    /// omitted when empty -- purely additive, so an absent key reads back
    /// as "no overrides", the safe default for every fragment cached before
    /// this field existed, which is what lets it join the schema with the
    /// same v19 cache bump `registrations` does rather than a bump of its
    /// own. Read by the resolver's member-level `overrides` pass to find
    /// the nearest in-graph base member of the same name and arity.
    #[serde(
        default,
        rename = "overrideMethods",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub override_methods: Vec<String>,
    #[serde(default, rename = "endLine", skip_serializing_if = "is_zero")]
    /// The end line value.
    pub end_line: usize,
}

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

/// One declared type fact: the type NAME, plus its top-level
/// type-argument descriptors when the declaration carried any. Field order
/// (`type`, `args`) is significant, and `args` is omitted when absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragFact {
    #[serde(rename = "type")]
    /// The type name value.
    pub type_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The args value.
    pub args: Option<Vec<String>>,
}

/// One `extensionMethods` entry. Serialized field order (`name`, `thisType`,
/// `arityMin`, `arityMax`, `thisArgs`) is significant, and serde emits struct
/// fields in declaration order. The two arity halves are NOT optional: they joined the
/// schema with the v6 cache rename, so no fragment this reader can meet is
/// missing them. `arity_max` is signed because -1 is the unbounded-`params`
/// sentinel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragExtensionMethod {
    /// The name value.
    pub name: String,
    #[serde(rename = "thisType")]
    /// The this type value.
    pub this_type: String,
    #[serde(rename = "arityMin")]
    /// The arity min value.
    pub arity_min: usize,
    #[serde(rename = "arityMax")]
    /// The arity max value.
    pub arity_max: i64,
    /// Present only when the this-parameter type is generic (the key is omitted
    /// key otherwise), so a non-generic entry keeps four fields exactly.
    #[serde(default, rename = "thisArgs", skip_serializing_if = "Option::is_none")]
    pub this_args: Option<Vec<String>>,
}

/// One untyped lambda parameter's callee slot (see extract.rs's `LambdaSlot`).
///
/// Field order (`owner`, `member`, `argCount`, `argIndex`,
/// `arity`, `index`) is significant: serde emits struct fields in
/// declaration order under `#[serde(rename_all = "camelCase")]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FragLambdaSlot {
    /// The callee's type-name text.
    pub owner: String,
    /// The callee method name.
    pub member: String,
    /// The argument count of the callee invocation.
    pub arg_count: usize,
    /// The position of the lambda among the callee invocation's arguments.
    pub arg_index: usize,
    /// The lambda's own parameter count.
    pub arity: usize,
    /// This parameter's position inside the lambda's parameter list.
    pub index: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
/// Represents `FragUsing`.
pub enum FragUsing {
    /// The value value.
    Alias {
        /// The alias name.
        alias: String,
        /// The aliased target.
        target: String,
        /// Whether the directive is global.
        global: bool,
    },
    /// The value value.
    Plain {
        /// The imported namespace text.
        text: String,
        /// Whether the directive is global.
        global: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Represents `FragRef`.
pub struct FragRef {
    /// The kind value.
    pub kind: String,
    /// The name value.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The qualified value.
    pub qualified: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The member value.
    pub member: Option<String>,
    /// The line value.
    pub line: usize,
    /// Always serialized, including when `null` (imports refs) -- NOT
    /// `skip_serializing_if`, unlike `qualified`/`member` which are omitted
    /// entirely when absent. See extract.rs's RefRecord doc comment.
    pub namespace: Option<String>,
    #[serde(
        default,
        rename = "typeArgCount",
        skip_serializing_if = "Option::is_none"
    )]
    /// The type arg count value.
    pub type_arg_count: Option<usize>,
    /// Type-certainty flag (see extract.rs's RefRecord). Serialized last and
    /// only when `true`; an absent key reads back as false, which also makes
    /// an older fragment JSON parse safely.
    #[serde(default, skip_serializing_if = "is_false")]
    pub generic: bool,
    /// Receiver fact (see extract.rs's RefRecord). Appended AFTER `generic`,
    /// and set only when a fact actually fired -- so it serializes last and
    /// only when present, and an absent key reads back as "no fact", the safe
    /// default.
    #[serde(
        default,
        rename = "receiverType",
        skip_serializing_if = "Option::is_none"
    )]
    pub receiver_type: Option<String>,
    /// The callee arg count (see extract.rs's RefRecord). Appended AFTER
    /// `receiverType`, set only when the member access was the callee of an
    /// invocation -- so it serializes last and only when present, and an
    /// absent key reads back as "not a call", which is what keeps a property
    /// read out of the extension tier.
    #[serde(default, rename = "argCount", skip_serializing_if = "Option::is_none")]
    pub arg_count: Option<usize>,
    /// Receiver generic-arg descriptors (see extract.rs's RefRecord). Appended
    /// LAST, after `argCount`, set only when the receiver's DECLARED type was
    /// generic -- an absent key reads back as "not generic", which is what
    /// makes a generic-vs-non-generic pairing fail to unify.
    #[serde(
        default,
        rename = "receiverArgs",
        skip_serializing_if = "Option::is_none"
    )]
    pub receiver_args: Option<Vec<String>>,
    /// Enclosing-type stack (see extract.rs's RefRecord). Appended LAST, after
    /// `receiverArgs`, and set only when non-empty. A Vec rather than an Option
    /// because empty and absent mean the same thing here -- a ref at namespace
    /// level and an older cached fragment both read back as "no enclosing
    /// type", which is what keeps them off the step.
    #[serde(default, rename = "outerTypes", skip_serializing_if = "Vec::is_empty")]
    pub outer_types: Vec<String>,
    /// Generic-arg descriptors for a 'ctor-param' ref (see extract.rs's
    /// RefRecord). Appended LAST of all, after `outerTypes`, and set only when
    /// the parameter's type was generic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// (See extract.rs's RefRecord.) The type whose PROPERTY the
    /// qualifier's last segment is, for a two-segment chain whose head the
    /// enclosing scope could type. Appended after `args`, and never present
    /// alongside `receiverType`.
    #[serde(
        default,
        rename = "receiverPropertyOwner",
        skip_serializing_if = "Option::is_none"
    )]
    pub receiver_property_owner: Option<String>,
    /// (See extract.rs's RefRecord.) The type whose METHOD a
    /// `var x = Q.M(...)` initializer called, and that method's name. Appended
    /// LAST of all, always as a pair, and never alongside `receiverType`: an
    /// absent pair reads back as "no call fact", which is what leaves the local
    /// taken-but-unknown.
    #[serde(
        default,
        rename = "receiverCallOwner",
        skip_serializing_if = "Option::is_none"
    )]
    pub receiver_call_owner: Option<String>,
    #[serde(
        default,
        rename = "receiverCallMember",
        skip_serializing_if = "Option::is_none"
    )]
    /// The receiver call member value.
    pub receiver_call_member: Option<String>,
    /// `true` for a `base.` qualifier -- the member lookup starts at the
    /// enclosing type's bases and never considers the enclosing type
    /// itself (see `extract.rs`'s `RefRecord`). Appended LAST of all, and
    /// omitted when `false` -- an absent key reads back as `false`, the
    /// same as a plain `this.` receiver and as every ref kind that never
    /// sets it, and also what makes a v15 cached fragment parse safely.
    #[serde(default, rename = "receiverBase", skip_serializing_if = "is_false")]
    pub receiver_base: bool,
    /// `true` when `receiverCallOwner`/`receiverCallMember` came from an
    /// AWAITED call (see `extract.rs`'s `RefRecord`). Appended LAST of all,
    /// after `receiverBase`, and omitted when `false` -- an absent key reads
    /// back as `false`, the same as an unawaited call fact and as every ref
    /// kind that never sets it, which is also what lets a cached fragment
    /// from before this field existed parse safely with no cache-version
    /// bump: it simply reads back as "not awaited", the same answer the
    /// resolver gave before this field existed.
    #[serde(default, rename = "receiverAwaited", skip_serializing_if = "is_false")]
    pub receiver_awaited: bool,
    /// `true` when this ref's qualifier is a bare identifier for which the
    /// enclosing MEMBER's own fact table (locals, parameters, lambda
    /// parameters, patterns, `out` designations) holds ANY entry for the
    /// name, typed or taken-but-unknown (see `extract.rs`'s `RefRecord` and
    /// `Scope::has_local_fact`). Read ONLY by the bare-identifier
    /// field/property fallback: a member-scoped name always shadows a
    /// same-named field, whether or not anything vouches for its type, so
    /// that fallback never runs when this is `true`. `false` for every
    /// dotted or generic qualifier, for `this.`/`base.` (never asked of the
    /// enclosing scope's local table at all), and for every ref kind but
    /// `uses-member`. Appended LAST of all, after `receiverAwaited`, and
    /// omitted when `false` -- an absent key reads back as `false`, the
    /// same as every ref kind that never sets it, which is also what lets a
    /// cached fragment from before this field existed parse safely with no
    /// cache-version bump.
    #[serde(default, rename = "receiverLocal", skip_serializing_if = "is_false")]
    pub receiver_local: bool,
    /// Set when this ref's qualifier is an untyped lambda parameter whose
    /// type is a delegate parameter of some OTHER callee (see extract.rs's
    /// `RefRecord`). Never present alongside `receiverType` or
    /// `receiverCallOwner`. Appended LAST of all, after `receiverLocal`,
    /// omitted when absent. Joined the schema with the v18 cache bump: a
    /// v17 fragment read back carries none, and every untyped-lambda
    /// callee-slot lookup this field backs would silently see no
    /// candidate.
    #[serde(
        default,
        rename = "receiverLambda",
        skip_serializing_if = "Option::is_none"
    )]
    pub receiver_lambda: Option<FragLambdaSlot>,
    /// `true` when `receiverType` came off a declaration whose own type
    /// node carried a `?` (see extract.rs's `RefRecord`). Appended LAST of
    /// all, after `receiverLambda`, and omitted when `false` -- an absent
    /// key reads back as `false`, the same as a non-nullable receiver and
    /// as every ref kind that never sets it. Joined the schema with the
    /// v20 cache bump: a v19 fragment read back carries none, so a
    /// `T?` receiver's own `Nullable<T>.Value`/`HasValue`/
    /// `GetValueOrDefault` unwrap would silently keep resolving against
    /// `T`'s own same-named member.
    #[serde(default, rename = "receiverNullable", skip_serializing_if = "is_false")]
    pub receiver_nullable: bool,
    /// The parameter count of each delegate-shaped argument of the
    /// invocation this ref is the callee of -- a lambda literal, or a
    /// local function passed as a method group (see extract.rs's
    /// `RefRecord`). Appended LAST of all, after `receiverNullable`,
    /// omitted when absent. Joined the schema with the v20 cache bump: a
    /// v19 fragment read back carries none, and a candidate whose delegate
    /// parameter shape the call's own argument cannot fill would silently
    /// keep earning a precise edge.
    #[serde(
        default,
        rename = "lambdaArgArity",
        skip_serializing_if = "Option::is_none"
    )]
    pub lambda_arg_arity: Option<Vec<Option<usize>>>,
}

pub(crate) fn is_false(b: &bool) -> bool {
    !*b
}

pub(crate) fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// One declared member, with the line its own NAME token sits on.
/// Field order (`name`, `kind`, `line`, `owner`) is significant, and `owner`
/// is omitted when empty -- which is how
/// a markup or resource key, owned by no C# type, serializes with three fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragName {
    /// The name value.
    pub name: String,
    /// The kind value.
    pub kind: String,
    /// The line value.
    pub line: usize,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    /// The owner value.
    pub owner: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Represents `Fragment`.
pub struct Fragment {
    /// The defs value.
    pub defs: Vec<FragDef>,
    /// The usings value.
    pub usings: Vec<FragUsing>,
    /// The refs value.
    pub refs: Vec<FragRef>,
    /// Appended LAST, after `refs`. Always serialized, like its
    /// three siblings: a fragment's top-level arrays are a fixed shape, and
    /// only the fields INSIDE a record follow the omit-when-empty rule.
    #[serde(default)]
    pub names: Vec<FragName>,
    /// The file's two-type-argument DI service registrations -- see
    /// `FragRegistration`. Appended LAST, after `names`, and omitted when
    /// empty (unlike its four siblings above, which pre-date the
    /// omit-when-empty convention): a file recording none serializes exactly
    /// as it did before this field existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub registrations: Vec<FragRegistration>,
}

/// The two shapes a cached fragment can have. Each rel is keyed to whichever
/// shape the file's grammar produced; the `ts: 1` tag is what tells them
/// apart, on disk and at the
/// resolver's door. Untagged, and TS FIRST: a C#/markup fragment carries no
/// `ts` key (so the TS arm always fails on it) and a TS fragment carries no
/// `usings` (so the C# arm always fails on it) -- the discrimination is total
/// in both directions, never a first-match-wins guess.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnyFragment {
    /// Represents `Ts`.
    Ts(extract::TsFragment),
    /// Represents `Cs`.
    Cs(Fragment),
}

impl From<Fragment> for AnyFragment {
    fn from(f: Fragment) -> Self {
        AnyFragment::Cs(f)
    }
}

impl From<extract::TsFragment> for AnyFragment {
    fn from(f: extract::TsFragment) -> Self {
        AnyFragment::Ts(f)
    }
}
