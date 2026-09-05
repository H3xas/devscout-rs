// A `#if` group nested inside another `#if` group interrupts a fluent
// chain: `#if TRACE` sits inside `#if DEBUG`. Before parsing, the outer
// group's own truth value decides whether any of it reaches the parser --
// an inactive outer arm blanks everything inside it, inner directive
// lines included, regardless of the inner symbol.
//
// With no build symbols defined, `DEBUG` is false, so the entire group --
// both `.WriteTo.Trace()` (line 25) and `.WriteTo.Debug()` (line 27) --
// is absent from the extracted refs. Only the trailing
// `.MinimumLevel.Override(...)` (line 29) after the group survives.
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
