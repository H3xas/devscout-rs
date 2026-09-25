namespace CsharpContext.Clean;

// Lives outside fixtures/csharp-context, the --root this fixture is analysed
// under, and is linked into Clean.csproj by relative path -- the fixture
// case for the "linked-outside-root" drop reason.
internal static class Linked
{
    internal static int Value => 2;
}
