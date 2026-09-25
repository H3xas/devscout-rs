// ---------------------------------------------------------------------------
// Graph fragment extraction and its helpers. Scope: defs =
// namespace-level+nested types with public method names plus enum members;
// refs = using directives, base-list types,
// object-creation types, field/property/parameter/return types, generic
// type arguments, and member-access qualifier.member candidates. No
// call-graph resolution.
// ---------------------------------------------------------------------------

/// Represents `DefRecord`.
pub struct DefRecord {
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
    /// Declared property names, source order, deduped
    /// (indexers excluded by construction: `indexer_declaration` is a
    /// different grammar node). NO accessibility filter, deliberately
    /// asymmetric with `methods`: static/const/readonly/expression-bodied all
    /// count and only indexers are excluded, so a private member is still a
    /// member of the type. Empty for every
    /// enum-member def.
    pub properties: Vec<String>,
    /// Declared field names, source order, deduped, every
    /// declarator of every `field_declaration` ("private int a, b;"
    /// contributes both). `event_field_declaration` is a distinct node type
    /// and is NOT a field for this purpose. Same no-accessibility-filter
    /// rule as `properties`.
    pub fields: Vec<String>,
    /// (method name, declared return type NAME) pairs in
    /// FIRST-declaration source order, parallel to `methods` (same
    /// `is_recorded_method` predicate, so the two can never drift). The
    /// first declaration of a name claims the slot outright: a later
    /// overload with a different return type is ignored, and a first
    /// declaration whose return type yields no fact (void, var, a
    /// predefined type) BLOCKS the name rather than letting a later
    /// overload stand in for it. A Vec of pairs, not a map: the serialized
    /// key order is significant (see graph.rs's `FragDef`).
    pub method_returns: Vec<(String, String)>,
    /// The extension methods this type declares: every method
    /// whose FIRST parameter carries the `this` modifier, in source order,
    /// deduped by (name, thisType, arityMin, arityMax). Appended LAST, after
    /// `method_returns`, and omitted at serialization when empty. Unlike
    /// `methods` there is NO accessibility filter (`internal static class
    /// FooExtensions` is the shape this feature exists for) and no `static`
    /// check on either the method or its class -- C# already disallows a
    /// `this` parameter anywhere else, so the parameter modifier IS the
    /// discriminator.
    pub extension_methods: Vec<ExtensionMethod>,
    /// The DIRECT base-type identifiers this
    /// declaration lists, in source order, deduped. Same `base_list` traversal
    /// `record_base_list` walks for its `inherits` refs, reduced to a base
    /// IDENTIFIER because the resolver re-RESOLVES these names through the
    /// ordinary ladder rather than matching them; the resulting closure is
    /// what lets the extension tier see an inherited instance member and
    /// decline. Appended after `extension_methods` and omitted when empty.
    pub bases: Vec<String>,
    /// The declaring type's OWN type-parameter names (`class
    /// MongoRepository<T>` records `["T"]`), empty for every non-generic
    /// declaration. This is the ctor-DI resolver's "is this def itself an
    /// open-generic implementation" signal. Appended after `bases`, omitted
    /// when empty.
    pub type_params: Vec<String>,
    /// Per base name that carried a type-argument list, that
    /// list's generic-arg descriptors relative to `type_params` (a `"*"`
    /// wildcard marks a position that is a pass-through of the declaring
    /// type's own parameter). A `Vec` of pairs, not a map, for the same
    /// reason `method_returns` is: the serialized key order is significant.
    /// Appended after `type_params`, omitted when empty; a base
    /// with no type-argument list at all contributes no entry.
    pub base_generic_args: Vec<(String, Vec<String>)>,
    /// The methods this type declares that a test
    /// framework would DISCOVER as tests, in source order, deduped. Appended
    /// LAST, after `base_generic_args`, and omitted at serialization when
    /// empty. The attribute IS the fact, which is why this is the one member
    /// fact with no accessibility filter at all and why no file, folder or
    /// type-name convention is read anywhere.
    pub test_methods: Vec<String>,
    /// (property name, declared type fact) pairs for exactly the
    /// properties `properties` records, in the same source order and under the
    /// same dedup. A property whose declared type yields no fact (a predefined
    /// type) has no entry, so the two lists are parallel but not equal in
    /// length. A `Vec` of pairs, not a map, for the same reason
    /// `method_returns` is one: the serialized key order is significant.
    /// Appended LAST, after `test_methods`.
    pub property_types: Vec<(String, Fact)>,
    /// (field name, declared type fact) pairs for exactly the
    /// fields `fields` records, in the same source order and under the same
    /// dedup. A field whose declared type yields no fact (a predefined type)
    /// has no entry, so the two lists are parallel but not equal in length.
    /// Every declarator of one `field_declaration` shares that declaration's
    /// own type node, so `private int a, b;` gives `a` and `b` the SAME
    /// fact -- the same declarator sharing `fields`'s own collection loop
    /// already relies on. A `Vec` of pairs, not a map, for the same reason
    /// `property_types` is one: the serialized key order is significant.
    /// Appended LAST, after `property_types`.
    pub field_types: Vec<(String, Fact)>,
    /// Per method name that `method_returns` also records an
    /// entry for, that return type's top-level generic-arg descriptors --
    /// the same capture `base_generic_args` keeps beside `bases`, for the
    /// same reason: `method_returns` records only the return type's bare
    /// identifier ("Task" for `Task<Order> GetAsync()`), and this is what
    /// lets the resolver unwrap ONE `Task<...>`/`ValueTask<...>` layer off
    /// an AWAITED call's callee without ever touching `method_returns`
    /// itself, which an unawaited use of the very same callee reads
    /// unchanged. A `Vec` of pairs, not a map, for the same reason
    /// `method_returns` is one: the serialized key order is significant.
    /// Appended LAST of all, after `field_types`; a method whose return
    /// type carries no type-argument list at all contributes no entry.
    pub method_return_args: Vec<(String, Vec<String>)>,
    /// Declared METHOD names this type does NOT record in `methods`,
    /// because `is_recorded_method` gates that list on a literal `public`
    /// modifier (or `kind == "interface"`, where every method already
    /// counts as public and this list is always empty). Same predicate,
    /// same `method_declaration` node kind, same source order and dedup as
    /// `methods` -- the two lists partition a type's method declarations
    /// with no overlap and no gap. Unlike `properties`/`fields`, which
    /// already carry every accessibility with no filter at all, `methods`
    /// needed a counterpart list for a non-public method (protected,
    /// internal, private or no-modifier/private-by-default) to be
    /// recorded anywhere. Consulted by the resolver ONLY for
    /// hierarchy-internal receivers -- a `base.` lookup and the `this.`
    /// shape's own typed-receiver walk -- never by the scored tier's
    /// vouching or veto, which keep reading `methods` alone. Appended
    /// LAST of all, after `method_return_args`.
    pub non_public_methods: Vec<String>,
    /// Method name -> the (min, max) argument-count RANGE every overload
    /// sharing that name accepts, one tuple per overload, in declaration
    /// order -- covers EVERY `method_declaration`, public and non-public
    /// alike (unlike `methods`/`non_public_methods`, which only say a name
    /// exists, this says what a CALL of that name needs to look like).
    /// `min` excludes a parameter carrying a default value or the `params`
    /// modifier; `max` is the total parameter count, or -1 (unbounded) when
    /// the trailing parameter is a `params` array -- the same sentinel
    /// `ExtensionMethod::arity_max` already uses. A `Vec` of pairs, not a
    /// map, for the same reason `method_returns` is one: the serialized key
    /// order is significant. Consulted by the resolver's arity-aware call
    /// vouching: a ref carrying an `argCount` binds to `methods`/
    /// `non_public_methods` only when SOME overload's range admits it: a
    /// same-named instance member at the wrong arity does not shadow the
    /// extension tier. A read (no `argCount`) never consults this table.
    /// Appended LAST of all, after `non_public_methods`.
    pub method_arities: Vec<(String, Vec<(usize, i64)>)>,
    /// Method name -> per-overload parameter type descriptors (see
    /// `type_descriptor`), one `Vec<String>` per overload in declaration
    /// order -- covers EVERY `method_declaration`, public and non-public
    /// alike, same method set as `method_arities`. A position naming the
    /// enclosing method's or type's own type parameter is recorded as `"*"`
    /// (wildcard); a parameter carrying the `this` modifier is written
    /// `"this <descriptor>"` (the extension-method marker). For a
    /// `delegate` def this carries exactly one entry, keyed `"Invoke"`,
    /// built from the delegate's own parameter list (a delegate has no
    /// body, so it is not one of the `method_declaration` overloads this
    /// otherwise iterates). A `Vec` of pairs, not a map, for the same
    /// reason `method_returns` is one: the serialized key order is
    /// significant. Appended LAST of all, after `method_arities`.
    pub method_params: Vec<(String, Vec<Vec<String>>)>,
    /// Declared method names carrying the literal `override` modifier,
    /// source order, deduped. Appended LAST of all, after `method_params`.
    /// Consulted only by the resolver's member-level `overrides` pass,
    /// itself run only for a type the resolver already knows is a
    /// registered DI implementation -- see `graph.rs`'s `FragDef`.
    pub override_methods: Vec<String>,
    /// Per base name that carries a top-level type argument which is
    /// ITSELF generic, that argument list rendered by `type_descriptor`
    /// (nested structure intact) rather than the flattened bare
    /// identifiers `base_generic_args` carries -- `IConsumer<Batch<
    /// LoanRequested>>` records `[("IConsumer", ["Batch<LoanRequested>"])]`
    /// here, so the inner message type survives extraction instead of
    /// collapsing to `Batch`. A base whose arguments are all non-generic
    /// contributes no entry: `base_generic_args` already carries that
    /// shape, so a second copy would add no fact. Appended LAST of all,
    /// after `override_methods`.
    pub base_type_args: Vec<(String, Vec<String>)>,
    /// Every distinct single type argument the type's own properties wrap
    /// (`Event<LoanRequested>` records `LoanRequested`), in declaration
    /// order. A long-running handler names the further messages it binds
    /// this way rather than on its base list, so the base list alone would
    /// miss them. Recorded for every type; the resolver keeps these only
    /// for a type it already holds to be a handler. Appended LAST of all,
    /// after `base_type_args`.
    pub property_message_args: Vec<String>,
    /// Base names (a subset of `bases`) whose message-position argument --
    /// `base_generic_args`' own position 0 -- is itself an array type
    /// (`IConsumer<M[]>`). `base_generic_args`/`generic_arg_descriptors`
    /// both already look THROUGH an `array_type` to its element (the same
    /// unwrapping `base_type_identifier` performs generally), so this is
    /// the only place left that still knows the wrapper was there at all --
    /// the lane-owned sibling of the array bit `extract::receivers::type_fact`
    /// keeps for a declared local/parameter. A base whose nested argument
    /// already carries its own array suffix through `base_type_args`
    /// (`IConsumer<Batch<M[]>>`) needs no entry here: that reading is
    /// structural, off the rendered descriptor, and never loses the
    /// bracket. Appended LAST of all, after `property_message_args`.
    pub array_message_bases: Vec<String>,
    /// 1-based last line of the complete declaration node.
    pub end_line: usize,
}

