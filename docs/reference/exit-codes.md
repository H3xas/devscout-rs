# Exit codes

> **Generated.** Every byte of this page is written by `tests/docs_sync.rs` from
> the definitions the binary itself uses, and the page is never edited by hand: a change that
> belongs here belongs in the source. Regenerate it with `BLESS=1 cargo test --test docs_sync`,
> and resolve a merge conflict in it by regenerating rather than by merging edits.

| Code | Constant | Meaning | Produced by |
| --- | --- | --- | --- |
| 0 | `EXIT_OK` | The command completed and answered. | `help`, `--help` and `-h`; `noop`; `hook <read\|bash>`; and any `init`, `map`, `stats`, `clear`, `import-edges`, `compiler-facts` or `export` run that finished its work. |
| 1 | `EXIT_FAILURE` | The command ran and could not answer: a refusal, an ambiguous seed, an environment it cannot read. | an ambiguous or unresolved seed; a refusal (`export`); an unreadable environment; `--json` together with `--compact`; a `-C <dir>` that cannot be applied; a Git ignore query that cannot be run or read, which stops `map` before it writes; and the usage paths of the plumbing verbs — `parse`, `spans` and `extract-dump` with no file, `hook` with no filter, an `audit` argument error, and the unknown-command line. |
| 2 | `EXIT_USAGE` | The invocation was malformed, and the verb printed its usage line instead of doing any work. | a query verb's missing argument or unknown flag; an out-of-range `--pick N`; `init` refusing a directory that itself contains git repositories; an ambiguous `clear --session` prefix; a malformed `export` invocation. |
| 3 | `EXIT_NO_RESULT` | A query resolved but reached nothing: the index was read and the answer is empty. Neither success nor failure. | `refs`, `read`, `impact` and `tests`, when the graph was searched and no row matched. |

`hook` is the one verb that always exits 0, and the plumbing verbs' usage paths exit 1 where a query verb's would exit 2.

A status devscout does not choose — a panic, a signal, a failure before `main` — is reported by the runtime rather than by a verb, and is not one of these four.

## Where the values live

Each status is a named constant in the library's `exit` module, and a verb names the constant rather than passing a bare number: `cargo test --test docs_sync` fails when a numeric status appears in the source, or when a constant's value changes without this page being regenerated with `BLESS=1 cargo test --test docs_sync`.
