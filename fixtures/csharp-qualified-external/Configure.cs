namespace App.Bus
{
    public class Configure
    {
        public void Run(object payload, System.Linq.Expressions.MemberExpression expr)
        {
            var foreign = RabbitMQ.Client.ExchangeType.Fanout;
            var own = App.Transports.Fabric.ExchangeType.Topic;
            var text = System.Text.Json.JsonSerializer.Serialize(payload);
            var local = App.Infra.JsonSerializer.Serialize(payload);
            var name = expr.Member.Name;
        }
    }
}
