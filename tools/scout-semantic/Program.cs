using System.Runtime.CompilerServices;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;

using Microsoft.Build.Locator;

namespace ScoutSemantic;

/// <summary>Parsed command line (§3.2). Holds no MSBuild or Roslyn types.</summary>
internal sealed class Options
{
    public string Input { get; set; } = "";

    public string Root { get; set; } = "";

    public string Out { get; set; } = "";

    public string? Units { get; set; }

    public string? Defs { get; set; }

    public List<string> Scope { get; } = new();

    public List<Regex> ProjectGlobs { get; } = new();

    public string? Tfm { get; set; }

    public Dictionary<string, string> Properties { get; } = new(StringComparer.OrdinalIgnoreCase);

    public bool Strict { get; set; }

    public HashSet<string> Emit { get; } = new(StringComparer.Ordinal);

    public string? Facts { get; set; }

    public string? Repo { get; set; }

    public bool NoGit { get; set; }

    public List<string> PublishCalls { get; } = new();

    public List<string> ConsumerBases { get; } = new();
}

/// <summary>Entry point. Registers MSBuild before any MSBuild-touching type is JIT-ed.</summary>
internal static class Program
{
    private const string Usage = """
        usage: scout-semantic <path.sln|path.csproj> --root <repo-root> --out <refs.jsonl>
                   [--emit mode[,mode]]      oracle | flowtrace-facts, repeatable (default: oracle)
                   [--units <units.jsonl>] [--defs <defs.jsonl>]
                   [--facts <path|->]        fact document, default out/facts/<repo>.json, - is stdout
                   [--repo <id>]             repo id in the fact header (default: --root's last segment)
                   [--no-git]                do not stamp git identity in the fact header
                   [--publish-calls a,b]     extra publish method names, repeatable
                   [--consumer-bases a,b]    extra consumer base type names, repeatable
                   [--scope dir[,dir]]       walk only documents under these root-relative dirs
                   [--projects glob[,glob]]  load/walk only projects whose name matches
                   [--tfm net9.0]            variant to keep for multi-targeting projects
                   [-p Name=Value | -p:Name=Value]   MSBuild global property, repeatable
                   [--strict]                exit 2 on a failed project or an unresolved fact site

        --out is required only when `oracle` is among the emitted modes.

        exit codes: 0 ok, 1 usage/IO, 2 strict failure, 3 zero projects loaded
        """;

    /// <summary>Today's refs/units/defs output.</summary>
    public const string EmitOracle = "oracle";

    /// <summary>The flow tracer's fact document.</summary>
    public const string EmitFacts = "flowtrace-facts";

    public static int Main(string[] args)
    {
        Options options;
        try
        {
            options = Parse(args);
        }
        catch (ArgumentException e)
        {
            Console.Error.WriteLine($"error: {e.Message}");
            Console.Error.WriteLine(Usage);
            return 1;
        }

        if (!File.Exists(options.Input))
        {
            Console.Error.WriteLine($"error: no such file: {options.Input}");
            return 1;
        }

        if (!Directory.Exists(options.Root))
        {
            Console.Error.WriteLine($"error: --root is not a directory: {options.Root}");
            return 1;
        }

        try
        {
            if (!MSBuildLocator.IsRegistered)
            {
                var instance = MSBuildLocator.RegisterDefaults();
                Console.Error.WriteLine($"msbuild {instance.Version} at {instance.MSBuildPath}");
            }
        }
        catch (InvalidOperationException e)
        {
            // Roslyn >= 4.9 evaluates projects in an out-of-process BuildHost, so a
            // missing in-process MSBuild is a warning rather than a hard failure.
            Console.Error.WriteLine($"warning: MSBuildLocator.RegisterDefaults failed: {e.Message}");
        }

        return Runner.Run(options);
    }

