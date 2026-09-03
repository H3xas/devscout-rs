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
}

/// <summary>Entry point. Registers MSBuild before any MSBuild-touching type is JIT-ed.</summary>
internal static class Program
{
    private const string Usage = """
        usage: scout-semantic <path.sln|path.csproj> --root <repo-root> --out <refs.jsonl>
                   [--units <units.jsonl>] [--defs <defs.jsonl>]
                   [--scope dir[,dir]]       walk only documents under these root-relative dirs
                   [--projects glob[,glob]]  load/walk only projects whose name matches
                   [--tfm net9.0]            variant to keep for multi-targeting projects
                   [-p Name=Value | -p:Name=Value]   MSBuild global property, repeatable
                   [--strict]                exit 2 if any project failed to load

        exit codes: 0 ok, 1 usage/IO, 2 strict failure, 3 zero projects loaded
        """;

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

        if (options.Out.Length == 0)
        {
            throw new ArgumentException("missing --out");
        }

        return options;
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

        var walker = new Walker(paths, assemblyToUnit);
        var refs = new List<RefRecord>();
        var defs = new List<DefRecord>();
        var units = new List<UnitRecord>();
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
                    walker.WalkDocument(model, tree, rel, loaded.Name, refs);
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

        load.Workspace.Dispose();

        var sortedRefs = Dedup(refs.OrderBy(r => r, RefComparer.Instance).ToList());
        try
        {
            WriteJsonl(options.Out, sortedRefs);
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

        var hardFailure = failedUnits > 0
            || load.Failures.Any(f => f.Kind == Microsoft.CodeAnalysis.WorkspaceDiagnosticKind.Failure);
        if (options.Strict && hardFailure)
        {
            Console.Error.WriteLine("error: --strict and at least one project failed to load");
            return 2;
        }

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
