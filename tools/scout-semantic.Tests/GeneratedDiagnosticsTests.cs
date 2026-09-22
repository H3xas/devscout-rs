using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;

using ScoutSemantic;

using Xunit;

namespace ScoutSemanticTests;

/// <summary>
/// Exercises <c>ContextBuilder.GeneratedDiagnosticsOf</c> directly against a
/// hand-built compilation. No generator in this repository's own fixtures
/// (two <c>System.Text.Json</c> serializer contexts) ever reports a
/// diagnostic of its own, so <c>generated.diagnostics</c> is <c>[]</c> in
/// every committed envelope and a defect in the attribution path would be
/// invisible to every other gate -- this is the positive exercise the
/// ticket's round-2 review (M2) asked for.
/// </summary>
public sealed class GeneratedDiagnosticsTests
{
    [Fact]
    public void a_diagnostic_on_a_generated_tree_is_attributed_to_it_not_to_an_authored_sibling()
    {
        var authored = CSharpSyntaxTree.ParseText("class Authored {}", path: "Authored.cs");
        // CS0103 ('Missing' does not exist in the current context): a real
        // binding error located on this tree, the same shape Roslyn reports
        // for a generator's own faulty output.
        var generated = CSharpSyntaxTree.ParseText(
            "class Generated { object M() => Missing; }", path: "Generated.g.cs");

        var compilation = CSharpCompilation.Create(
            "GeneratedDiagnosticsTest",
            new[] { authored, generated },
            new[] { MetadataReference.CreateFromFile(typeof(object).Assembly.Location) },
            new CSharpCompilationOptions(OutputKind.DynamicallyLinkedLibrary));

        var generatedTrees = new HashSet<SyntaxTree> { generated };

        var result = ContextBuilder.GeneratedDiagnosticsOf(compilation, generatedTrees);

        Assert.Contains(result, d => d.Contains("CS0103"));
        Assert.DoesNotContain(result, d => d.Contains("Authored"));
    }

    [Fact]
    public void a_generated_only_diagnostic_never_lands_on_diagnostics_compiler()
    {
        // The same shape ContextBuilder.BuildRecord relies on: an authored
        // tree that binds cleanly beside a generated tree that does not, so
        // the two accounts (diagnostics.compiler for authored documents,
        // generated.diagnostics for generated ones) do not cross-attribute.
        var authored = CSharpSyntaxTree.ParseText("class Authored { int X = 1; }", path: "Authored.cs");
        var generated = CSharpSyntaxTree.ParseText(
            "class Generated { object M() => Missing; }", path: "Generated.g.cs");

        var compilation = CSharpCompilation.Create(
            "GeneratedDiagnosticsAttributionTest",
            new[] { authored, generated },
            new[] { MetadataReference.CreateFromFile(typeof(object).Assembly.Location) },
            new CSharpCompilationOptions(OutputKind.DynamicallyLinkedLibrary));

        var authoredOnlyDiagnostics = compilation.GetDiagnostics()
            .Where(d => d.Severity == DiagnosticSeverity.Error && d.Location.SourceTree == authored)
            .ToList();

        Assert.Empty(authoredOnlyDiagnostics);
        Assert.NotEmpty(ContextBuilder.GeneratedDiagnosticsOf(compilation, new HashSet<SyntaxTree> { generated }));
    }

    [Fact]
    public void no_generated_trees_yields_no_diagnostics()
    {
        var tree = CSharpSyntaxTree.ParseText("class C { object M() => Missing; }");
        var compilation = CSharpCompilation.Create(
            "GeneratedDiagnosticsEmptyTest",
            new[] { tree },
            new[] { MetadataReference.CreateFromFile(typeof(object).Assembly.Location) },
            new CSharpCompilationOptions(OutputKind.DynamicallyLinkedLibrary));

        var result = ContextBuilder.GeneratedDiagnosticsOf(compilation, new HashSet<SyntaxTree>());

        Assert.Empty(result);
    }
}
