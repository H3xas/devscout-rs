// An `#if TRACE #else #endif` group interrupts a fluent chain midway
// through it, with a real call on either side of the whole group. The
// pre-pass blanks whichever arm is inactive, together with the directive
// lines, so the chain reads as one uninterrupted statement. With no build
// symbols defined, `TRACE` is false: the `#if` arm's `.WriteTo.Trace()`
// (line 23) is blanked; the `#else` arm's `.WriteTo.Console()` (line 25)
// reaches the parser and keeps the chain intact but, like every step of
// this chain, yields no ref of its own; only the trailing
// `.MinimumLevel.Override(...)` argument (line 27) does.
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
