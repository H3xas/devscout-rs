using System.Diagnostics;
using System.IO.Compression;
using System.Security.Cryptography;
using System.Text.Json;

using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;

using Xunit;

namespace ScoutSemanticTests;

/// <summary>
/// Drives the built CLI as a real subprocess (the same convention
/// <c>CliProjectsFilterTests</c> already uses), because the offline restore
/// step's own observable behaviour -- whether <c>obj/project.assets.json</c>
/// gets written, whether a compilation actually binds -- only exists once
/// <c>Program.Main</c>'s full pipeline runs end to end; no unit inside it
/// alone can stand in for a real, unrestored working copy. Every package
/// this file's tests need is built and laid out entirely offline: a stub
/// assembly compiled in-process with Roslyn, packed into a real
/// <c>.nupkg</c>, and placed directly into a temporary NuGet global packages
/// folder -- never a network call, never the machine's own
/// <c>~/.nuget/packages</c>.
/// </summary>
public sealed class OfflineRestoreCliTests
{
    private static readonly string RepoRoot = FindRepoRoot();
    private static readonly string CliDll = Path.Combine(AppContext.BaseDirectory, "scout-semantic.dll");

    private const string PackageId = "StubPkg";
    private const string PackageVersion = "1.0.0";
    private const string Tfm = "net9.0";

    private static string FindRepoRoot()
    {
        var dir = AppContext.BaseDirectory;
        while (dir is not null && !File.Exists(Path.Combine(dir, "fixtures", "csharp-semantic", "Fixture.sln")))
        {
            var parent = Path.GetDirectoryName(dir.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar));
            dir = parent == dir ? null : parent;
        }

