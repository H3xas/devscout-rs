// KG-1 control fixture -- a `#if DEBUG` wrapping a WHOLE statement (not
// interrupting an expression/fluent chain mid-way). This is the common,
// well-formed use of conditional compilation and both engines already parse
// it identically without the preproc_promoted_qualifier compensation. Kept
// as a fixture to guard against the compensation over-firing on ordinary
// statement-level `#if` blocks.
//
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
