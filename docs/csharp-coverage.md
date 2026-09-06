# C# construct catalogue

Every C# construct the extractor meets, with a verdict on whether it must produce a graph
fact, and what actually happens today. The catalogue is pinned by
[`fixtures/csharp-syntax/`](../fixtures/csharp-syntax/) (one hand-written, compilable file per
construct group) and [`tests/csharp_syntax_matrix.rs`](../tests/csharp_syntax_matrix.rs), which
fails when a fixture stops parsing clean, when a `must` row that produces today stops producing,
or when the fixture set and this document drift apart.

## Grammar pin

| Crate | Version |
|---|---|
| `tree-sitter-c-sharp` | 0.23.5 |
| `tree-sitter` | 0.27.0 |

The grammar parses every fixture file without an `ERROR` or `MISSING` node except
`GrammarGaps.cs`, which deliberately holds the constructs it cannot parse (two `ERROR` nodes,
pinned by the test's `GRAMMAR_GAPS` allowlist). Gaps found by probing the grammar directly:

| Construct | Language version | What the grammar does | Fixture |
|---|---|---|---|
| `extension` blocks (`static class E { extension(string s) { ... } }`) | C# 14 | No node kind; the block is recovered as `ERROR` | none: SDK 9 cannot compile it |
| `where T : allows ref struct` | C# 13 | `ERROR` inside the constraints clause; the method still parses | `GrammarGaps.cs` |
| List pattern with a slice designation `[.. var rest]` | C# 11 | `ERROR` inside `list_pattern`; a bare slice `[_, ..]` parses | `GrammarGaps.cs` |

Everything else probed parses clean, including the C# 12/13 surface: primary constructors on
classes and structs, collection expressions and spreads, `params ReadOnlySpan<T>`, the `field`
keyword, partial properties, `ref` fields, `scoped`, `file`-local types, `required`/`init`,
inline arrays, raw and u8 strings, lambda default parameters and return types, generic
attributes, static abstract and virtual interface members, and every preprocessor directive.

**Grammar bump checklist.** After changing either crate version: run
`cargo test --test csharp_syntax_matrix`; if `GrammarGaps.cs` reports fewer defects, move the
newly parsed construct out of that file into its construct group, drop it from the gaps table,
and update `GRAMMAR_GAPS`; if any other file reports a defect, the bump regressed the grammar
and the construct joins the gaps table instead. Then re-run the fixture's `dotnet build` (the
fixture must keep compiling) and re-measure every row whose node kind changed.

## Reading the tables

- **Node kind(s)**: the `tree-sitter-c-sharp` node kinds the construct produces.
- **Verdict**: what the syntax alone says, independent of meaning. `def` declares a symbol;
  `scope` opens or aliases a namespace; `type-ref` names a type; `member-ref` names a member on
  a receiver; `receiver-type` types a local, parameter, or loop variable so later member
  references on it resolve; `none` does none of those.
- **Obligation**: `must` — the extractor must produce the fact and the pin test asserts it
  wherever it produces today; `may` — useful but not load-bearing for `refs`/`impact`, never
  asserted; `must-not` — the construct must produce no fact of its own (comments, string
  bodies, dead preprocessor branches).
- **Base**: measured by running `devscout init` over the fixture and reading `graph.json`.
  `produces` — every shape in the row yields its fact; `partial` — some shapes do, or the fact
  reaches the graph only through the scored `guess` tier (receiver untyped); `silent` — no
  fact; `leaks` — a `must-not` row produced a fact; `n-a` — no fact expected and none produced,
  or no fixture.
- **Consumer**: the resolver step that reads the fact. `def index` (declarations, arity,
  partial merge), `type ladder` (qualified, alias, namespace walk for `uses-type`/`inherits`),
  `precise member` (typed receiver → declared member), `ext` (extension bucket), `guess`
  (scored name match), `ctor-di` (constructor seams), `imports`.
- **Pin**: `asserted` when the pin test checks the row; `follow-up` when obligation and base
  disagree and the row waits for extractor work; `—` when nothing is asserted by design.

## Declarations

