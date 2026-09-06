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
