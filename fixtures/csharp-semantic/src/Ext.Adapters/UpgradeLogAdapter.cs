// Case b: UpgradeLogAdapter implements IUpgradeLog by forwarding to a field typed as the
// framework's ILogger, so `_inner.LogInformation` (external) and `IUpgradeLog.LogInformation`
// (in-tree) share the same member name at adjacent call sites.
// Case b (MigrationRunner): `_log.LogInformation` resolves through the field's IUpgradeLog type
// (tier e, precise) -- the contrasting precise case next to the ambiguous adapter call above.
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
