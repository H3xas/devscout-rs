namespace Fixture.Service.Consumers;

using MassTransit;
using Fixture.Contracts;

public class SubmitOrderConsumer : IConsumer<SubmitOrder>
{
    public async Task Consume(ConsumeContext<SubmitOrder> context)
    {
        var endpoint = await context.GetSendEndpoint(new Uri("queue:" + QueueNames.Audit));
    }
}

public class AuditConsumer : IConsumer<OrderSubmitted>
{
    public Task Consume(ConsumeContext<OrderSubmitted> context) => Task.CompletedTask;
}

public class AuditConsumerDefinition : ConsumerDefinition<AuditConsumer>
{
    public AuditConsumerDefinition()
    {
        EndpointName = "audit-intake";
        Endpoint(e => e.Name = QueueNames.Audit);
    }
}

public class OrderState : SagaStateMachineInstance
{
    public Guid CorrelationId { get; set; }
}

public class OrderStateMachine : MassTransitStateMachine<OrderState>
{
}

public class OrdersDbContext
{
}
