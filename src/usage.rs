//! The usage lines every verb prints when its own arguments do not parse.
//!
//! One definition per verb, in one module: a verb names its usage constant
//! where it would print the line, `cli.md` renders these values as the
//! verbatim blocks under each verb's heading, and no verb keeps a second copy
//! of its own usage text.

/// `devscout audit`, the semantic-oracle scorer.
pub const AUDIT_USAGE: &str =
    "usage: devscout audit --semantic <refs.jsonl> [--units F] [--defs F] [--json] [--assert F] [--fp-sites F]";

/// `devscout clear --older-than <days>`.
pub const CLEAR_OLDER_THAN_USAGE: &str = "usage: devscout clear --older-than <days>";

/// `devscout clear --session <id-or-prefix>`.
pub const CLEAR_SESSION_USAGE: &str = "usage: devscout clear --session <id-or-prefix>";

/// `devscout compiler-facts` with no sub-command, or an unrecognized one. The
/// line the command prints is this value followed by the three sub-command
/// usages below.
pub const COMPILER_FACTS_USAGE: &str = "usage: devscout compiler-facts <run|import|status>";

/// `devscout compiler-facts run`.
pub const COMPILER_FACTS_RUN_USAGE: &str = "usage: devscout compiler-facts run --solution <path.sln|path.csproj> [--target T] [--configuration C] [--platform P] [--capabilities a,b] [--timeout-ms N] [--output-cap-bytes N]";

/// `devscout compiler-facts import`.
pub const COMPILER_FACTS_IMPORT_USAGE: &str =
    "usage: devscout compiler-facts import <file> [--target T] [--configuration C] [--platform P]";

/// `devscout compiler-facts status`.
pub const COMPILER_FACTS_STATUS_USAGE: &str = "usage: devscout compiler-facts status";

/// `devscout tests <symbol>`.
pub const TESTS_USAGE: &str =
    "usage: devscout tests <symbol> [--no-guess] [--no-dispatch] [--no-bus] [--pick N] [--json|--compact]";

/// `devscout parse <file.cs>`.
pub const PARSE_USAGE: &str = "usage: devscout parse <file.cs>";

/// `devscout spans <file.cs>`.
pub const SPANS_USAGE: &str = "usage: devscout spans <file.cs>";

/// `devscout extract-dump <file.cs>`.
pub const EXTRACT_DUMP_USAGE: &str = "usage: devscout extract-dump <file.cs>";

/// `devscout hook <read|bash>`.
pub const HOOK_USAGE: &str = "usage: devscout hook <read|bash>";

/// The line an unrecognized sub-command prints, listing every verb.
pub const COMMANDS_USAGE: &str = "usage: devscout <noop|parse|spans|extract-dump|hook|refs|read|impact|tests|find|import-edges|compiler-facts|export|map|stats|clear|init> [args]";

/// `devscout export`.
pub const EXPORT_USAGE: &str = "usage: devscout export [--out FILE]";

/// `devscout find <query>`.
pub const FIND_USAGE: &str = "usage: devscout find <query> [--resources]";

/// `devscout impact <file|symbol>`.
pub const IMPACT_USAGE: &str = "usage: devscout impact <file|symbol> [--hops N] [--no-iface] [--no-guess] [--no-dispatch] [--no-bus] [--no-imports] [--iface-max-fanin N] [--hub-max-indegree N] [--pick N] [--json|--compact]";

/// `devscout import-edges <file>`.
pub const IMPORT_EDGES_USAGE: &str = "usage: devscout import-edges <file> --repo <id>";

/// `devscout read <symbol>`.
pub const READ_USAGE: &str =
    "usage: devscout read <symbol> [--no-guess] [--no-dispatch] [--no-bus] [--pick N] [--json|--compact]";

/// `devscout refs <symbol>`.
pub const REFS_USAGE: &str = "usage: devscout refs <symbol> [--out] [--all] [--no-guess] [--no-dispatch] [--no-bus] [--pick N] [--json|--compact]";
