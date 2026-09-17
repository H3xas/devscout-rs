using System.Text;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.Text;

namespace TargetQualification.Deep.Generated
{
    /// <summary>
    /// Minimal incremental generator: emits one fixed source file declaring a generated type.
    /// The generated-input deep case asserts that the consuming profile's project-loading
    /// obligation reaches this type even though it never exists as a hand-authored file.
    /// </summary>
    [Generator(LanguageNames.CSharp)]
    public sealed class GeneratedInputGenerator : IIncrementalGenerator
    {
        public void Initialize(IncrementalGeneratorInitializationContext context)
        {
            context.RegisterPostInitializationOutput(ctx =>
            {
                var source = new StringBuilder();
                source.AppendLine("namespace TargetQualification.Deep.Generated");
                source.AppendLine("{");
                source.AppendLine("    public class GeneratedMarker");
                source.AppendLine("    {");
                source.AppendLine("        public string Origin() => \"generated\";");
                source.AppendLine("    }");
                source.AppendLine("}");
                ctx.AddSource("GeneratedMarker.g.cs", SourceText.From(source.ToString(), Encoding.UTF8));
            });
        }
    }
}