        return dir ?? throw new InvalidOperationException(
            $"could not locate the repository root (fixtures/csharp-semantic/Fixture.sln) walking up from {AppContext.BaseDirectory}");
    }

    /// <summary>The installed SDK's own netcoreapp reference-assembly set (never the runtime
    /// implementation assemblies <c>typeof(object).Assembly.Location</c> would give): compiling
    /// the stub against these keeps its metadata's own type identities (<c>System.Object</c>'s
    /// declaring assembly, in particular) exactly what an ordinary SDK-style build's own compiler
    /// invocation -- the one that later compiles the consumer project against this stub -- itself
    /// expects, so the two independently-compiled assemblies agree on every shared BCL type.</summary>
    private static string[] ReferenceAssemblyPaths()
    {
        var runtimeDir = Path.GetDirectoryName(typeof(object).Assembly.Location)!;
        var dotnetRoot = Directory.GetParent(runtimeDir)!.Parent!.Parent!.FullName;
        var refPackRoot = Path.Combine(dotnetRoot, "packs", "Microsoft.NETCore.App.Ref");
        var versionDir = Directory.EnumerateDirectories(refPackRoot)
            .OrderByDescending(d => d, StringComparer.Ordinal)
            .First();
        var netDir = Directory.EnumerateDirectories(Path.Combine(versionDir, "ref"))
            .OrderByDescending(d => d, StringComparer.Ordinal)
            .First();
        return Directory.GetFiles(netDir, "*.dll");
    }

    /// <summary>Compiles a trivial stub assembly with Roslyn and packs it, by hand, into the
    /// exact expanded layout a NuGet global packages folder uses -- nuspec, a real <c>.nupkg</c>
    /// (an OPC-shaped zip is not required for the local V3 resource to read it), and its content
    /// hash -- so a <c>--source</c> pointed at this folder resolves the package with no network.</summary>
    private static string WriteStubPackage(string gpfRoot, params string[] libTfms)
    {
        if (libTfms.Length == 0)
        {
            libTfms = new[] { Tfm };
        }

        var syntaxTree = CSharpSyntaxTree.ParseText(
            $$"""namespace {{PackageId}} { public class StubClass { public static string Hello() => "hi"; } }""");
        var compilation = CSharpCompilation.Create(
            PackageId,
            new[] { syntaxTree },
            ReferenceAssemblyPaths().Select(p => MetadataReference.CreateFromFile(p)),
            new CSharpCompilationOptions(OutputKind.DynamicallyLinkedLibrary));

        using var dllStream = new MemoryStream();
        var emitResult = compilation.Emit(dllStream);
        Assert.True(emitResult.Success, "stub assembly failed to compile: " + string.Join("; ", emitResult.Diagnostics));

        var idLower = PackageId.ToLowerInvariant();
        var versionDir = Path.Combine(gpfRoot, idLower, PackageVersion);
        var dllPaths = new List<(string Tfm, string Path)>();
        foreach (var libTfm in libTfms)
        {
            var libDir = Path.Combine(versionDir, "lib", libTfm);
            Directory.CreateDirectory(libDir);
            var dllPath = Path.Combine(libDir, PackageId + ".dll");
            File.WriteAllBytes(dllPath, dllStream.ToArray());
            dllPaths.Add((libTfm, dllPath));
        }

        var nuspecPath = Path.Combine(versionDir, idLower + ".nuspec");
        File.WriteAllText(nuspecPath, $"""
            <?xml version="1.0" encoding="utf-8"?>
            <package xmlns="http://schemas.microsoft.com/packaging/2013/05/nuspec.xsd">
              <metadata>
                <id>{PackageId}</id>
                <version>{PackageVersion}</version>
                <authors>test</authors>
                <description>a hermetic test's own throwaway stub package</description>
              </metadata>
            </package>
            """);

        var nupkgPath = Path.Combine(versionDir, $"{idLower}.{PackageVersion}.nupkg");
        using (var zip = ZipFile.Open(nupkgPath, ZipArchiveMode.Create))
        {
            zip.CreateEntryFromFile(nuspecPath, idLower + ".nuspec");
            foreach (var (libTfm, dllPath) in dllPaths)
            {
                zip.CreateEntryFromFile(dllPath, $"lib/{libTfm}/{PackageId}.dll");
            }
        }

        var hash = Convert.ToBase64String(SHA512.HashData(File.ReadAllBytes(nupkgPath)));
        File.WriteAllText(Path.Combine(versionDir, $"{idLower}.{PackageVersion}.nupkg.sha512"), hash);
        return nupkgPath;
    }

    /// <summary>A one-project consumer whose only source file actually uses the stub package's
    /// type, so an unrestored copy fails to bind it (the missing-PackageReference failure this
    /// step exists to prevent) rather than silently compiling clean because nothing in the source
    /// touches the package. A
    /// plain class member, not a top-level statement, so the project stays an ordinary library
    /// and the compile's only possible error is the reference this file's tests are about.</summary>
    private static string WriteConsumerProject(string root, string extraProperties = "")
    {
        Directory.CreateDirectory(root);
        File.WriteAllText(Path.Combine(root, "Consumer.csproj"), $"""
            <Project Sdk="Microsoft.NET.Sdk">
              <PropertyGroup>
                <TargetFramework>{Tfm}</TargetFramework>
                <Nullable>enable</Nullable>
                {extraProperties}
              </PropertyGroup>
              <ItemGroup>
                <PackageReference Include="{PackageId}" Version="{PackageVersion}" />
              </ItemGroup>
            </Project>
            """);
        File.WriteAllText(Path.Combine(root, "Program.cs"),
            $"namespace Consumer;\n\npublic class Caller\n{{\n    public static string Call() => {PackageId}.StubClass.Hello();\n}}\n");
        return Path.Combine(root, "Consumer.csproj");
    }

    /// <summary>A library the consumer references by <c>ProjectReference</c>, its own target
    /// framework the same as the consumer's own but left out of target selection by the run's own
    /// <c>--projects</c> filter (a project reference excluded the same way a declared-but-not-
    /// requested TFM variant is: present in <c>load.Solution.Projects</c> because the consumer's
    /// own reference pulls it into the workspace, never in <c>load.Projects</c> because target
    /// selection excluded it) -- the same "reachable but not selected" shape a target-framework
    /// mismatch produces, without a second installed SDK's own framework-reference-pack
    /// availability as a confound. Its own method returns the stub package's type by value, so an
    /// unrestored copy's missing package cascades into an unresolved return type the referencing
    /// consumer itself cannot bind, the same shape the real defect this test guards took.</summary>
    private static string WriteReferencedLibProject(string root)
    {
        Directory.CreateDirectory(root);
        File.WriteAllText(Path.Combine(root, "ReferencedLib.csproj"), $"""
            <Project Sdk="Microsoft.NET.Sdk">
              <PropertyGroup>
                <TargetFramework>{Tfm}</TargetFramework>
              </PropertyGroup>
              <ItemGroup>
                <PackageReference Include="{PackageId}" Version="{PackageVersion}" />
              </ItemGroup>
            </Project>
            """);
        File.WriteAllText(Path.Combine(root, "Wrapper.cs"),
            $"namespace ReferencedLib;\n\npublic class Wrapper\n{{\n    public static {PackageId}.StubClass GetStub() => new {PackageId}.StubClass();\n}}\n");
        return Path.Combine(root, "ReferencedLib.csproj");
    }

    private static string WriteConsumerReferencingProject(string root, string referencedLibProjectPath)
    {
        Directory.CreateDirectory(root);
        var relative = Path.GetRelativePath(root, referencedLibProjectPath).Replace('\\', '/');
        File.WriteAllText(Path.Combine(root, "Consumer.csproj"), $"""
            <Project Sdk="Microsoft.NET.Sdk">
              <PropertyGroup>
                <TargetFramework>{Tfm}</TargetFramework>
              </PropertyGroup>
              <ItemGroup>
                <ProjectReference Include="{relative}" />
              </ItemGroup>
            </Project>
            """);
        File.WriteAllText(Path.Combine(root, "Program.cs"),
            "namespace Consumer;\n\npublic class Caller\n{\n    public static object Call() => ReferencedLib.Wrapper.GetStub();\n}\n");
        return Path.Combine(root, "Consumer.csproj");
    }

    /// <summary>A <c>netstandard2.0</c> library, the shape the in-process evaluator cannot
    /// evaluate, so its assets file location has to come from the SDK's own MSBuild. Implicit
    /// framework references are off so the restore needs nothing but the stub package, and the
    /// test stays independent of the machine's own package cache.</summary>
    private static string WriteNetStandardLibProject(string dir)
    {
        Directory.CreateDirectory(dir);
        File.WriteAllText(Path.Combine(dir, "Lib.csproj"), $"""
            <Project Sdk="Microsoft.NET.Sdk">
              <PropertyGroup>
                <TargetFramework>netstandard2.0</TargetFramework>
                <DisableImplicitFrameworkReferences>true</DisableImplicitFrameworkReferences>
              </PropertyGroup>
              <ItemGroup>
                <PackageReference Include="{PackageId}" Version="{PackageVersion}" />
              </ItemGroup>
            </Project>
            """);
        File.WriteAllText(Path.Combine(dir, "Lib.cs"), "namespace Lib { public class Holder { } }\n");
        return Path.Combine(dir, "Lib.csproj");
    }

    private static void WriteCustomIntermediatePathProps(string root)
    {
        Directory.CreateDirectory(root);
        File.WriteAllText(Path.Combine(root, "Directory.Build.props"), """
            <Project>
              <PropertyGroup>
                <BaseIntermediateOutputPath>$(MSBuildThisFileDirectory)artifacts/obj/$(MSBuildProjectName)/</BaseIntermediateOutputPath>
              </PropertyGroup>
            </Project>
            """);
    }

    private static (int ExitCode, string Output) RunDotnetRestore(string project, string nugetPackages, string source)
    {
        var psi = new ProcessStartInfo("dotnet")
        {
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
            WorkingDirectory = Path.GetDirectoryName(project)!,
        };
        foreach (var arg in new[] { "restore", project, "--source", source, "-p:NuGetAudit=false" })
        {
            psi.ArgumentList.Add(arg);
        }

        psi.Environment["NUGET_PACKAGES"] = nugetPackages;
        using var process = Process.Start(psi) ?? throw new InvalidOperationException("failed to start dotnet");
        var stderrTask = process.StandardError.ReadToEndAsync();
        var stdout = process.StandardOutput.ReadToEnd();
        process.WaitForExit();
        return (process.ExitCode, stdout + stderrTask.Result);
    }

    private static (int ExitCode, string StdOut, string StdErr, TimeSpan Elapsed) RunCli(
        string workingDirectory, string nugetPackages, params string[] args)
    {
        var psi = new ProcessStartInfo("dotnet")
        {
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
            WorkingDirectory = workingDirectory,
        };
        psi.ArgumentList.Add(CliDll);
        foreach (var arg in args)
        {
            psi.ArgumentList.Add(arg);
        }

        // Every source this process's own NuGet configuration could otherwise
        // reach is replaced with an isolated, offline global packages folder
        // -- the subprocess (and the `dotnet restore` it may itself spawn)
        // inherits this override, never the real machine-wide cache.
        psi.Environment["NUGET_PACKAGES"] = nugetPackages;

        var stopwatch = Stopwatch.StartNew();
        using var process = Process.Start(psi) ?? throw new InvalidOperationException("failed to start dotnet");
        var stderrTask = process.StandardError.ReadToEndAsync();
        var stdout = process.StandardOutput.ReadToEnd();
        process.WaitForExit();
        var stderr = stderrTask.Result;
        stopwatch.Stop();
        return (process.ExitCode, stdout, stderr, stopwatch.Elapsed);
    }

    private static JsonElement SingleCompilation(string contextPath)
    {
        using var document = JsonDocument.Parse(File.ReadAllText(contextPath));
        return document.RootElement.GetProperty("compilations").EnumerateArray().Single().Clone();
    }

    private static List<string> WorkspaceMessages(JsonElement compilation) =>
        compilation.GetProperty("diagnostics").GetProperty("workspace").EnumerateArray()
            .Select(d => d.GetProperty("message").GetString() ?? "")
            .ToList();

    private static List<string> IncompleteUnitReasons(string compilerFactsPath)
    {
        using var document = JsonDocument.Parse(File.ReadAllText(compilerFactsPath));
        var coverage = document.RootElement.GetProperty("coverage");
        return coverage.TryGetProperty("incompleteUnits", out var units)
            ? units.EnumerateArray().Select(u => u.GetProperty("reason").GetString() ?? "").ToList()
            : new List<string>();
    }

    private static void DeleteAll(params string[] paths)
    {
        foreach (var path in paths)
        {
            if (Directory.Exists(path))
            {
                Directory.Delete(path, recursive: true);
            }
            else if (File.Exists(path))
            {
                File.Delete(path);
            }
        }
    }

    [Fact]
    public void a_project_missing_its_assets_file_is_restored_and_reports_complete()
    {
        Assert.True(File.Exists(CliDll), $"expected the built CLI at {CliDll}");
        var root = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-missing-{Guid.NewGuid():N}");
        var gpf = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-missing-gpf-{Guid.NewGuid():N}");
        var outPath = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-missing-out-{Guid.NewGuid():N}.json");
        try
        {
            Directory.CreateDirectory(gpf);
            WriteStubPackage(gpf);
            WriteConsumerProject(root);
            Assert.False(File.Exists(Path.Combine(root, "obj", "project.assets.json")));

            var (exitCode, _, stderr, _) = RunCli(
                root, gpf,
                "Consumer.csproj", "--root", ".", "--emit", "context", "--context", outPath, "--tfm", Tfm);

            Assert.True(exitCode == 0, $"expected exit 0, got {exitCode}. stderr:\n{stderr}");
            Assert.True(File.Exists(Path.Combine(root, "obj", "project.assets.json")), "the offline restore should have written an assets file");

            var compilation = SingleCompilation(outPath);
            Assert.Equal("complete", compilation.GetProperty("state").GetString());
            Assert.Equal("complete", compilation.GetProperty("reason").GetString());
        }
        finally
        {
            if (Directory.Exists(root))
            {
                Directory.Delete(root, recursive: true);
            }

            if (Directory.Exists(gpf))
            {
                Directory.Delete(gpf, recursive: true);
            }

            if (File.Exists(outPath))
            {
                File.Delete(outPath);
            }
        }
    }

    [Fact]
    public void a_project_already_restored_is_never_restored_again()
    {
        var root = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-noop-{Guid.NewGuid():N}");
        var gpf = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-noop-gpf-{Guid.NewGuid():N}");
        var emptyGpf = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-noop-empty-{Guid.NewGuid():N}");
        var outPath = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-noop-out-{Guid.NewGuid():N}.json");
        try
        {
            Directory.CreateDirectory(gpf);
            Directory.CreateDirectory(emptyGpf);
            WriteStubPackage(gpf);
            WriteConsumerProject(root);

            // Restored ahead of time by a plain dotnet restore, so the assets
            // file under comparison was never written by the engine.
            var setupRestore = RunDotnetRestore(Path.Combine(root, "Consumer.csproj"), gpf, gpf);
            Assert.True(setupRestore.ExitCode == 0, $"setup restore failed: {setupRestore.Output}");
            var assetsPath = Path.Combine(root, "obj", "project.assets.json");
            Assert.True(File.Exists(assetsPath));
            var bytesBefore = File.ReadAllBytes(assetsPath);
            var writeTimeBefore = File.GetLastWriteTimeUtc(assetsPath);

            // The second run points NUGET_PACKAGES at an EMPTY folder: if the
            // engine attempted a restore here at all, it would fail loudly
            // (the package is nowhere to be found) rather than silently
            // leaving the file alone -- a stronger proof of "never restored
            // again" than merely diffing bytes.
            var (exitCode, _, stderr, _) = RunCli(
                root, emptyGpf,
                "Consumer.csproj", "--root", ".", "--emit", "context", "--context", outPath, "--tfm", Tfm);

            Assert.True(exitCode == 0, $"expected exit 0, got {exitCode}. stderr:\n{stderr}");
            var compilation = SingleCompilation(outPath);
            Assert.Equal("complete", compilation.GetProperty("state").GetString());
            Assert.Equal(bytesBefore, File.ReadAllBytes(assetsPath));
            Assert.Equal(writeTimeBefore, File.GetLastWriteTimeUtc(assetsPath));
        }
        finally
        {
            if (Directory.Exists(root))
            {
                Directory.Delete(root, recursive: true);
            }

            foreach (var dir in new[] { gpf, emptyGpf })
            {
                if (Directory.Exists(dir))
                {
                    Directory.Delete(dir, recursive: true);
                }
            }

            if (File.Exists(outPath))
            {
                File.Delete(outPath);
            }
        }
    }

    [Fact]
    public void a_project_reference_the_target_selection_excludes_is_still_restored()
    {
        var root = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-excluded-ref-{Guid.NewGuid():N}");
        var gpf = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-excluded-ref-gpf-{Guid.NewGuid():N}");
        var outPath = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-excluded-ref-out-{Guid.NewGuid():N}.json");
        try
        {
            Directory.CreateDirectory(gpf);
            WriteStubPackage(gpf);
            var referencedLibDir = Path.Combine(root, "ReferencedLib");
            var consumerDir = Path.Combine(root, "Consumer");
            var referencedLibProject = WriteReferencedLibProject(referencedLibDir);
            var consumerProject = WriteConsumerReferencingProject(consumerDir, referencedLibProject);
            Assert.False(File.Exists(Path.Combine(referencedLibDir, "obj", "project.assets.json")));
            Assert.False(File.Exists(Path.Combine(consumerDir, "obj", "project.assets.json")));

            // A bare .csproj input, not a solution: ReferencedLib reaches the
            // workspace only because Consumer's own ProjectReference pulls it
            // in, exactly the "single-project input" shape the fix also
            // covers, not only a solution-declared sibling. --projects
            // Consumer leaves ReferencedLib out of target selection (the
            // same "reachable but not selected" shape a declared-but-not-
            // requested TFM variant produces) without a second target
            // framework's own ref-pack availability as a confound. --root is
            // relative to the working directory, matching every other test
            // in this file, so a macOS temp-directory symlink (Path
            // .GetTempPath() vs. the real path a child process's own cwd
            // resolves to) can never make the two disagree.
            var (exitCode, _, stderr, _) = RunCli(
                consumerDir, gpf,
                "Consumer.csproj", "--root", "..", "--projects", "Consumer",
                "--emit", "context", "--context", outPath, "--tfm", Tfm);

            Assert.True(exitCode == 0, $"expected exit 0, got {exitCode}. stderr:\n{stderr}");
            Assert.True(
                File.Exists(Path.Combine(referencedLibDir, "obj", "project.assets.json")),
                "the excluded project reference should have been restored too, not only the selected consumer");
            Assert.True(File.Exists(Path.Combine(consumerDir, "obj", "project.assets.json")));

            using var document = JsonDocument.Parse(File.ReadAllText(outPath));
            var consumerRecord = document.RootElement.GetProperty("compilations").EnumerateArray()
                .Single(c => c.GetProperty("identity").GetProperty("projectName").GetString() == "Consumer");
            Assert.Equal("complete", consumerRecord.GetProperty("state").GetString());
            Assert.Equal("complete", consumerRecord.GetProperty("reason").GetString());
        }
        finally
        {
            if (Directory.Exists(root))
            {
                Directory.Delete(root, recursive: true);
            }

            if (Directory.Exists(gpf))
            {
                Directory.Delete(gpf, recursive: true);
            }

            if (File.Exists(outPath))
            {
                File.Delete(outPath);
            }
        }
    }

    [Fact]
    public void a_package_absent_from_the_local_cache_is_reported_unrestored_and_fails_fast()
    {
        var root = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-cache-miss-{Guid.NewGuid():N}");
        var emptyGpf = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-cache-miss-gpf-{Guid.NewGuid():N}");
        var outPath = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-cache-miss-out-{Guid.NewGuid():N}.json");
        var factsPath = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-cache-miss-facts-{Guid.NewGuid():N}.json");
        try
        {
            Directory.CreateDirectory(emptyGpf);
            WriteConsumerProject(root);
            var assetsPath = Path.Combine(root, "obj", "project.assets.json");
            string[] args =
            {
                "Consumer.csproj", "--root", ".", "--emit", "context,compiler-facts", "--context", outPath,
                "--compiler-facts", factsPath, "--tfm", Tfm,
            };

            var first = RunCli(root, emptyGpf, args);

            Assert.True(first.ExitCode == 0, $"expected exit 0 (a partial artifact still writes), got {first.ExitCode}. stderr:\n{first.StdErr}");
            // An empty global packages folder is the only source, so a miss
            // returns at once; a restore that reached for a remote feed would
            // wait on the network instead.
            Assert.True(first.Elapsed < TimeSpan.FromSeconds(30), $"restore should fail fast offline, took {first.Elapsed}");
            Assert.Contains("restoring (offline)", first.StdErr);
            AssertUnrestored(outPath, factsPath);

            // The failed restore still writes an assets file recording the
            // error. A later run must read that record rather than mistake the
            // file for a successful restore or restore the project again.
            Assert.True(File.Exists(assetsPath), "a failed restore is expected to leave an assets file behind");
            var bytesBefore = File.ReadAllBytes(assetsPath);
            var writeTimeBefore = File.GetLastWriteTimeUtc(assetsPath);

            var second = RunCli(root, emptyGpf, args);

            Assert.True(second.ExitCode == 0, $"expected exit 0 on the second run, got {second.ExitCode}. stderr:\n{second.StdErr}");
            Assert.DoesNotContain("restoring (offline)", second.StdErr);
            AssertUnrestored(outPath, factsPath);
            Assert.Equal(bytesBefore, File.ReadAllBytes(assetsPath));
            Assert.Equal(writeTimeBefore, File.GetLastWriteTimeUtc(assetsPath));
        }
        finally
        {
            DeleteAll(root, emptyGpf, outPath, factsPath);
        }

        static void AssertUnrestored(string contextPath, string compilerFactsPath)
        {
            var compilation = SingleCompilation(contextPath);
            Assert.Equal("partial", compilation.GetProperty("state").GetString());
            Assert.Equal("unrestored", compilation.GetProperty("reason").GetString());
            var messages = WorkspaceMessages(compilation);
            Assert.Contains(messages, m => m.StartsWith("unrestored:", StringComparison.Ordinal) && m.Contains("NU1101", StringComparison.Ordinal));

            var reasons = IncompleteUnitReasons(compilerFactsPath);
            var reason = Assert.Single(reasons);
            Assert.StartsWith("unrestored:", reason, StringComparison.Ordinal);
            Assert.Contains("NU1101", reason, StringComparison.Ordinal);
        }
    }

    [Fact]
    public void a_project_level_additional_source_is_never_contacted()
    {
        var root = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-addl-source-{Guid.NewGuid():N}");
        var emptyGpf = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-addl-source-gpf-{Guid.NewGuid():N}");
        var outPath = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-addl-source-out-{Guid.NewGuid():N}.json");
        try
        {
            Directory.CreateDirectory(emptyGpf);
            // Nothing listens on this port: a restore that consults the feed
            // fails with a source error (NU1301) instead of a cache miss.
            WriteConsumerProject(
                root,
                "<RestoreAdditionalProjectSources>https://127.0.0.1:7/v3/index.json</RestoreAdditionalProjectSources>");

            var (exitCode, _, stderr, _) = RunCli(
                root, emptyGpf,
                "Consumer.csproj", "--root", ".", "--emit", "context", "--context", outPath, "--tfm", Tfm);

            Assert.True(exitCode == 0, $"expected exit 0, got {exitCode}. stderr:\n{stderr}");
            var compilation = SingleCompilation(outPath);
            Assert.Equal("unrestored", compilation.GetProperty("reason").GetString());
            var detail = Assert.Single(WorkspaceMessages(compilation), m => m.StartsWith("unrestored:", StringComparison.Ordinal));
            Assert.Contains("NU1101", detail, StringComparison.Ordinal);
            Assert.Contains(Path.GetFileName(emptyGpf), detail, StringComparison.Ordinal);
            Assert.DoesNotContain("NU1301", detail, StringComparison.Ordinal);
            Assert.DoesNotContain("127.0.0.1", detail, StringComparison.Ordinal);

            var assets = File.ReadAllText(Path.Combine(root, "obj", "project.assets.json"));
            Assert.DoesNotContain("NU1301", assets, StringComparison.Ordinal);
            Assert.DoesNotContain("127.0.0.1", assets, StringComparison.Ordinal);
        }
        finally
        {
            DeleteAll(root, emptyGpf, outPath);
        }
    }

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public void an_already_restored_netstandard_project_is_never_restored_again(bool customIntermediatePath)
    {
        var root = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-netstandard-{Guid.NewGuid():N}");
        var staging = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-netstandard-staging-{Guid.NewGuid():N}");
        var feed = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-netstandard-feed-{Guid.NewGuid():N}");
        var gpf = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-netstandard-gpf-{Guid.NewGuid():N}");
        var outPath = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-netstandard-out-{Guid.NewGuid():N}.json");
        try
        {
            Directory.CreateDirectory(feed);
            Directory.CreateDirectory(gpf);
            var nupkg = WriteStubPackage(staging, "netstandard2.0");
            File.Copy(nupkg, Path.Combine(feed, Path.GetFileName(nupkg)));

            if (customIntermediatePath)
            {
                WriteCustomIntermediatePathProps(root);
            }

            var project = WriteNetStandardLibProject(Path.Combine(root, "Lib"));
            var assetsPath = customIntermediatePath
                ? Path.Combine(root, "artifacts", "obj", "Lib", "project.assets.json")
                : Path.Combine(root, "Lib", "obj", "project.assets.json");

            // Restored from a folder feed that is not the global packages
            // folder: restoring again from the global packages folder would
            // rewrite the recorded sources, so any second restore shows up as
            // changed bytes.
            var setup = RunDotnetRestore(project, gpf, feed);
            Assert.True(setup.ExitCode == 0, $"setup restore failed: {setup.Output}");
            Assert.True(File.Exists(assetsPath), $"expected the setup restore to write {assetsPath}");
            var bytesBefore = File.ReadAllBytes(assetsPath);
            var writeTimeBefore = File.GetLastWriteTimeUtc(assetsPath);

            var (exitCode, _, stderr, _) = RunCli(
                root, gpf,
                Path.Combine("Lib", "Lib.csproj"), "--root", ".", "--emit", "context", "--context", outPath,
                "--tfm", "netstandard2.0");

            Assert.True(exitCode == 0, $"expected exit 0, got {exitCode}. stderr:\n{stderr}");
            Assert.DoesNotContain("restoring (offline)", stderr);
            Assert.Equal(bytesBefore, File.ReadAllBytes(assetsPath));
            Assert.Equal(writeTimeBefore, File.GetLastWriteTimeUtc(assetsPath));
            if (customIntermediatePath)
            {
                Assert.False(Directory.Exists(Path.Combine(root, "Lib", "obj")), "nothing should be restored into the conventional obj/");
            }
        }
        finally
        {
            DeleteAll(root, staging, feed, gpf, outPath);
        }
    }

    [Fact]
    public void a_never_restored_netstandard_project_is_restored_where_msbuild_places_its_assets_file()
    {
        var root = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-netstandard-custom-{Guid.NewGuid():N}");
        var gpf = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-netstandard-custom-gpf-{Guid.NewGuid():N}");
        var outPath = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-netstandard-custom-out-{Guid.NewGuid():N}.json");
        try
        {
            Directory.CreateDirectory(gpf);
            WriteStubPackage(gpf, "netstandard2.0");
            WriteCustomIntermediatePathProps(root);
            WriteNetStandardLibProject(Path.Combine(root, "Lib"));
            var customAssets = Path.Combine(root, "artifacts", "obj", "Lib", "project.assets.json");

            // A leftover file at the conventional location must not stand in
            // for the missing one at the evaluated location.
            var staleAssets = Path.Combine(root, "Lib", "obj", "project.assets.json");
            Directory.CreateDirectory(Path.GetDirectoryName(staleAssets)!);
            File.WriteAllText(staleAssets, "{}");

            var (exitCode, _, stderr, _) = RunCli(
                root, gpf,
                Path.Combine("Lib", "Lib.csproj"), "--root", ".", "--emit", "context", "--context", outPath,
                "--tfm", "netstandard2.0");

            Assert.True(exitCode == 0, $"expected exit 0, got {exitCode}. stderr:\n{stderr}");
            Assert.Contains("restoring (offline)", stderr);
            Assert.True(File.Exists(customAssets), $"expected the restore to write {customAssets}. stderr:\n{stderr}");
            Assert.Null(ScoutSemantic.OfflineRestore.RecordedRestoreError(customAssets));
            Assert.Equal("{}", File.ReadAllText(staleAssets));
        }
        finally
        {
            DeleteAll(root, gpf, outPath);
        }
    }

    [Fact]
    public void no_restore_skips_the_step_entirely_and_reproduces_the_pre_offline_restore_behaviour()
    {
        var root = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-optout-{Guid.NewGuid():N}");
        var gpf = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-optout-gpf-{Guid.NewGuid():N}");
        var outPath = Path.Combine(Path.GetTempPath(), $"scout-semantic-restore-optout-out-{Guid.NewGuid():N}.json");
        try
        {
            Directory.CreateDirectory(gpf);
            WriteStubPackage(gpf);
            WriteConsumerProject(root);

            var (exitCode, _, stderr, _) = RunCli(
                root, gpf,
                "Consumer.csproj", "--root", ".", "--emit", "context", "--context", outPath, "--tfm", Tfm, "--no-restore");

            Assert.True(exitCode == 0, $"expected exit 0, got {exitCode}. stderr:\n{stderr}");
            Assert.False(
                File.Exists(Path.Combine(root, "obj", "project.assets.json")),
                "--no-restore must leave an unrestored input exactly as unrestored as it found it");

            var compilation = SingleCompilation(outPath);
            Assert.Equal("partial", compilation.GetProperty("state").GetString());
            Assert.Equal("binding-error", compilation.GetProperty("reason").GetString());
        }
        finally
        {
            if (Directory.Exists(root))
            {
                Directory.Delete(root, recursive: true);
            }

            if (Directory.Exists(gpf))
            {
                Directory.Delete(gpf, recursive: true);
            }

            if (File.Exists(outPath))
            {
                File.Delete(outPath);
            }
        }
    }
}
