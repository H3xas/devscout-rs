// A `#if DEBUG` directive interrupts a fluent member-access/invocation
// chain midway through it. Before parsing, every byte of the inactive arm
// -- and of the `#if`/`#endif` directive lines themselves -- is blanked
// with spaces, so the chain reads as one uninterrupted statement running
// straight from `.File(...)` into `.MinimumLevel.Override(...)`.
//
// With no build symbols defined, `DEBUG` is false, so the call inside the
// arm -- `.WriteTo.Debug()` on line 31 -- must be absent from the
// extracted refs entirely. The calls before and after the directive keep
// their real line numbers and stay in ascending order: `Interval.Day`
// (line 26) precedes the `MinimumLevel.Override` arguments (lines 33-34).
//
// Fully synthetic -- no identifiers below come from any real codebase.
namespace Fixtures.Preproc
{
  public class ChainWithIfDirective
  {
    private Pipeline Build()
    {
      var pipeline = new Pipeline()
        .Enrich.WithTag("Release", "1.0")
        .Filter.ByExcluding(e => e.Level == Level.Error)
        .WriteTo.File(new Formatter(),
          _path,
          retainedFileCountLimit: 5,
          rollingInterval: Interval.Day,
          rollOnFileSizeLimit: true,
          fileSizeLimitBytes: 104_857_600,
          shared: true)
#if DEBUG
        .WriteTo.Debug()
#endif
        .MinimumLevel.Override("Microsoft", Level.Information)
        .MinimumLevel.Override("System", Level.Information)
        .Build();

      return pipeline;
    }
  }
}
