namespace ScoutSemantic;

/// <summary>The written fields of one compiler diagnostic, in the order they
/// are compared. <c>Id</c> is the context record's <c>id</c> and the
/// compiler-facts <c>code</c>.</summary>
internal readonly record struct CompilerDiagnosticKey(
    string? File,
    int? Line,
    string Id,
    string Message,
    string Severity);

/// <summary>
/// The one order both compiler-diagnostic arrays are written in: the context
/// record's <c>diagnostics.compiler</c> and the compiler-facts top-level
/// <c>diagnostics</c>. Roslyn's <c>GetDiagnostics()</c> order is not stable
/// between identical runs, so the writers impose this order instead of
/// trusting the order they are given. The key covers every written field, so
/// two entries that compare equal write the same bytes; a missing file or
/// line sorts first, where compiler facts already place a location-less
/// diagnostic (<c>""</c>, <c>0</c>). Every string comparison is ordinal, so
/// the process culture cannot change the order.
/// </summary>
internal static class CompilerDiagnosticOrder
{
    public static readonly IComparer<CompilerDiagnosticKey> Comparer =
        Comparer<CompilerDiagnosticKey>.Create(Compare);

    public static CompilerDiagnosticKey KeyOf(CompilerDiagnosticRecord record) =>
        new(record.File, record.Line, record.Id, record.Message, record.Severity);

    public static CompilerDiagnosticKey KeyOf(CompilerDiagnosticFact fact) =>
        new(fact.File, fact.Line, fact.Code, fact.Message, fact.Severity);

    public static List<CompilerDiagnosticRecord> Sorted(IEnumerable<CompilerDiagnosticRecord> records) =>
        records.OrderBy(KeyOf, Comparer).ToList();

    public static List<CompilerDiagnosticFact> Sorted(IEnumerable<CompilerDiagnosticFact> facts) =>
        facts.OrderBy(KeyOf, Comparer).ToList();

    public static int Compare(CompilerDiagnosticKey a, CompilerDiagnosticKey b)
    {
        var order = string.CompareOrdinal(a.File, b.File);
        if (order != 0)
        {
            return order;
        }

        order = Nullable.Compare(a.Line, b.Line);
        if (order != 0)
        {
            return order;
        }

        order = string.CompareOrdinal(a.Id, b.Id);
        if (order != 0)
        {
            return order;
        }

        order = string.CompareOrdinal(a.Message, b.Message);
        return order != 0 ? order : string.CompareOrdinal(a.Severity, b.Severity);
    }
}
