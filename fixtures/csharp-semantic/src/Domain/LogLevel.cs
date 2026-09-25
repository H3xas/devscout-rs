namespace Fixture.Domain;

public enum LogLevel
{
    None,
    Fatal
}

public class LevelReader
{
    public LogLevel Current() => LogLevel.None;
}
