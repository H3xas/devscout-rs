// Grammar gaps: constructs the pinned tree-sitter-c-sharp grammar cannot parse yet, kept apart so no other fixture file carries an ERROR node.
namespace Syntax.Gaps;

public class GapHost
{
    public int Value;

    public static int Count<T>(T item) where T : allows ref struct
    {
        return 1;
    }

    public int Slice(int[] xs)
    {
        return xs is [var first, .. var rest] ? first + rest.Length : 0;
    }

    public void Run()
    {
        _ = Count(1);
        _ = Slice([1, 2]);
        _ = new GapHost().Value;
    }
}
