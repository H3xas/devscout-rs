// KG-1 regression fixture -- the `#if X #else #endif` variant of
// preproc_chain_interrupt_if.cs: both the `#if` arm and the `#else` arm
// interrupt the same fluent chain, and each opening token (`#if`, `#else`)
// needs its own promoted-qualifier compensation. `#endif` never promotes --
// Node's own preproc_if absorbs it as a trailing token rather than
// splitting the statement there, confirmed by the trailing
// `.MinimumLevel.Override(...)` continuation NOT producing a
// "MinimumLevel"/"Override" candidate on either side.
//
// Fully synthetic -- no identifiers below come from any real codebase.
namespace Fixtures.Preproc
{
  public class ChainWithIfElseDirective
  {
    private Pipeline Build()
    {
      var pipeline = new Pipeline()
        .Enrich.WithTag("Release", "1.0")
        .WriteTo.File(new Formatter(),
          _path,
          shared: true)
#if TRACE
        .WriteTo.Trace()
#else
        .WriteTo.Console()
#endif
        .MinimumLevel.Override("Microsoft", Level.Information)
        .Build();

      return pipeline;
    }
  }
}
