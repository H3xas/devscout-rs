// An `#if TRACE #else #endif` group interrupts a fluent chain midway
// through it, with a real call on either side of the whole group. The
// pre-pass blanks whichever arm is inactive -- together with the
// `#if`/`#else`/`#endif` directive lines -- so the chain reads as one
// uninterrupted statement.
//
// With no build symbols defined, `TRACE` is false: the `#if` arm's
// `.WriteTo.Trace()` (line 23) is absent from the refs; the `#else`
// arm's `.WriteTo.Console()` (line 25) and the trailing
// `.MinimumLevel.Override(...)` (line 27) survive untouched.
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
