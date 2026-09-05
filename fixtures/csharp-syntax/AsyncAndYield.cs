// References: await expressions, await using/foreach, async lambdas, iterator methods.
namespace Syntax.Async;

class AsyncItem
{
    public int Value;

    public int Score() => Value;
}

class AsyncService
{
    public Task<AsyncItem> GetAsync() => Task.FromResult(new AsyncItem());

    public ValueTask<int> CountAsync() => new(1);

    public async IAsyncEnumerable<AsyncItem> StreamAsync()
    {
        await Task.Yield();
        yield return new AsyncItem();
    }
}

class AsyncRes : IAsyncDisposable, IDisposable
{
    public ValueTask DisposeAsync() => default;

    public void Dispose()
    {
    }

    public void Touch()
    {
    }
}

class AsyncUser
{
    AsyncService service = new();

    public async Task<int> Run()
    {
        var item = await service.GetAsync();
        item.Score();

        _ = (await service.GetAsync()).Score();
        var n = await service.CountAsync();

        await using var res = new AsyncRes();
        res.Touch();

        await using (var res2 = new AsyncRes())
        {
            res2.Touch();
        }

        await foreach (var s in service.StreamAsync())
        {
            s.Score();
        }

        await foreach (AsyncItem typed in service.StreamAsync().ConfigureAwait(false))
        {
            typed.Score();
        }

        await Task.WhenAll(service.GetAsync(), service.GetAsync());
        await service.GetAsync().ConfigureAwait(false);

        AsyncItem viaCa = await service.GetAsync().ConfigureAwait(false);
        viaCa.Score();
        var viaVar = await service.GetAsync().ConfigureAwait(false);
        viaVar.Score();

        Func<Task<int>> af = async () => (await service.GetAsync()).Score();
        await Task.Run(async () => (await service.GetAsync()).Score());

        return n + af().Result;
    }

    public async void Handler()
    {
        await service.GetAsync();
    }

    public Task<int> NonAsyncTask() => Task.FromResult(1);

    public IEnumerable<int> Seq()
    {
        yield return 1;
        yield break;
    }

    public IEnumerable<AsyncItem> Items()
    {
        yield return new AsyncItem();
    }

    public async ValueTask<AsyncItem> Wrap() => await service.GetAsync();
}