/// One message-bus publish-site fact: an invocation handing a message
/// instance to a bus, mediator or scheduler.
///
/// `message` is a RESOLVED type descriptor -- read off a generic type
/// argument, a constructed type, or a declared local/parameter type
/// through the ordinary scope ladder, never a bare identifier merely
/// co-located on the same source line as the call. See `extract/bus.rs`.
#[derive(Debug, Clone, PartialEq)]
pub struct PublishRecord {
    /// The invoked method name (`Publish`, `PublishAsync`, `SubmitJob`,
    /// `Reply`, `Send`).
    pub verb: String,
    /// The resolved message-type descriptor the call hands off.
    pub message: String,
    /// The call's enclosing namespace.
    pub namespace: String,
    /// The call's 1-based line -- the SAME line no matter how many source
    /// lines the call itself spans, so a split call still records exactly
    /// one site.
    pub line: usize,
    /// The walk's own `type_stack` (see `RefRecord::outer_types`).
    pub outer_types: Vec<String>,
    /// The name of the method this call sits in, recorded ONLY when the
    /// message is an unbound type parameter. That pairing is the signature
    /// of a forwarding wrapper -- a method that hands its own caller's
    /// message on to a bus -- and the method name is what the resolver
    /// needs to recognize this repository's own calls to it. An ordinary
    /// publish site names a concrete message and records nothing here.
    /// Appended LAST, after `outer_types`.
    pub enclosing_method: Option<String>,
    /// The call's own argument count -- see `extract::refs::invocation_arg_count`.
    /// Appended LAST, after `enclosing_method`.
    pub arg_count: usize,
    /// Set when the call sits inside a lambda body that is itself an
    /// argument of an ENCLOSING invocation (a test-double setup/verify
    /// lambda, or an ordinary delegate lambda -- this fact alone does not
    /// distinguish the two; see `extract::bus::enclosing_call_through_lambda`).
    /// Appended LAST, after `arg_count`.
    pub enclosing_call: Option<EnclosingCallFact>,
}

