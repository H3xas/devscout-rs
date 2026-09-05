// A namespace-prefix trap: this namespace starts with the same letters as a
// framework root ("System") without actually being under that framework's
// namespace tree, so a naive string-prefix check (as opposed to a proper
// segment match) has something to get wrong.
namespace Systematic.Billing;

public sealed record Invoice(string Number, decimal Amount);
