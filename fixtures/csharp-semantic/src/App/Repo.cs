using Fixture.Domain;

namespace Fixture.App;

public static class Repo
{
    public static Task<Order> LoadAsync() => Task.FromResult(Order.Load("r"));
}
