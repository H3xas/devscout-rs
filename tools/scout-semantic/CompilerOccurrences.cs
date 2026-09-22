using System.Security.Cryptography;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;

using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.CSharp.Syntax;
using Microsoft.CodeAnalysis.Text;

namespace ScoutSemantic;

/// <summary>A caller's or a bound target's identity, in the same shape
/// <see cref="CompilerSymbolFact"/> already uses for a declared member.</summary>
internal sealed record OccurrenceIdentity(
    string Assembly, string Type, string? Member, int GenericArity, string OverloadSignature);

internal readonly record struct OccurrenceSpan(int StartLine, int StartChar, int EndLine, int EndChar);

internal readonly record struct OccurrenceNamePosition(int Line, int Char);

/// <summary>
/// One per-reference occurrence fact: a compiler-bound reference site with its
/// own resolution state, independent of <see cref="CompilerSymbolFact"/>'s
/// declared-symbol catalog. <see cref="CompilationIdentity"/>/
/// <see cref="CompilationFingerprint"/> are filled in once this occurrence's
/// own compilation's <see cref="ContextRecord"/> is known -- unset (null) the
/// instant this record is created, backfilled by the caller via <c>with</c>.
/// </summary>
internal sealed record CompilerOccurrenceFact(
    string File,
    string Shape,
    OccurrenceSpan Span,
    OccurrenceNamePosition Name,
    OccurrenceIdentity Caller,
    string Resolution,
    string CandidateReason,
    OccurrenceIdentity? Target,
    List<OccurrenceIdentity> Candidates,
    string DocumentContentIdentity,
    List<string> TargetDocumentContentIdentities,
    ContextIdentity? CompilationIdentity = null,
    string? CompilationFingerprint = null);

/// <summary>
/// Walks every invocation, member-access, conditional-member and bare
/// identifier reference in a compiled document, driven by
/// <see cref="SemanticModel.GetSymbolInfo(SyntaxNode, System.Threading.CancellationToken)"/>.
/// Deliberately independent of <see cref="Walker"/> (the oracle): its own
/// record type, its own accumulator, and its own <c>Accept</c> filter --
/// every <see cref="MethodKind"/> is in scope here, including constructors
/// and local functions, which the oracle deliberately excludes. Shares
/// <see cref="Ids"/> for identity formatting only, which carries no walk
/// logic of its own.
/// </summary>
internal sealed class CompilerOccurrenceAccumulator
{
    public List<CompilerOccurrenceFact> Sites { get; } = new();

    // Per-relative-path memoization: many occurrences share a caller or
    // target document, so hashing it once per run (not once per occurrence)
    // is the concrete cost control this walker owns.
    private readonly Dictionary<string, string> _contentIdentityCache = new(StringComparer.Ordinal);

