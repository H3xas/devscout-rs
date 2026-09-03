// Case a: entity.Property(...) EF fluent calls vs. FilterConfig/ColumnConfig.Property POCO
// properties (src/Domain/Configs.cs) -- probes guess-tier false positives from the Property(...)
// name collision, alongside the precise entity.HasKey(e => e.Id) / e.Name / e.Total POCO hits.
// Case c3: the file-scoped `global using` below is consumed project-wide; src/App/Worker.cs adds
// a redundant local `using` for the same namespace (case c1), and
// src/Ext.Adapters/ServiceCollectionExtensions.cs resolves the same extension via enclosing
// namespace only, with no using at all (case c2).
global using Fixture.Ext.Adapters.Registration;

using Microsoft.EntityFrameworkCore;
using Fixture.Domain;

namespace Fixture.App;

public class AppDbContext : DbContext
{
    public DbSet<Order> Orders => Set<Order>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        modelBuilder.Entity<Order>(entity =>
        {
            entity.HasKey(e => e.Id);
            entity.Property(e => e.Name).HasMaxLength(64);
            entity.Property(e => e.Total);
        });
    }
}
