using System.Runtime.InteropServices;
using System.Text;

using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.CSharp.Syntax;
using Microsoft.CodeAnalysis.Text;

namespace ScoutSemantic;

/// <summary>
/// Repo-root relative path arithmetic plus the directory-skip and --scope
/// filters (§3.4). Directory names match devscout's <c>walk::SKIP_DIRS</c>.
/// </summary>
internal sealed class RepoPaths
{
    private static readonly HashSet<string> SkipDirs = new(StringComparer.Ordinal)
    {
        "bin", "obj", "node_modules", ".git", ".scout", "dist", "coverage", ".next", "target",
    };

    // Linux file systems are case sensitive; macOS and Windows are not by default.
    private static readonly StringComparison PathCmp =
        RuntimeInformation.IsOSPlatform(OSPlatform.Linux)
            ? StringComparison.Ordinal
            : StringComparison.OrdinalIgnoreCase;

    private readonly string _root;
    private readonly List<string> _scope;

    public RepoPaths(string root, IEnumerable<string> scope)
    {
        _root = Normalize(Path.GetFullPath(root)).TrimEnd('/');
        _scope = scope
            .Select(s => Normalize(s).Trim('/'))
            .Where(s => s.Length > 0 && s != ".")
            .ToList();
    }

    public string Root => _root;

    private static string Normalize(string p) => p.Replace('\\', '/');

    /// <summary>
    /// Root-relative path of <paramref name="absolute"/>, or null when the file
    /// is outside the root, under a skipped directory, or outside --scope.
    /// </summary>
    public string? Relative(string? absolute)
    {
        if (string.IsNullOrEmpty(absolute))
        {
            return null;
        }

        string full;
        try
        {
            full = Normalize(Path.GetFullPath(absolute));
        }
        catch (Exception e) when (e is ArgumentException or NotSupportedException or PathTooLongException)
        {
            return null;
        }

        if (full.Length <= _root.Length + 1 || !full.StartsWith(_root, PathCmp) || full[_root.Length] != '/')
        {
            return null;
        }

        var rel = full[(_root.Length + 1)..];
        var parts = rel.Split('/');
        // The file name itself is never a directory component.
        for (var i = 0; i < parts.Length - 1; i++)
        {
            if (SkipDirs.Contains(parts[i]))
            {
                return null;
            }
        }

        if (_scope.Count > 0 && !_scope.Any(s => rel.Length > s.Length && rel.StartsWith(s, PathCmp) && rel[s.Length] == '/'))
        {
            return null;
        }

        return rel;
    }
}

/// <summary>
/// Walks one compiled project and turns every member-reference syntax site into
/// <see cref="RefRecord"/> rows (§3.4). Sequential, one document at a time.
/// </summary>
internal sealed class Walker
{
    // devscout's test-attribute sets (src/extract.rs). MSTest's pair is gated on
    // a class-level [TestClass]; the xUnit/NUnit names are not.
    private static readonly HashSet<string> DirectTestAttributes = new(StringComparer.Ordinal)
    {
        "Fact", "Theory", "Test", "TestCase", "TestCaseSource",
    };

    private static readonly HashSet<string> MsTestTestAttributes = new(StringComparer.Ordinal)
    {
        "TestMethod", "DataTestMethod",
    };

    private readonly RepoPaths _paths;
    private readonly IReadOnlyDictionary<string, string> _assemblyToUnit;

    public Walker(RepoPaths paths, IReadOnlyDictionary<string, string> assemblyToUnit)
    {
        _paths = paths;
        _assemblyToUnit = assemblyToUnit;
    }

