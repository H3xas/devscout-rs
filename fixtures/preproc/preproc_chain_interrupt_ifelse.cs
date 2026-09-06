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
