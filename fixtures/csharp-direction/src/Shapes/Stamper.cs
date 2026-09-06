// The class half of the IStamp.cs pair: a public Stamp() that satisfies IStamp for any derived
// type, and that a derived type's explicit IStamp.Stamp() never hides from a class-typed receiver.
namespace Fixture.Shapes;

public class Stamper
{
    public void Stamp() { }
}
