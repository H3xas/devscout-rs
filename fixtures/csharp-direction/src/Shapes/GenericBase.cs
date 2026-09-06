namespace Fixture.Shapes;

public class GenericBase<T>
{
    public T Value = default!;

    public void Store(T item) { }
}
