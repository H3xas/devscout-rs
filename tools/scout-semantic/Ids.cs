using Microsoft.CodeAnalysis;

namespace ScoutSemantic;

/// <summary>
/// Symbol id normalisation (§3.5). Reproduces the def ids devscout writes:
/// namespace, then the type chain joined with '+' from outermost to innermost,
/// so a nested type is <c>Ns.Outer+Inner</c> and never carries generic arity.
/// </summary>
internal static class Ids
{
    /// <summary>Id of any type, or null when the type is unknown.</summary>
    public static string? TypeId(ITypeSymbol? type) => type switch
    {
        null => null,
        IArrayTypeSymbol a => TypeId(a.ElementType) is { } e ? e + "[]" : null,
        IPointerTypeSymbol p => TypeId(p.PointedAtType) is { } e ? e + "*" : null,
        INamedTypeSymbol n => NamedTypeId(n),
        // Type parameters, dynamic, function pointers: no devscout def can exist,
        // so the display form is the most useful thing to report.
        _ => type.ToDisplayString(),
    };

    /// <summary>Id of a named type: <c>Ns.Outer+Inner</c>, arity dropped.</summary>
    public static string NamedTypeId(INamedTypeSymbol type)
    {
        var t = type.OriginalDefinition;
        var chain = new List<string>(2);
        for (INamedTypeSymbol? c = t; c is not null; c = c.ContainingType)
        {
            chain.Add(c.Name);
        }

        chain.Reverse();
        var body = string.Join("+", chain);
        var ns = t.ContainingNamespace is { IsGlobalNamespace: false } n ? n.ToDisplayString() : "";
        return ns.Length == 0 ? body : ns + "." + body;
    }

    /// <summary>devscout's def kind for a named type.</summary>
    public static string TypeKindName(INamedTypeSymbol t)
    {
        if (t.IsRecord)
        {
            return "record";
        }

        return t.TypeKind switch
        {
            TypeKind.Class => "class",
            TypeKind.Struct => "struct",
            TypeKind.Interface => "interface",
            TypeKind.Enum => "enum",
            TypeKind.Delegate => "delegate",
            _ => t.TypeKind.ToString().ToLowerInvariant(),
        };
    }

    /// <summary>True for a member of an enum type (a named enum constant).</summary>
    public static bool IsEnumMember(ISymbol s) =>
        s is IFieldSymbol { ContainingType.TypeKind: TypeKind.Enum };

    /// <summary>
    /// The (target, targetKind) pair of §3.5: an enum member resolves to
    /// <c>Ns.E.Member</c>, everything else to its containing type's id.
    /// </summary>
    public static (string? Target, string? Kind) MemberTarget(ISymbol s)
    {
        var ct = s.ContainingType;
        if (ct is null)
        {
            return (null, null);
        }

        return IsEnumMember(s)
            ? (NamedTypeId(ct) + "." + s.Name, "enum-member")
            : (NamedTypeId(ct), TypeKindName(ct));
    }

    /// <summary>method | property | field | event | enum-member.</summary>
    public static string MemberKind(ISymbol s) => s switch
    {
        IFieldSymbol f when IsEnumMember(f) => "enum-member",
        IMethodSymbol => "method",
        IPropertySymbol => "property",
        IFieldSymbol => "field",
        IEventSymbol => "event",
        _ => s.Kind.ToString().ToLowerInvariant(),
    };
}
