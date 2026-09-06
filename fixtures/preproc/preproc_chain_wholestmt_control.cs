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
