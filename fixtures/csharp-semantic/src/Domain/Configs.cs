// Case a: FilterConfig.Property and ColumnConfig.Property are POCO properties that share the
// name "Property" with EF's Entity<T>.Property(...) fluent method -- probes guess-tier false
// positives when AppDbContext's entity.Property(...) calls resolve against these unrelated
// POCOs by name only (see src/App/AppDbContext.cs, also case a).
namespace Fixture.Domain;

public class FilterConfig
{
    public string Property { get; set; } = string.Empty;
}

public class ColumnConfig
{
    public string Property { get; set; } = string.Empty;
}