    /// <summary>Walks one already-compiled document, appending every accepted
    /// occurrence to <see cref="Sites"/> with its compilation identity/
    /// fingerprint left unset -- the caller backfills both once this
    /// document's compilation's own <see cref="ContextRecord"/> exists.</summary>
    public void WalkDocument(SemanticModel model, SyntaxTree tree, string relFile, RepoPaths paths)
    {
        var ownContentIdentity = ContentIdentityOf(relFile, tree.GetText().ToString());
        var claimed = new HashSet<SyntaxToken>();

        foreach (var node in tree.GetRoot().DescendantNodes())
        {
            string shape;
            SyntaxToken nameToken;

            switch (node)
            {
                case MemberAccessExpressionSyntax ma when ma.IsKind(SyntaxKind.SimpleMemberAccessExpression):
                    shape = "access";
                    nameToken = ma.Name.Identifier;
                    claimed.Add(nameToken);
                    break;

                case MemberBindingExpressionSyntax mb:
                    shape = "conditional";
                    nameToken = mb.Name.Identifier;
                    claimed.Add(nameToken);
                    break;

                case InvocationExpressionSyntax inv
                    when inv.Expression is IdentifierNameSyntax or GenericNameSyntax:
                    shape = "invocation";
                    nameToken = ((SimpleNameSyntax)inv.Expression).Identifier;
                    claimed.Add(nameToken);
                    break;

                case IdentifierNameSyntax or GenericNameSyntax:
                    nameToken = ((SimpleNameSyntax)node).Identifier;
                    if (claimed.Contains(nameToken))
                    {
                        continue;
                    }

                    shape = "identifier";
                    break;

                default:
                    continue;
            }

            var info = model.GetSymbolInfo(node);
            string resolution;
            ISymbol? confirmed = null;
            List<ISymbol> candidateSymbols = new();

            if (info.Symbol is not null)
            {
                if (!IsAcceptedKind(info.Symbol))
                {
                    // Resolves to a type, namespace, local variable or
                    // parameter: no devscout graph edge exists for those, so
                    // this node is not an occurrence at all.
                    continue;
                }

                resolution = "confirmed";
                confirmed = info.Symbol;
            }
            else
            {
                var raw = info.CandidateSymbols.IsDefaultOrEmpty
                    ? new List<ISymbol>()
                    : info.CandidateSymbols.ToList();

                if (raw.Count == 0)
                {
                    // Every site failing to bind is recorded, never dropped --
                    // there is nothing here to kind-filter by.
                    resolution = "unresolved";
                }
                else
                {
                    resolution = info.CandidateReason switch
                    {
                        CandidateReason.Inaccessible => "inaccessible",
                        CandidateReason.LateBound => "dynamic",
                        _ => "ambiguous",
                    };

                    candidateSymbols = raw.Where(IsAcceptedKind).ToList();
                    if (candidateSymbols.Count == 0)
                    {
                        // Every candidate was a non-member kind (e.g. an
                        // ambiguous type reference): not a member occurrence.
                        continue;
                    }
                }
            }

            var span = tree.GetLineSpan(node.Span);
            var namePos = tree.GetLineSpan(new TextSpan(nameToken.SpanStart, 0)).StartLinePosition;
            var callerIdentity = CallerIdentityOf(node, model);
            var target = confirmed is not null ? IdentityOf(confirmed) : null;
            var candidates = candidateSymbols.Select(IdentityOf).ToList();
            var targetDocs = confirmed is not null
                ? TargetDocumentIdentitiesOf(confirmed, paths)
                : new List<string>();

            Sites.Add(new CompilerOccurrenceFact(
                File: relFile,
                Shape: shape,
                Span: new OccurrenceSpan(
                    span.StartLinePosition.Line + 1, span.StartLinePosition.Character,
                    span.EndLinePosition.Line + 1, span.EndLinePosition.Character),
                Name: new OccurrenceNamePosition(namePos.Line + 1, namePos.Character),
                Caller: callerIdentity,
                Resolution: resolution,
                CandidateReason: info.CandidateReason.ToString(),
                Target: target,
                Candidates: candidates,
                DocumentContentIdentity: ownContentIdentity,
                TargetDocumentContentIdentities: targetDocs));
        }
    }

    private string ContentIdentityOf(string cacheKey, string text)
    {
        if (_contentIdentityCache.TryGetValue(cacheKey, out var cached))
        {
            return cached;
        }

        var identity = "sha1:" + FactsWriter.Sha1(text);
        _contentIdentityCache[cacheKey] = identity;
        return identity;
    }

    private List<string> TargetDocumentIdentitiesOf(ISymbol symbol, RepoPaths paths)
    {
        var original = symbol.OriginalDefinition;
        if (original.DeclaringSyntaxReferences.Length == 0)
        {
            // An external symbol (from a referenced assembly): an explicit
            // empty list, not a silently dropped field.
            return new List<string>();
        }

        var result = new List<string>(original.DeclaringSyntaxReferences.Length);
        var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (var reference in original.DeclaringSyntaxReferences)
        {
            var declTree = reference.SyntaxTree;
            var key = paths.Relative(declTree.FilePath) ?? declTree.FilePath;
            if (!seen.Add(key))
            {
                continue;
            }

            result.Add(ContentIdentityOf(key, declTree.GetText().ToString()));
        }

        return result;
    }

    private static bool IsAcceptedKind(ISymbol s) => s switch
    {
        IMethodSymbol => true,
        IPropertySymbol => true,
        IFieldSymbol => true,
        IEventSymbol => true,
        _ => false,
    };

    /// <summary>The innermost enclosing member's identity, found by walking
    /// ancestors for the nearest method/constructor/accessor/local function.
    /// A node outside any member (e.g. a top-level statement) falls back to
    /// its enclosing type's identity with <c>member: null</c>.</summary>
    private static OccurrenceIdentity CallerIdentityOf(SyntaxNode node, SemanticModel model)
    {
        foreach (var ancestor in node.Ancestors())
        {
            ISymbol? declared = ancestor switch
            {
                MethodDeclarationSyntax m => model.GetDeclaredSymbol(m),
                ConstructorDeclarationSyntax c => model.GetDeclaredSymbol(c),
                AccessorDeclarationSyntax a => model.GetDeclaredSymbol(a),
                LocalFunctionStatementSyntax l => model.GetDeclaredSymbol(l),
                _ => null,
            };

            if (declared is not null)
            {
                return IdentityOf(declared);
            }

            if (ancestor is BaseTypeDeclarationSyntax typeDecl
                && model.GetDeclaredSymbol(typeDecl) is { } typeSymbol)
            {
                return new OccurrenceIdentity(
                    typeSymbol.ContainingAssembly?.Name ?? "", Ids.NamedTypeId(typeSymbol), null, 0, "");
            }
        }

        return new OccurrenceIdentity("", "", null, 0, "");
    }