    /// <summary>Emits one record per accepted symbol at every member site in the tree.</summary>
    public void WalkDocument(SemanticModel model, SyntaxTree tree, string relFile, string unit, List<RefRecord> sink)
    {
        foreach (var node in tree.GetRoot().DescendantNodes())
        {
            string shape;
            string receiverKind;
            ExpressionSyntax? receiverExpr;
            SyntaxToken nameToken;

            switch (node)
            {
                case MemberAccessExpressionSyntax ma when ma.IsKind(SyntaxKind.SimpleMemberAccessExpression):
                    shape = "access";
                    receiverExpr = ma.Expression;
                    receiverKind = ReceiverKindOf(ma.Expression);
                    nameToken = ma.Name.Identifier;
                    break;

                case MemberBindingExpressionSyntax mb:
                    shape = "conditional";
                    receiverExpr = ConditionalTargetOf(mb);
                    receiverKind = "conditional";
                    nameToken = mb.Name.Identifier;
                    break;

                case InvocationExpressionSyntax inv
                    when inv.Expression is IdentifierNameSyntax or GenericNameSyntax:
                    shape = "bare";
                    receiverExpr = null;
                    receiverKind = "implicit";
                    nameToken = ((SimpleNameSyntax)inv.Expression).Identifier;
                    break;

                default:
                    continue;
            }

            var info = model.GetSymbolInfo(node);
            List<ISymbol> symbols;
            bool ambiguous;
            if (info.Symbol is not null)
            {
                symbols = new List<ISymbol>(1) { info.Symbol };
                ambiguous = false;
            }
            else if (!info.CandidateSymbols.IsDefaultOrEmpty)
            {
                symbols = info.CandidateSymbols.ToList();
                ambiguous = true;
            }
            else
            {
                continue;
            }

            var startLine = LineOf(tree, node.SpanStart);
            var line = LineOf(tree, nameToken.SpanStart);
            var receiverText = Collapse(receiverExpr?.ToString());
            var receiver = receiverExpr is null
                ? null
                : Ids.TypeId(model.GetTypeInfo(receiverExpr).Type);

            foreach (var candidate in symbols)
            {
                var symbol = candidate;
                var ext = false;
                if (symbol is IMethodSymbol { ReducedFrom: { } reduced })
                {
                    ext = true;
                    symbol = reduced;
                }

                symbol = symbol.OriginalDefinition;
                if (!Accept(symbol))
                {
                    continue;
                }

                var (target, targetKind) = Ids.MemberTarget(symbol);
                sink.Add(new RefRecord
                {
                    File = relFile,
                    StartLine = startLine,
                    Line = line,
                    Shape = shape,
                    ReceiverKind = receiverKind,
                    ReceiverText = receiverText,
                    Receiver = receiver,
                    Member = symbol.Name,
                    MemberKind = Ids.MemberKind(symbol),
                    Target = target,
                    TargetKind = targetKind,
                    TargetFile = TargetFileOf(symbol),
                    TargetUnit = UnitOf(symbol),
                    Ext = ext,
                    External = symbol.DeclaringSyntaxReferences.Length == 0,
                    Ambiguous = ambiguous,
                    Unit = unit,
                });
            }
        }
    }

    /// <summary>In-tree named types and enum members of one document (§3.8).</summary>
    public void CollectDefs(SemanticModel model, SyntaxTree tree, string relFile, string unit, List<DefRecord> sink)
    {
        foreach (var node in tree.GetRoot().DescendantNodes())
        {
            INamedTypeSymbol? type = node switch
            {
                BaseTypeDeclarationSyntax b => model.GetDeclaredSymbol(b),
                DelegateDeclarationSyntax d => model.GetDeclaredSymbol(d),
                _ => null,
            };

            if (type is null)
            {
                continue;
            }

            var id = Ids.NamedTypeId(type);
            sink.Add(new DefRecord
            {
                Id = id,
                Kind = Ids.TypeKindName(type),
                File = relFile,
                Line = LineOf(tree, node.SpanStart),
                Unit = unit,
                Test = HasTestMethod(type),
            });

            if (node is EnumDeclarationSyntax en)
            {
                foreach (var member in en.Members)
                {
                    sink.Add(new DefRecord
                    {
                        Id = id + "." + member.Identifier.ValueText,
                        Kind = "enum-member",
                        File = relFile,
                        Line = LineOf(tree, member.SpanStart),
                        Unit = unit,
                        Test = false,
                    });
                }
            }
        }
    }

