# csharp-direction fixtures

Base/interface DIRECTION fixture: every shape where the compiler's answer to
"which type declares this member" runs along an `inherits` edge. `Driver.cs`
and `Driver2.cs` each declare one local per receiver shape and then call or
read a member through it, one call per source line; a `// case <id>` comment
above the line ties it to the oracle table and to the rows below.

| File | What it exercises |
| --- | --- |
| `Root.cs` | Root of a three-level class hierarchy (`Root -> Mid -> Leaf`); exercises how the resolver walks `this.`, `base.` and bare member references up and down a base chain. `Inherited` is never overridden anywhere; `Overridden` is virtual and overridden once in `Mid`; `ReOverridden` is virtual and overridden in both `Mid` and `Leaf`; `Hidden` is non-virtual and hidden (not overridden) with `new` in `Mid`; `StaticInherited` is static and called through the derived type name in `Driver.cs`; `InheritedProp` is a property never overridden; `OverriddenProp` is a virtual property overridden once in `Mid`. |
| `Mid.cs` | Middle of the hierarchy: overrides `Overridden` and `ReOverridden` (`Leaf` re-overrides `ReOverridden` only), hides `Hidden` with `new` (no polymorphism through a `Root`-typed receiver), and overrides `OverriddenProp`. `MidOnly` is declared only here; exercises a bare call resolving through an intermediate base. |
| `Leaf.cs` | Bottom of the hierarchy; re-overrides `ReOverridden` (`Mid`'s override is skipped by any call not routed through `base.`). `Probe()` exercises `this.`-qualified, `base.`-qualified and bare member references against the same hierarchy from inside the most derived type, one call per source line (cases B1-B3, C1-C4, D1-D3). |
| `AbstractBase.cs` | An abstract class with one abstract member (implemented in `Concrete.cs`) and one concrete member inherited as-is; exercises abstract-override resolution alongside a plain inherited method on the same base. |
| `GenericBase.cs` | The one generic type in this fixture, closed by `ClosedDerived.cs`. Generic arity is owned by a different fixture; this exercises a member declared on an open generic base reached through a closed derived receiver. |
| `ClosedDerived.cs` | Closes `GenericBase<T>` at `int`, with no members of its own; every call against it resolves up to the open generic declaration (cases G1-G2). |
| `IExtended.cs` | Extends `IContract` with one member of its own; exercises a receiver typed to the derived interface reaching a member declared on the base interface (`Both.cs` implements both). |
| `Inheriting.cs` | Inherits `Implicit`'s implementation of `IContract` without redeclaring it; exercises a call through a derived class reaching an interface member whose implementation lives two hops up (`Implicit` implements the interface, `Inheriting` only adds its own member). |
| `Driver.cs` | Declares one local per receiver shape used across the class-hierarchy, abstract/override, interface-implementation and generic cases (class hierarchy, abstract/override, interface implementations, and the one generic case), then reads or calls a member through each, one call per source line with a `// case` comment above it locating it against the oracle table (cases A1-A16, E1-E12, G1-G2). |
| `Driver2.cs` | Declares one local per receiver shape for declared-type-versus-initializer locals, chain tails, qualified and generic static qualifiers, accessibility and explicit-implementation shapes, and a same-arity overload split, one call per source line with a `// case` comment above it (cases H1, H6, X5-X17); the chain-tail line (case H6) yields two oracle records, the inner call and the tail. |
