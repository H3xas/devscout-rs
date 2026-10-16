namespace Fixture.Services
{
    public interface IClock
    {
        int Now();
    }

    public class Clock : IClock
    {
        public int Now() => 0;
    }

    public interface IServiceCollection
    {
        IServiceCollection AddScoped<TService, TImplementation>();
    }

    public static class Composition
    {
        public static void Configure(IServiceCollection services)
        {
            services.AddScoped<IClock, Clock>();
        }
    }

    public class Schedule
    {
        private readonly IClock _clock;

        public Schedule(IClock clock) => _clock = clock;

        public int Twice() => _clock.Now() + _clock.Now();
    }
}
