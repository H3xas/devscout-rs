// KG-1 control fixture -- a `#if` nested inside another `#if` interrupting
// the same fluent chain. Unlike the single-level if/if-else cases in
// preproc_chain_interrupt_if.cs and preproc_chain_interrupt_ifelse.cs, this
// shape already parses identically on both engines WITHOUT the
// preproc_promoted_qualifier compensation (native tree-sitter's error
// recovery happens to build a proper `preproc_if` node here too, matching
// web-tree-sitter). Kept as a fixture specifically to guard against the
// compensation over-firing on this shape in the future -- it must stay a
// no-op here.
//
// Fully synthetic -- no identifiers below come from any real codebase.
namespace Fixtures.Preproc
{
  public class ChainWithNestedIfDirective
  {
    private Pipeline Build()
    {
      var pipeline = new Pipeline()
        .Enrich.WithTag("Release", "1.0")
        .WriteTo.File(new Formatter(),
          _path,
          shared: true)
#if DEBUG
#if TRACE
        .WriteTo.Trace()
#endif
        .WriteTo.Debug()
#endif
        .MinimumLevel.Override("Microsoft", Level.Information)
        .Build();

      return pipeline;
    }
  }
}