/// The enclosing invocation a publish call's own lambda sits inside, when
/// it sits inside one at all. See `PublishRecord::enclosing_call`.
#[derive(Debug, Clone, PartialEq)]
pub struct EnclosingCallFact {
    /// The enclosing invocation's own callee bare name.
    pub verb: String,
    /// The 0-based position the lambda occupies among the enclosing
    /// invocation's arguments.
    pub arg_position: usize,
    /// The enclosing invocation's own total argument count.
    pub arg_count: usize,
}

/// One `extensionMethods` entry. Field order (`name`, `thisType`, `arityMin`,
/// `arityMax`, `thisArgs`) is significant: it fixes the serialized field
/// order.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionMethod {
    /// The name value.
    pub name: String,
    /// The this type value.
    pub this_type: String,
    /// Non-this parameters a caller cannot
    /// leave out: no default value AND no `params` modifier.
    pub arity_min: usize,
    /// Total non-this parameter count, or -1 when the trailing parameter is a
    /// `params` array (unbounded above). Signed precisely so -1 can be the
    /// sentinel written as a JSON number.
    pub arity_max: i64,
    /// The this-parameter type's TOP-LEVEL
    /// type-argument descriptors, present only when that type is generic.
    /// A position naming one of the method's or the enclosing class's own type
    /// parameters is recorded as "*" (wildcard).
    pub this_args: Option<Vec<String>>,
}

