namespace CsharpContext.Clean;

// Lives under a devscout skip directory (bin) even though the project
// explicitly compiles it -- the fixture case for the "skipped-directory"
// drop reason.
internal static class Stub
{
    internal static int Value => 1;
}
