//! The four generated reference pages, and the generator-owned prose they
//! carry.
//!
//! Every fact a page states comes from one of two places: a library constant
//! (`devscout_rs::{exit, layout, usage}` and the version constants their owners
//! expose), or the help text the binary printed. The prose around those facts
//! -- what a variable does, which conditions produce a status -- is owned here,
//! beside the checks that compare it with the source.
//!
//! Page content is produced by pure functions, so the same tree renders the
//! same bytes in a check, in a bless and in a diff of a stale page.

use devscout_rs::graph;
use devscout_rs::layout;
use devscout_rs::telemetry;
use devscout_rs::usage;

/// The regenerate command every notice names.
pub const REGENERATE: &str = "BLESS=1 cargo test --test docs_sync";

/// The heading a page's notice sits under, shared by every page.
const NOTICE: &str =
    "> **Generated.** Every byte of this page is written by `tests/docs_sync.rs` from\n\
> the definitions the binary itself uses, and the page is never edited by hand: a change that\n\
> belongs here belongs in the source. Regenerate it with `BLESS=1 cargo test --test docs_sync`,\n\
> and resolve a merge conflict in it by regenerating rather than by merging edits.";

/// One `usage: devscout …` constant, by the name `cli.md` renders it under.
pub const USAGE_CONSTANTS: &[(&str, &str)] = &[
    ("AUDIT_USAGE", usage::AUDIT_USAGE),
    ("CLEAR_OLDER_THAN_USAGE", usage::CLEAR_OLDER_THAN_USAGE),
    ("CLEAR_SESSION_USAGE", usage::CLEAR_SESSION_USAGE),
    ("COMMANDS_USAGE", usage::COMMANDS_USAGE),
    ("COMPILER_FACTS_USAGE", usage::COMPILER_FACTS_USAGE),
    (
        "COMPILER_FACTS_IMPORT_USAGE",
        usage::COMPILER_FACTS_IMPORT_USAGE,
    ),
    ("COMPILER_FACTS_RUN_USAGE", usage::COMPILER_FACTS_RUN_USAGE),
    (
        "COMPILER_FACTS_STATUS_USAGE",
        usage::COMPILER_FACTS_STATUS_USAGE,
    ),
    ("EXPORT_USAGE", usage::EXPORT_USAGE),
    ("EXTRACT_DUMP_USAGE", usage::EXTRACT_DUMP_USAGE),
    ("FIND_USAGE", usage::FIND_USAGE),
    ("HOOK_USAGE", usage::HOOK_USAGE),
    ("IMPACT_USAGE", usage::IMPACT_USAGE),
    ("IMPORT_EDGES_USAGE", usage::IMPORT_EDGES_USAGE),
    ("PARSE_USAGE", usage::PARSE_USAGE),
    ("READ_USAGE", usage::READ_USAGE),
    ("REFS_USAGE", usage::REFS_USAGE),
    ("SPANS_USAGE", usage::SPANS_USAGE),
    ("TESTS_USAGE", usage::TESTS_USAGE),
];

/// The value a usage constant holds, by name.
pub fn usage_value(name: &str) -> Option<&'static str> {
    USAGE_CONSTANTS
        .iter()
        .find(|(constant, _)| *constant == name)
        .map(|(_, value)| *value)
}

