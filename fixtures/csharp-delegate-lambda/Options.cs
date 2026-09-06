namespace Fixture.Domain;

public class Options
{
    public bool Enabled { get; set; }

    public void Configure() { }

    public void Tune() { }
}

public class Endpoint
{
    public void Bind() { }
}

public class Channel
{
    public void Open() { }
}
