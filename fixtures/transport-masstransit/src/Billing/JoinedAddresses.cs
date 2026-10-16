namespace Fixture.Billing
{
    public static class BrokerParts
    {
        public const string Scheme = "rabbitmq://";
        public const string User = "ledger";
        public const string SendTail = "@broker.internal/billing/joined-invoices";
        public const string RefundTail = "@broker.internal/billing/joined-refunds";
        public const string HostTail = "@broker.internal/joined";
    }

    public class JoinedAddresses
    {
        private readonly ISendEndpointProvider _provider;

        public JoinedAddresses(ISendEndpointProvider provider)
        {
            _provider = provider;
        }

        public async Task Send()
        {
            await _provider.GetSendEndpoint(new Uri("rabbitmq://ledger:s3cret-joined-send" + BrokerParts.SendTail));
            await _provider.GetSendEndpoint(new Uri(BrokerParts.Scheme + BrokerParts.User + ":s3cret-joined-middle" + BrokerParts.RefundTail));
        }

        public static void Configure(IRabbitMqBusFactoryConfigurator cfg)
        {
            cfg.Host(new Uri("rabbitmq://ledger:s3cret-joined-host" + BrokerParts.HostTail));
        }
    }
}