| Construct | Node kind(s) | Verdict | Obligation | Base | Consumer | Fixture | Pin |
|---|---|---|---|---|---|---|---|
| Block namespace, dotted block namespace, `using` inside a block | `namespace_declaration` | scope | must | produces | def index, imports | `NamespaceBlock.cs` | asserted |
| File-scoped namespace | `file_scoped_namespace_declaration` | scope | must | produces | def index | `NamespaceFileScoped.cs` | asserted |
| `class`, incl. `abstract`, `sealed`, `static`, `file` | `class_declaration`, `modifier` | def | must | produces (every modifier lands as kind `class`) | def index | `TypeKinds.cs` | asserted |
| `struct`, `readonly struct`, `ref struct` | `struct_declaration` | def | must | produces (kind `struct`) | def index | `TypeKinds.cs` | asserted |
| `record`, `record class` | `record_declaration` | def | must | produces (kind `record`) | def index | `TypeKinds.cs` | asserted |
| `record struct`, `readonly record struct` | `record_declaration` | def | must | produces (kind `record`; the value-type distinction is not in the graph) | def index | `TypeKinds.cs` | asserted |
| `interface` | `interface_declaration` | def | must | produces | def index | `TypeKinds.cs` | asserted |
| `enum` with members, explicit values, base type, `[Flags]` | `enum_declaration`, `enum_member_declaration` | def | must | produces (`Ns.Enum.Member` ids) | def index | `TypeKinds.cs` | asserted |
| `delegate` | `delegate_declaration` | def | must | produces | def index | `TypeKinds.cs` | asserted |
| Nested types of every kind, multi-level nesting | `declaration_list` > `*_declaration` | def | must | produces (`Outer+Inner+Deep` ids) | def index | `NestedTypes.cs` | asserted |
| Nested type named through its outer type (`Outer.Inner` annotation, `new Outer.A.B()`, `Outer.Enum.Member`) | `qualified_name` | type-ref | must | partial: field annotations, `new`, and enum members produce; a qualified nested name as a local's annotation yields nothing, and a member read on such a local is a guess | type ladder | `NestedTypes.cs` | asserted (working shape); follow-up |
| Partial type across files (class, struct, interface, record) | `modifier` `partial` | def | must | produces (one def, `also_in` lists the other site, member lists merged) | def index | `PartialTypesA.cs`, `PartialTypesB.cs` | asserted |
| Partial method (declaring and implementing parts) | `method_declaration` | def | must | produces (one name per part) | def index | `PartialTypesA.cs`, `PartialTypesB.cs` | asserted |
| Partial property | `property_declaration` | def | must | produces (one name per part) | def index | `PartialTypesA.cs`, `PartialTypesB.cs` | asserted |
| Generic type parameters; same name at two arities | `type_parameter_list` | def | must | produces (both arities are defs; the id carries no arity, the def index keys on it) | def index | `Generics.cs` | asserted |
| Generic constraints (`class`, `new()`, `unmanaged`, `notnull`, self-referential, `Enum`) | `type_parameter_constraints_clause` | type-ref | may | silent | — | `Generics.cs` | — |
| Generic method with explicit and inferred type arguments at the call | `method_declaration`, `type_argument_list` | def, member-ref | must | produces | precise member | `Generics.cs` | asserted |
| Variance annotations (`in`, `out`) | `type_parameter` | none | may | n-a | — | `Generics.cs` | — |
| Record primary constructor (parameters as properties) | `record_declaration` > `parameter_list` | def | must | silent: the record is a def, its parameters never become property names, so `record.Age` resolves to nothing | def index | `PrimaryConstructors.cs` | follow-up |
| Class and struct primary constructor, captured parameter | `class_declaration` > `parameter_list` | def, receiver-type | must | produces (the captured parameter types its member accesses) | precise member | `PrimaryConstructors.cs` | asserted |
| Primary constructor base call `: Base(args)` | `primary_constructor_base_type` | type-ref | must | produces (`inherits`) | type ladder | `PrimaryConstructors.cs` | asserted |
| Static class and its static members | `modifier` `static` | def | must | produces | def index, precise member | `StaticAndExtension.cs` | asserted |
| Extension method declaration (`this` parameter) | `method_declaration` > `parameter` > `modifier` `this` | def | must | produces | def index, ext | `StaticAndExtension.cs` | asserted |
| Extension call on a typed receiver | `invocation_expression` > `member_access_expression` | member-ref | must | partial: a local typed by `new` binds through the `ext` tier; a literal or a `new T()` receiver yields nothing, an array-literal-typed local only a guess | ext | `StaticAndExtension.cs` | asserted (working shape); follow-up |
| Extension method called as a static method | `invocation_expression` | member-ref | must | produces | precise member | `StaticAndExtension.cs` | asserted |
| Fields: instance, static, readonly, const, volatile, multiple declarators | `field_declaration`, `variable_declarator` | def | must | produces (one name per declarator) | def index | `Members.cs` | asserted |
| Properties: auto, backed, get-only, expression-bodied, private set | `property_declaration` | def | must | produces | def index | `Members.cs` | asserted |
| `init` and `required` properties | `property_declaration`, `modifier` `required` | def | must | produces | def index | `Members.cs` | asserted |
| `field` keyword accessor | `property_declaration` | def | must | produces (resolves like any property) | def index, precise member | `Members.cs` | asserted |
| Indexer (one and two parameters), explicit interface indexer | `indexer_declaration` | def | may | silent (no name, no reference for `x[i]`) | — | `Members.cs`, `InterfaceMembers.cs` | — |
| Field-like event | `event_field_declaration` | def | must | produces (name only; the event type is not a type-ref) | def index | `Members.cs` | asserted |
| Event with `add`/`remove` accessors | `event_declaration` | def | must | produces (name only) | def index | `Members.cs` | asserted |
| Instance constructor, chained `: this(...)`, static constructor | `constructor_declaration`, `constructor_initializer` | def | must | silent: no name for any constructor form; `: this(...)` links to nothing | — | `Members.cs` | follow-up |
| Constructor parameter as an injection seam (`ctor-di`) | `constructor_declaration` > `parameter` | type-ref | must | partial: an explicit constructor's interface parameter resolves to its implementation; a primary constructor's parameter never produces a seam | ctor-di | `Members.cs`, `PrimaryConstructors.cs` | asserted (working shape); follow-up |
| Finalizer | `destructor_declaration` | none | may | n-a | — | `Members.cs` | — |
| Methods: instance, static, async, expression-bodied, virtual, override, abstract, new, protected internal | `method_declaration` | def | must | produces | def index | `Members.cs` | asserted |
| Method overloads | `method_declaration`, `parameter_list` | def | must | produces (one name per overload; a reference carries the member name, arity is applied at resolution) | def index, precise member | `Members.cs` | asserted |
| Return and parameter types of methods and constructors | `method_declaration` > `type`, `parameter` > `type` | type-ref | must | produces | type ladder | `Members.cs` | asserted |
| `ref readonly` return and parameter | `ref_type` | type-ref | may | silent (only primitive element types in the fixture) | — | `Members.cs`, `RefAndUnsafe.cs` | — |
| Operator overloads (binary, unary, comparison, `checked`, `++`, `true`/`false`) | `operator_declaration` | none | may | n-a (operators open a member scope and produce no name) | — | `Operators.cs` | — |
| Conversion operators (implicit, explicit) | `conversion_operator_declaration` | type-ref | may | partial: an implicit conversion's parameter type produces, an explicit conversion's target type does not | type ladder | `Operators.cs` | — |
| Operator use sites (`a + b`, `(decimal)a`, `checked(a + b)`, `a++`) | `binary_expression`, `cast_expression`, `checked_expression` | none | may | n-a | — | `Operators.cs` | — |
| Static abstract operator in an interface, implemented by a struct | `operator_declaration` | none | may | n-a | — | `Operators.cs` | — |
| Local function (plain, static, generic, params, async, recursive, forward-declared, nested) | `local_function_statement` | none | may | n-a (not a symbol) | — | `LocalFunctions.cs` | — |
| References inside a local function body and a lambda that calls it | `local_function_statement` > `block` | member-ref | must | produces | precise member | `LocalFunctions.cs` | asserted |
| Top-level statements | `global_statement` | scope | must | partial: `new T()` and `T.Static()` resolve; a top-level local typed by `var x = new T()` does not type `x.M()` | type ladder, precise member | `Program.cs` | asserted (working shape); follow-up |
| Types declared after top-level statements | `class_declaration` after `global_statement` | def | must | produces (namespace kept) | def index | `Program.cs` | asserted |
| Explicit interface implementation (method, property, event) | `explicit_interface_specifier` | def | may | partial: method, property, and event land under the implementing type's plain member name; the indexer does not | def index | `InterfaceMembers.cs` | — |
| Default interface member body | `method_declaration` in `interface_declaration` | def | must | produces | def index | `InterfaceMembers.cs` | asserted |
| Static abstract and static virtual interface members | `modifier` `static abstract` | def | must | produces | def index | `InterfaceMembers.cs` | asserted |
| Interface static field and static method | `field_declaration` in `interface_declaration` | def | must | produces | def index, precise member | `InterfaceMembers.cs` | asserted |
| Call through a type parameter (`TShape.Create()`) | `member_access_expression` on a type parameter | member-ref | may | partial (guess tier, ambiguous across unrelated types) | guess | `InterfaceMembers.cs` | — |
| Generic math constraint (`where T : INumber<T>`) | `type_parameter_constraints_clause` | type-ref | may | silent | — | `InterfaceMembers.cs` | — |

