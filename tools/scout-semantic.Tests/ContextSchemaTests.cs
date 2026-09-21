using ScoutSemantic;

using Xunit;

namespace ScoutSemanticTests;

/// <summary>
/// <see cref="ContextSchema.Validate"/> checks exactly what a C# `required`
/// init-only property cannot express: forward slashes, 1-based lines, a
/// non-empty reason, and no absolute local path in a path-shaped field. A
/// well-formed minimal record validates cleanly; each violation throws
/// <see cref="ContextSchemaException"/> individually.
/// </summary>
public sealed class ContextSchemaTests
{
    private static ContextRecord Minimal(string projectPath = "src/App/App.csproj", string state = "complete", string reason = "complete") => new()
    {
        Identity = new ContextIdentity
        {
            ProjectPath = projectPath,
            ProjectName = "App",
            RequestedTfm = null,
        },
        State = state,
        Reason = reason,
    };

    [Fact]
    public void well_formed_minimal_record_validates_cleanly()
    {
        var record = Minimal();
        var exception = Record.Exception(() => ContextSchema.Validate(record));
        Assert.Null(exception);
    }

    [Fact]
    public void unknown_state_throws()
    {
        var record = Minimal(state: "bogus");
        Assert.Throws<ContextSchemaException>(() => ContextSchema.Validate(record));
    }

    [Fact]
    public void empty_reason_throws()
    {
        var record = Minimal(reason: "");
        Assert.Throws<ContextSchemaException>(() => ContextSchema.Validate(record));
    }

    [Fact]
    public void backslashed_project_path_throws()
    {
        var record = Minimal(projectPath: @"src\App\App.csproj");
        Assert.Throws<ContextSchemaException>(() => ContextSchema.Validate(record));
    }

    [Fact]
    public void an_absolute_local_path_in_a_dropped_document_throws()
    {
        var record = Minimal();
        record.Documents.Dropped.Add(new DroppedDocument
        {
            Path = "/Users/someone/repo/src/App/Ghost.cs",
            Reason = "missing",
        });
        Assert.Throws<ContextSchemaException>(() => ContextSchema.Validate(record));
    }

    [Fact]
    public void a_windows_style_absolute_path_in_an_import_identity_throws()
    {
        var record = Minimal();
        record.Imports.Add(new ContextImport { Identity = @"C:\nuget\packages\foo.props", Hash = "abc" });
        Assert.Throws<ContextSchemaException>(() => ContextSchema.Validate(record));
    }

    [Fact]
    public void a_compiler_diagnostic_line_before_one_throws()
    {
        var record = Minimal();
        record.Diagnostics.Compiler.Add(new CompilerDiagnosticRecord
        {
            Severity = "Error",
            Id = "CS0000",
            Message = "bogus",
            File = "src/App/App.cs",
            Line = 0,
        });
        Assert.Throws<ContextSchemaException>(() => ContextSchema.Validate(record));
    }

    [Fact]
    public void an_empty_dropped_document_reason_throws()
    {
        var record = Minimal();
        record.Documents.Dropped.Add(new DroppedDocument { Path = "src/App/Ghost.cs", Reason = "" });
        Assert.Throws<ContextSchemaException>(() => ContextSchema.Validate(record));
    }

    [Fact]
    public void an_absolute_local_path_quoted_inside_a_workspace_diagnostic_message_throws()
    {
        var record = Minimal();
        record.Diagnostics.Workspace.Add(new WorkspaceDiagnosticRecord
        {
            Kind = "Failure",
            Message = "Msbuild failed when processing the file '/Users/someone/repo/src/App/App.csproj' with message: bogus",
        });
        Assert.Throws<ContextSchemaException>(() => ContextSchema.Validate(record));
    }

    [Fact]
    public void an_absolute_local_path_inside_a_compiler_diagnostic_message_throws()
    {
        var record = Minimal();
        record.Diagnostics.Compiler.Add(new CompilerDiagnosticRecord
        {
            Severity = "Error",
            Id = "CS0000",
            Message = "see /Users/someone/repo/src/App/App.cs for details",
        });
        Assert.Throws<ContextSchemaException>(() => ContextSchema.Validate(record));
    }

    [Fact]
    public void a_workspace_diagnostic_message_naming_only_a_relative_path_validates_cleanly()
    {
        var record = Minimal();
        record.Diagnostics.Workspace.Add(new WorkspaceDiagnosticRecord
        {
            Kind = "Failure",
            Message = "Msbuild failed when processing the file 'src/Broken/Broken.csproj' with message: bogus",
        });
        var exception = Record.Exception(() => ContextSchema.Validate(record));
        Assert.Null(exception);
    }
}