/// Represents `RefRecord`.
pub struct RefRecord {
    /// The kind value.
    pub kind: String,
    /// The name value.
    pub name: String,
    /// The qualified value.
    pub qualified: Option<String>,
    /// The member value.
    pub member: Option<String>,
    /// The line value.
    pub line: usize,
    /// `None` only for 'imports' refs -- a using directive is not
    /// namespace-scoped, so `ns` is deliberately `null`. Every other ref
    /// carries `Some(ns)`, where `ns`
    /// may itself be the empty string at file scope.
    pub namespace: Option<String>,
    /// The type arg count value.
    pub type_arg_count: Option<usize>,
    /// `true` when a uses-member qualifier carried a
    /// type-argument list anywhere (`Cache<T>.x`, `Ns.Cache<T>.x`): syntax
    /// only a TYPE can carry, so the resolver treats it as type-certainty
    /// for non-enum member emission. Always `false` for every other ref
    /// kind.
    pub generic: bool,
    /// The declared type NAME of the receiver a uses-member
    /// qualifier names, when the enclosing scope holds exactly one fact for
    /// it (a declared local, a `var x = new T()` local, a parameter, a
    /// field, or a primary-constructor parameter). Set ONLY for a BARE,
    /// non-generic qualifier -- see the `member_access_expression` arm's
    /// guard, which is what makes the chain-tail hazard structurally
    /// impossible. Always `None` for every other ref kind.
    pub receiver_type: Option<String>,
    /// The argument count of the call this
    /// member access is the CALLEE of (`argument_list` named-child count),
    /// recorded only when the access is the `function` field of an
    /// `invocation_expression`. A property read (`x.P`) carries `None`, which
    /// is what keeps it out of the arity-matched extension tier entirely.
    /// Appended AFTER `receiver_type`. Always `None` for every other ref kind.
    pub arg_count: Option<usize>,
    /// The DECLARED receiver type's top-level
    /// type-argument descriptors, present only when that type is generic. A
    /// position naming a type parameter of the enclosing method or class is
    /// recorded as "*" (wildcard): nothing at the fact site knows its binding.
    /// Travels with `receiver_type` -- the fact carries both or neither.
    /// Appended LAST, after `arg_count`.
    pub receiver_args: Option<Vec<String>>,
    /// The walk's own `type_stack` verbatim: the enclosing type simple
    /// names, OUTERMOST first, the same order `type_id` joins with "+" to build
    /// a nested def id. Empty at namespace level, and empty is exactly what an
    /// absent key deserializes to. Appended LAST, after `receiver_args`.
    pub outer_types: Vec<String>,
    /// Generic-arg descriptors for a 'ctor-param' ref's
    /// parameter type (same descriptor shape as `receiver_args`: a `"*"`
    /// wildcard for a pass-through of the enclosing type's own type
    /// parameter), present only when that type is generic. `None` for every
    /// other ref kind. Appended LAST of all, after `outer_types` -- 'ctor-param'
    /// is a ref kind no other caller touches, so putting it after that
    /// "last of all" field costs no other ref kind a single byte.
    pub args: Option<Vec<String>>,
    /// The type whose PROPERTY the qualifier's last segment is, for
    /// a two-segment chain whose head the enclosing scope has a fact for
    /// ("a.Settings" in `a.Settings.Reload()`). Never travels with
    /// `receiver_type`: that one is bare-qualifier-only and this one is
    /// dotted-qualifier-only, which is what keeps the chain-tail hazard
    /// structurally impossible -- the resolver reaches the tail's type through
    /// the head type's RECORDED property types, never by inheriting the head's
    /// own. Appended after `args`.
    pub receiver_property_owner: Option<String>,
    /// The type whose METHOD a `var x = Q.M(...)` initializer
    /// called, and that method's name. Always set as a pair, and never
    /// alongside `receiver_type`: the local's type is whatever `M` returns, a
    /// lookup only the resolver can do. Appended LAST of all.
    pub receiver_call_owner: Option<String>,
    /// The receiver call member value.
    pub receiver_call_member: Option<String>,
    /// `true` for a `base.M` qualifier -- the member lookup starts at the
    /// enclosing type's bases and never considers the enclosing type
    /// itself. `false` for every other ref kind, including a plain `this.M`
    /// qualifier (which resolves through the ordinary typed-receiver path
    /// against the enclosing type itself). Appended LAST of all.
    pub receiver_base: bool,
    /// `true` when this ref's `receiver_call_owner`/`receiver_call_member`
    /// pair came from an AWAITED call (`var x = await Q.M()`, `_client` a
    /// field: `var order = await _client.FetchAsync()`) -- the resolver reads
    /// it to decide whether to unwrap exactly one `Task<...>`/`ValueTask<...>`
    /// layer off `M`'s recorded return type before typing the local. `false`
    /// for an unawaited call fact and for every ref carrying no call fact at
    /// all, which is what keeps `var t = x.FetchAsync(); t.Wait();` typed as
    /// `Task` rather than unwrapped. Appended LAST of all, after
    /// `receiver_base`.
    pub receiver_awaited: bool,
    /// `true` when this ref's qualifier is a bare identifier for which the
    /// enclosing MEMBER's own fact table -- `Scope::has_local_fact`, the
    /// SAME `member_facts` table `receiver_fact_for` reads first -- holds
    /// ANY entry for the name: a local, a parameter, an explicitly-typed
    /// lambda parameter, a pattern designation, or an `out` designation,
    /// typed or taken-but-unknown. A member-scoped name always shadows a
    /// same-named field regardless of whether anything vouches for its
    /// type, so the resolver's bare-identifier field/property fallback
    /// (Unit B) reads this to refuse running at all when it is `true` --
    /// the one shape a resolved `receiver_type`/`receiver_call_owner` of
    /// `None` cannot itself distinguish from "no fact anywhere for this
    /// name". `false` for a dotted or generic qualifier, for `this.`/
    /// `base.` (never asked of the enclosing scope's local table), and for
    /// every ref kind but `uses-member`. Appended LAST of all, after
    /// `receiver_awaited`.
    pub receiver_local: bool,
    /// Set when this ref's qualifier is an untyped lambda parameter whose
    /// type is a delegate parameter of some OTHER callee -- a lookup only
    /// the resolver can do. Never set alongside `receiver_type` or
    /// `receiver_call_owner`: the three are mutually exclusive on one ref.
    /// `receiver_local` still reads `true` for such a ref (the parameter
    /// IS a member-scoped name), and `receiver_type` stays `None`. Appended
    /// LAST of all, after `receiver_local`.
    pub receiver_lambda: Option<LambdaSlot>,
    /// `true` when `receiver_type` came off a declaration whose own type
    /// node was a `nullable_type` (`Widget? w`) -- see `Fact::nullable`,
    /// which this carries verbatim off the SAME receiver fact
    /// `receiver_type`/`receiver_args` already came off. `false` for every
    /// ref kind, and for a `receiver_type` that came from anywhere but a
    /// plain declared-type fact (a call-hop return, a lambda-slot type, a
    /// bare field/property fallback: none of those can be nullable-
    /// annotated, since none of them read a type node directly). Appended
    /// LAST of all, after `receiver_lambda`.
    pub receiver_nullable: bool,
    /// The parameter count of each delegate-shaped argument of the
    /// invocation this ref is the callee of -- a lambda literal, or a bare
    /// identifier naming one unshadowed local function in scope -- one
    /// entry per argument position (`None` at every other position), or
    /// `None` entirely when the ref is not invocation-shaped or names no
    /// such argument at all. `x => ...` counts 1 parameter, `(a, b) => ...`
    /// counts 2, `() => ...` counts 0, a local function counts its own
    /// declared parameters -- the argument's OWN arity, never the delegate
    /// type it will end up bound to (that binding is exactly what this fact
    /// lets the resolver judge, rather than assume). Appended LAST of all,
    /// after `receiver_nullable`.
    pub lambda_arg_arity: Option<Vec<Option<usize>>>,
}