## Usings and type annotations

| Construct | Node kind(s) | Verdict | Obligation | Base | Consumer | Fixture | Pin |
|---|---|---|---|---|---|---|---|
| `using Ns;` | `using_directive` | scope | must | produces (`imports` edge) | imports, type ladder | `NamespaceBlock.cs`, `Program.cs` | asserted |
| `using static T;` | `using_directive` | scope | must | partial: the `imports` edge names the type, but the directive is recorded as a plain using, so a bare `Reset()` or `Counter` it enables resolves to nothing | imports | `Usings.cs` | asserted (working shape); follow-up |
| `using Alias = T;` | `using_directive` | scope | must | partial: the `imports` edge names the target and drops the alias; `new Alias()` and members on an alias-typed local resolve to nothing | imports | `Usings.cs` | asserted (working shape); follow-up |
| `using Alias = (int A, int B);`, `using Alias = List<T>;` | `using_directive` | scope | may | partial (target text kept verbatim; alias usage unresolved) | imports | `Usings.cs` | — |
| `global using` (plain, static, alias) | `using_directive` > `global` | scope | must | produces | imports, type ladder | `Usings.cs` | asserted |
| `extern alias` | `extern_alias_directive` | none | may | n-a (no fixture: needs a second assembly) | — | — | — |
| Base list: base class and interface list | `base_list` | type-ref | must | produces (`inherits`) | type ladder | `TypeKinds.cs`, `InterfaceMembers.cs` | asserted |
| Type annotations on fields, properties, parameters, returns | `type` | type-ref | must | produces | type ladder | `Members.cs`, `MemberAccess.cs` | asserted |
| Type annotation on a local (`T x = ...`) | `variable_declaration` > `type` | type-ref, receiver-type | must | partial: the local is typed (member reads resolve) but no `uses-type` edge is written for the annotation itself | receiver facts | `MemberAccess.cs`, `ObjectCreation.cs` | asserted (working shape); follow-up |
| Generic type arguments in annotations and creations | `type_argument_list` | type-ref | must | partial: every type argument produces; the generic type itself in a local's annotation does not | type ladder | `Generics.cs` | asserted (working shape); follow-up |
| Nullable and array annotations (`T?`, `T[]`, `T[][]`) | `nullable_type`, `array_type` | type-ref | must | partial: `T?` unwraps; an array-typed local or `new T[n]` yields nothing | type ladder | `MemberAccess.cs`, `ObjectCreation.cs` | asserted (working shape); follow-up |
| Tuple type annotation `(T A, int N)` | `tuple_type` | type-ref | must | produces (one `uses-type` per element) | type ladder | `TuplesAndWith.cs` | asserted |
| Predefined types (`int`, `string`, `object`) | `predefined_type` | none | must-not | n-a (never a fact) | — | any | — |
| `dynamic`, `nint`, `nuint` | `identifier` as type | none | may | n-a | — | `RefAndUnsafe.cs` | — |
| Pointer and function pointer types | `pointer_type`, `function_pointer_type` | type-ref | may | silent (even for a user struct) | — | `RefAndUnsafe.cs` | — |

