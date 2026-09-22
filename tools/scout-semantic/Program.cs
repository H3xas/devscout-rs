using System.ComponentModel;
using System.Diagnostics;
using System.Reflection;
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;
using System.Runtime.CompilerServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;

using Microsoft.Build.Exceptions;
using Microsoft.Build.Locator;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;

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

    /// <summary>Every requested target, in the order given; repeatable, each its own compilation identity.</summary>
    public List<string> Tfms { get; } = new();

    public Dictionary<string, string> Properties { get; } = new(StringComparer.OrdinalIgnoreCase);

    public bool Strict { get; set; }

    public HashSet<string> Emit { get; } = new(StringComparer.Ordinal);

    public string? Facts { get; set; }

    /// <summary>Build-context envelope output path (<c>--emit context</c>).</summary>
    public string? Context { get; set; }

    public string? Repo { get; set; }

    public bool NoGit { get; set; }

    public List<string> PublishCalls { get; } = new();

    public List<string> ConsumerBases { get; } = new();

    public string? CompilerFacts { get; set; }

    public List<string> Capabilities { get; } = new();
}

/// <summary>Entry point. Registers MSBuild before any MSBuild-touching type is JIT-ed.</summary>
internal static class Program
{
    private const string Usage = """
        usage: scout-semantic <path.sln|path.csproj> --root <repo-root> --out <refs.jsonl>
                   [--emit mode[,mode]]      oracle | flowtrace-facts | compiler-facts | context, repeatable (default: oracle)
                   [--units <units.jsonl>] [--defs <defs.jsonl>]
                   [--facts <path|->]        fact document, default out/facts/<repo>.json, - is stdout
                   [--compiler-facts <path|->]  compiler-facts protocol document, - is stdout
                   [--capabilities a,b]      requested capability names, repeatable, compiler-facts only
                   [--context <path|->]      build-context envelope, required with `--emit context`, - is stdout
                   [--repo <id>]             repo id in the fact/context header (default: --root's last segment)
                   [--no-git]                do not stamp git identity in the fact header
                   [--publish-calls a,b]     extra publish method names, repeatable
                   [--consumer-bases a,b]    extra consumer base type names, repeatable
                   [--scope dir[,dir]]       walk only documents under these root-relative dirs
                   [--projects glob[,glob]]  load/walk only projects whose name matches
                   [--tfm net9.0]            requested target, repeatable; each is its own compilation identity
                   [-p Name=Value | -p:Name=Value]   MSBuild global property, repeatable
                   [--strict]                exit 2 on a failed project, an unresolved fact site, or (with
                                              `--emit context`) an artifact whose rollup state is not complete

        --out is required only when `oracle` is among the emitted modes.
        --compiler-facts is required only when `compiler-facts` is among the emitted modes; a
        requested target/configuration/platform reuses --tfm and -p Configuration=/-p Platform=,
        the same machinery every other mode already has.

        exit codes: 0 ok, 1 usage/IO, 2 strict failure, 3 zero projects loaded
        """;

    /// <summary>Today's refs/units/defs output.</summary>
    public const string EmitOracle = "oracle";

    /// <summary>The flow tracer's fact document.</summary>
    public const string EmitFacts = "flowtrace-facts";

    /// <summary>The compiler-facts protocol document.</summary>
    public const string EmitCompilerFacts = "compiler-facts";

    /// <summary>The build-context envelope: one record per compilation identity, its own output path.</summary>
    public const string EmitContext = "context";

    /// <summary>Version of the MSBuild instance <see cref="MSBuildLocator"/> registered, or "unknown".</summary>
    internal static string MsBuildVersion { get; private set; } = "unknown";

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
                MsBuildVersion = instance.Version.ToString();
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
                        if (mode is not (EmitOracle or EmitFacts or EmitCompilerFacts or EmitContext))
                        {
                            throw new ArgumentException($"unknown --emit mode: {mode}");
                        }

