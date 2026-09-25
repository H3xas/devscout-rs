namespace Fixtures.Enrichment
{
    // Exists only for `OverloadedSameLine`'s own exact-identity fixture case:
    // two overloads of one member, called on one physical source line, so an
    // admitted artifact can carry one confirmed fact per overload at the
    // SAME (file, startLine, member) compatibility key -- proving those
    // facts survive as distinct facts rather than collapsing into one
    // ambiguous outcome the bare key alone would produce.
    public class Options
    {
        public void Configure() { }

        public void Configure(bool verbose) { }
    }
}
