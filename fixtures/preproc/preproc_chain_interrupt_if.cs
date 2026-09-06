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
