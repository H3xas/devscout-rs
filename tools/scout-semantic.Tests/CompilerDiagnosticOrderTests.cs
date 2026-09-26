using System.Buffers;
using System.Globalization;
using System.Text.Json;

using ScoutSemantic;

using Xunit;

namespace ScoutSemanticTests;

/// <summary>
/// Roslyn's diagnostic collection order is not stable between identical runs,
/// so both compiler-diagnostic arrays are written in one ordinal order over
/// their written fields: file (missing first), line (missing first), id or
/// code, message, severity. Every arrival order of a fixed set must therefore
/// write the same bytes, under any process culture.
/// </summary>
public sealed class CompilerDiagnosticOrderTests
{
    private const string Main = "Program does not contain a static 'Main' method suitable for an entry point";
    private const string Frob = "'Widget' does not contain a definition for 'Frob'";
    private const string Bar = "'Widget' does not contain a definition for 'Bar'";

    private static readonly CompilerDiagnosticRecord[] RecordSet =
    {
        Rec(null, null, "CS5001", Main, "error"),
        Rec("é.cs", null, "CS8021", "No value for RuntimeMetadataVersion found", "warning"),
        Rec("a.cs", 9, "CS0103", "The name 'missing' does not exist in the current context", "error"),
        Rec("a.cs", 9, "CS1061", Frob, "error"),
        Rec("a.cs", 9, "CS1061", Bar, "error"),
        Rec("a.cs", 14, "CS0122", "'Widget.Hidden' is inaccessible due to its protection level", "error"),
        Rec("B.cs", 12, "CS1503", "Argument 1: cannot convert from 'string' to 'int'", "error"),
        Rec("i.cs", 3, "CS0168", "The variable 'e' is declared but never used", "warning"),
        Rec("I.cs", 3, "CS0219", "The variable 'x' is assigned but its value is never used", "warning"),
    };

    private static readonly string[] RecordOrder =
    {
        "null|null|CS5001|" + Main + "|error",
        "B.cs|12|CS1503|Argument 1: cannot convert from 'string' to 'int'|error",
        "I.cs|3|CS0219|The variable 'x' is assigned but its value is never used|warning",
        "a.cs|9|CS0103|The name 'missing' does not exist in the current context|error",
        "a.cs|9|CS1061|" + Bar + "|error",
        "a.cs|9|CS1061|" + Frob + "|error",
        "a.cs|14|CS0122|'Widget.Hidden' is inaccessible due to its protection level|error",
        "i.cs|3|CS0168|The variable 'e' is declared but never used|warning",
        "é.cs|null|CS8021|No value for RuntimeMetadataVersion found|warning",
    };

    private static readonly CompilerDiagnosticFact[] FactSet =
    {
        new("error", "CS5001", Main, "", 0),
        new("error", "CS1061", Frob, "a.cs", 9),
        new("error", "CS1061", Bar, "a.cs", 9),
        new("warning", "CS1061", Bar, "a.cs", 9),
        new("error", "CS1503", "Argument 1: cannot convert from 'string' to 'int'", "B.cs", 12),
        new("warning", "CS0168", "The variable 'e' is declared but never used", "i.cs", 3),
        new("warning", "CS0219", "The variable 'x' is assigned but its value is never used", "I.cs", 3),
        new("warning", "CS0105", "The using directive for 'System' appeared previously in this namespace", "é.cs", 5),
    };

    private static readonly string[] FactOrder =
    {
        "|0|CS5001|" + Main + "|error",
        "B.cs|12|CS1503|Argument 1: cannot convert from 'string' to 'int'|error",
        "I.cs|3|CS0219|The variable 'x' is assigned but its value is never used|warning",
        "a.cs|9|CS1061|" + Bar + "|error",
        "a.cs|9|CS1061|" + Bar + "|warning",
        "a.cs|9|CS1061|" + Frob + "|error",
        "i.cs|3|CS0168|The variable 'e' is declared but never used|warning",
        "é.cs|5|CS0105|The using directive for 'System' appeared previously in this namespace|warning",
    };

    public static TheoryData<string> Cultures => new() { "en-US", "tr-TR" };

    private static CompilerDiagnosticRecord Rec(string? file, int? line, string id, string message, string severity) =>
        new() { File = file, Line = line, Id = id, Message = message, Severity = severity };

    private static ContextRecord RecordWith(List<CompilerDiagnosticRecord> compiler) => new()
    {
        Identity = new ContextIdentity { ProjectPath = "src/A/A.csproj", ProjectName = "A", RequestedTfm = "net9.0" },
        State = "partial",
        Reason = "compiler-error",
        Diagnostics = new ContextDiagnostics { Compiler = compiler },
    };

    private static string TextOrNull(JsonElement value) =>
        value.ValueKind == JsonValueKind.Null ? "null" : value.ToString();

    private static string[] CompilerArrayOf(JsonElement record) => record
        .GetProperty("diagnostics")
        .GetProperty("compiler")
        .EnumerateArray()
        .Select(d => string.Join('|',
            TextOrNull(d.GetProperty("file")),
            TextOrNull(d.GetProperty("line")),
            d.GetProperty("id").GetString(),
            d.GetProperty("message").GetString(),
            d.GetProperty("severity").GetString()))
        .ToArray();

