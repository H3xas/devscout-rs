// References: verbatim/interpolated/raw/u8 string forms, XML doc comments, verbatim identifiers, comment/string trivia.
namespace Syntax.Trivia;

/// <summary>Reads <see cref="TriviaHost.Name"/> and <seealso cref="Touch"/>.</summary>
class TriviaHost
{
    public string? Name;
    public int Value;

    public void Touch()
    {
    }

    public string Describe() => "";

    /// <param name="text">Input text.</param>
    /// <returns>The same text.</returns>
    public string Format(string text) => text;

    /// <inheritdoc/>
    public override string ToString() => Describe();
}

// line comment
class Zażółć
{
    public int Wartość;
}

class TriviaHost2
{
}

/** Verbatim identifier fixture. */
class @class
{
    public int @event;

    public void @int()
    {
    }
}

class TriviaUser
{
    const string Prefix = "n=";

    public void Run(TriviaHost host)
    {
        string verbatimPath = @"C:\path";
        string interpolated = $"{host.Name} {host.Value:D3} {host.Describe()}";
        string verbatimInterpA = $@"{host.Name}\n";
        string verbatimInterpB = @$"{host.Name}\n";

        string raw = """
            line one
            line two
            """;

        string rawInterp = $$"""{ {{host.Value}} }""";

        ReadOnlySpan<byte> utf8Bytes = "abc"u8;
        char letter = 'x';
        string escaped = "\u0041";
        int x = 1 /* block comment */ + 2;

        string concatenated = Prefix + host.Name;
        string withNameof = $"arg={nameof(host)}";

        var @namespace = new @class();
        @namespace.@event = 1;
        @namespace.@int();

        int ćwierć = 4;
        string nonAsciiText = "zażółć";

        // var ghost = new TriviaGhost(); ghost.Touch();
        string ghostInString = "new TriviaGhost().Touch()";
        string ghostInRaw = """
            class TriviaGhost { }
            """;

        _ = verbatimPath;
        _ = interpolated;
        _ = verbatimInterpA;
        _ = verbatimInterpB;
        _ = raw;
        _ = rawInterp;
        _ = utf8Bytes.Length;
        _ = letter;
        _ = escaped;
        _ = x;
        _ = concatenated;
        _ = withNameof;
        _ = ćwierć;
        var żółw = new Zażółć();
        _ = żółw.Wartość;
        _ = nonAsciiText;
        _ = ghostInString;
        _ = ghostInRaw;
    }
}
