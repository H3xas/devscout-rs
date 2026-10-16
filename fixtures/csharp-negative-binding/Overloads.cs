namespace NegativeBinding.Overload
{
    public class Sink
    {
        public void Write(int value) { }
    }

    public class BufferedSink : Sink
    {
        public new void Write(int value) { }
    }

    public class Recorder
    {
        private Sink sink = new BufferedSink();

        public void Capture()
        {
            sink.Write(1);
        }
    }

    public class Emitter
    {
        public virtual void Send(string body) { }
    }

    public class JsonEmitter : Emitter
    {
        public override void Send(string body) { }
    }

    public class Dispatcher
    {
        private Emitter emitter = new JsonEmitter();

        public void Dispatch()
        {
            emitter.Send("payload");
        }
    }

    public class Source
    {
        public void Read() { }
    }

    public class Buffer : Source
    {
        public new void Read() { }
    }

    public class Reader
    {
        private Source source = new Buffer();

        public void Drain()
        {
            var buffer = source as Buffer;
            buffer!.Read();
        }
    }

    public interface IChannel
    {
        void Push(string body);
    }

    public class Channel : IChannel
    {
        public void Push(string body) { }
    }

    public class Relay
    {
        public void Forward(IChannel channel)
        {
            channel.Push("payload");
        }
    }
}
