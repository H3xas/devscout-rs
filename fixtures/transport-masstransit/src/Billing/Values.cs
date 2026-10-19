namespace Fixture.Billing
{
    public class Values
    {
        private string _plain = "plain-field";

        public void Configure(IRabbitMqBusFactoryConfigurator cfg, string queueName, string host)
        {
            var localName = "local-name";
            cfg.ReceiveEndpoint(queueName, e => { });
            cfg.ReceiveEndpoint(localName, e => { });
            cfg.ReceiveEndpoint(QueueNames.Rotating, e => { });
            cfg.ReceiveEndpoint(_plain, e => { });
            cfg.ReceiveEndpoint(NextName(), e => { });
            cfg.ReceiveEndpoint(queueName.Length > 3 ? "long-name" : "short-name", e => { });
            cfg.ReceiveEndpoint(VendorKit.Queues.Default, e => { });
            cfg.ReceiveEndpoint(QueueNames.Missing, e => { });
            cfg.ReceiveEndpoint(QueueNames.Loop, e => { });
            cfg.ReceiveEndpoint("bad name!", e => { });
            cfg.ReceiveEndpoint(QueueNames.Mutable, e => { });
            cfg.Message<InvoiceDue>(m => m.SetEntityName(nameof(InvoiceDue)));
            cfg.Host(host, h => { });
        }

        private static string NextName() => "next";
    }
}
