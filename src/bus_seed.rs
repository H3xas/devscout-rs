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
/// behind this list or [`has_dispatch_root`], because that shape is far too
/// common in any codebase to record everywhere. This list alone seeds
/// `derive_publish_verbs` before the corpus's own forwarding wrappers extend it.
pub const PUBLISH_VERBS: &[&str] = &["Publish", "PublishAsync", "SubmitJob", "Reply", "Send"];

/// Whole words that name a dispatch in a repository's own publish method.
pub const DISPATCH_ROOTS: &[&str] = &[
    "Publish", "Send", "Submit", "Reply", "Dispatch", "Enqueue", "Emit", "Post", "Raise", "Notify",
];

/// First words that describe inspection or lifecycle work rather than dispatch.
pub const DISPATCH_GUARDS: &[&str] = &["Get", "Cancel", "Connect", "Can", "Is", "Has"];

fn word_ends_here(suffix: &str) -> bool {
    suffix.is_empty()
        || suffix
            .as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
}

/// Whether the bare method name contains an unguarded dispatch root word.
/// Seed and wrapper-promoted verbs are admitted separately by the caller.
pub fn has_dispatch_root(verb: &str) -> bool {
    let first = verb
        .strip_prefix("Try")
        .filter(|tail| tail.as_bytes().first().is_some_and(u8::is_ascii_uppercase))
        .unwrap_or(verb);
    if DISPATCH_GUARDS
        .iter()
        .any(|guard| first.strip_prefix(guard).is_some_and(word_ends_here))
    {
        return false;
    }
    DISPATCH_ROOTS.iter().any(|root| {
        verb.match_indices(root)
            .any(|(start, _)| word_ends_here(&verb[start + root.len()..]))
    })
}

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

#[cfg(test)]
mod tests {
    use super::{has_dispatch_root, DISPATCH_GUARDS, DISPATCH_ROOTS};

    #[test]
    fn dispatch_root_word_rule() {
        for root in DISPATCH_ROOTS {
            assert!(has_dispatch_root(root), "{root}");
            assert!(has_dispatch_root(&format!("Try{root}")), "Try{root}");
            assert!(
                has_dispatch_root(&format!("{root}Messages")),
                "{root}Messages"
            );
            assert!(has_dispatch_root(&format!("{root}2")), "{root}2");
            assert!(has_dispatch_root(&format!("{root}_Now")), "{root}_Now");
            assert!(!has_dispatch_root(&format!("{root}ed")), "{root}ed");
        }
        for guard in DISPATCH_GUARDS {
            for prefix in ["", "Try"] {
                assert!(!has_dispatch_root(&format!("{prefix}{guard}Publish")));
                assert!(!has_dispatch_root(&format!("{prefix}{guard}2Publish")));
            }
        }
        for name in [
            "IssuePublish",
            "HashPublish",
            "TryPublish",
            "Published",
            "Sender",
            "Dispatcher",
            "Postgres",
            "Publishé",
            "Publish-Again",
            "ProjectTo",
            "IsAny",
            "",
            "TryGetPublishedMessages",
        ] {
            let expected = matches!(name, "IssuePublish" | "HashPublish" | "TryPublish");
            assert_eq!(has_dispatch_root(name), expected, "{name}");
        }
    }
}
