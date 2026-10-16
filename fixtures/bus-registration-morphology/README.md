# bus-registration-morphology fixtures

A bindery, invented and neutral: nothing here is named after a bus the engine
ships handling for, nor after the vocabulary of any other fixture directory.
The estate is a print shop -- a press, a registry of installations, editions
and ledgers -- and every name is a word this repository chose.

The one question every case asks is where the name rule that reads a
one-type-argument call as a handler registration stops. The rule admits a
call whose method name starts with an install prefix or ends with a role
suffix, and the resolver keeps a base only when the registered type is not
itself a sent message and actually receives a message the corpus sends. The
cases below pin both halves at their CURRENT outcome: nothing here widens or
narrows the rule, and nothing here fixes what it finds.

Every case maps the shared core beside exactly one registration file, so a
case that derives nothing proves its own registration file is why.

| File | What it covers |
| --- | --- |
| `Press.cs` | The local stand-ins: `IPressBus`, `IPressRegistry` with one method per spelling below, the house handler base `PressHandlerBase<TNotice>` and the middleware base `PressMiddlewareBase<TNotice>`. |
| `Messages.cs` | `EditionNotice` (sent), `LedgerNotice` (never sent anywhere in this directory), and `EditionState`, which one registration passes as a second type argument. |
| `Handlers.cs` | The core's one handler, `EditionHandler : PressHandlerBase<EditionNotice>`, receiving its own argument. |
| `Publishers.cs` | The core's one publisher, sending `EditionNotice`. |
| `RegistrationTargets.cs` | The types the middleware and no-base cases register or leave unregistered: four carriers of `PressMiddlewareBase`, and `LooseHandler`, which declares no base at all. Included only by the cases that name them. |
| `Register*.cs` | One registration file per case, each with its own installer class. |

## Cases

The control and the nine unseen spellings each map with the core alone.

- **Control.** `registry.AddEdition<EditionHandler>()` -- an invoked member
  call with one type argument and a name the rule admits. Current outcome:
  the registration is recorded, the house base is admitted, and the corpus
  emits one `bus-hop` from `Publishers.cs` to `EditionHandler` with
  `stats.bus_vocabulary_derived: true`.
- **Non-generic install call.** `registry.AddEdition(typeof(EditionHandler))`
  -- an admitted name and one plain argument rather than a type argument.
  Current outcome: no registration fact, no hop, no vocabulary claim.
- **Non-generic plural role call.** `registry.Handlers(typeof(EditionHandler))`
  -- the role word in its plural spelling, which the suffix list does not
  carry, and again no type argument. Current outcome: no fact, no hop, no
  claim.
- **Name in neither list, `With`.** `registry.With<EditionHandler>()`.
  Current outcome: no fact, no hop, no claim.
- **Name in neither list, `Endpoint`.** `registry.Endpoint<EditionHandler>()`.
  Current outcome: no fact, no hop, no claim.
- **Two type arguments.** `registry.AddFolder<EditionHandler, EditionState>()`
  -- an admitted name whose argument list pairs the handler with its state.
  Current outcome: no fact, no hop, no claim.
- **Assembly scan by install name.**
  `registry.AddEditionsFromAssembly(typeof(EditionHandler).Assembly)`.
  Current outcome: no fact, no hop, no claim.
- **Assembly scan by registration name.**
  `registry.RegisterEditionTypes(typeof(EditionHandler).Assembly)`.
  Current outcome: no fact, no hop, no claim.
- **Receiver-less generic call.** `AddEdition<EditionHandler>()`, invoked on
  the enclosing class rather than on a member of anything. Current outcome:
  no fact, no hop, no claim.
- **Conditional-access generic call.** `registry?.AddEdition<EditionHandler>()`.
  Current outcome: no fact, no hop, no claim.

These nine are pinned as they are today, deliberately. Recording any of them
would change the assertions in `tests/bus_registration_morphology.rs`; that is
the point of pinning them, so a widening is a visible, intended test change
rather than a silent one.

## Admission cases

Each of these maps the core with `RegistrationTargets.cs` and one registration
file.

- **A registration whose named type has no base.** `LooseHandler` declares no
  generic base. Current outcome: the registration fact is recorded and no hop
  is emitted, so the corpus makes no vocabulary claim at all.
- **A middleware-shaped registration of an unsent argument.**
  `registry.UseMiddleware<LedgerBridge>()`, where `LedgerNotice` is sent
  nowhere. Current outcome: the fact is recorded; no hop for `LedgerBridge`,
  and none for `EditionBridge`, a carrier of the same base whose own argument
  `EditionNotice` IS sent, because the base earns no place in the vocabulary.
- **A middleware-shaped registration whose argument nothing receives.**
  `registry.UseMiddleware<QuietBridge>()`, where `EditionNotice` is sent but
  no declaration of `QuietBridge` takes it. Current outcome: the fact is
  recorded; no hop for either `QuietBridge` or `EditionBridge`, the same
  carrier as above.
- **A middleware-shaped registration of a sent, received argument.**
  `registry.UseMiddleware<EagerBridge>()`. Current outcome: the base IS
  admitted, and admission is base-wide -- `EagerBridge`, `QuietBridge` and
  `EditionBridge` each gain a hop from `EditionNotice`'s publish site, even
  though only `EagerBridge` and `EditionBridge` declare a parameter for it.
  `stats.bus_vocabulary_derived` is `true`.
- **A registration naming a method's own type parameter.**
  `Install<TShelf>(...)` calls `registry.AddEdition<TShelf>()`. Current
  outcome: the fact is recorded under the name `TShelf` (the extraction scope
  carries only the enclosing TYPE's parameters, so a method's own parameter
  renders as its name rather than as a wildcard), and no type of that name is
  declared, so no base is taught: no hop, no claim.
