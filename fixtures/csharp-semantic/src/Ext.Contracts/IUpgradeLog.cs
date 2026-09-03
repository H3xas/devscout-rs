// Case b: IUpgradeLog.LogInformation is an in-tree interface method whose name and signature
// shadow Microsoft.Extensions.Logging's ILogger LogInformation extension -- probes external
// vs. in-tree name-collision guessing (see src/Ext.Adapters/UpgradeLogAdapter.cs, also case b).
namespace Fixture.Ext.Contracts;

public interface IUpgradeLog
{
    void LogInformation(string format, params object[] args);
}
