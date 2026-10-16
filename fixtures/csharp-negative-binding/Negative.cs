namespace NegativeBinding.Negative
{
    public class Picker
    {
        public void Choose(short value) { }
        public void Choose(long value) { }
    }

    public class Sender
    {
        public void Deliver(string body) { }
        public void Deliver(byte[] body) { }
    }

    public class Vault
    {
        private void Open() { }

        public void Store()
        {
            Open();
        }
    }

    public class Locker
    {
        private int code;
    }

    public class Calling
    {
        public void Go()
        {
            var picker = new Picker();
            picker.Choose(1);

            var sender = new Sender();
            sender.Deliver(true);

            var vault = new Vault();
            vault.Open();

            var locker = new Locker();
            _ = locker.code;
        }
    }
}
