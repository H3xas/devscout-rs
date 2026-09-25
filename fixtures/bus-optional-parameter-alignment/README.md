# bus-optional-parameter-alignment fixtures

A signal-probe estate, invented and neutral: it reuses no vocabulary from `fixtures/bus-signals/`,
`fixtures/bus-vocabulary/`, `fixtures/bus-vocabulary-admission/`, `fixtures/bus-array-identity/` or
`fixtures/bus-extension-refusal/`. Every case below calls a repository-declared helper whose overload
carries an OPTIONAL trailing parameter (a default value). A call may pass that parameter or leave it
out, so the test-double refusal aligns the lambda argument by the overload's range of accepted
argument counts; an exact-count match would skip the call that leaves it out and keep its hop.

Everything parses offline: `Framework.cs` declares the bus and the consumer interface as local
stand-ins, so no package reference is needed.

| File | What it covers |
| --- | --- |
| `Framework.cs` | The local stand-ins: `ISignalBus`, `IConsumer<TMessage>`, and `Probe.Any<T>()`, a matcher stand-in. |
| `Messages.cs` | `Beacon`, the one message this fixture sends and handles. |
| `BeaconListener.cs` | The real handler and the real, non-test-double publish site: proof that this fixture's own ordinary hop still exists. |
| `CheckHelpers.cs` | The repository-declared helpers, each with an optional trailing `note` parameter defaulted to `null`: `Confirmed`, `(this TTarget source, Expression<Action<TTarget>> expression, string? note = null)`; `ConfirmedStatic`, the same shape without the `this` marker; and `RunsLive`, `(this TTarget source, Action<TTarget> callback, string? note = null)` -- a plain delegate, not an expression tree. |
| `ExtensionOmittedOptional.cs` | `Confirmed` called in EXTENSION form with only the lambda argument, the optional `note` left out. The lambda publishes `Beacon` with a matcher argument. This line may earn no hop. |
| `StaticOmittedOptional.cs` | `ConfirmedStatic` called with two arguments (the receiver explicit, the optional `note` left out). This line may earn no hop. |
| `ExtensionSuppliedOptional.cs` | `Confirmed` called in extension form WITH the optional `note` argument supplied. Protects the receiver exclusion: without it, this call's own arity would align the lambda with the `this`-marked receiver slot and wrongly keep the hop. |
| `ExtensionPlainDelegateOmittedOptional.cs` | `RunsLive` called in extension form with only the lambda, the optional `note` left out. Its second parameter is a plain delegate, not an expression tree, so the call is real dispatch and must keep its hop. |

## Cases

- **Extension-form verify lambda with an omitted optional parameter emits no hop.**
  `ExtensionOmittedOptional.cs`:
  `a_publish_inside_an_extension_helper_verify_lambda_with_an_omitted_optional_parameter_emits_no_hop`.
- **Static-form verify lambda with an omitted optional parameter emits no hop.**
  `StaticOmittedOptional.cs`:
  `a_publish_inside_a_static_helper_verify_lambda_with_an_omitted_optional_parameter_emits_no_hop`.
- **Extension-form call with the optional argument supplied emits no hop.** A lambda never binds
  an extension call's receiver, so the reading that would put it on the `this`-marked slot is not
  admissible. `ExtensionSuppliedOptional.cs`:
  `a_publish_inside_an_extension_helper_verify_lambda_with_its_optional_argument_supplied_emits_no_hop`.
- **A plain-delegate extension call with an omitted optional parameter keeps its hop.**
  `ExtensionPlainDelegateOmittedOptional.cs`:
  `a_publish_inside_an_extension_helper_taking_a_plain_delegate_with_an_omitted_optional_parameter_keeps_its_hop`.

## Not covered

A STATIC-form call of a `this`-marked helper that leaves out an optional argument, while the
extension reading of the same call also fits and puts the lambda on a parameter that is not
expression-tree typed, keeps its hop. The recorded facts cannot tell a static-form call from an
extension-form call in that case, and keeping the site is the fail-safe default. Telling the two
apart needs the enclosing call's receiver recorded as a type or a value.
