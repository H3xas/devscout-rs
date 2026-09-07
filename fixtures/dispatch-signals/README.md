# dispatch-signals fixtures

Invented vocabulary (`Signals`, `Beacon`, `Relay`) for the DI-registration
`implements`/`overrides` edges: two-type-argument service registrations, an
interface with two implementations, an `override` chain, and one arity-tied
member that must resolve to no edge at all.

| File | What it exercises |
| --- | --- |
| `IBeacon.cs` | The service interface `LightBeacon`/`LoudBeacon` both implement; declares one method, `Flash()`. |
| `BaseBeacon.cs` | A base class declaring one `virtual` method, `Dim()`, never itself registered. |
| `LightBeacon.cs` | One of `IBeacon`'s two implementations; also overrides `BaseBeacon.Dim()`, so it carries both a member-level `implements` edge (`Flash`) and an `overrides` edge (`Dim`). |
| `LoudBeacon.cs` | `IBeacon`'s other implementation; a plain class with no base beyond the interface, so `refs IBeacon` must list both implementations, not just one. |
| `SilentBeacon.cs` | A third `IBeacon` registration that declares no `: IBeacon` base at all -- the type-level `implements` edge is the ONLY path from this type to the interface, which is what isolates `impact`'s widened interface hop (and `--no-dispatch`'s removal of it) from the ordinary base-list `inherits` case `LightBeacon`/`LoudBeacon` also exercise. |
| `IRelay.cs` | A second service interface, declaring `Notify(int level)` at arity 1. |
| `NoisyBeacon.cs` | `IRelay`'s registered implementation, declaring TWO overloads of `Notify` (`int`, `string`) that both sit at arity 1 -- the arity-tied case: the type-level `implements` edge still resolves (the registration itself is unambiguous), but no member-level edge names `Notify`, because the resolver cannot tell which overload the interface member pairs with. |
| `Caller.cs` | Calls `IBeacon.Flash()` through a plain `IBeacon`-typed parameter; the site `impact` on either registered implementation must reach through the widened interface hop. |
| `Wiring.cs` | The registration call sites: `AddScoped<IBeacon, LightBeacon>()`, `AddSingleton<IBeacon, LoudBeacon>()`, `AddScoped<IBeacon, SilentBeacon>()`, `AddSingleton<IRelay, NoisyBeacon>()` (the two-type-argument shape, two different lifetime spellings), plus `AddSingleton<IRelay>()` -- a one-type-argument call that records no registration fact at all. |