/// Represents `UsingRecord`.
pub enum UsingRecord {
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

/// Represents `Extraction`.
pub struct Extraction {
    /// The purpose value.
    pub purpose: Option<String>,
    /// The defs value.
    pub defs: Vec<DefRecord>,
    /// The usings value.
    pub usings: Vec<UsingRecord>,
    /// The refs value.
    pub refs: Vec<RefRecord>,
    /// Every member the file's types declare, appended LAST after
    /// `refs` in both the fragment and the `extract-dump` shape.
    pub names: Vec<NameRecord>,
    /// Every two-type-argument DI service registration the file's
    /// invocations record, appended LAST after `names`. See
    /// `RegistrationRecord`.
    pub registrations: Vec<RegistrationRecord>,
    /// Every message-bus publish-site fact the file's invocations record,
    /// appended LAST after `registrations`. See `PublishRecord`.
    pub publishes: Vec<PublishRecord>,
    /// Every one-type-argument handler registration the file's invocations
    /// record, appended LAST after `publishes`. See
    /// `HandlerRegistrationRecord`.
    pub handler_registrations: Vec<HandlerRegistrationRecord>,
}

/// One one-type-argument handler registration: an invocation whose name
/// reads as an installation and which names exactly one type.
///
/// This is the fact a repository's own message vocabulary is derived from.
/// The named type's base list says which generic base that repository
/// treats as a handler shape and at which position it carries its message,
/// so a bus whose type names appear nowhere in this engine is still
/// recognized from the repository's own registrations. `handler` carries
/// the type argument's descriptor as written; resolving it is the
/// resolver's job, not the extractor's.
#[derive(Debug, Clone, PartialEq)]
pub struct HandlerRegistrationRecord {
    /// The registered type's descriptor, as written.
    pub handler: String,
    /// The registration call's enclosing namespace.
    pub namespace: String,
    /// The registration call's 1-based line.
    pub line: usize,
}

/// One two-type-argument DI service registration.
///
/// The shape is an invocation whose method name begins `Add` or `TryAdd`
/// and ends `Singleton`, `Scoped` or `Transient`, carrying exactly two type
/// arguments -- the first the service type, the second the implementation
/// type. `service`/`implementation` carry the type argument's raw text
/// verbatim (dotted when the source wrote it qualified); splitting a dotted
/// name into its bare tail plus its full qualified form, the way an
/// ordinary type reference does, is the resolver's job, not the
/// extractor's.
#[derive(Debug, Clone, PartialEq)]
pub struct RegistrationRecord {
    /// The service (first type argument) name, as written.
    pub service: String,
    /// The implementation (second type argument) name, as written.
    pub implementation: String,
    /// The registration call's enclosing namespace.
    pub namespace: String,
    /// The registration call's 1-based line.
    pub line: usize,
}

/// One declared name and the line its own NAME TOKEN sits on. Deliberately
/// not the declaration node's start row: a member's span begins
/// at its attribute list, so an attributed member would point a reader at the
/// `[` line instead of the line carrying the name they searched for.
///
/// Serialized field order (`name`, `kind`, `line`, `owner`) is significant;
/// `owner` is omitted when empty, which is how a markup or resource
/// key -- built by `markup.rs`, owned by no C# type -- serializes with three
/// fields.
#[derive(Debug, Clone, PartialEq)]
pub struct NameRecord {
    /// The name value.
    pub name: String,
    /// The kind value.
    pub kind: String,
    /// The line value.
    pub line: usize,
    /// The owner value.
    pub owner: String,
}

/// One untyped lambda parameter's callee slot.
///
/// The name's type is the delegate parameter of the invocation the lambda
/// sits inside as an argument -- a lookup only the resolver can do (it has
/// to look up the callee's own delegate-typed parameter at `arg_index` and
/// read ITS `index`-th parameter type). `owner`/`member` name the callee like
/// a call fact's `type_name`/`call` do; `arg_count`/`arg_index` locate the
/// lambda among the callee's arguments; `arity`/`index` locate this
/// parameter inside the lambda itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LambdaSlot {
    /// The type-name text the callee is invoked on: the identifier's
    /// in-file fact type, the identifier itself when the name is not taken
    /// (a static class), or the innermost enclosing type for `this.M(...)`
    /// / bare `M(...)`.
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

/// One receiver fact: the declared type NAME plus that type's top-level
/// type-argument descriptors when it carried any. Two facts agree only when
/// BOTH halves do, so `Box<int> a` and `Box<string> a` in sibling blocks
/// conflict exactly like two different type names would.
#[derive(Clone, PartialEq, Eq)]
pub struct Fact {
    /// The type name value.
    pub type_name: String,
    /// The args value.
    pub args: Option<Vec<String>>,
    /// When set, the name's type is whatever the method of this name returns
    /// on the type `type_name` stands for, a lookup only the resolver can do.
    /// Part of the equality the table compares, so `var x = A.Make()` and
    /// `var x = A.Build()` in sibling blocks conflict exactly like two
    /// different type names would.
    pub call: Option<String>,
    /// `true` when this is a call fact (`call.is_some()`) built from an
    /// AWAITED invocation (`var x = await Q.M()`); meaningless -- and always
    /// `false` -- when `call` is `None`. Part of the equality the table
    /// compares, so `var x = Q.M()` and `var x = await Q.M()` in sibling
    /// blocks conflict exactly like two different callees would, rather than
    /// silently picking one awaited-ness for a name the source gives two.
    pub awaited: bool,
    /// `true` when the declaration's own type node was, at its TOP level, an
    /// `array_type` (`Widget[]`) -- set ONLY by `type_fact`, `false`
    /// everywhere else (a call fact, a receiver-args unwrap, `this`/`base`'s
    /// own fact: none of those can be an array). `base_type_identifier`
    /// already collapses `Widget[]` to the same `type_name`/`args` shape a
    /// plain `Widget` field would carry (the element type's own base
    /// identifier, no type-argument list), so this bit is the ONLY signal
    /// left that distinguishes "one Widget" from "an array of Widget" --
    /// needed by the lambda-parameter element-typing rule, which must tell
    /// `Widget[]` apart from `Widget` even though both facts otherwise read
    /// identically. Part of the equality the table compares.
    pub is_array: bool,
    /// `true` when the declaration's own type node was, at its TOP level, a
    /// `nullable_type` (`Widget?`) -- set ONLY by `type_fact`, `false`
    /// everywhere else, mirroring `is_array`: a call fact, a receiver-args
    /// unwrap and `this`/`base`'s own fact are never nullable-annotated.
    /// `base_type_identifier` already unwraps `Widget?` to the same
    /// `type_name` a plain `Widget` field would carry, so this bit is the
    /// only signal left that a `System.Nullable<T>` sits between the
    /// declared name and the value -- needed so the resolver can refuse a
    /// member the wrapper itself owns (`.Value`, `.HasValue`,
    /// `.GetValueOrDefault`) rather than binding `T`'s own same-named
    /// member. Part of the equality the table compares.
    pub nullable: bool,
    /// When set, the name is an untyped lambda parameter whose type is the
    /// corresponding delegate parameter of the callee this slot names -- a
    /// lookup only the resolver can do; `type_name` is empty and `call` is
    /// `None`. Part of the equality the table compares, so two sibling
    /// lambdas binding the same parameter name to different callees
    /// conflict exactly like two different type names would.
    pub lambda: Option<LambdaSlot>,
}