    private static Options Parse(string[] args)
    {
        var options = new Options();
        var haveInput = false;

        for (var i = 0; i < args.Length; i++)
        {
            var arg = args[i];
            string Value(string name)
            {
                if (i + 1 >= args.Length)
                {
                    throw new ArgumentException($"{name} needs a value");
                }

                return args[++i];
            }

            switch (arg)
            {
                case "--root":
                    options.Root = Value(arg);
                    break;
                case "--out":
                    options.Out = Value(arg);
                    break;
                case "--units":
                    options.Units = Value(arg);
                    break;
                case "--defs":
                    options.Defs = Value(arg);
                    break;
                case "--emit":
                    foreach (var mode in Split(Value(arg)))
                    {
                        if (mode is not (EmitOracle or EmitFacts))
                        {
                            throw new ArgumentException($"unknown --emit mode: {mode}");
                        }

                        options.Emit.Add(mode);
                    }

                    break;
                case "--facts":
                    options.Facts = Value(arg);
                    break;
                case "--repo":
                    options.Repo = RepoId(Value(arg));
                    break;
                case "--no-git":
                    options.NoGit = true;
                    break;
                case "--publish-calls":
                    options.PublishCalls.AddRange(Split(Value(arg)));
                    break;
                case "--consumer-bases":
                    options.ConsumerBases.AddRange(Split(Value(arg)));
                    break;
                case "--scope":
                    options.Scope.AddRange(Split(Value(arg)));
                    break;
                case "--projects":
                    options.ProjectGlobs.AddRange(Split(Value(arg)).Select(Glob));
                    break;
                case "--tfm":
                    options.Tfm = Value(arg);
                    break;
                case "--strict":
                    options.Strict = true;
                    break;
                case "-p":
                    AddProperty(options, Value(arg));
                    break;
                default:
                    if (arg.StartsWith("-p:", StringComparison.Ordinal))
                    {
                        AddProperty(options, arg[3..]);
                    }
                    else if (arg.StartsWith('-'))
                    {
                        throw new ArgumentException($"unknown option: {arg}");
                    }
                    else if (haveInput)
                    {
                        throw new ArgumentException($"unexpected argument: {arg}");
                    }
                    else
                    {
                        options.Input = arg;
                        haveInput = true;
                    }

                    break;
            }
        }

        if (!haveInput)
        {
            throw new ArgumentException("missing <path.sln|path.csproj>");
        }

        if (options.Root.Length == 0)
        {
            throw new ArgumentException("missing --root");
        }

        if (options.Emit.Count == 0)
        {
            options.Emit.Add(EmitOracle);
        }

        if (options.Emit.Contains(EmitOracle) && options.Out.Length == 0)
        {
            throw new ArgumentException("missing --out");
        }

        return options;
    }

    /// <summary>
    /// The repo id is one path segment: it names the default fact document
    /// under <c>out/facts/</c>, so a separator or a <c>..</c> in it would let
    /// the output escape that directory.
    /// </summary>
    private static string RepoId(string value)
    {
        if (value.Length == 0)
        {
            throw new ArgumentException("--repo needs a value");
        }

        if (value.Contains('/') || value.Contains('\\') || value == ".." || value == ".")
        {
            throw new ArgumentException($"--repo must be a single path segment, got: {value}");
        }

        return value;
    }

    private static void AddProperty(Options options, string pair)
    {
        var eq = pair.IndexOf('=');
        if (eq <= 0)
        {
            throw new ArgumentException($"-p expects Name=Value, got: {pair}");
        }

        options.Properties[pair[..eq]] = pair[(eq + 1)..];
    }

    private static IEnumerable<string> Split(string value) =>
        value.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);

    private static Regex Glob(string pattern) =>
        new("^" + Regex.Escape(pattern).Replace("\\*", ".*").Replace("\\?", ".") + "$",
            RegexOptions.CultureInvariant);
}

/// <summary>
/// All Roslyn/MSBuild work. Kept out of <see cref="Program.Main"/> and marked
/// NoInlining so that MSBuildLocator has already run before the JIT resolves
/// any <c>Microsoft.CodeAnalysis.MSBuild</c> type (§3.1).
/// </summary>
internal static class Runner
{
    private static readonly JsonSerializerOptions Json = new()
    {
        // The default encoder escapes '+' as the \u002B escape, which would corrupt the
        // Ns.Outer+Inner nested-type spelling.
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        WriteIndented = false,
        DefaultIgnoreCondition = JsonIgnoreCondition.Never,
    };