                        options.Emit.Add(mode);
                    }

                    break;
                case "--facts":
                    options.Facts = Value(arg);
                    break;
                case "--compiler-facts":
                    options.CompilerFacts = Value(arg);
                    break;
                case "--capabilities":
                    options.Capabilities.AddRange(Split(Value(arg)));
                    break;
                case "--context":
                    options.Context = Value(arg);
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
                    // Repeatable, matching --publish-calls's own pattern: each
                    // requested target becomes its own compilation identity,
                    // and nothing here silently substitutes another variant.
                    options.Tfms.AddRange(Split(Value(arg)));
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

        if (options.Emit.Contains(EmitCompilerFacts) && string.IsNullOrEmpty(options.CompilerFacts))
        {
            throw new ArgumentException("missing --compiler-facts");
        }

        if (options.Emit.Contains(EmitContext) && string.IsNullOrEmpty(options.Context))
        {
            throw new ArgumentException("missing --context");
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
        // A context run every one of whose requested targets is undeclared
        // everywhere still has something to report: one unsupported record
        // per request, naming both sides, with zero facts under any of them.
        // load.Filtered is deliberately excluded from this bypass: a
        // --projects value that matches nothing leaves nothing to report but
        // the fact that nothing matched, and every other emit mode already
        // treats that as the historical "zero projects loaded" usage error --
        // this run stays exit 3 rather than writing an envelope of nothing
        // but excluded records with no artifact-level state to say so.
        if (load.Projects.Count == 0
            && !(options.Emit.Contains(Program.EmitContext)
                && (load.Unsupported.Count > 0 || load.Excluded.Count > 0)))
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
        var wantCompilerFacts = options.Emit.Contains(Program.EmitCompilerFacts);
        // `wantExplicitContext` gates --emit context's own artifact (file
        // write, --strict rollup check): only true when that mode was
        // actually requested. `wantContext` additionally gates the
        // underlying per-compilation record computation, now also needed
        // by compiler-facts (Slice B), whether or not `--emit context`
        // itself was requested: its own header needs the real envelope to
        // embed and the real per-compilation fingerprints to fold.
        var wantExplicitContext = options.Emit.Contains(Program.EmitContext);
        var wantContext = wantExplicitContext || wantCompilerFacts;
        var factsWalker = options.Emit.Contains(Program.EmitFacts)
            ? new FactsWalker(options.PublishCalls, options.ConsumerBases)
            : null;
        var compilerFacts = wantCompilerFacts ? new CompilerFactsAccumulator() : null;

        // Resolved once, before the per-project loop, so the occurrence
        // walker below knows at walk time whether it should run at all.
        var (_, providedCapabilities) = wantCompilerFacts
            ? CompilerFactsEmitter.ResolveCapabilities(options)
            : (new List<string>(), new List<string>());
        var wantOccurrences = wantCompilerFacts && providedCapabilities.Contains("occurrences");
        var compilerOccurrences = wantOccurrences ? new CompilerOccurrenceAccumulator() : null;

        using var buildCollection = wantContext ? new Microsoft.Build.Evaluation.ProjectCollection() : null;
        var fingerprintCache = new Dictionary<ProjectId, string>();
        var contextVersions = wantContext ? DetectVersions() : null;
        var contextRecords = new List<ContextRecord>();

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
            var occurrenceStart = compilerOccurrences?.Sites.Count ?? 0;
            var unitId = $"{loaded.Name}|{loaded.Tfm ?? "?"}";

            if (compilation is null)
            {
                failedUnits++;
                compilerFacts?.Missing.Add(unitId);
            }
            else
            {
                compilerFacts?.CollectFromCompilation(compilation, unitId, paths);
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

                    compilerOccurrences?.WalkDocument(model, tree, rel, paths);
                }
            }

            files.Sort(StringComparer.Ordinal);
            Console.Error.WriteLine(
                $"  {loaded.Name} [{loaded.Tfm ?? "?"}] {status} {files.Count} files {refs.Count - before} refs");

            if (wantContext)
            {
                var record = ContextBuilder.BuildRecord(
                    loaded, compilation, paths, buildCollection!, options, files, load.Failures,
                    fingerprintCache, contextVersions!);
                contextRecords.Add(record);

                // Every occurrence collected for this project's documents,
                // just above, belongs to this one compilation: backfilled
                // here rather than re-walked, since the record (and its
                // fingerprint) only exists once BuildRecord returns.
                if (compilerOccurrences is not null)
                {
                    for (var i = occurrenceStart; i < compilerOccurrences.Sites.Count; i++)
                    {
                        compilerOccurrences.Sites[i] = compilerOccurrences.Sites[i] with
                        {
                            CompilationIdentity = record.Identity,
                            CompilationFingerprint = record.Fingerprint,
                        };
                    }
                }
            }

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

        if (wantContext)
        {
            foreach (var unsupported in load.Unsupported)
            {
                contextRecords.Add(new ContextRecord
                {
                    Identity = new ContextIdentity
                    {
                        ProjectPath = paths.RelativeProjectPath(unsupported.ProjectFilePath) ?? unsupported.ProjectFilePath,
                        ProjectName = unsupported.ProjectName,
                        RequestedTfm = unsupported.RequestedTfm,
                        EffectiveTfm = null,
                        DeclaredTfms = unsupported.DeclaredTfms,
                    },
                    State = "unsupported",
                    Reason = "undeclared-target",
                });
            }

            foreach (var excluded in load.Excluded)
            {
                contextRecords.Add(new ContextRecord
                {
                    Identity = new ContextIdentity
                    {
                        ProjectPath = paths.RelativeProjectPath(excluded.ProjectFilePath) ?? excluded.ProjectFilePath,
                        ProjectName = excluded.ProjectName,
                        RequestedTfm = null,
                        EffectiveTfm = excluded.Tfm,
                    },
                    State = "excluded",
                    Reason = "not-requested",
                });
            }

            // A project the caller's own --projects filter left out never
            // reached target selection, so it carries no tfm identity at
            // all -- but it is exactly as deliberately excluded as an
            // unselected multi-target variant, and the solution cross-check
            // just below must not mistake it for a project that vanished.
            foreach (var filtered in load.Filtered)
            {
                contextRecords.Add(new ContextRecord
                {
                    Identity = new ContextIdentity
                    {
                        ProjectPath = paths.RelativeProjectPath(filtered.ProjectFilePath) ?? filtered.ProjectFilePath,
                        ProjectName = filtered.ProjectName,
                        RequestedTfm = null,
                        EffectiveTfm = null,
                    },
                    State = "excluded",
                    Reason = "not-requested",
                });
            }

            // A project a .sln/.slnx names but that never reaches
            // solution.Projects at all (an unresolvable path, or evaluation
            // failing before MSBuildWorkspace can construct even a
            // degenerate Project) is otherwise invisible: it produces no
            // LoadedProject, no Unsupported, no Excluded entry, nothing an
            // ordinary oracle run would ever warn about beyond one stderr
            // line. Roslyn is otherwise extremely reluctant to hand back a
            // null compilation for a project it did accept (verified against
            // a missing project reference, an unresolvable Sdk and malformed
            // project XML, none of which produced one) -- a vanished project
            // is the one case this ticket has found that actually reaches
            // the `failed` state, so it is checked for independently of
            // Roslyn's own solution object, from the same direct solution
            // parse ContextInventory's own document inventory already uses.
            var extension = Path.GetExtension(options.Input).ToLowerInvariant();
            if (extension is ".sln" or ".slnx")
            {
                var accounted = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
                foreach (var p in load.Projects.Select(l => l.Project.FilePath)
                    .Concat(load.Unsupported.Select(u => u.ProjectFilePath))
                    .Concat(load.Excluded.Select(x => x.ProjectFilePath))
                    .Concat(load.Filtered.Select(f => f.ProjectFilePath)))
                {
                    if (p is { Length: > 0 })
                    {
                        accounted.Add(Path.GetFullPath(p));
                    }
                }

                foreach (var expected in ContextInventory.ExpectedProjectsOfSolution(options.Input))
                {
                    if (accounted.Contains(Path.GetFullPath(expected.AbsolutePath)))
                    {
                        continue;
                    }

                    // The caller's own --projects filter is applied to every
                    // solution-declared project, loaded or not: a project the
                    // filter would also have left out is exactly as
                    // deliberately excluded as one Loader itself filtered
                    // before target selection, not a project that failed to
                    // load. Only a name the filter admits, and that still
                    // never reached the workspace, is genuinely `failed`.
                    if (options.ProjectGlobs.Count > 0 && !options.ProjectGlobs.Any(g => g.IsMatch(expected.Name)))
                    {
                        contextRecords.Add(new ContextRecord
                        {
                            Identity = new ContextIdentity
                            {
                                ProjectPath = paths.RelativeProjectPath(expected.AbsolutePath) ?? expected.AbsolutePath,
                                ProjectName = expected.Name,
                                RequestedTfm = null,
                                EffectiveTfm = null,
                            },
                            State = "excluded",
                            Reason = "not-requested",
                        });
                        continue;
                    }

                    contextRecords.Add(new ContextRecord
                    {
                        Identity = new ContextIdentity
                        {
                            ProjectPath = paths.RelativeProjectPath(expected.AbsolutePath) ?? expected.AbsolutePath,
                            ProjectName = expected.Name,
                            RequestedTfm = null,
                            EffectiveTfm = null,
                        },
                        State = "failed",
                        Reason = "project-not-loaded",
                        Versions = contextVersions,
                    });
                }
            }

            Console.Error.WriteLine(
                $"context: fingerprint computation totaled {ContextBuilder.FingerprintElapsed.TotalMilliseconds:F1}ms "
                + $"across {contextRecords.Count(r => r.Fingerprint is not null)} compilations");
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

        if (compilerFacts is not null)
        {
            var written = CompilerFactsEmitter.Write(
                options, compilerFacts, OrderContextRecords(contextRecords), compilerOccurrences?.Sites);
            if (written != 0)
            {
                return written;
            }
        }

        if (wantExplicitContext)
        {
            var written = WriteContext(options, paths, contextRecords);
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

        if (options.Strict && wantExplicitContext)
        {
            var rollup = ArtifactRollup(contextRecords);
            if (rollup != "complete")
            {
                Console.Error.WriteLine($"error: --strict and the context artifact is '{rollup}', not complete");
                return 2;
            }
        }

        return 0;
    }

    /// <summary><c>complete</c> only when every non-<c>excluded</c> record is <c>complete</c>; else the worst state present, in <c>failed</c> &gt; <c>unsupported</c> &gt; <c>partial</c> order.</summary>
    /// <summary>Internal rather than private so <c>scout-semantic.Tests</c> can exercise the rollup rule directly, via <c>InternalsVisibleTo</c>.</summary>
    internal static string ArtifactRollup(List<ContextRecord> records)
    {
        var relevant = records.Where(r => r.State != "excluded").ToList();
        if (relevant.Count == 0 || relevant.All(r => r.State == "complete"))
        {
            return "complete";
        }

        if (relevant.Any(r => r.State == "failed"))
        {
            return "failed";
        }

        return relevant.Any(r => r.State == "unsupported") ? "unsupported" : "partial";
    }

    /// <summary>SDK, MSBuild, compiler and engine versions, read once per run.</summary>
    private static ContextVersions DetectVersions() => new()
    {
        Sdk = DetectSdkVersion(),
        Msbuild = DetectMsBuildVersion(),
        Compiler = DetectCompilerVersion(),
        Engine = FactsWriter.ProducerVersion(),
    };

    private static string DetectSdkVersion() => RunDotnet("--version");

    /// <summary>
    /// <see cref="Program.MsBuildVersion"/> (from <c>MSBuildLocator.RegisterDefaults().Version</c>)
    /// is, for a .NET SDK instance, the SDK version, not MSBuild's own engine version -- verified
    /// empirically (both print <c>9.0.305</c> on this machine while <c>dotnet msbuild -version</c>
    /// prints <c>17.14.21...</c>), which folded the SDK into the fingerprint's `versions` group
    /// twice under two different field names and left no independent MSBuild axis. `-nologo`
    /// suppresses the copyright banner so the version is the only line on stdout.
    /// </summary>
    private static string DetectMsBuildVersion() => RunDotnet("msbuild -version -nologo");

    private static string RunDotnet(string arguments)
    {
        try
        {
            using var process = Process.Start(new ProcessStartInfo("dotnet", arguments)
            {
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
            });

            if (process is null)
            {
                return "unknown";
            }

            process.ErrorDataReceived += static (_, _) => { };
            process.BeginErrorReadLine();
            var output = process.StandardOutput.ReadToEnd();
            process.WaitForExit();
            var lastLine = output
                .Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
                .LastOrDefault();
            return process.ExitCode == 0 && !string.IsNullOrEmpty(lastLine) ? lastLine : "unknown";
        }
        catch (Exception e) when (e is Win32Exception or InvalidOperationException or IOException or PlatformNotSupportedException)
        {
            return "unknown";
        }
    }

    private static string DetectCompilerVersion()
    {
        var assembly = typeof(CSharpCompilation).Assembly;
        var informational = assembly.GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion;
        return !string.IsNullOrEmpty(informational) ? informational : assembly.GetName().Version?.ToString() ?? "unknown";
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

    /// <summary>Orders the compilations, builds the envelope header and writes the document; 0 on success.</summary>
    /// <summary>The one fixed compilation-record order every emitter that
    /// carries context records uses: <c>--emit context</c>'s own top-level
    /// array, and the real envelope <c>--emit compiler-facts</c> embeds --
    /// so the derived context summary (Slice B) folds the same order on
    /// every run, not a re-sort a second producer could silently diverge
    /// from.</summary>
    private static List<ContextRecord> OrderContextRecords(List<ContextRecord> records) => records
        .OrderBy(r => r.Identity.ProjectName, StringComparer.Ordinal)
        .ThenBy(r => r.Identity.RequestedTfm ?? "", StringComparer.Ordinal)
        .ThenBy(r => r.Identity.EffectiveTfm ?? "", StringComparer.Ordinal)
        .ToList();

    private static int WriteContext(Options options, RepoPaths paths, List<ContextRecord> records)
    {
        var root = paths.Root;
        var repo = options.Repo is { Length: > 0 } given ? given : root[(root.LastIndexOf('/') + 1)..];
        var ordered = OrderContextRecords(records);

        var envelope = new ContextEnvelope
        {
            Producer = "scout-semantic",
            Version = FactsWriter.ProducerVersion(),
            Repo = repo,
            Solution = paths.Relative(options.Input) ?? Path.GetFileName(options.Input),
            Compilations = ordered,
        };

        try
        {
            ContextWriter.Write(options.Context!, envelope);
        }
        catch (ContextSchemaException e)
        {
            Console.Error.WriteLine($"error: context schema violation: {e.Message}");
            return 1;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            Console.Error.WriteLine($"error: {e.Message}");
            return 1;
        }

        Console.Error.WriteLine($"context: {ordered.Count} compilation records -> {options.Context}");
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

/// <summary>
/// Assembles one <see cref="ContextRecord"/> per successfully matched
/// compilation: the independent inventory diff, the generated-document
/// account, raw diagnostics, the fingerprint and its reference graph. Kept
/// out of <see cref="Runner"/> so that class stays focused on the oracle/
/// facts walk this ticket must leave untouched.
/// </summary>
internal static class ContextBuilder
{
    /// <summary>
    /// Total wall time <see cref="GetOrComputeFingerprint"/> has spent across
    /// every call this process has made (cache hits are free; a cycle guard
    /// short-circuits before this accrues), so a run can report the
    /// fingerprint's own cost rather than leave it unmeasured -- it performs
    /// a second, independent project evaluation per compilation.
    /// </summary>
    public static TimeSpan FingerprintElapsed;

    public static ContextRecord BuildRecord(
        LoadedProject loaded,
        Compilation? compilation,
        RepoPaths paths,
        Microsoft.Build.Evaluation.ProjectCollection buildCollection,
        Options options,
        List<string> loadedFiles,
        List<WorkspaceDiagnostic> workspaceFailures,
        Dictionary<ProjectId, string> fingerprintCache,
        ContextVersions versions)
    {
        var project = loaded.Project;
        var projectPathRel = paths.RelativeProjectPath(project.FilePath) ?? project.FilePath ?? loaded.Name;

        if (compilation is null)
        {
            // The workspace never produced a compilation at all; a Roslyn
            // WorkspaceDiagnostic carries no per-project id, so every
            // diagnostic from this load is attached here rather than guessed
            // at per project -- the one place this run can attribute them at
            // all without inventing a location the API does not report.
            return new ContextRecord
            {
                Identity = new ContextIdentity
                {
                    ProjectPath = projectPathRel,
                    ProjectName = loaded.Name,
                    RequestedTfm = loaded.RequestedTfm,
                    EffectiveTfm = loaded.Tfm,
                },
                State = "failed",
                Reason = "workspace-failure",
                Versions = versions,
                Diagnostics = new ContextDiagnostics
                {
                    Workspace = workspaceFailures
                        .Select(d => new WorkspaceDiagnosticRecord { Kind = d.Kind.ToString(), Message = RedactDiagnosticMessage(d.Message, paths) })
                        .ToList(),
                },
            };
        }

        ProjectInventory? inventory = null;
        try
        {
            inventory = ContextInventory.Evaluate(
                buildCollection, project.FilePath!, options.Properties, loaded.RequestedTfm ?? loaded.Tfm, paths);
        }
        catch (Exception e) when (e is InvalidProjectFileException or IOException or InvalidOperationException)
        {
            Console.Error.WriteLine($"warning: context: {loaded.Name}: independent inventory failed: {e.Message}");
        }

        var loadedSet = new HashSet<string>(loadedFiles, StringComparer.Ordinal);
        var documents = inventory is not null
            ? new ContextDocuments { Loaded = loadedFiles, Expected = inventory.ExpectedDisplayPaths, Dropped = inventory.Dropped }
            : new ContextDocuments { Loaded = loadedFiles, Expected = new List<string>(loadedFiles), InventoryAvailable = false };

        var generated = GeneratedDocumentsOf(project, compilation, paths, loadedSet, out var generatedTrees);
        generated.Diagnostics.AddRange(GeneratedDiagnosticsOf(compilation, generatedTrees));

        var compilerDiagnostics = compilation.GetDiagnostics()
            .Where(d => d.Severity == DiagnosticSeverity.Error)
            .Select(d => new CompilerDiagnosticRecord
            {
                Severity = d.Severity.ToString(),
                Id = d.Id,
                Message = d.GetMessage(System.Globalization.CultureInfo.InvariantCulture),
                File = d.Location.SourceTree is { } tree ? paths.Relative(tree.FilePath) : null,
                Line = d.Location.SourceTree is not null
                    ? d.Location.GetLineSpan().StartLinePosition.Line + 1
                    : null,
            })
            .ToList();

        // A Roslyn WorkspaceDiagnostic carries no project id at all (verified
        // empirically: SkipUnrecognizedProjects keeps a project whose own
        // ProjectReference target is missing on disk in the solution, with a
        // non-null compilation, and reports the break only as a free-text
        // workspace diagnostic naming the project's own file path in its
        // message) -- so a diagnostic naming this project's absolute path is
        // the only attribution the public API leaves reachable at all, and is
        // attached here rather than left unattributed.
        var ownWorkspaceDiagnostics = project.FilePath is { Length: > 0 } ownPath
            ? workspaceFailures.Where(d => d.Message.Contains(ownPath, StringComparison.Ordinal)).ToList()
            : new List<WorkspaceDiagnostic>();
        var workspaceDiagnosticRecords = ownWorkspaceDiagnostics
            .Select(d => new WorkspaceDiagnosticRecord { Kind = d.Kind.ToString(), Message = RedactDiagnosticMessage(d.Message, paths) })
            .ToList();

        // Every reason RepoPaths.Classify can name is an expected document
        // that did not load, whether by accident (missing) or by an
        // explicit, otherwise-legitimate exclusion (linked-outside-root,
        // skipped-directory, out-of-scope): completion is earned, so any of
        // them demotes the record exactly like a missing one. Priority
        // ("missing" first) only decides which single reason surfaces on the
        // record when more than one kind of drop occurs together.
        var droppedReasonPriority = new[] { "missing", "linked-outside-root", "skipped-directory", "out-of-scope" };
        var firstDroppedReason = droppedReasonPriority.FirstOrDefault(r => documents.Dropped.Any(d => d.Reason == r));
        var hasUnresolvedReference = ownWorkspaceDiagnostics.Any(d => d.Kind == WorkspaceDiagnosticKind.Failure);
        var inventoryUnavailable = inventory is null;

        string state;
        string reason;
        if (compilerDiagnostics.Count > 0)
        {
            state = "partial";
            reason = "binding-error";
        }
        else if (hasUnresolvedReference)
        {
            state = "partial";
            reason = "workspace-failure";
        }
        else if (firstDroppedReason is not null)
        {
            state = "partial";
            reason = firstDroppedReason;
        }
        else if (inventoryUnavailable)
        {
            // The independent inventory is this record's only source of
            // "what should have loaded" -- without it, an empty dropped list
            // means nothing was missing and cannot be distinguished from "we
            // could not tell". Completion is earned, never inferred from a
            // fallback, so this demotes the record even though nothing else
            // found a defect.
            state = "partial";
            reason = "inventory-unavailable";
        }
        else
        {
            state = "complete";
            reason = "complete";
        }

        var fingerprintTimer = Stopwatch.StartNew();
        var fingerprint = GetOrComputeFingerprint(
            project, paths, buildCollection, options, fingerprintCache, new HashSet<ProjectId>(), versions);
        fingerprintTimer.Stop();
        FingerprintElapsed += fingerprintTimer.Elapsed;

        var references = new List<ContextReference>();
        foreach (var reference in compilation.References.OfType<PortableExecutableReference>())
        {
            var name = (compilation.GetAssemblyOrModuleSymbol(reference) as IAssemblySymbol)?.Identity.Name
                ?? Path.GetFileNameWithoutExtension(reference.FilePath ?? "unknown");
            references.Add(new ContextReference
            {
                Kind = "metadata",
                Name = name,
                Identity = MetadataReferenceIdentity(reference, compilation),
            });
        }

        foreach (var projectReference in project.ProjectReferences)
        {
            var referenced = project.Solution.GetProject(projectReference.ProjectId);
            if (referenced is null)
            {
                continue;
            }

            references.Add(new ContextReference
            {
                Kind = "project",
                Name = Loader.SplitName(referenced.Name).Name,
                Fingerprint = fingerprintCache.TryGetValue(projectReference.ProjectId, out var refFp) ? refFp : null,
            });
        }

        return new ContextRecord
        {
            Identity = new ContextIdentity
            {
                ProjectPath = projectPathRel,
                ProjectName = loaded.Name,
                RequestedTfm = loaded.RequestedTfm,
                EffectiveTfm = inventory?.EffectiveTfm ?? loaded.Tfm,
                Configuration = inventory?.Configuration,
                Platform = inventory?.Platform,
            },
            State = state,
            Reason = reason,
            Fingerprint = fingerprint,
            Versions = versions,
            References = references,
            Imports = inventory?.Imports ?? new List<ContextImport>(),
            LanguageOptions = LanguageOptionsOf(compilation, inventory),
            PreprocessorSymbols = inventory?.PreprocessorSymbols ?? new List<string>(),
            Documents = documents,
            Generated = generated,
            Diagnostics = new ContextDiagnostics { Workspace = workspaceDiagnosticRecords, Compiler = compilerDiagnostics },
        };
    }

    /// <summary>
    /// Primary path: the Workspace API's own generator enumeration, which
    /// exists and returns each generated document's <c>HintName</c> (verified
    /// by reflection against the restored 4.14.0 assemblies) but exposes no
    /// generator-identity property at all, so every entry is reported with
    /// the documented <c>unknown</c> generator identity rather than a guess.
    /// Falls back to diffing <see cref="Compilation.SyntaxTrees"/> against the
    /// authored documents only if the primary call itself is unavailable.
    /// Collects each generated document's own <see cref="SyntaxTree"/> along
    /// the way, so <see cref="GeneratedDiagnosticsOf"/> can attribute a
    /// diagnostic to a generator by tree identity rather than by a "not
    /// among the authored files" guess -- an SDK-emitted, ordinary compile
    /// item this tool's own skip-dir convention happens to exclude (the
    /// generated <c>obj/*.GlobalUsings.g.cs</c>, for one) is not a
    /// generator's output and must not be mistaken for one.
    /// </summary>
    private static ContextGenerated GeneratedDocumentsOf(
        Project project, Compilation compilation, RepoPaths paths, HashSet<string> loadedSet, out HashSet<SyntaxTree> generatedTrees)
    {
        var generated = new ContextGenerated();
        generatedTrees = new HashSet<SyntaxTree>();
        try
        {
            foreach (var document in project.GetSourceGeneratedDocumentsAsync().GetAwaiter().GetResult())
            {
                generated.Documents.Add(new GeneratedDocument { HintName = document.HintName, Generator = "unknown" });
                if (document.GetSyntaxTreeAsync().GetAwaiter().GetResult() is { } tree)
                {
                    generatedTrees.Add(tree);
                }
            }
        }
        catch (Exception e) when (e is NotImplementedException or InvalidOperationException or NotSupportedException)
        {
            Console.Error.WriteLine(
                $"warning: context: {project.Name}: GetSourceGeneratedDocumentsAsync unavailable ({e.GetType().Name}), using the SyntaxTrees fallback");
            foreach (var tree in compilation.SyntaxTrees)
            {
                var rel = paths.Relative(tree.FilePath);
                if (rel is not null && !loadedSet.Contains(rel))
                {
                    generated.Documents.Add(new GeneratedDocument { HintName = Path.GetFileName(tree.FilePath), Generator = "unknown" });
                    generatedTrees.Add(tree);
                }
            }
        }

        // GetSourceGeneratedDocumentsAsync's own enumeration order is not
        // guaranteed stable run to run; every other list in this record is
        // already ordinal-sorted, so this one is too.
        generated.Documents.Sort((a, b) => string.CompareOrdinal(a.HintName, b.HintName));
        return generated;
    }

    /// <summary>
    /// A generator's own diagnostic is reported by Roslyn as an ordinary
    /// compilation diagnostic located on the tree it authored, so it is
    /// distinguished from an authored-document diagnostic (already covered
    /// by <c>diagnostics.compiler</c>) by tree identity against exactly the
    /// trees <see cref="GeneratedDocumentsOf"/> just enumerated. Any
    /// severity is inventoried here -- a generator can report an
    /// informational or a warning diagnostic without ever becoming an
    /// error, and this account exists to make that visible too. Internal
    /// rather than private so <c>scout-semantic.Tests</c> can exercise the
    /// attribution rule directly against a hand-built compilation, via
    /// <c>InternalsVisibleTo</c> -- no fixture generator this ticket carries
    /// ever reports a diagnostic of its own to exercise this positively any
    /// other way.
    /// </summary>
    internal static List<string> GeneratedDiagnosticsOf(Compilation compilation, HashSet<SyntaxTree> generatedTrees)
    {
        var result = new List<string>();
        if (generatedTrees.Count == 0)
        {
            return result;
        }

        foreach (var diagnostic in compilation.GetDiagnostics())
        {
            if (diagnostic.Location.SourceTree is { } tree && generatedTrees.Contains(tree))
            {
                result.Add($"{diagnostic.Severity} {diagnostic.Id}: {diagnostic.GetMessage(System.Globalization.CultureInfo.InvariantCulture)}");
            }
        }

        result.Sort(StringComparer.Ordinal);
        return result;
    }

    private static Dictionary<string, string?> LanguageOptionsOf(Compilation compilation, ProjectInventory? inventory)
    {
        var options = new Dictionary<string, string?>(StringComparer.Ordinal);
        if (compilation is CSharpCompilation csharp)
        {
            options["languageVersion"] = csharp.LanguageVersion.ToDisplayString();
        }

        options["nullable"] = inventory?.Nullable;
        options["allowUnsafeBlocks"] = inventory is null ? null : inventory.AllowUnsafeBlocks.ToString();
        return options;
    }

    /// <summary>
    /// Post-order over the project-reference DAG: a referenced project's own
    /// fingerprint is computed (and cached) before the fingerprint that folds
    /// it. A cycle demotes to a stable placeholder instead of recursing
    /// forever -- no fixture in this ticket exercises one, so this is a
    /// defensive guard rather than a proven path.
    /// </summary>
    private static string GetOrComputeFingerprint(
        Project project,
        RepoPaths paths,
        Microsoft.Build.Evaluation.ProjectCollection buildCollection,
        Options options,
        Dictionary<ProjectId, string> cache,
        HashSet<ProjectId> visiting,
        ContextVersions versions)
    {
        if (cache.TryGetValue(project.Id, out var cached))
        {
            return cached;
        }

        if (!visiting.Add(project.Id))
        {
            return "cyclic-reference-graph";
        }

        string result;
        try
        {
            var compilation = project.GetCompilationAsync().GetAwaiter().GetResult();
            var metadataIdentities = compilation is null
                ? Enumerable.Empty<string>()
                : compilation.References.OfType<PortableExecutableReference>()
                    .Select(r => MetadataReferenceIdentity(r, compilation));

            var projectRefFingerprints = new List<string>();
            foreach (var reference in project.ProjectReferences)
            {
                var referenced = project.Solution.GetProject(reference.ProjectId);
                if (referenced is not null)
                {
                    projectRefFingerprints.Add(
                        GetOrComputeFingerprint(referenced, paths, buildCollection, options, cache, visiting, versions));
                }
            }

            var (_, nameTfm) = Loader.SplitName(project.Name);
            ProjectInventory? inventory = null;
            try
            {
                inventory = ContextInventory.Evaluate(buildCollection, project.FilePath!, options.Properties, nameTfm, paths);
            }
            catch (Exception e) when (e is InvalidProjectFileException or IOException or InvalidOperationException)
            {
            }

            result = ContextFingerprint.Compute(
                metadataIdentities,
                projectRefFingerprints,
                inventory?.Imports.Select(i => i.Hash) ?? Enumerable.Empty<string>(),
                AnalyzerReferenceIdentitiesOf(project),
                GeneratorInputIdentitiesOf(project, paths),
                inventory?.PreprocessorSymbols ?? new List<string>(),
                LanguageOptionsOf(compilation ?? CSharpCompilation.Create("empty"), inventory),
                versions,
                inventory?.Configuration ?? "unknown",
                inventory?.Platform ?? "unknown",
                inventory?.EffectiveTfm ?? nameTfm ?? "unknown",
                inventory?.AssemblyName,
                inventory?.RootNamespace);
        }
        finally
        {
            visiting.Remove(project.Id);
        }

        cache[project.Id] = result;
        return result;
    }

    /// <summary>
    /// One identity per analyzer reference (an analyzer package, a source
    /// generator among them): file name plus a content hash, never the
    /// absolute local path, matching every other reference identity in this
    /// record.
    /// </summary>
    private static List<string> AnalyzerReferenceIdentitiesOf(Project project)
    {
        var result = new List<string>();
        foreach (var reference in project.AnalyzerReferences)
        {
            var name = Path.GetFileName(reference.FullPath ?? reference.Display ?? "unknown");
            result.Add(reference.FullPath is { } path && File.Exists(path)
                ? $"{name}|sha1:{ContentSha1(path)}"
                : $"{name}|unknown");
        }

        return result;
    }

    /// <summary>
    /// One identity per <c>AdditionalFiles</c> item: a generator's own input
    /// beyond the compiled source itself (an embedded schema, a config file
    /// a source generator reads), root-relative path plus a content hash.
    /// </summary>
    private static List<string> GeneratorInputIdentitiesOf(Project project, RepoPaths paths)
    {
        var result = new List<string>();
        foreach (var document in project.AdditionalDocuments)
        {
            var text = document.GetTextAsync().GetAwaiter().GetResult().ToString();
            var rel = paths.Relative(document.FilePath) ?? document.Name;
            result.Add($"{rel}|{FactsWriter.Sha1(text)}");
        }

        return result;
    }

    private static string ContentSha1(string path)
    {
        using var stream = File.OpenRead(path);
        return Convert.ToHexString(SHA1.HashData(stream)).ToLowerInvariant();
    }

    /// <summary>
    /// Root-relativizes any occurrence of this run's own analysed path, then
    /// replaces any other absolute-path-shaped substring with just its file
    /// name, so a build tool's own free-text diagnostic (which can quote a
    /// project's absolute file path in its message) never carries a local
    /// machine path the way a path-shaped field already cannot --
    /// <see cref="ContextSchema"/> checks this at write time as defense in
    /// depth, so this is the first, not the only, guard.
    /// </summary>
    private static string RedactDiagnosticMessage(string message, RepoPaths paths)
    {
        var rooted = message.Replace(paths.Root + "/", string.Empty, StringComparison.Ordinal);
        return AbsolutePathToken.Replace(rooted, m => Path.GetFileName(m.Value));
    }

    // The negative lookbehind requires the leading slash (or drive letter) to
    // start a fresh token rather than sit mid-path: without it, the first
    // '/' inside an already-relative path like "src/Broken/Broken.csproj"
    // would itself look like the start of an absolute path and get eaten.
    private static readonly Regex AbsolutePathToken =
        new(@"(?<![\w./-])(?:[A-Za-z]:[\\/]|/)[^\s'""]+", RegexOptions.Compiled);

    /// <summary>Assembly name plus module-version-id, falling back to a content hash when the MVID cannot be read.</summary>
    private static string MetadataReferenceIdentity(PortableExecutableReference reference, Compilation compilation)
    {
        var name = (compilation.GetAssemblyOrModuleSymbol(reference)) switch
        {
            IAssemblySymbol asm => asm.Identity.Name,
            IModuleSymbol mod => mod.Name,
            _ => null,
        } ?? Path.GetFileNameWithoutExtension(reference.FilePath ?? "unknown");

        if (reference.FilePath is { } path && File.Exists(path))
        {
            try
            {
                using var stream = File.OpenRead(path);
                using var peReader = new PEReader(stream);
                var metadataReader = peReader.GetMetadataReader();
                var mvid = metadataReader.GetGuid(metadataReader.GetModuleDefinition().Mvid);
                return $"{name}|mvid:{mvid}";
            }
            catch (Exception e) when (e is BadImageFormatException or IOException or InvalidOperationException)
            {
            }

            try
            {
                using var stream = File.OpenRead(path);
                return $"{name}|sha1:{Convert.ToHexString(SHA1.HashData(stream)).ToLowerInvariant()}";
            }
            catch (IOException)
            {
            }
        }

        return $"{name}|unknown";
    }
}