    private static string[] FactArrayOf(JsonElement document) => document
        .GetProperty("diagnostics")
        .EnumerateArray()
        .Select(d => string.Join('|',
            d.GetProperty("file").GetString(),
            d.GetProperty("line").GetInt32(),
            d.GetProperty("code").GetString(),
            d.GetProperty("message").GetString(),
            d.GetProperty("severity").GetString()))
        .ToArray();

    private static string[] WrittenCompilerArray(ContextRecord record)
    {
        var buffer = new ArrayBufferWriter<byte>();
        using (var writer = new Utf8JsonWriter(buffer))
        {
            ContextWriter.WriteRecord(writer, record);
        }

        using var doc = JsonDocument.Parse(buffer.WrittenMemory);
        return CompilerArrayOf(doc.RootElement);
    }

    /// <summary>Calls <paramref name="visit"/> once for every ordering of
    /// <paramref name="items"/> (Heap's algorithm), reusing one array.</summary>
    private static int ForEachPermutation<T>(IReadOnlyList<T> items, Action<T[]> visit)
    {
        var current = items.ToArray();
        var counters = new int[current.Length];
        var visited = 1;
        visit(current);
        var i = 1;
        while (i < current.Length)
        {
            if (counters[i] < i)
            {
                var j = i % 2 == 0 ? 0 : counters[i];
                (current[j], current[i]) = (current[i], current[j]);
                visit(current);
                visited++;
                counters[i]++;
                i = 1;
            }
            else
            {
                counters[i] = 0;
                i++;
            }
        }

        return visited;
    }

    private static int Factorial(int n) => n <= 1 ? 1 : n * Factorial(n - 1);

    private static void InCulture(string name, Action body)
    {
        var culture = CultureInfo.GetCultureInfo(name);
        var savedCulture = CultureInfo.CurrentCulture;
        var savedUiCulture = CultureInfo.CurrentUICulture;
        CultureInfo.CurrentCulture = culture;
        CultureInfo.CurrentUICulture = culture;
        try
        {
            // A culture that already orders these names ordinally would let a
            // culture-sensitive sort pass unnoticed.
            Assert.NotEqual(
                Math.Sign(string.CompareOrdinal("a.cs", "B.cs")),
                Math.Sign(string.Compare("a.cs", "B.cs", StringComparison.CurrentCulture)));
            body();
        }
        finally
        {
            CultureInfo.CurrentCulture = savedCulture;
            CultureInfo.CurrentUICulture = savedUiCulture;
        }
    }

    [Theory]
    [MemberData(nameof(Cultures))]
    public void context_compiler_diagnostics_are_written_in_one_ordinal_order_for_every_arrival_order(string culture)
    {
        InCulture(culture, () =>
        {
            var compiler = new List<CompilerDiagnosticRecord>();
            var record = RecordWith(compiler);
            var visited = ForEachPermutation(RecordSet, permutation =>
            {
                compiler.Clear();
                compiler.AddRange(permutation);
                Assert.Equal(RecordOrder, WrittenCompilerArray(record));
            });

            Assert.Equal(Factorial(RecordSet.Length), visited);
        });
    }

    [Theory]
    [MemberData(nameof(Cultures))]
    public void a_missing_line_is_written_before_any_line_of_the_same_file(string culture)
    {
        InCulture(culture, () =>
        {
            var lined = Rec("a.cs", 1, "CS0103", "m", "error");
            var lineless = Rec("a.cs", null, "CS0103", "m", "error");
            foreach (var arrival in new[] { new[] { lined, lineless }, new[] { lineless, lined } })
            {
                Assert.Equal(
                    new[] { "a.cs|null|CS0103|m|error", "a.cs|1|CS0103|m|error" },
                    WrittenCompilerArray(RecordWith(arrival.ToList())));
            }
        });
    }

    [Theory]
    [MemberData(nameof(Cultures))]
    public void compiler_facts_diagnostics_and_the_embedded_context_record_are_written_in_one_ordinal_order(string culture)
    {
        InCulture(culture, () =>
        {
            var options = new Options { Root = "unused", CompilerFacts = "unused.json" };
            var acc = new CompilerFactsAccumulator();
            var compiler = new List<CompilerDiagnosticRecord>();
            var records = new List<ContextRecord> { RecordWith(compiler) };
            var shuffle = new Random(20260926);
            var visited = ForEachPermutation(FactSet, permutation =>
            {
                acc.Diagnostics.Clear();
                acc.Diagnostics.AddRange(permutation);
                compiler.Clear();
                compiler.AddRange(RecordSet.OrderBy(_ => shuffle.Next()));

                var bytes = CompilerFactsEmitter.Render(options, acc, git: null, records, occurrences: null);
                using var doc = JsonDocument.Parse(bytes);
                Assert.Equal(FactOrder, FactArrayOf(doc.RootElement));
                var embedded = doc.RootElement
                    .GetProperty("context")
                    .GetProperty("envelope")
                    .GetProperty("compilations")[0];
                Assert.Equal(RecordOrder, CompilerArrayOf(embedded));
            });

            Assert.Equal(Factorial(FactSet.Length), visited);
        });
    }
}