    [MethodImpl(MethodImplOptions.NoInlining)]
    public static int Run(Options options)
    {
        var load = Loader.LoadAsync(options).GetAwaiter().GetResult();
        if (load.Projects.Count == 0)
        {
            Console.Error.WriteLine("error: zero projects loaded");
            return 3;
        }

        var paths = new RepoPaths(options.Root, options.Scope);
        var assemblyToUnit = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var loaded in load.Projects)
        {
            var assembly = loaded.Project.AssemblyName;
            if (!string.IsNullOrEmpty(assembly))
            {
                assemblyToUnit.TryAdd(assembly, loaded.Name);
            }
        }

        var wantOracle = options.Emit.Contains(Program.EmitOracle);
        var factsWalker = options.Emit.Contains(Program.EmitFacts)
            ? new FactsWalker(options.PublishCalls, options.ConsumerBases)
            : null;

        var walker = new Walker(paths, assemblyToUnit);
        var refs = new List<RefRecord>();
        var defs = new List<DefRecord>();
        var units = new List<UnitRecord>();
        var facts = new List<FactRecord>();
        var failedUnits = 0;

        foreach (var loaded in load.Projects)
        {
            var project = loaded.Project;
            var compilation = project.GetCompilationAsync().GetAwaiter().GetResult();
            var status = compilation is null ? "failed" : "ok";
            var files = new List<string>();
            var before = refs.Count;

            if (compilation is null)
            {
                failedUnits++;
            }
            else
            {
                foreach (var document in project.Documents)
                {
                    var rel = paths.Relative(document.FilePath);
                    if (rel is null)
                    {
                        continue;
                    }

                    var tree = document.GetSyntaxTreeAsync().GetAwaiter().GetResult();
                    if (tree is null)
                    {
                        continue;
                    }

                    files.Add(rel);
                    var model = compilation.GetSemanticModel(tree);
                    if (wantOracle)
                    {
                        walker.WalkDocument(model, tree, rel, loaded.Name, refs);
                    }

                    if (factsWalker is not null)
                    {
                        try
                        {
                            factsWalker.WalkDocument(model, tree, rel, facts);
                        }
                        catch (Exception e)
                        {
                            // One document's shape must not cost the rest of the
                            // run, but it must not pass for a clean walk either:
                            // it is reported and counted, so --strict still fails.
                            Console.Error.WriteLine($"warning: facts: {rel}: {e.GetType().Name}: {e.Message}");
                            factsWalker.CountUnresolved();
                        }
                    }

                    if (options.Defs is not null)
                    {
                        walker.CollectDefs(model, tree, rel, loaded.Name, defs);
                    }
                }
            }

            files.Sort(StringComparer.Ordinal);
            Console.Error.WriteLine(
                $"  {loaded.Name} [{loaded.Tfm ?? "?"}] {status} {files.Count} files {refs.Count - before} refs");

            if (options.Units is not null)
            {
                var errors = compilation?.GetDiagnostics()
                    .Count(d => d.Severity == Microsoft.CodeAnalysis.DiagnosticSeverity.Error) ?? 0;
                units.Add(new UnitRecord
                {
                    Name = loaded.Name,
                    Path = paths.Relative(project.FilePath) ?? project.FilePath,
                    Tfm = loaded.Tfm,
                    Test = Loader.IsTestProject(project.FilePath),
                    Status = status,
                    Diagnostics = errors,
                    Refs = Loader.ReferenceNames(load.Solution, project),
                    Files = files,
                });
            }
        }

        var unitIds = load.Projects
            .Select(p => $"{p.Name}|{p.Tfm ?? "?"}")
            .OrderBy(id => id, StringComparer.Ordinal)
            .ToList();

        load.Workspace.Dispose();

        var sortedRefs = Dedup(refs.OrderBy(r => r, RefComparer.Instance).ToList());
        try
        {
            if (wantOracle)
            {
                WriteJsonl(options.Out, sortedRefs);
            }

            if (options.Units is not null)
            {
                // Same tie-break as Loader's project sort, and for the same
                // reason: List.Sort is unstable, names are not unique, and
                // units.jsonl is diffed byte-for-byte against a committed
                // snapshot.
                units.Sort((a, b) =>
                {
                    var byName = string.CompareOrdinal(a.Name, b.Name);
                    return byName != 0 ? byName : string.CompareOrdinal(a.Path, b.Path);
                });
                WriteJsonl(options.Units, units);
            }

            if (options.Defs is not null)
            {
                WriteJsonl(options.Defs, DedupDefs(defs));
            }
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            Console.Error.WriteLine($"error: {e.Message}");
            return 1;
        }