## References and statements

| Construct | Node kind(s) | Verdict | Obligation | Base | Consumer | Fixture | Pin |
|---|---|---|---|---|---|---|---|
| Member access on a field, property, parameter, or local receiver | `member_access_expression` | member-ref | must | produces | precise member | `MemberAccess.cs` | asserted |
| Invocation `a.M()` | `invocation_expression` > `member_access_expression` | member-ref | must | produces | precise member | `MemberAccess.cs` | asserted |
| Generic invocation `a.M<T>()` | `generic_name` | member-ref | must | produces | precise member | `MemberAccess.cs` | asserted |
| Static member access `T.M`, `T.M()` | `member_access_expression` on a type name | member-ref | must | produces | precise member | `MemberAccess.cs`, `StaticAndExtension.cs` | asserted |
| `this.M`, `this.Field` | `member_access_expression` > `this` | member-ref | must | produces (enclosing type and its bases) | precise member | `MemberAccess.cs` | asserted |
| `base.M()` | `member_access_expression` > `base` | member-ref | must | produces | precise member | `MemberAccess.cs` | asserted |
| `global::Ns.T.M()` | `alias_qualified_name` | member-ref | must | silent | — | `MemberAccess.cs` | follow-up |
| Conditional access `a?.M()` | `conditional_access_expression`, `member_binding_expression` | member-ref | must | produces (at the expression's own line) | precise member | `MemberAccess.cs` | asserted |
| Chained conditional access `a?.B?.C` | nested `conditional_access_expression` | member-ref | must | partial: the first hop produces, the hop reached through the second `?.` is dropped | precise member | `MemberAccess.cs` | asserted (working shape); follow-up |
| Null-forgiving receiver `a.B!.C` | `postfix_unary_expression` | member-ref | must | partial: `a.B` produces, the member read through `!` is dropped | precise member | `MemberAccess.cs` | follow-up |
| Element access `a[i]`, `a?[i]`, `a[i].M()` | `element_access_expression`, `element_binding_expression` | member-ref | may | silent | — | `MemberAccess.cs` | — |
| One-hop call chain `a.M().N()` | nested `invocation_expression` | member-ref | must | produces (both hops) | precise member | `MemberAccess.cs` | asserted |
| Member hop then call `a.B.M()` | nested `member_access_expression` | member-ref | must | partial: `a.B` produces, the call on `B`'s declared type is a guess | precise member, guess | `MemberAccess.cs` | asserted (working shape); follow-up |
| Two-hop chain `a.B.C.M()` | nested `member_access_expression` | member-ref | may | partial (tail is a guess) | guess | `MemberAccess.cs` | — |
| Invocation on a creation `new T().M()` | `object_creation_expression` as receiver | member-ref | must | silent (the `new T()` type-ref itself produces) | — | `MemberAccess.cs`, `StaticAndExtension.cs` | follow-up |
| Invocation on a cast `((T)o).M()` | `parenthesized_expression` > `cast_expression` | member-ref | must | silent | — | `MemberAccess.cs`, `TypeOperators.cs` | follow-up |
| Invocation on an await `(await X()).M()` | `parenthesized_expression` > `await_expression` | member-ref | must | silent | — | `MemberAccess.cs`, `AsyncAndYield.cs` | follow-up |
| Method group `Action a = t.M;` | `member_access_expression` | member-ref | must | produces | precise member | `MemberAccess.cs` | asserted |
| Bare invocation or read of an own member `M()`, `Count` | `invocation_expression` > `identifier`, `identifier` | member-ref | must | silent | — | `PartialTypesB.cs`, `Usings.cs` | follow-up |
| Receiver typed by `var x = new T()` | `variable_declaration` > `implicit_type` | receiver-type | must | produces | receiver facts, precise member | `MemberAccess.cs` | asserted |
| Receiver typed by an explicit local type `T x = ...` | `variable_declaration` > `type` | receiver-type | must | produces | receiver facts, precise member | `MemberAccess.cs` | asserted |
| Receiver typed by a parameter, field, property, or static property | `parameter`, `field_declaration`, `property_declaration` | receiver-type | must | produces | receiver facts, precise member | `MemberAccess.cs`, `PrimaryConstructors.cs` | asserted |
| Object creation `new T()`, `new T(args)`, `new T<U>()` | `object_creation_expression` | type-ref | must | produces | type ladder | `ObjectCreation.cs` | asserted |
| Target-typed `new()` on a typed local, field, or argument | `implicit_object_creation_expression` | receiver-type | must | produces (the declared type types the receiver; `new()` itself names nothing) | receiver facts, precise member | `ObjectCreation.cs` | asserted |
| Object initializer `{ A = 1 }`, nested initializer | `initializer_expression` | member-ref | may | silent | — | `ObjectCreation.cs` | — |
| Collection and dictionary initializers | `initializer_expression` | none | may | n-a | — | `ObjectCreation.cs` | — |
| Collection expression `[a, b]` and spread `..x` | `collection_expression`, `spread_element` | none | may | n-a | — | `ObjectCreation.cs` | — |
| Array creation `new T[n]`, `new T[] { }`, jagged, implicit `new[] { }` | `array_creation_expression`, `implicit_array_creation_expression` | type-ref | must | silent | — | `ObjectCreation.cs` | follow-up |
| Anonymous type `new { }` | `anonymous_object_creation_expression` | none | must-not | n-a (correct) | — | `ObjectCreation.cs` | — |
| Attribute on a type, member, parameter, return, with `field:`/`method:` targets | `attribute_list`, `attribute` | type-ref | must | silent (attribute names are read only to detect test methods) | — | `Attributes.cs` | follow-up |
| Assembly-level attribute `[assembly: X]` | `global_attribute` | type-ref | may | silent | — | `Attributes.cs` | — |
| Generic attribute `[X<int>]` | `attribute` > `generic_name` | type-ref | may | silent | — | `Attributes.cs` | — |
| Attributes on lambdas, local functions, type parameters | `attribute_list` | type-ref | may | silent | — | `Attributes.cs` | — |
| Attribute arguments `typeof(T)`, `nameof(T)` | `attribute_argument` | type-ref | may | partial (`typeof` produces, `nameof` does not) | type ladder | `Attributes.cs` | — |
| Test-framework attributes marking test methods and classes | `attribute` | def (`testMethods`) | must | produces | def index | `fixtures/csharp-semantic` | asserted elsewhere (`tests/cli_no_guess.rs`, `tests/semantic_audit.rs`) |
| `typeof(T)`, `typeof(T[])` | `typeof_expression` | type-ref | must | produces | type ladder | `TypeOperators.cs` | asserted |
| `typeof(T<>)` unbound and `typeof(T<int>)` closed | `typeof_expression` > `generic_name` | type-ref | must | produces (a BCL generic such as `List<>` is external, hence no edge) | type ladder | `TypeOperators.cs` | asserted |
| `nameof(T)`, `nameof(T.M)`, `nameof(x)` | `invocation_expression` (`nameof`) | none | may | partial (`nameof(T.M)` emits a member-ref; the others nothing) | precise member | `TypeOperators.cs` | — |
| `sizeof(T)`, `default(T)`, `default` | `sizeof_expression`, `default_expression` | type-ref | may | silent | — | `TypeOperators.cs` | — |
| Cast `(T)x` as a bare expression | `cast_expression` | type-ref | may | partial (only a generic argument inside the cast produces) | type ladder | `TypeOperators.cs` | — |
| Cast typing a local `var x = (T)o` | `variable_declaration` > `cast_expression` | receiver-type | must | produces | receiver facts, precise member | `TypeOperators.cs` | asserted |
| `x as T`, `x is T` | `as_expression`, `is_expression` | type-ref | may | silent | — | `TypeOperators.cs` | — |
| Declaration pattern `x is T t` typing `t` | `declaration_pattern` | receiver-type | must | produces (the `T` itself is not a type-ref) | receiver facts, precise member | `TypeOperators.cs`, `Patterns.cs` | asserted |
| Property pattern `{ A: 1 }`, extended `{ A.B: 1 }` | `property_pattern_clause` | member-ref | may | silent | — | `Patterns.cs` | — |
| Positional pattern `T(1, 2)`, `(1, _)` via `Deconstruct` | `positional_pattern_clause` | type-ref | may | silent | — | `Patterns.cs` | — |
| List pattern `[1, ..]`, `[var first, ..]` | `list_pattern` | none | may | n-a | — | `Patterns.cs` | — |
| List pattern with a slice designation `[.. var rest]` | `list_pattern` | none | may | grammar gap | — | `GrammarGaps.cs` | grammar pin |
| Relational, `and`/`or`/`not`, constant, `var`, discard patterns | `relational_pattern`, `and_pattern`, `or_pattern`, `negated_pattern`, `constant_pattern`, `var_pattern` | none | must-not | n-a (correct) | — | `Patterns.cs` | — |
| Switch expression arms and `when` guards walked | `switch_expression`, `switch_expression_arm`, `when_clause` | member-ref | must | produces | precise member | `Patterns.cs` | asserted |
| Switch statement sections with patterns and `when` guards walked | `switch_statement`, `switch_section`, `when_clause` | member-ref | must | produces (a case-label declaration pattern types its variable) | precise member | `Patterns.cs` | asserted |
| Enum member in a pattern or case label | `member_access_expression` | member-ref | must | produces | precise member | `Patterns.cs` | asserted |
| Lambda with one implicit parameter as the first argument on an identifier receiver (`xs.Where(x => x.M())`) | `lambda_expression`, `implicit_parameter` | receiver-type | must | produces (receiver a field, parameter, local, array, or `IEnumerable<T>`) | receiver facts, precise member | `Lambdas.cs` | asserted |
| Lambda parameter name reused by another lambda in the same member | `lambda_expression` | receiver-type | must | partial: a second lambda in the same member that reuses the name with an ineligible shape (two parameters, chained receiver) drops the fact for every lambda using that name | receiver facts | `Lambdas.cs` | follow-up |
| Lambda with two implicit parameters or on a non-first argument | `lambda_expression` | receiver-type | may | silent | — | `Lambdas.cs` | — |
| Lambda with an explicitly typed parameter `(T x) => x.M()` | `lambda_expression` > `parameter` | receiver-type | must | partial: produces when the parameter name is unique within the member; a reused name falls under the name-reuse row above | receiver facts, precise member | `Lambdas.cs` | asserted (working shape); follow-up |
| Lambda bodies (expression and block) walked | `lambda_expression` > `block` | member-ref | must | produces | precise member | `Lambdas.cs` | asserted |
| Anonymous method `delegate (T i) { }` | `anonymous_method_expression` | receiver-type | must | partial: same rule as the typed lambda, produces for a unique parameter name | receiver facts, precise member | `Lambdas.cs` | asserted (working shape); follow-up |
| Static lambda, explicit return type, default and `params` parameters, discards | `lambda_expression` | none | must-not | n-a (correct) | — | `Lambdas.cs` | — |
| Method group as a delegate value `Func<int,int> f = Twice;` | `identifier` | member-ref | may | silent | — | `Lambdas.cs` | — |
| Lambdas inside chained LINQ calls (`xs.Where(...).Select(x => x.M())`) | chained `invocation_expression` | member-ref | must | partial: bodies are walked, but a lambda on a chained receiver has an untyped parameter, so its accesses are guesses | guess | `Lambdas.cs`, `Linq.cs` | follow-up |
| LINQ query clauses (`from`, `where`, `orderby`, `select`, `let`, `group`, `join`, `into`) | `query_expression` and clauses | none | may | n-a | — | `Linq.cs` | — |
| Member access inside a query clause (`i.Owner.Name`) | `member_access_expression` in `query_expression` | member-ref | must | partial: accesses are walked, the range variable is untyped, so they are guesses | guess | `Linq.cs` | follow-up |
| Range variable typed from its source (`from i in items`) | `from_clause` | receiver-type | may | silent (guess only) | guess | `Linq.cs` | — |
| Explicitly typed range variable `from T i in xs` | `from_clause` > `type` | receiver-type | may | silent | — | `Linq.cs` | — |
| Tuple literal, named tuple element access | `tuple_expression` | none | must-not | n-a (correct) | — | `TuplesAndWith.cs` | — |
| Deconstruction `var (a, b) = x.M()` and into existing locals | `declaration_expression`, `tuple_expression`, `assignment_expression` | receiver-type | may | partial (the deconstructed call produces, the elements are untyped) | precise member | `TuplesAndWith.cs` | — |
| Deconstruction in `foreach (var (k, v) in dict)` | `foreach_statement` > `tuple_pattern` | receiver-type | may | silent (guess only) | guess | `TuplesAndWith.cs` | — |
| `with` expression on a record, record struct, anonymous type | `with_expression`, `with_initializer` | member-ref | may | silent | — | `TuplesAndWith.cs` | — |
| Index and range `a[^1]`, `a[1..]`, `Range`, `Index` | `range_expression`, `prefix_unary_expression` | none | must-not | n-a (correct) | — | `TuplesAndWith.cs` | — |
| `var x = await M()` typing `x` from `Task<T>` | `await_expression` | receiver-type | must | produces | receiver facts, precise member | `AsyncAndYield.cs` | asserted |
| `var x = await M().ConfigureAwait(false)` | `await_expression` > `invocation_expression` | receiver-type | may | silent (guess only; an explicitly typed local works) | guess | `AsyncAndYield.cs` | — |
| `await using var r = new R()` and `await using (var r = new R())` | `using_statement`, `local_declaration_statement` with `await` | receiver-type | must | produces | receiver facts, precise member | `AsyncAndYield.cs` | asserted |
| `await foreach (T x in ...)` and `await foreach (var x in M())` | `foreach_statement` with `await` | receiver-type | must | partial: the explicitly typed form produces; the `var` form over an `IAsyncEnumerable<T>` is a guess | receiver facts, guess | `AsyncAndYield.cs` | asserted (working shape); follow-up |
| Iterator `yield return`, `yield break` | `yield_statement` | none | must-not | n-a (correct) | — | `AsyncAndYield.cs` | — |
| Async lambda, `async void` handler, `Task.Run(async () => ...)` | `lambda_expression` > `modifier` `async` | member-ref | must | produces | precise member | `AsyncAndYield.cs` | asserted |
| `foreach (var x in xs)` typing `x` from `List<T>` or `T[]` | `foreach_statement` | receiver-type | must | produces (a `Dictionary`'s `KeyValuePair.Value` hop is a BCL member and resolves to nothing, as expected) | receiver facts, precise member | `StatementsAndScopes.cs` | asserted |
| `foreach (T x in xs)` explicit element type | `foreach_statement` > `type` | receiver-type | must | produces (the annotation is not written as a `uses-type`) | receiver facts, precise member | `StatementsAndScopes.cs` | asserted |
| Control-flow bodies walked (if/else, for, while, do, switch, try, labels, goto) | `if_statement`, `for_statement`, `while_statement`, `do_statement`, `try_statement`, `labeled_statement`, `goto_statement` | member-ref | must | produces | precise member | `StatementsAndScopes.cs` | asserted |
| `catch (T e)` declaration and `when` filter | `catch_declaration`, `catch_filter_clause` | type-ref, receiver-type | must | partial: the caught type is not a type-ref and `e` is untyped, so the filter's access is a guess | guess | `StatementsAndScopes.cs` | follow-up |
| `throw new T()`, throw expression `?? throw new T()`, rethrow | `throw_statement`, `throw_expression` | type-ref | must | produces | type ladder | `StatementsAndScopes.cs` | asserted |
| `lock (x)`, `checked { }`, `unchecked { }` bodies and targets walked | `lock_statement`, `checked_statement` | member-ref | must | produces (the lock target's own typing follows the local's rule: a `??`-initialised local is a guess) | precise member, guess | `StatementsAndScopes.cs` | asserted (lock target, any tier) |
| `using (var r = new R())` and `using var r = new R();` typing `r` | `using_statement`, `local_declaration_statement` | receiver-type | must | produces | receiver facts, precise member | `StatementsAndScopes.cs` | asserted |
| Local declarations: `var` from `new`, explicit type, multiple declarators, `const` | `local_declaration_statement`, `variable_declaration` | receiver-type | must | produces (a local initialised by `??` or another inferred expression is untyped by design) | receiver facts | `StatementsAndScopes.cs`, `MemberAccess.cs` | asserted |
| Conditional `?:`, `??`, `??=`, compound assignment operands walked | `conditional_expression`, `binary_expression`, `assignment_expression` | member-ref | must | n-a (the fixture's operands are primitives; walked by default recursion) | precise member | `StatementsAndScopes.cs` | — |
| `ref`/`in`/`out` parameters and arguments | `parameter` > `modifier`, `argument` > `modifier` | type-ref | must | produces | type ladder | `RefAndUnsafe.cs` | asserted |
| `out T x` typing `x` | `declaration_expression` | receiver-type | must | produces | receiver facts, precise member | `RefAndUnsafe.cs` | asserted |
| `out var x` typing `x` from the callee | `declaration_expression` > `implicit_type` | receiver-type | may | silent (inference the syntax does not show) | — | `RefAndUnsafe.cs` | — |
| `ref` locals, `ref readonly` returns, `scoped` | `ref_expression`, `ref_type`, `scoped_type` | none | may | n-a | — | `RefAndUnsafe.cs` | — |
| `params T[]` and `params ReadOnlySpan<T>` | `parameter` > `modifier` `params` | def (unbounded arity) | must | produces | def index | `RefAndUnsafe.cs` | asserted |
| `ref struct`, `readonly ref struct`, `ref` field | `struct_declaration` > `modifier` `ref`, `ref_type` | def | must | produces (kind `struct`) | def index | `RefAndUnsafe.cs` | asserted |
| `stackalloc`, `Span<T>` locals, `InlineArray` | `stackalloc_expression`, `implicit_stackalloc_expression` | none | may | n-a | — | `RefAndUnsafe.cs` | — |
| `unsafe` blocks, pointers `*p`, `&x`, `p->M()`, `fixed` | `unsafe_statement`, `pointer_type`, `member_access_expression` (`->`), `fixed_statement` | member-ref | may | silent (`p->X` on a typed pointer yields nothing) | — | `RefAndUnsafe.cs` | — |
| Function pointer `delegate*<int, int>` and `&Static` | `function_pointer_type` | none | may | n-a | — | `RefAndUnsafe.cs` | — |
| `dynamic d; d.M()` | `member_access_expression` on `dynamic` | none | must-not | n-a (correct: no guess fires) | — | `RefAndUnsafe.cs` | — |
| Interpolated string holes (`$"{a.B} {a.M()}"`, verbatim, raw `$$"""`) | `interpolated_string_expression`, `interpolation` | member-ref | must | produces | precise member | `StringsAndTrivia.cs` | asserted |
| Verbatim identifiers `@class`, `@event` | `identifier` | def, member-ref | must | produces (the `@` stays in the id) | def index, precise member | `StringsAndTrivia.cs` | asserted |
| Non-ASCII identifiers (types, members, locals) and strings | `identifier` | def | must | produces (locals are not symbols) | def index, precise member | `StringsAndTrivia.cs` | asserted |

## Directives and trivia

| Construct | Node kind(s) | Verdict | Obligation | Base | Consumer | Fixture | Pin |
|---|---|---|---|---|---|---|---|
| `#if` / `#elif` / `#else` / `#endif` live branch walked | `preproc_if`, `preproc_elif`, `preproc_else` | none | must | produces | imports, precise member | `Preprocessor.cs` | asserted |
| Dead `#if` branch (false symbol) | `preproc_if` | none | must-not | leaks: a class declared in a dead branch is a def with all its member names, and member references inside dead bodies are edges; only a dead method's own name is dropped | — | `Preprocessor.cs` | follow-up |
| `#define` / `#undef` | `preproc_define`, `preproc_undef` | scope | may | silent (no symbol table; every branch is walked) | — | `Preprocessor.cs` | — |
| Nested `#if` and `&&`/`!` conditions | `preproc_if` | none | must | produces (live bodies walked) | precise member | `Preprocessor.cs` | asserted |
| `#if` inside an expression chain | `preproc_if` inside an expression | none | must | produces | precise member | `fixtures/preproc` | asserted elsewhere (extractor unit tests) |
| `#region` / `#endregion` | `preproc_region`, `preproc_endregion` | none | must-not | n-a (correct) | — | `Preprocessor.cs` | — |
| `#nullable`, `#pragma`, `#line`, `#warning` | `preproc_nullable`, `preproc_pragma`, `preproc_line`, `preproc_warning` | none | must-not | n-a (correct; `#line` does not move attribution) | — | `Preprocessor.cs` | — |
| Line, block, and XML doc comments | `comment` | none | must-not | n-a (correct) | — | `StringsAndTrivia.cs` | — |
| `<see cref="X"/>` in doc comments | `comment` | none | may | n-a | — | `StringsAndTrivia.cs` | — |
| Code inside a comment, string, or raw string | `comment`, `string_literal`, `raw_string_literal` | none | must-not | n-a (correct: no ghost symbol anywhere in the graph) | — | `StringsAndTrivia.cs` | — |
| Verbatim, interpolated, raw, u8, char literals | `verbatim_string_literal`, `raw_string_literal`, `string_literal` | none | must-not | n-a (correct; holes are the row above) | — | `StringsAndTrivia.cs` | — |

## Follow-ups

Rows whose obligation and base status disagree, grouped by the extractor change they wait for.
Where a row is `partial`, the pin test asserts the shape that produces today and nothing about
the missing shape; a `silent` or `leaks` row is not asserted in either direction.

- **Receiver shapes with no fact**: `global::` qualified access; the hop after a second `?.`
  or a `!`; the tail of a member hop `a.B.M()`; invocations on a parenthesised `new T()`,
  cast, or `await`; a top-level `var` local; the `var` form of `await foreach`; a lambda on a
  chained receiver and a query range variable; `catch (T e)` inside its filter; a lambda
  parameter name reused by an ineligible lambda in the same member.
- **Bare own-member references** (`M()`, `Count`, and the members a `using static` imports):
  the most common intra-type call shape; silent throughout.
- **Declarations with no fact**: constructors and `: this(...)` chains; record primary
  constructor parameters as properties; primary constructor parameters as `ctor-di` seams.
- **Type references with no edge**: attribute usages; array creation `new T[n]` and array
  annotations; a local's type annotation `T x = ...` (the local is typed, the annotation writes
  no `uses-type`), including the generic type itself and any qualified nested type in it; the
  caught exception type; `using` aliases (the alias name is dropped) and alias-typed locals.
- **Leak**: a type declared inside a dead `#if` branch, all of its member names, and every
  member reference inside a dead body.
