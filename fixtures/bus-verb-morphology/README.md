# Publish verb morphology fixture

The local parcel bus and worker registration use invented names. `Installation`
registers handlers for both messages so a refused call cannot escape solely
because its message lacks a handler.

| File | Expected publish sites |
| --- | --- |
| `TryRoot.cs` | `TryEmit` yields a hop in generic, constructed and identifier argument forms. |
| `MoreRoot.cs` | `EnqueueMessages` yields a hop in the same three forms. |
| `Guarded.cs` | All six first-word guards, with and without `Try`, emit no hop. |
| `EmbeddedRoot.cs` | `Published`, `Sender`, `Dispatcher` and `Postgres` emit no hop. |
| `Rootless.cs` | Generic mock-matcher-like and projection-like calls emit no hop. |
| `Wrapper.cs` | `Forward` around `TryEmit` is not promoted and emits no hop. |
