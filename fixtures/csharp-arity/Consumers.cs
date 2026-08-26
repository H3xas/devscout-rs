using Plain;
using Generic;

namespace Consumers;

public class BareConsumer
{
    private Widget value;
}

public class GenericConsumer
{
    private Widget<int> value;
}

public class MissingConsumer
{
    private Widget<int, string> value;
}

public class OpenGenericConsumer
{
    private object one = typeof(Overloaded.Foo<>);
    private object two = typeof(Overloaded.Foo<,>);
}
