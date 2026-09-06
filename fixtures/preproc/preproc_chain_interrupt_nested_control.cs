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
