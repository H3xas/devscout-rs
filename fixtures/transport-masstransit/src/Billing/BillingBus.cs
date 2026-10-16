namespace Fixture.Billing
{
    public static class BillingBus
    {
        public static IBusControl Create()
        {
            return Bus.Factory.CreateUsingRabbitMq(cfg =>
            {
                cfg.Host(new Uri("rabbitmq://broker.internal/billing"));
                cfg.ReceiveEndpoint("order-intake", e => { });
            });
        }

        public static void Bind(IServiceCollection services)
        {
            services.AddOptions<RabbitMqTransportOptions>().BindConfiguration("Ops");
        }

        public static void Twice(IServiceCollection services)
        {
            services.AddMassTransit(x => x.UsingRabbitMq((context, cfg) =>
            {
                cfg.Host("broker-a.internal");
                cfg.Host("broker-b.internal");
                cfg.ReceiveEndpoint("twice-hosted", e => { });
                cfg.ConfigureEndpoints(context, new SnakeCaseEndpointNameFormatter("billing", false));
                cfg.ConfigureEndpoints(context, new LedgerNameFormatter());
                cfg.ConfigureEndpoints(context, filter => { });
            }));
        }

        public static void Hostless(IServiceCollection services)
        {
            services.AddMassTransit(x => x.UsingRabbitMq((context, cfg) =>
            {
                cfg.ReceiveEndpoint("hostless", e => { });
            }));
        }
    }

    public class LedgerNameFormatter
    {
    }

    public class InvoiceSender
    {
        private readonly ISendEndpointProvider _provider;
        private readonly string _queue = "instance-queue";

        public InvoiceSender(ISendEndpointProvider provider)
        {
            _provider = provider;
        }

        public IBus Publisher { get; set; }

        public async Task Send(string name)
        {
            await _provider.GetSendEndpoint(new Uri("rabbitmq://billing:s3cret-uri-pass@broker.internal/billing/invoices"));
            await Publisher.GetSendEndpoint(new Uri($"queue:{name}"));
            ISendEndpointProvider local = _provider;
            await local.GetSendEndpoint(new Uri(QueueNames.Rotating));
        }

        public async Task Consume(ConsumeContext<InvoiceDue> context)
        {
            await context.GetSendEndpoint(new Uri("amqp://ops:s3cret-amqp-pass@broker.internal/invoices"));
            await context.GetSendEndpoint(new Uri("queue"));
            await context.GetSendEndpoint(new Uri(_queue));
        }
    }
}
