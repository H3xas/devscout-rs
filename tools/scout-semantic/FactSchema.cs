namespace ScoutSemantic;

/// <summary>Raised when a fact does not satisfy the flow tracer's fact schema.</summary>
internal sealed class FactSchemaException : Exception
{
    /// <summary>Creates the exception carrying the reason reported on stderr.</summary>
    public FactSchemaException(string reason)
        : base(reason)
    {
    }
}

/// <summary>
/// One fact of the flow tracer's fact set: an ordered list of (key, value) pairs
/// whose order is the serialised property order. Values are <see cref="string"/>
/// or <see cref="int"/>; a null optional value is simply not added.
/// </summary>
internal sealed class FactRecord
{
    private readonly List<KeyValuePair<string, object?>> _fields = new(8);

    /// <summary>Starts a fact with the three keys every fact carries, in order.</summary>
    public FactRecord(string type, string file, int line)
    {
        Type = type;
        File = file;
        Line = line;
        _fields.Add(new KeyValuePair<string, object?>("type", type));
        _fields.Add(new KeyValuePair<string, object?>("file", file));
        _fields.Add(new KeyValuePair<string, object?>("line", line));
    }

    /// <summary>The fact kind, one of <see cref="FactSchema"/>'s known types.</summary>
    public string Type { get; }

    /// <summary>Repository-relative, forward-slashed path of the site.</summary>
    public string File { get; }

    /// <summary>1-based line of the site.</summary>
    public int Line { get; }

    /// <summary>The fact's keys and values in serialisation order.</summary>
    public IReadOnlyList<KeyValuePair<string, object?>> Fields => _fields;

    /// <summary>Appends a string field; a null value is dropped, which is how optional fields are omitted.</summary>
    public FactRecord With(string key, string? value)
    {
        if (value is not null)
        {
            _fields.Add(new KeyValuePair<string, object?>(key, value));
        }

        return this;
    }

    /// <summary>Appends an integer field.</summary>
    public FactRecord With(string key, int value)
    {
        _fields.Add(new KeyValuePair<string, object?>(key, value));
        return this;
    }

    /// <summary>True when the fact carries <paramref name="key"/> at all.</summary>
    public bool Has(string key)
    {
        foreach (var field in _fields)
        {
            if (string.Equals(field.Key, key, StringComparison.Ordinal))
            {
                return true;
            }
        }

        return false;
    }

    /// <summary>The value stored under <paramref name="key"/>, or null when absent.</summary>
    public object? Value(string key)
    {
        foreach (var field in _fields)
        {
            if (string.Equals(field.Key, key, StringComparison.Ordinal))
            {
                return field.Value;
            }
        }

        return null;
    }
}

/// <summary>
/// Mirrors the flow tracer's fact schema (docs/fact-schema.md, lib/facts.js) as of
/// its commit 56d5f60; every fact needs type, file (repository-relative, forward
/// slashes), line (1-based).
/// </summary>
internal static class FactSchema
{
    // The whole backend table is embedded, including the kinds this sidecar does
    // not emit yet: the table is the contract, not the subset currently produced.
    private static readonly Dictionary<string, string[]> RequiredFields = new(StringComparer.Ordinal)
    {
        ["route"] = new[] { "controller", "action", "verb", "template" },
        ["http_out"] = new[] { "configKey", "template" },
        ["publish"] = new[] { "message" },
        ["consume"] = new[] { "message", "consumer" },
        ["worker_processor"] = new[] { "workType", "processor" },
        ["signalr_push"] = new[] { "method" },
        ["exchange_name"] = new[] { "name", "constant" },
        ["di_binding"] = new[] { "iface", "impl" },
        ["iface_impl"] = new[] { "class", "iface" },
        ["ctor_field"] = new[] { "class", "field", "paramType" },
        ["method_call"] = new[] { "class", "method", "field", "calledMethod" },
        ["branch_point"] = new[] { "class", "method", "kind", "text", "endLine" },
        ["redis_publish"] = new[] { "channel" },
        ["message_class"] = new[] { "name", "fqn" },
        ["queue_name"] = new[] { "message", "name" },
        ["method_span"] = new[] { "class", "method", "endLine" },
        ["param_source"] = new[] { "class", "method", "param", "source", "via" },
        ["exception_map"] = new[] { "scope", "class", "exception", "status" },
    };

    /// <summary>Every fact type the schema knows, ordinal-sorted.</summary>
    public static IEnumerable<string> KnownTypes => RequiredFields.Keys.OrderBy(k => k, StringComparer.Ordinal);

    /// <summary>
    /// Throws <see cref="FactSchemaException"/> when the fact is not serialisable
    /// as a valid fact: unknown type, empty or back-slashed file, a line before 1,
    /// a missing or null required field, or a self-declared provenance.
    /// </summary>
    public static void Validate(FactRecord fact)
    {
        if (!RequiredFields.TryGetValue(fact.Type, out var required))
        {
            throw new FactSchemaException($"unknown fact type '{fact.Type}'");
        }

        if (fact.File.Length == 0)
        {
            throw new FactSchemaException($"{fact.Type} fact has an empty file");
        }

        if (fact.File.Contains('\\', StringComparison.Ordinal))
        {
            throw new FactSchemaException($"{fact.Type} fact file '{fact.File}' is not forward-slashed");
        }

        if (fact.Line < 1)
        {
            throw new FactSchemaException($"{fact.Type} fact in '{fact.File}' has line {fact.Line}, expected 1-based");
        }

        foreach (var name in required)
        {
            if (fact.Value(name) is null)
            {
                throw new FactSchemaException(
                    $"{fact.Type} fact in '{fact.File}' line {fact.Line} is missing required field '{name}'");
            }
        }

        if (fact.Has("provenance"))
        {
            throw new FactSchemaException(
                $"{fact.Type} fact in '{fact.File}' line {fact.Line} declares its own provenance");
        }
    }
}
