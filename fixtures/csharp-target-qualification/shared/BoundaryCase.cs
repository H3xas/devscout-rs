namespace TargetQualification.Shared
{
    /// <summary>
    /// The target-API boundary case: the two-argument <c>string.Contains(string, StringComparison)</c>
    /// overload exists starting with .NET Core 2.0 / netstandard2.1; net40/net472/net48/netstandard2.0
    /// bind only the one-argument overload, so this call fails to compile there (CS1501: no overload
    /// takes 2 arguments) rather than binding silently to a different member. Each profile's compiler
    /// diagnostics decide this case's row, not a hand-written expectation, which is what makes the
    /// bind/non-bind split compiler-verified rather than asserted.
    /// </summary>
    public class BoundaryProbe
    {
        public bool Bind(string haystack, string needle)
        {
            return haystack.Contains(needle, System.StringComparison.Ordinal);
        }
    }
}