    private static bool HasTestMethod(INamedTypeSymbol type)
    {
        var gated = type.GetAttributes().Any(a => ShortName(a) == "TestClass");
        foreach (var method in type.GetMembers().OfType<IMethodSymbol>())
        {
            foreach (var attribute in method.GetAttributes())
            {
                var name = ShortName(attribute);
                if (name is null)
                {
                    continue;
                }

                if (DirectTestAttributes.Contains(name) || (gated && MsTestTestAttributes.Contains(name)))
                {
                    return true;
                }
            }
        }

        return false;
    }

    private static string? ShortName(AttributeData a)
    {
        var name = a.AttributeClass?.Name;
        if (name is null)
        {
            return null;
        }

        const string suffix = "Attribute";
        return name.Length > suffix.Length && name.EndsWith(suffix, StringComparison.Ordinal)
            ? name[..^suffix.Length]
            : name;
    }

    private static bool Accept(ISymbol s) => s switch
    {
        IMethodSymbol m => m.MethodKind is not (MethodKind.LocalFunction
            or MethodKind.Constructor or MethodKind.StaticConstructor
            or MethodKind.Destructor or MethodKind.AnonymousFunction),
        IPropertySymbol => true,
        IFieldSymbol => true,
        IEventSymbol => true,
        _ => false,
    };

    private static string ReceiverKindOf(ExpressionSyntax expr) => expr switch
    {
        ThisExpressionSyntax => "this",
        BaseExpressionSyntax => "base",
        IdentifierNameSyntax or GenericNameSyntax => "ident",
        MemberAccessExpressionSyntax or QualifiedNameSyntax => "qualified",
        InvocationExpressionSyntax => "call",
        _ => "other",
    };

    /// <summary>The expression a <c>?.</c> chain is conditioned on, for `x?.Y()`.</summary>
    private static ExpressionSyntax? ConditionalTargetOf(MemberBindingExpressionSyntax binding)
    {
        for (SyntaxNode? p = binding.Parent; p is not null; p = p.Parent)
        {
            if (p is ConditionalAccessExpressionSyntax c)
            {
                return c.Expression;
            }

            if (p is StatementSyntax or MemberDeclarationSyntax)
            {
                break;
            }
        }

        return null;
    }

    private static int LineOf(SyntaxTree tree, int position) =>
        tree.GetLineSpan(new TextSpan(position, 0)).StartLinePosition.Line + 1;

    private static string? Collapse(string? text)
    {
        if (text is null)
        {
            return null;
        }

        var sb = new StringBuilder(text.Length);
        var space = false;
        foreach (var ch in text)
        {
            if (char.IsWhiteSpace(ch))
            {
                space = sb.Length > 0;
                continue;
            }

            if (space)
            {
                sb.Append(' ');
                space = false;
            }

            sb.Append(ch);
        }

        return sb.Length > 120 ? sb.ToString(0, 120) : sb.ToString();
    }

    private string? TargetFileOf(ISymbol symbol)
    {
        var own = FirstInTree(symbol);
        if (own is not null)
        {
            return own;
        }

        return symbol.ContainingType is { } ct ? FirstInTree(ct) : null;
    }

    private string? FirstInTree(ISymbol symbol)
    {
        foreach (var reference in symbol.DeclaringSyntaxReferences)
        {
            var rel = _paths.Relative(reference.SyntaxTree.FilePath);
            if (rel is not null)
            {
                return rel;
            }
        }

        return null;
    }

    private string? UnitOf(ISymbol symbol)
    {
        var assembly = symbol.ContainingAssembly?.Name;
        return assembly is not null && _assemblyToUnit.TryGetValue(assembly, out var unit) ? unit : null;
    }
}
