using Microsoft.Extensions.Logging;
using Fixture.Ext.Contracts;

namespace Fixture.Ext.Adapters;

public class UpgradeLogAdapter : IUpgradeLog
{
    private readonly ILogger _inner;

    public UpgradeLogAdapter(ILogger inner)
    {
        _inner = inner;
    }

    public void LogInformation(string format, params object[] args) => _inner.LogInformation(format, args);
}

public class MigrationRunner
{
    private readonly IUpgradeLog _log;

    public MigrationRunner(IUpgradeLog log)
    {
        _log = log;
    }

    public void Run() => _log.LogInformation("m {0}", 1);
}
