using System;

namespace CsharpContext.Legacy;

public static class Program
{
    public static bool HasExclamation(string text)
    {
        // string.Contains(char, StringComparison) does not exist on the
        // net472 reference assemblies this project also targets -- the
        // fixture's control for a requested target whose compilation binds
        // but reports a real compiler diagnostic.
        return text.Contains('!', StringComparison.Ordinal);
    }
}
