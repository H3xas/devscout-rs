// An abstract class with one abstract member (implemented in Concrete.cs) and one concrete
// member inherited as-is, to probe abstract-override resolution alongside a plain inherited
// method on the same base.
namespace Fixture.Shapes;

public abstract class AbstractBase
{
    public abstract void Implement();

    public void Concrete() { }
}
