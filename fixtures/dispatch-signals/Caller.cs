namespace Signals;

public class Caller
{
    public void Run(IBeacon beacon)
    {
        beacon.Flash();
    }
}
