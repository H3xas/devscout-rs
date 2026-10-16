namespace Fixture.Billing
{
    public class RefundSender
    {
        private readonly ISendEndpointProvider provider;
        private readonly string queue = "this-instance-queue";

        public RefundSender(ISendEndpointProvider provider)
        {
            this.provider = provider;
        }

        public IBus Bus { get; set; }

        public async Task Send()
        {
            await this.provider.GetSendEndpoint(new Uri("queue:this-field-refunds"));
            await this.Bus.GetSendEndpoint(new Uri("queue:this-property-refunds"));
        }

        public async Task SendShadowed(string provider)
        {
            await this.provider.GetSendEndpoint(new Uri("queue:this-shadowed-refunds"));
            await this.provider.GetSendEndpoint(new Uri(this.queue));
        }
    }
}
