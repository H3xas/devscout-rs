// ---------------------------------------------------------------------------
// The message-bus seed: the names this engine recognizes before a repository
// has said anything about itself. A seed rather than the answer, because a
// useful bus is a library those names cannot enumerate -- a repository names
// its own handlers through registration calls and wraps its own publish verbs,
// and resolve/bus_vocab.rs extends both sets from exactly that. Every derived
// set starts here.
//
// One home for all four lists, because the same five publish verbs gate two
// different decisions in two different layers: extraction decides whether a
// plain identifier argument is worth recording as a publish site at all, and
// resolution decides which recorded sites emit a hop. Held as two literals
// those gates drift apart, and the drift is asymmetric -- a verb added on the
// resolve side keeps working for the two call shapes that name their message
// at the call site and quietly stops working for the identifier shape, by far
// the most common one, while adding it on the extraction side alone does
// nothing at all.
//
// A crate-level leaf module rather than a child of either layer: `extract` is
// imported by `resolve` and never the reverse, so hanging the seed off either
// one would state an ownership this vocabulary does not have, and would put a
// second dependency edge under a list that needs none. Nothing here reads
// anything else in the crate.
// ---------------------------------------------------------------------------

/// Verbs this engine reads as handing a message instance to another dispatch.
///
/// Matched on the invoked method's bare name only. Extraction keeps the
/// identifier-argument shape -- a call whose message is a plain variable --
/// behind this list, because that shape is far too common in any codebase to
/// record everywhere, while a message the call NAMES itself (a generic type
/// argument, or a type constructed as its first argument) is recorded whatever
/// the method is called. Both layers then read the same five names: this list
/// is also the seed `derive_publish_verbs` starts from before the corpus's own
/// forwarding wrappers extend it.
pub const PUBLISH_VERBS: &[&str] = &["Publish", "PublishAsync", "SubmitJob", "Reply", "Send"];

/// Handler bases every repository gets for free.
///
/// This is the seed a repository with no registration call at all falls back
/// to, never the limit of what can be recognized: a registered type's own base
/// list is what teaches the resolver a house base.
pub const CONSUMER_BASES: &[&str] = &[
    "IConsumer",
    "BaseConsumer",
    "IAmInitiatedBy",
    "IWorkHandler",
    "IRequestHandler",
];

/// Name prefixes that say something is being installed.
///
/// A registration call hands a container a type it does not otherwise use, so
/// its own name carries the only signal available before resolution, and both
/// halves below are role words rather than vendor names.
pub const INSTALL_PREFIXES: &[&str] = &["Add", "Register", "Use", "Map", "Subscribe"];

/// Name suffixes naming the role an installed type plays.
///
/// Together with [`INSTALL_PREFIXES`] this decides whether a
/// one-type-argument call is a handler registration at all; a name matching
/// neither cannot be one, and a name matching either still has to survive the
/// resolver's own check that the type it names carries a generic base.
pub const ROLE_SUFFIXES: &[&str] = &[
    "Consumer",
    "Handler",
    "Processor",
    "Worker",
    "Saga",
    "Subscriber",
    "Listener",
];

/// Whether `verb` is one of the seed's publish verbs.
///
/// The named form of the question both layers ask of [`PUBLISH_VERBS`], so the
/// identifier-argument gate is one predicate over one list rather than a
/// second literal that has to be kept in step by hand.
pub fn is_publish_verb(verb: &str) -> bool {
    PUBLISH_VERBS.contains(&verb)
}
