// KG-1 regression fixture -- a `#if DEBUG` directive interrupting a fluent
// member-access/invocation chain. Native tree-sitter's error recovery for
// this exact shape differs from web-tree-sitter's even on the identical
// pinned grammar (tree-sitter-c-sharp 0.23.5): WASM cleanly splits the
// statement and re-parses the directive as a `preproc_if` node whose
// continuation starts at a bare identifier; native tree-sitter instead
// swallows the directive as a small ERROR-node extra child while continuing
// the same chain uninterrupted, burying that continuation's leading
// identifier as a member_access_expression's `name` field instead of
// exposing it as a qualifier. See extract.rs's
// `preproc_promoted_qualifier` for the compensation.
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
