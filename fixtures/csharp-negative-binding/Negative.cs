namespace NegativeBinding.Negative
{
    public class Picker
    {
        public void Choose(int left, double right) { }
        public void Choose(double left, int right) { }
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
        private int code { get; set; }
    }

    public class Calling
    {
        public void Go()
        {
            var picker = new Picker();
            picker.Choose(1, 1);

            var sender = new Sender();
            sender.Deliver(true);

            var vault = new Vault();
            vault.Open();

            var locker = new Locker();
            _ = locker.code;
        }
    }
}
