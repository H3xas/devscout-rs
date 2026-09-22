namespace Truth.DroppedProject.A;

public sealed class Consumer
{
    public void UseB(Truth.DroppedProject.B.Loader loader)
    {
        loader.Load();
    }
}
