// A `#if DEBUG` block wraps a WHOLE statement rather than interrupting a
// fluent chain mid-expression. Before parsing, an inactive block -- and
// its `#if`/`#endif` directive lines -- is blanked with spaces.
//
// With no build symbols defined, `DEBUG` is false, so the guarded call on
// line 17 is absent from the extracted refs; the unguarded call on line
// 15 is untouched.
// Fully synthetic -- no identifiers below come from any real codebase.
namespace Fixtures.Preproc
{
  public class WholeStatementGuard
  {
    private void Configure(Registry registry)
    {
      registry.Attach(GetPrimarySink());
#if DEBUG
      registry.Attach(GetDebugSink());
#endif
    }
  }
}
