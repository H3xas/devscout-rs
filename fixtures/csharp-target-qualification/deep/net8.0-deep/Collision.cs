namespace TargetQualification.Deep.Collision
{
    public class WidgetGateway
    {
        public string Send()
        {
            return "gateway";
        }
    }

    /// <summary>
    /// Shares the member name <c>Send</c> with <see cref="WidgetGateway"/> but has no
    /// structural relationship to it; a typed caller must resolve to the receiver's own
    /// declared member, never to the unrelated same-named one.
    /// </summary>
    public class MailQueue
    {
        public string Send()
        {
            return "mail";
        }
    }

    public class CollisionCaller
    {
        public string CallGateway(WidgetGateway gateway)
        {
            return gateway.Send();
        }

        public string CallQueue(MailQueue queue)
        {
            return queue.Send();
        }
    }
}
