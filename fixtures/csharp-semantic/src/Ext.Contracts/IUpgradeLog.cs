namespace Fixture.Ext.Contracts;

public interface IUpgradeLog
{
    void LogInformation(string format, params object[] args);
}
