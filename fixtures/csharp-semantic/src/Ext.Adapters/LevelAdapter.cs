using Microsoft.Extensions.Logging;

namespace Fixture.Ext.Adapters;

public class LevelAdapter
{
    public LogLevel Undeclared() => LogLevel.Warning;

    public LogLevel Declared() => LogLevel.None;
}