        Console.Error.WriteLine(
            $"{sortedRefs.Count} refs, {load.Projects.Count} units ({failedUnits} failed), "
            + $"{load.Failures.Count} workspace diagnostics");

        if (factsWalker is not null)
        {
            var written = WriteFacts(options, paths, unitIds, facts, factsWalker.Unresolved);
            if (written != 0)
            {
                return written;
            }
        }

        var hardFailure = failedUnits > 0
            || load.Failures.Any(f => f.Kind == Microsoft.CodeAnalysis.WorkspaceDiagnosticKind.Failure);
        if (options.Strict && hardFailure)
        {
            Console.Error.WriteLine("error: --strict and at least one project failed to load");
            return 2;
        }

        if (options.Strict && factsWalker is { Unresolved: > 0 })
        {
            Console.Error.WriteLine("error: --strict and at least one unresolved fact site");
            return 2;
        }

        return 0;
    }

    /// <summary>Builds the header, orders the facts and writes the document; 0 on success.</summary>
    private static int WriteFacts(
        Options options, RepoPaths paths, List<string> unitIds, List<FactRecord> facts, int unresolved)
    {
        var root = paths.Root;
        var repo = options.Repo is { Length: > 0 } given ? given : root[(root.LastIndexOf('/') + 1)..];
        var path = options.Facts is { Length: > 0 } target ? target : Path.Combine("out", "facts", repo + ".json");
        var version = FactsWriter.ProducerVersion();
        var header = new FactsHeader
        {
            Version = version,
            Repo = repo,
            Solution = paths.Relative(options.Input) ?? Path.GetFileName(options.Input),
            Units = unitIds,
            Git = options.NoGit ? null : FactsWriter.Probe(root),
        };

        var ordered = FactsWriter.Order(facts);
        try
        {
            FactsWriter.Write(path, header, ordered);
        }
        catch (FactSchemaException e)
        {
            Console.Error.WriteLine($"error: fact schema violation: {e.Message}");
            return 1;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            Console.Error.WriteLine($"error: {e.Message}");
            return 1;
        }

        Console.Error.WriteLine($"facts: {ordered.Count} facts, {unresolved} unresolved -> {path}");
        return 0;
    }

    private sealed class RefComparer : IComparer<RefRecord>
    {
        public static readonly RefComparer Instance = new();

        public int Compare(RefRecord? x, RefRecord? y) =>
            x is null ? (y is null ? 0 : -1) : y is null ? 1 : x.CompareKeyTo(y);
    }

    private static List<RefRecord> Dedup(List<RefRecord> sorted)
    {
        var kept = new List<RefRecord>(sorted.Count);
        foreach (var record in sorted)
        {
            if (kept.Count == 0 || kept[^1].CompareKeyTo(record) != 0)
            {
                kept.Add(record);
            }
        }

        return kept;
    }

    private static List<DefRecord> DedupDefs(List<DefRecord> defs)
    {
        var sorted = defs
            .OrderBy(d => d.Id, StringComparer.Ordinal)
            .ThenBy(d => d.File, StringComparer.Ordinal)
            .ThenBy(d => d.Line)
            .ToList();
        var kept = new List<DefRecord>(sorted.Count);
        foreach (var def in sorted)
        {
            var last = kept.Count == 0 ? null : kept[^1];
            if (last is null || last.Id != def.Id || last.File != def.File || last.Line != def.Line)
            {
                kept.Add(def);
            }
        }

        return kept;
    }

    private static void WriteJsonl<T>(string path, IEnumerable<T> records)
    {
        var directory = Path.GetDirectoryName(Path.GetFullPath(path));
        if (!string.IsNullOrEmpty(directory))
        {
            Directory.CreateDirectory(directory);
        }

        using var writer = new StreamWriter(path, false, new UTF8Encoding(false)) { NewLine = "\n" };
        foreach (var record in records)
        {
            writer.WriteLine(JsonSerializer.Serialize(record, Json));
        }
    }
}