    private static OccurrenceIdentity IdentityOf(ISymbol symbolIn)
    {
        var symbol = symbolIn.OriginalDefinition;
        var assembly = symbol.ContainingAssembly?.Name ?? "";

        if (symbol is INamedTypeSymbol namedType)
        {
            return new OccurrenceIdentity(assembly, Ids.NamedTypeId(namedType), null, namedType.Arity, "");
        }

        var containingType = symbol.ContainingType;
        var typeId = containingType is not null ? Ids.NamedTypeId(containingType) : "";
        var arity = symbol is IMethodSymbol method ? method.Arity : 0;
        var signature = symbol is IMethodSymbol m
            ? "(" + string.Join(",", m.Parameters.Select(
                p => p.Type.ToDisplayString(SymbolDisplayFormat.MinimallyQualifiedFormat)))
              + ")->" + (m.ReturnsVoid ? "void" : m.ReturnType.ToDisplayString(SymbolDisplayFormat.MinimallyQualifiedFormat))
            : "";

        return new OccurrenceIdentity(assembly, typeId, symbol.Name, arity, signature);
    }
}

/// <summary>
/// The derived context-summary fold: folds the embedded envelope's own
/// ordered per-compilation <c>identity</c>/<c>fingerprint</c> pairs into one
/// document-level SHA-256 digest. Pure, no I/O. Genuinely new code, not a
/// reuse of <see cref="ContextFingerprint.Compute"/> (which folds inputs to
/// one compilation's own fingerprint, a different, lower-level concept).
/// Reimplemented independently -- byte-for-byte, not by shared code -- on the
/// Rust admission side; see that side's own unit tests for cross-order
/// verification of the same rule.
/// </summary>
internal static class DerivedContextSummary
{
    private const string NullFingerprintSentinel = "null";

    private static readonly JsonWriterOptions Compact = new()
    {
        Indented = false,
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
    };

    /// <summary>Folds <paramref name="orderedRecords"/> (in the order they
    /// are embedded -- not re-sorted here) into a lower-case hex SHA-256
    /// digest of their canonicalized <c>identity</c>+<c>fingerprint</c>
    /// pairs, one per line.</summary>
    public static string Compute(IReadOnlyList<ContextRecord> orderedRecords)
    {
        var lines = new List<string>(orderedRecords.Count);
        foreach (var record in orderedRecords)
        {
            var canonicalIdentity = CanonicalIdentityJson(record.Identity);
            var fingerprint = record.Fingerprint ?? NullFingerprintSentinel;
            lines.Add(canonicalIdentity + '' + fingerprint);
        }

        var preimage = string.Join('\n', lines);
        var digest = SHA256.HashData(Encoding.UTF8.GetBytes(preimage));
        return Convert.ToHexString(digest).ToLowerInvariant();
    }

    private static string CanonicalIdentityJson(ContextIdentity identity)
    {
        using var buffer = new MemoryStream();
        using (var writer = new Utf8JsonWriter(buffer, Compact))
        {
            // Reuses ContextWriter's own field-by-field identity writer, so
            // this fold never drifts from what the embedded envelope itself
            // carries under `compilations[i].identity`.
            ContextWriter.WriteIdentity(writer, identity);
        }

        var node = JsonNode.Parse(buffer.ToArray());
        return Canonicalize(node);
    }

    /// <summary>Sorts every object's keys ordinally, recursively, and writes
    /// compact JSON; arrays keep their given order. Independent of any
    /// producer's own writer-key order.</summary>
    private static string Canonicalize(JsonNode? node)
    {
        using var buffer = new MemoryStream();
        using (var writer = new Utf8JsonWriter(buffer, Compact))
        {
            WriteCanonical(writer, node);
        }

        return Encoding.UTF8.GetString(buffer.ToArray());
    }

    private static void WriteCanonical(Utf8JsonWriter writer, JsonNode? node)
    {
        switch (node)
        {
            case null:
                writer.WriteNullValue();
                break;

            case JsonObject obj:
                writer.WriteStartObject();
                foreach (var key in obj.Select(kv => kv.Key).OrderBy(k => k, StringComparer.Ordinal))
                {
                    writer.WritePropertyName(key);
                    WriteCanonical(writer, obj[key]);
                }

                writer.WriteEndObject();
                break;

            case JsonArray arr:
                writer.WriteStartArray();
                foreach (var item in arr)
                {
                    WriteCanonical(writer, item);
                }

                writer.WriteEndArray();
                break;

            default:
                node.WriteTo(writer);
                break;
        }
    }
}
