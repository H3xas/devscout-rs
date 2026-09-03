using System.Text.Json.Serialization;

namespace ScoutSemantic;

/// <summary>
/// One member reference emitted to <c>refs.jsonl</c>.
/// Key order in the JSON output is fixed by <see cref="JsonPropertyOrderAttribute"/>
/// and every key is always written, <c>null</c> when unknown.
/// </summary>
internal sealed class RefRecord
{
    [JsonPropertyName("file")] [JsonPropertyOrder(0)] public string File { get; init; } = "";
    [JsonPropertyName("startLine")] [JsonPropertyOrder(1)] public int StartLine { get; init; }
    [JsonPropertyName("line")] [JsonPropertyOrder(2)] public int Line { get; init; }
    [JsonPropertyName("shape")] [JsonPropertyOrder(3)] public string Shape { get; init; } = "";
    [JsonPropertyName("receiverKind")] [JsonPropertyOrder(4)] public string ReceiverKind { get; init; } = "";
    [JsonPropertyName("receiverText")] [JsonPropertyOrder(5)] public string? ReceiverText { get; init; }
    [JsonPropertyName("receiver")] [JsonPropertyOrder(6)] public string? Receiver { get; init; }
    [JsonPropertyName("member")] [JsonPropertyOrder(7)] public string Member { get; init; } = "";
    [JsonPropertyName("memberKind")] [JsonPropertyOrder(8)] public string MemberKind { get; init; } = "";
    [JsonPropertyName("target")] [JsonPropertyOrder(9)] public string? Target { get; init; }
    [JsonPropertyName("targetKind")] [JsonPropertyOrder(10)] public string? TargetKind { get; init; }
    [JsonPropertyName("targetFile")] [JsonPropertyOrder(11)] public string? TargetFile { get; init; }
    [JsonPropertyName("targetUnit")] [JsonPropertyOrder(12)] public string? TargetUnit { get; init; }
    [JsonPropertyName("ext")] [JsonPropertyOrder(13)] public bool Ext { get; init; }
    [JsonPropertyName("external")] [JsonPropertyOrder(14)] public bool External { get; init; }
    [JsonPropertyName("ambiguous")] [JsonPropertyOrder(15)] public bool Ambiguous { get; init; }
    [JsonPropertyName("unit")] [JsonPropertyOrder(16)] public string Unit { get; init; } = "";

    /// <summary>The sort/dedup key of §3.6: (file, startLine, line, member, target, ambiguous).</summary>
    public int CompareKeyTo(RefRecord other)
    {
        int c = string.CompareOrdinal(File, other.File);
        if (c != 0) return c;
        c = StartLine.CompareTo(other.StartLine);
        if (c != 0) return c;
        c = Line.CompareTo(other.Line);
        if (c != 0) return c;
        c = string.CompareOrdinal(Member, other.Member);
        if (c != 0) return c;
        c = string.CompareOrdinal(Target ?? "", other.Target ?? "");
        if (c != 0) return c;
        return Ambiguous.CompareTo(other.Ambiguous);
    }
}

/// <summary>One project (unit) emitted to <c>units.jsonl</c>.</summary>
internal sealed class UnitRecord
{
    [JsonPropertyName("name")] [JsonPropertyOrder(0)] public string Name { get; init; } = "";
    [JsonPropertyName("path")] [JsonPropertyOrder(1)] public string? Path { get; init; }
    [JsonPropertyName("tfm")] [JsonPropertyOrder(2)] public string? Tfm { get; init; }
    [JsonPropertyName("test")] [JsonPropertyOrder(3)] public bool Test { get; init; }
    [JsonPropertyName("status")] [JsonPropertyOrder(4)] public string Status { get; init; } = "";
    [JsonPropertyName("diagnostics")] [JsonPropertyOrder(5)] public int Diagnostics { get; init; }
    [JsonPropertyName("refs")] [JsonPropertyOrder(6)] public List<string> Refs { get; init; } = new();
    [JsonPropertyName("files")] [JsonPropertyOrder(7)] public List<string> Files { get; init; } = new();
}

/// <summary>One in-tree named type or enum member emitted to <c>defs.jsonl</c>.</summary>
internal sealed class DefRecord
{
    [JsonPropertyName("id")] [JsonPropertyOrder(0)] public string Id { get; init; } = "";
    [JsonPropertyName("kind")] [JsonPropertyOrder(1)] public string Kind { get; init; } = "";
    [JsonPropertyName("file")] [JsonPropertyOrder(2)] public string File { get; init; } = "";
    [JsonPropertyName("line")] [JsonPropertyOrder(3)] public int Line { get; init; }
    [JsonPropertyName("unit")] [JsonPropertyOrder(4)] public string Unit { get; init; } = "";
    [JsonPropertyName("test")] [JsonPropertyOrder(5)] public bool Test { get; init; }
}
