namespace Fixtures.Enrichment.Widgets
{
    // Exists only so `Render` has a second, unrelated declarer: the
    // qualified-property-access site's own receiver type is invisible to
    // the syntax ladder (see Registry.cs), so its fallback member-name-
    // uniqueness guess needs a REAL second candidate to land on -- making
    // that guess AMBIGUOUS and, crucially, wrong at that site, the same
    // shape the Config.Load() fixture already proves for a different
    // receiver category. Never referenced by any real call.
    public class GammaWidget
    {
        public void Render() { }
    }
}