/// Where a variable's default comes from, so the page composes the path from
/// the same constants the code builds it from.
pub enum DefaultValue {
    /// A sentence, for a value that is not a path.
    Text(&'static str),
    /// A file under `$HOME`'s agent directory and scout directory, named here.
    HomeStore(&'static str),
}

/// One environment variable's row.
pub struct EnvRow {
    pub name: &'static str,
    pub effect: &'static str,
    pub default_value: DefaultValue,
}

/// The variables a page documents, in the order it lists them.
pub const ENVIRONMENT: &[EnvRow] = &[
    EnvRow {
        name: "SCOUT_REGISTRY",
        effect: "Path to the registry JSON: the roots devscout knows about, and the scopes each \
                 was registered with.",
        default_value: DefaultValue::HomeStore(layout::REGISTRY_FILE),
    },
    EnvRow {
        name: "SCOUT_CONTENT_DB",
        effect: "Path to the shared content-dedup SQLite database. Written only by the agent \
                 hooks (`devscout hook read|bash`), which use it to answer a repeated read \
                 without walking the tree again.",
        default_value: DefaultValue::HomeStore(layout::CONTENT_DB_FILE),
    },
    EnvRow {
        name: "SCOUT_DEBUG",
        effect: "`1` turns on hook debug output, the same as creating a `.scout/debug` entry.",
        default_value: DefaultValue::Text("unset — no debug output"),
    },
    EnvRow {
        name: "SCOUT_TELEMETRY",
        effect: "Opt-in query telemetry. Export `1` in the shell that runs \
                 `find`/`refs`/`read`/`impact`/`tests` to append one JSON line per answered \
                 invocation to the telemetry log; a usage error, or a seed with no resolved \
                 repository or graph, logs nothing. Unset (or any other value) writes nothing. \
                 The agent hooks never run these verbs, so `devscout init` does not set this \
                 variable for them.",
        default_value: DefaultValue::Text("unset — nothing is written"),
    },
    EnvRow {
        name: "SCOUT_COMPILER_ENGINE",
        effect: "Path to a built compiler-facts engine executable. Read only by \
                 `compiler-facts run`.",
        default_value: DefaultValue::Text("unset — no engine is located and nothing is downloaded"),
    },
    EnvRow {
        name: "SCOUT_EDGE_PROVENANCE",
        effect: "Path to a diagnostic artifact `map` writes while it resolves `uses-member` \
                 edges: one record per edge, naming the emission step that produced it. Written \
                 beside the artifacts named under [Storage](storage.md); nothing reads it back.",
        default_value: DefaultValue::Text("unset — the diagnostic artifact is not written"),
    },
    EnvRow {
        name: "HOME",
        effect: "Locates the registry, the content database and the agent settings file.",
        default_value: DefaultValue::Text("the value the process was started with"),
    },
];

/// Names a page lists as read by no release, kept out of the table above so a
/// reader cannot set one expecting an effect.
pub const RETIRED: &[EnvRow] = &[EnvRow {
    name: "SCOUT_MTIME_REUSE",
    effect: "No effect. `map` keys fragment reuse on the content of the bytes it indexes, so \
             there is no mtime mode left to switch to, and the variable is not read.",
    default_value: DefaultValue::Text("not read"),
}];

/// One exit status's row.
pub struct ExitRow {
    pub constant: &'static str,
    pub value: i64,
    pub meaning: &'static str,
    pub produced_by: &'static str,
}

/// The four statuses, in value order.
pub const EXIT_CODES: &[ExitRow] = &[
    ExitRow {
        constant: "EXIT_OK",
        value: devscout_rs::exit::EXIT_OK as i64,
        meaning: "The command completed and answered.",
        produced_by: "`help`, `--help` and `-h`; `noop`; `hook <read|bash>`; and any `init`, \
                      `map`, `stats`, `clear`, `import-edges`, `compiler-facts` or `export` run \
                      that finished its work.",
    },
    ExitRow {
        constant: "EXIT_FAILURE",
        value: devscout_rs::exit::EXIT_FAILURE as i64,
        meaning: "The command ran and could not answer: a refusal, an ambiguous seed, an \
                  environment it cannot read.",
        produced_by: "an ambiguous or unresolved seed; a refusal (`export`); an unreadable \
                      environment; `--json` together with `--compact`; a `-C <dir>` that cannot \
                      be applied; a Git ignore query that cannot be run or read, which stops \
                      `map` before it writes; and the usage paths of the plumbing verbs — \
                      `parse`, `spans` and `extract-dump` with no file, `hook` with no filter, an \
                      `audit` argument error, and the unknown-command line.",
    },
    ExitRow {
        constant: "EXIT_USAGE",
        value: devscout_rs::exit::EXIT_USAGE as i64,
        meaning: "The invocation was malformed, and the verb printed its usage line instead of \
                  doing any work.",
        produced_by: "a query verb's missing argument or unknown flag; an out-of-range \
                      `--pick N`, or a `--pick` value that is neither a positive integer nor \
                      `all`; `init` refusing a directory that itself contains git \
                      repositories; an ambiguous `clear --session` prefix; a malformed `export` \
                      invocation.",
    },
    ExitRow {
        constant: "EXIT_NO_RESULT",
        value: devscout_rs::exit::EXIT_NO_RESULT as i64,
        meaning: "A query resolved but reached nothing: the index was read and the answer is \
                  empty. Neither success nor failure.",
        produced_by: "`refs`, `read`, `impact` and `tests`, when the graph was searched and no \
                      row matched.",
    },
];

/// Render `docs/reference/cli.md`.
pub fn cli_page(help: &str) -> String {
    let mut page = String::new();
    page.push_str("# CLI reference\n\n");
    page.push_str(NOTICE);
    page.push_str(
        "\n\n`devscout --help` prints the same command list. Every verb runs against the \
         repository that contains the current directory, or the one `-C <dir>` names.\n\n",
    );

    page.push_str("## Commands\n\n| Command | What you get |\n| --- | --- |\n");
    page.push_str(
        "| `devscout init [scope ...]` | Register the repo, create the artifact directory, \
         install the agent hooks, run a first map |\n",
    );
    page.push_str(
        "| `devscout map [scope ...]` | Build or refresh the index; incremental — unchanged \
         files are reused. `--no-semantic` skips an admitted compiler-facts artifact even when \
         one is present (see [Compiler facts](../guides/compiler-facts.md)) |\n",
    );
    page.push_str(
        "| `devscout find <query>` | Search the manifest by symbol name or by file purpose |\n",
    );
    page.push_str(
        "| `devscout refs <symbol>` | Inbound references to a symbol, grouped by edge kind \
         (`inherits`, `uses-type`, `uses-member`, `implements`, `overrides`), plus `bus-hop` \
         rows for a handler or for a type whose file publishes a message: possible routes from a \
         recognized publish or dispatch call to a matching handler, runtime routing unverified |\n",
    );
    page.push_str(
        "| `devscout read <symbol>` | The symbol's declaration span and verbatim source plus the \
         same inbound answer as `refs` |\n",
    );
    page.push_str(
        "| `devscout impact <file\\|symbol>` | Blast radius: the files reachable from a seed \
         within N hops |\n",
    );
    page.push_str(
        "| `devscout import-edges <file>` | Load a versioned cross-repo edge export; `--repo` is \
         required, because the repository id is never inferred. `impact` then reports files \
         reached only through it. See [Cross-repo impact](../guides/cross-repo-impact.md) |\n",
    );
    page.push_str(
        "| `devscout compiler-facts <run\\|import\\|status>` | Optional: acquire or import a \
         versioned compiler-derived fact artifact (see [Compiler \
         facts](../guides/compiler-facts.md)); `map` and every query never need it and never \
         start it |\n",
    );
    page.push_str(
        "| `devscout export` | Write the current repository's whole fact set as one versioned \
         NDJSON stream — the contract downstream tools read instead of querying symbol by \
         symbol. See [Fact export](fact-export.md) |\n",
    );
    page.push_str("| `devscout tests <symbol>` | The test files that reach a symbol |\n");
    page.push_str("| `devscout stats` | Index and cache summary for the current repo |\n");
    page.push_str("| `devscout clear` | Drop freshness rows by age or by session |\n\n");
    page.push_str("An unrecognized command prints this line and exits 1:\n\n");
    page.push_str(&usage_block("COMMANDS_USAGE"));

    page.push_str("## Plumbing verbs\n\n");
    page.push_str(
        "Plumbing verbs exist for debugging, for benchmarking and for the agent-hook \
         integration:\n\n",
    );
    page.push_str("| Command | What it does |\n| --- | --- |\n");
    page.push_str("| `devscout parse <file.cs>` | Dump the parse tree |\n");
    page.push_str("| `devscout spans <file.cs>` | Dump declaration spans |\n");
    page.push_str("| `devscout extract-dump <file.cs>` | Dump extraction records |\n");
    page.push_str(
        "| `devscout audit` | Score `uses-member` edges against a compiler oracle; see \
         [methodology](../benchmarks/methodology.md#resolver-precision) |\n",
    );
    page.push_str(
        "| `devscout hook <read\\|bash>` | The agent hook filters, stdin to stdout; see \
         [Agent hooks](../guides/agent-hooks.md) |\n",
    );
    page.push_str("| `devscout noop` | Exit 0, without doing anything |\n\n");
    page.push_str(
        "The `parse` and `spans` diagnostics show the raw tree, both arms of every preprocessor \
         group included.\n\n",
    );

    page.push_str("## Global options\n\n| Option | Effect |\n| --- | --- |\n");
    page.push_str("| `-C <dir>` | Run as if `devscout` started in `<dir>` |\n");
    page.push_str("| `-h`, `--help` | Show the command list |\n");
    page.push_str("| `-V`, `--version` | Show the version |\n\n");

    page.push_str("## Help text\n\n");
    page.push_str(
        "The whole help text, byte for byte, as `devscout help`, `devscout --help` and \
         `devscout -h` print it:\n\n",
    );
    page.push_str("```text\n");
    page.push_str(help);
    page.push_str("```\n\n");

    page.push_str("## Flags by verb\n\n");
    page.push_str(
        "### init\n\n`init` has no usage line of its own; [Help text](#help-text) carries its \
         spelling.\n\n",
    );
    page.push_str("| Flag | Effect |\n| --- | --- |\n");
    page.push_str(
        "| `scope ...` | Directories to index, relative to the repository root; none means the \
         whole root |\n",
    );
    page.push_str("| `--label L` | A label stored with the root in the registry |\n");
    page.push_str(
        "| `--no-hooks` | Skip the agent-hook install; see \
         [Agent hooks](../guides/agent-hooks.md) |\n",
    );
    page.push_str("| `--no-map` | Register the root without running the first map |\n\n");
    page.push_str(
        "### map\n\n`map` has no usage line of its own either; its spelling is in \
         [Help text](#help-text).\n\n",
    );
    page.push_str("| Flag | Effect |\n| --- | --- |\n");
    page.push_str("| `scope ...` | Directories to index |\n");
    page.push_str("| `--refresh` | Accepted and ignored; every `map` is incremental |\n");
    page.push_str(
        "| `--no-semantic` | Skip an admitted compiler-facts artifact and rebuild the \
         syntax-only graph |\n\n",
    );
    page.push_str(
        "In a Git root, `map` also skips the untracked files and directories Git reports as \
         ignored -- `.gitignore` at any depth, `.git/info/exclude`, `core.excludesFile`, with \
         negations and tracked-file exceptions exactly as Git answers them -- so a local build \
         into `build/`, `lib/` or `out/` cannot duplicate a source definition. A tracked file is \
         always indexed, and a root with no `.git` entry is walked by the fixed directory-name \
         list alone, with no `git` needed. In a Git root the ignore query is required: when \
         `git` cannot be run, or its answer cannot be read, `map` exits 1 with `error: ignore \
         query failed (…)` and leaves the previous index in place.\n\n",
    );
    page.push_str("### clear\n\n");
    page.push_str(&usage_block("CLEAR_OLDER_THAN_USAGE"));
    page.push_str(&usage_block("CLEAR_SESSION_USAGE"));
    page.push_str(
        "`--older-than <days>` drops freshness rows older than N days; `--session \
         <id-or-prefix>` drops one session's rows, and an ambiguous prefix is a usage error.\n\n",
    );
    page.push_str("### find\n\n");
    page.push_str(&usage_block("FIND_USAGE"));
    page.push_str("| Flag | Effect |\n| --- | --- |\n");
    page.push_str(
        "| `--resources` | Include resource keys in the match list; by default they are reduced \
         to a one-line trailer |\n\n",
    );
    page.push_str("### refs, read, impact and tests\n\n");
    for constant in ["REFS_USAGE", "READ_USAGE", "IMPACT_USAGE", "TESTS_USAGE"] {
        page.push_str(&usage_block(constant));
    }
    page.push_str("| Flag | Verbs | Effect |\n| --- | --- | --- |\n");
    page.push_str(
        "| `--json` | all four | A versioned, machine-readable answer; see \
         [JSON output](json-output.md) |\n",
    );
    page.push_str(
        "| `--compact` | all four | One line per hit, for piping; heuristic rows are marked `x` \
         (`ext`) and `h` (`guess`) |\n",
    );
    page.push_str(
        "| `--pick N`, `all` | all four | Answer the nth (one-based) candidate of an ambiguous \
         seed, or every candidate in its own section with `all`. A member seed's candidates are \
         its declaring types, and a type seed's are the defs its name matches; an out-of-range \
         `N` and a value that is neither a positive integer nor `all` are usage errors (exit \
         code 2) |\n",
    );
    page.push_str("| `--no-guess` | all four | Drop the `guess` tier, keeping `ext` |\n");
    page.push_str(
        "| `--no-dispatch` | all four | Drop the `implements` and `overrides` edges that \
         dependency-injection registrations record |\n",
    );
    page.push_str(
        "| `--no-bus` | all four | Skip `bus-hop` edges and restore the traversal and hub \
         classification these verbs had before them |\n",
    );
    page.push_str("| `--out` | `refs` | Also list outbound references |\n");
    page.push_str("| `--all` | `refs` | Lift the inbound and outbound row caps |\n");
    page.push_str("| `--hops N` | `impact` | How many hops from the seed to walk (at least 1) |\n");
    page.push_str(
        "| `--no-iface` | `impact` | Do not widen through an interface to its implementations' \
         callers |\n",
    );
    page.push_str(
        "| `--iface-max-fanin N` | `impact` | Treat an interface injected into more than N \
         constructors as infrastructure and stop widening through it; `0` disables the brake |\n",
    );
    page.push_str(
        "| `--hub-max-indegree N` | `impact` | Stop widening through a file that N or more other \
         files reference; `0` disables the in-degree half of the brake |\n",
    );
    page.push_str("| `--no-imports` | `impact` | Ignore an imported cross-repo edge export |\n\n");
    page.push_str(
        "The seed spellings these verbs accept are described in \
         [Concepts](../concepts.md#seeds).\n\n",
    );
    page.push_str("### import-edges\n\n");
    page.push_str(&usage_block("IMPORT_EDGES_USAGE"));
    page.push_str(
        "`--repo` is required. See [Cross-repo impact](../guides/cross-repo-impact.md).\n\n",
    );
    page.push_str("### compiler-facts\n\n");
    for constant in [
        "COMPILER_FACTS_USAGE",
        "COMPILER_FACTS_RUN_USAGE",
        "COMPILER_FACTS_IMPORT_USAGE",
        "COMPILER_FACTS_STATUS_USAGE",
    ] {
        page.push_str(&usage_block(constant));
    }
    page.push_str(
        "See [Compiler facts](../guides/compiler-facts.md) for what each sub-command admits and \
         reports.\n\n",
    );
    page.push_str("### export\n\n");
    page.push_str(&usage_block("EXPORT_USAGE"));
    page.push_str(
        "Writes the header line, one line per fact and the trailer to stdout, or to `FILE` \
         through a temporary file in the same directory and a rename. The stream is documented \
         in [Fact export](fact-export.md).\n\n",
    );
    page.push_str("| Flag | Effect |\n| --- | --- |\n");
    page.push_str(
        "| `--out FILE` | Write the stream to `FILE` instead of stdout. The directory must \
         already exist; a failed run leaves no file at the path and keeps any file already \
         there |\n\n",
    );
    page.push_str(
        "Any other argument — a positional, an unknown flag, `--out` with no value or with a \
         value that is itself a flag, or `--out` given twice — is a usage error (exit 2) with \
         the usage line on stderr and nothing on stdout.\n\n",
    );
    page.push_str("### audit\n\n");
    page.push_str(&usage_block("AUDIT_USAGE"));
    page.push_str(
        "See [methodology](../benchmarks/methodology.md#resolver-precision). An `audit` \
         argument error exits 1, not 2.\n\n",
    );
    page.push_str("### hook\n\n");
    page.push_str(&usage_block("HOOK_USAGE"));
    page.push_str(
        "`hook` always exits 0: the filters answer on stdout and a refusal is a filter result, \
         not a failed run. See [Agent hooks](../guides/agent-hooks.md).\n\n",
    );
    for constant in ["PARSE_USAGE", "SPANS_USAGE", "EXTRACT_DUMP_USAGE"] {
        let name = constant
            .trim_end_matches("_USAGE")
            .to_lowercase()
            .replace('_', "-");
        page.push_str(&format!("### {name}\n\n"));
        page.push_str(&usage_block(constant));
    }

    page.push_str("## Output formats\n\n");
    page.push_str(
        "`refs`, `read`, `impact`, and `tests` also take `--json` (machine-readable) or \
         `--compact` (one line per hit, for piping). A zero-hit query is reported as a zero hit, \
         not as an error — the tool never guesses a different symbol on your behalf, and an \
         ambiguous name prints every candidate instead of picking one. See \
         [JSON output](json-output.md) for the full contract.\n\n",
    );
    page.push_str(
        "A text or `--compact` answer to `refs`, `read`, `impact` and `tests` opens with the \
         **full** count before any row cap applies: `N inbound (showing M)` on `refs` and \
         `read`, `N files (showing M)` on `impact` and `tests`. A table that was truncated can \
         therefore never be mistaken for the whole answer. On `refs`/`read`, N counts the five \
         reference kinds and the four TypeScript kinds (`call`, `jsx-use`, `dispatch`, `import`) \
         and excludes the `bus-hop` rows, which are possible routes rather than references; on \
         `impact`, N is the number of files the walk reached, whether or not the row cap printed \
         them; on `tests`, no row cap applies and the two numbers agree. A resolved answer that \
         reached nothing states `0 inbound (showing 0)` or `0 files (showing 0)`. The line \
         belongs to those four verbs and those two formats: `--json` carries the same numbers as \
         its own table fields instead, `find` has neither the line nor a JSON shape, and an \
         ambiguous, not-found or fallback answer states no count at all.\n\n",
    );
    page.push_str(
        "A text or `--compact` answer to `refs`, `read`, `tests` and `impact`, and a text answer \
         to `find` — which has no format flag — ends with a `next:` line naming the cheapest \
         verb to run next:\n\n",
    );
    page.push_str("```text\n");
    page.push_str(
        "next: devscout tests Widgetworks.Catalog.InMemoryCatalogStore | devscout impact \
         InMemoryCatalogStore.cs\n",
    );
    page.push_str("```\n\n");
    page.push_str(
        "The line is written to be pasted, so an argument a shell would read as its own — a \
         space, `#`, `$`, a quote, a glob — is single-quoted. Every suggestion was resolved \
         against the verb it names before it was printed, so each command on the line runs as it \
         stands. An answer with nothing left to suggest omits the line entirely. Under `--json` \
         the same suggestions arrive as a `next` array of argv arrays, without the binary name; \
         see [JSON output](json-output.md#next).\n\n",
    );

    page.push_str("## Exit codes\n\n");
    page.push_str(
        "Every status a verb returns is one of four named constants; \
         [Exit codes](exit-codes.md) lists them, what produces each, and the statuses of the \
         plumbing verbs that differ from the query verbs' convention. On `refs`, `read`, \
         `impact` and `tests` the exit code follows the answer's outcome, as \
         [JSON output](json-output.md#outcome) describes.\n\n",
    );

    page.push_str("## read\n\n");
    page.push_str(
        "`devscout read <symbol>` returns the indexed declaration's start and end lines, the \
         verbatim source in that span, and its inbound references. `<symbol>` is a type name or \
         a member seed (see [Seeds](../concepts.md#seeds) — a bare name, `Type.Member`, or \
         `Namespace.Type.Member`); a member seed answers with its own declaration line and \
         inbound references, carrying no span (nothing records an end line for a member on its \
         own). Use `--compact` for a line-oriented summary or `--json` for structured output. \
         References that originate inside the target declaration itself are excluded from \
         inbound rows and counts, so recursive and other self-references do not look like \
         external callers.\n",
    );
    page
}

fn usage_block(constant: &str) -> String {
    let value = usage_value(constant).expect("a rendered usage constant is declared above");
    format!("```text\n{value}\n```\n\n")
}

/// Prose wrapped across source lines reads as one line in the page.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A table cell: one line, with a pipe escaped so it cannot end the cell.
fn cell(text: &str) -> String {
    one_line(text).replace('|', "\\|")
}

/// A row's default, composed from the constants the code builds the path from.
fn default_of(value: &DefaultValue) -> String {
    match value {
        DefaultValue::Text(text) => one_line(text),
        DefaultValue::HomeStore(name) => {
            format!("`$HOME/{}/{}/{name}`", layout::AGENT_DIR, layout::SCOUT_DIR)
        }
    }
}

/// Render `docs/reference/environment.md`.
pub fn environment_page() -> String {
    let mut page = String::new();
    page.push_str("# Environment variables\n\n");
    page.push_str(NOTICE);
    page.push_str("\n\n");
    page.push_str("| Variable | Effect | Default |\n| --- | --- | --- |\n");
    for row in ENVIRONMENT {
        page.push_str(&format!(
            "| `{}` | {} | {} |\n",
            row.name,
            one_line(row.effect),
            default_of(&row.default_value)
        ));
    }
    page.push_str("\n## Retired variables\n\n");
    page.push_str(
        "These names were read by earlier releases and are not read any more. Setting one has no \
         effect.\n\n",
    );
    page.push_str("| Variable | Status |\n| --- | --- |\n");
    for row in RETIRED {
        page.push_str(&format!("| `{}` | {} |\n", row.name, one_line(row.effect)));
    }
    page.push_str("\n## The content store follows `HOME`\n\n");
    page.push_str(
        "The content store follows `HOME` by default. Set `SCOUT_CONTENT_DB` to keep it at a \
         fixed path when `HOME` varies between invocations.\n\n",
    );
    page.push_str("Where each store lives: [Storage](storage.md).\n");
    page
}

/// Render `docs/reference/exit-codes.md`.
pub fn exit_codes_page() -> String {
    let mut page = String::new();
    page.push_str("# Exit codes\n\n");
    page.push_str(NOTICE);
    page.push_str("\n\n");
    page.push_str("| Code | Constant | Meaning | Produced by |\n| --- | --- | --- | --- |\n");
    for row in EXIT_CODES {
        page.push_str(&format!(
            "| {} | `{}` | {} | {} |\n",
            row.value,
            row.constant,
            cell(row.meaning),
            cell(row.produced_by)
        ));
    }
    page.push_str(
        "\n`hook` is the one verb that always exits 0, and the plumbing verbs' usage paths exit 1 \
         where a query verb's would exit 2.\n\n",
    );
    page.push_str(
        "A status devscout does not choose — a panic, a signal, a failure before `main` — is \
         reported by the runtime rather than by a verb, and is not one of these four.\n\n",
    );
    page.push_str("## Where the values live\n\n");
    page.push_str(&format!(
        "Each status is a named constant in the library's `exit` module, and a verb names the \
         constant rather than passing a bare number: `cargo test --test docs_sync` fails when a \
         numeric status appears in the source, or when a constant's value changes without this \
         page being regenerated with `{REGENERATE}`.\n",
    ));
    page
}

/// Render `docs/reference/storage.md`.
pub fn storage_page() -> String {
    let generation = graph::fragment_generation();
    let compiler_facts = graph::compiler_facts_file_name();
    let mut page = String::new();
    page.push_str("# Storage\n\n");
    page.push_str(NOTICE);
    page.push_str("\n\n## Where it stores things\n\n");
    page.push_str(
        "Inside a git repository, artifacts live under the git common directory, so they never \
         show up as untracked files. The index belongs to one worktree at a time — the worktree \
         that last ran `map` there, named by its Git administrative directory:\n\n",
    );
    page.push_str("```text\n");
    let mut listing: Vec<(String, String)> = Vec::new();
    let common = format!("<git-common-dir>/{}/", layout::SCOUT_DIR);
    let graph_dir = format!("{common}{}/", layout::GRAPH_DIR);
    for (path, purpose) in [
        (
            format!("{common}{}", layout::MANIFEST_FILE),
            "file -> purpose + symbol index",
        ),
        (
            format!("{common}{}", layout::INDEX_STATE_FILE),
            "the freshness record: owner, generation, indexed HEAD, content digests, and the\n\
             identities of the two artifacts",
        ),
        (
            format!("{common}{}", layout::INDEX_DIGESTS_FILE),
            "per-file content digest for every indexed file, for this generation",
        ),
        (
            format!("{common}{}", layout::MAP_LOCK_FILE),
            "the lock a `map` holds while it publishes",
        ),
        (
            format!("{graph_dir}{}", layout::GRAPH_FILE),
            "definitions, edges, and project units",
        ),
        (
            format!("{graph_dir}{}", layout::FRAGMENT_CACHE_FILE),
            "per-file extraction cache (incremental map), stamped with its owning worktree",
        ),
        (
            format!("{graph_dir}{}", layout::FRAGMENT_INDEX_FILE),
            "the fragment cache's own index of what it holds",
        ),
        (
            format!("{graph_dir}{}", layout::PROJECT_UNITS_FILE),
            "csproj staleness sidecar (present only with a project model)",
        ),
        (
            format!("{graph_dir}{}", layout::RUST_CRATES_FILE),
            "Cargo crate-model staleness sidecar (present only with a Cargo package in scope)",
        ),
        (
            format!("{graph_dir}{}", layout::TRANSPORT_INPUTS_FILE),
            "transport-input staleness sidecar (present only with a transport site)",
        ),
        (
            format!("{graph_dir}{}", layout::IMPORTED_EDGES_FILE),
            "the imported cross-repo edge set `import-edges` writes",
        ),
        (
            format!("{graph_dir}{compiler_facts}"),
            "optional versioned compiler-fact artifact; absent unless `compiler-facts \
             run|import` has admitted one",
        ),
        (
            format!("{common}{}/{}", layout::LOG_DIR, layout::QUERIES_LOG_FILE),
            "query-verb telemetry, one JSON line per answered invocation (opt-in; \
             SCOUT_TELEMETRY=1)",
        ),
    ] {
        listing.push((path, one_line(purpose)));
    }
    let width = listing
        .iter()
        .map(|(path, _)| path.len())
        .max()
        .unwrap_or(0)
        + 2;
    for (path, purpose) in listing {
        page.push_str(&format!("{path:<width$}{purpose}\n"));
    }
    page.push_str("```\n\n");
    page.push_str(&format!(
        "Every name above is a literal, and every one of them comes from a named constant in the \
         source, so the fragment generation is the `{}` this build writes rather than a \
         placeholder. The [upgrading guide](../guides/upgrading.md) covers what to do when a \
         release changes the layout.\n\n",
        layout::FRAGMENT_CACHE_FILE
    ));

    page.push_str("## Schema versions\n\n");
    page.push_str("| Artifact | Version |\n| --- | --- |\n");
    for (name, version) in [
        (
            layout::GRAPH_FILE.to_string(),
            graph::GRAPH_SCHEMA_VERSION.to_string(),
        ),
        (
            layout::INDEX_STATE_FILE.to_string(),
            devscout_rs::freshness::record::RECORD_VERSION.to_string(),
        ),
        (
            layout::INDEX_DIGESTS_FILE.to_string(),
            devscout_rs::freshness::record::DIGESTS_VERSION.to_string(),
        ),
        (
            layout::IMPORTED_EDGES_FILE.to_string(),
            graph::IMPORTED_EDGES_ARTIFACT_SCHEMA_VERSION.to_string(),
        ),
        (
            compiler_facts.clone(),
            format!(
                "artifact {}, contract {}",
                graph::COMPILER_FACTS_ARTIFACT_SCHEMA_VERSION,
                graph::COMPILER_FACTS_CONTRACT_VERSION
            ),
        ),
        (
            layout::FRAGMENT_CACHE_FILE.to_string(),
            format!("generation {generation} in the name"),
        ),
        (
            layout::QUERIES_LOG_FILE.to_string(),
            telemetry::TELEMETRY_SCHEMA_VERSION.to_string(),
        ),
    ] {
        page.push_str(&format!("| `{name}` | {version} |\n"));
    }
    page.push_str(&format!(
        "\nThe fragment pair and the compiler-facts artifact carry their version in the name; \
         the rest carry a `version` field. `{}` carries none: the manifest is an ordered JSON \
         object whose own shape is its schema.\n\n",
        layout::MANIFEST_FILE
    ));

    page.push_str("## One owner per index\n\n");
    page.push_str(
        "The paths above are shared by every linked worktree of a repository, but the index is \
         not. A `map` records the worktree that produced it, and a query asked from any other \
         worktree reports `unknown` with reason `other-worktree` rather than an answer labelled \
         `fresh`. The reading worktree runs `map` once to take the index over. A `map` in one \
         worktree also reuses no extracted fragment another worktree wrote: the fragment cache \
         carries its owner too.\n\n",
    );
    page.push_str(
        "The freshness record is written by atomic rename and before that is preceded by an \
         in-progress marker, so a query racing a `map` — or reading after a crash — sees the \
         previous generation intact or a run in flight, never a half-written pair. A `map` holds \
         the lock above for the whole run, so two maps of one repository serialize instead of \
         interleaving. The record is local to the repository; nothing about it is transmitted \
         anywhere.\n\n",
    );
    page.push_str(&format!(
        "Outside a git repository the same tree is written to `<root>/{}/` instead. `devscout \
         init` also adds that directory to the repository's local exclude file so the legacy \
         location cannot be committed by accident.\n\n",
        layout::FALLBACK_DIR
    ));
    page.push_str(
        "The compiler-fact artifact is described in [Compiler facts](../guides/compiler-facts.md), \
         and the imported cross-repo edge set that `import-edges` writes beside the graph in \
         [Cross-repo impact](../guides/cross-repo-impact.md).\n\n",
    );

    page.push_str("## Stores outside the repository\n\n");
    page.push_str("Three stores live outside the repo:\n\n");
    page.push_str(&format!(
        "- **Registry** — `$HOME/{}/{}/{}`, the list of roots `devscout` knows about.\n",
        layout::AGENT_DIR,
        layout::SCOUT_DIR,
        layout::REGISTRY_FILE
    ));
    page.push_str(&format!(
        "- **Read-freshness cache** — `<root>/{}/{}` plus a content-addressed `{}`, both SQLite. \
         These are only written by the agent hooks (`devscout hook read|bash`); plain CLI use \
         does not touch them.\n",
        layout::FALLBACK_DIR,
        layout::CACHE_DB_FILE,
        layout::CONTENT_DB_FILE
    ));
    page.push_str(&format!(
        "- **Hook debug output** — `<root>/{}/{}`, appended to only while `SCOUT_DEBUG=1` or a \
         `<root>/{}/{}` entry turns it on.\n\n",
        layout::FALLBACK_DIR,
        layout::DEBUG_LOG_FILE,
        layout::FALLBACK_DIR,
        layout::DEBUG_MARKER
    ));
    page.push_str(
        "[Environment variables](environment.md) move the registry and the content database.\n\n",
    );

    page.push_str("## Ordering\n\n");
    page.push_str(
        "Artifact ordering is fixed and deterministic by design; do not rely on it staying \
         byte-identical across minor versions. Ordering is load-bearing but not a stability \
         promise.\n\n",
    );

    page.push_str("## What is a contract and what is not\n\n");
    page.push_str(
        "None of the files above is a contract. They are this tool's own storage: their shape, \
         their key order and even their names change between releases, and a consumer that reads \
         them is reading an implementation detail.\n\n",
    );
    page.push_str(
        "[`devscout export`](fact-export.md) is the contract. It reads these artifacts and writes \
         one versioned, schema-described stream, so a storage change stays an internal change \
         rather than becoming a release every consumer has to follow. A fact that is not in that \
         stream is not published, whatever the private files happen to contain.\n",
    );
    page
}

/// Every page, as (path relative to `docs/reference`, content).
pub fn pages(help: &str) -> Vec<(&'static str, String)> {
    vec![
        ("cli.md", cli_page(help)),
        ("environment.md", environment_page()),
        ("exit-codes.md", exit_codes_page()),
        ("storage.md", storage_page()),
    ]
}

/// The artifact names `storage.md` lists, for the single-definition check.
pub fn listed_artifact_names() -> Vec<String> {
    vec![
        layout::MANIFEST_FILE.to_string(),
        layout::REGISTRY_FILE.to_string(),
        layout::CACHE_DB_FILE.to_string(),
        layout::CONTENT_DB_FILE.to_string(),
        layout::INDEX_STATE_FILE.to_string(),
        layout::INDEX_DIGESTS_FILE.to_string(),
        layout::MAP_LOCK_FILE.to_string(),
        layout::GRAPH_FILE.to_string(),
        layout::FRAGMENT_CACHE_FILE.to_string(),
        layout::FRAGMENT_INDEX_FILE.to_string(),
        layout::PROJECT_UNITS_FILE.to_string(),
        layout::RUST_CRATES_FILE.to_string(),
        layout::TRANSPORT_INPUTS_FILE.to_string(),
        layout::IMPORTED_EDGES_FILE.to_string(),
        layout::QUERIES_LOG_FILE.to_string(),
        layout::DEBUG_LOG_FILE.to_string(),
    ]
}

/// The section each usage constant is rendered under: the placement a check
/// reads back out of the page.
pub const USAGE_SECTIONS: &[(&str, &str)] = &[
    ("Commands", "COMMANDS_USAGE"),
    ("clear", "CLEAR_OLDER_THAN_USAGE"),
    ("clear", "CLEAR_SESSION_USAGE"),
    ("find", "FIND_USAGE"),
    ("refs, read, impact and tests", "REFS_USAGE"),
    ("refs, read, impact and tests", "READ_USAGE"),
    ("refs, read, impact and tests", "IMPACT_USAGE"),
    ("refs, read, impact and tests", "TESTS_USAGE"),
    ("import-edges", "IMPORT_EDGES_USAGE"),
    ("compiler-facts", "COMPILER_FACTS_USAGE"),
    ("compiler-facts", "COMPILER_FACTS_RUN_USAGE"),
    ("compiler-facts", "COMPILER_FACTS_IMPORT_USAGE"),
    ("compiler-facts", "COMPILER_FACTS_STATUS_USAGE"),
    ("export", "EXPORT_USAGE"),
    ("audit", "AUDIT_USAGE"),
    ("hook", "HOOK_USAGE"),
    ("parse", "PARSE_USAGE"),
    ("spans", "SPANS_USAGE"),
    ("extract-dump", "EXTRACT_DUMP_USAGE"),
];

/// Fence markers inside a body a page embeds verbatim: one would close the
/// block early and leave the rest of the page outside its own fence.
pub fn fence_markers(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~"))
        .map(|line| line.trim().to_string())
        .collect()
}

/// Every environment row missing a name, an effect or a default: the page has
/// to state all three for each variable it documents.
pub fn incomplete_environment_rows() -> Vec<String> {
    let mut problems = Vec::new();
    for row in ENVIRONMENT {
        if row.name.trim().is_empty() || row.effect.trim().is_empty() {
            problems.push(format!("{} has no effect", row.name));
        }
        let default_value = default_of(&row.default_value);
        if default_value.trim().is_empty() || default_value.trim() == "``" {
            problems.push(format!("{} has no default", row.name));
        }
    }
    problems
}
