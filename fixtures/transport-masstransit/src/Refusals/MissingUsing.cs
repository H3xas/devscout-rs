namespace Fixture.Refusals
{
    public class MissingUsing
    {
        public void Configure(IRabbitMqBusFactoryConfigurator cfg)
        {
            cfg.ReceiveEndpoint("refused-missing-using", e => { });
        }
    }
}
