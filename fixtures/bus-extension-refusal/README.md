# bus-extension-refusal fixtures

A signal-probe estate, invented and neutral: it reuses no vocabulary from
`fixtures/bus-signals/`, `fixtures/bus-vocabulary/`,
`fixtures/bus-vocabulary-admission/` or `fixtures/bus-array-identity/`. Every
case below probes the test-double refusal's own parameter alignment when the
repository-declared helper is an EXTENSION method: its `this`-marked first
parameter is never one of the call's own arguments when the call is written
in extension form, so the refusal has to skip that slot before it can read
the parameter the lambda actually landed on.

Everything parses offline: `Framework.cs` declares the bus and the consumer
interface as local stand-ins, so no package reference is needed.

| File | What it covers |
| --- | --- |
| `Framework.cs` | The local stand-ins: `IWatchBus`, `IConsumer<TMessage>`, and `Any.Of<T>()`, a matcher stand-in. |
| `Messages.cs` | `Signal`, the one message this fixture sends and handles. |
| `SignalHandler.cs` | The real handler and the real, non-test-double publish site: proof that this fixture's own ordinary hop still exists. |
| `ProbeHelpers.cs` | The repository-declared extension helpers: `VerifiedOnce`/`NeverCalled`, both `(this TTarget source, Expression<Action<TTarget>> expression)`, and `RunsDirectly`, `(this TTarget source, Action<TTarget> callback)` -- a plain delegate, not an expression tree. |
| `ExtensionFormVerifyLambda.cs` | Both expression-tree helpers called in EXTENSION form (`bus.VerifiedOnce(...)`, `bus.NeverCalled(...)`) on a lambda publishing `Signal` with a matcher argument. Neither line may earn a hop. |
| `StaticFormVerifyLambda.cs` | The same helper called in STATIC form (`ProbeHelpers.VerifiedOnce(bus, ...)`). The receiver is an explicit argument here, so today's alignment (no offset) already refuses it correctly. |
| `PlainDelegateExtension.cs` | `RunsDirectly` called in extension form wrapping the same publish. Its second parameter is a plain delegate, not an expression tree, so the call is real dispatch and must keep its hop. |

## Cases

- **Extension-form verify lambda emits no hop.** `ExtensionFormVerifyLambda.cs`:
  `a_publish_inside_a_repository_extension_helper_verify_lambda_emits_no_hop`.
- **Static-form call already aligns (guard, passes unmodified).**
  `StaticFormVerifyLambda.cs`:
  `a_publish_inside_a_repository_extension_helper_called_in_static_form_emits_no_hop`.
- **A plain-delegate extension call keeps its hop (guard, passes
  unmodified).** `PlainDelegateExtension.cs`:
  `a_publish_inside_an_extension_method_taking_a_plain_delegate_keeps_its_hop`.
