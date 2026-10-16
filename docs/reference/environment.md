# Environment variables

> **Generated.** Every byte of this page is written by `tests/docs_sync.rs` from
> the definitions the binary itself uses, and the page is never edited by hand: a change that
> belongs here belongs in the source. Regenerate it with `BLESS=1 cargo test --test docs_sync`,
> and resolve a merge conflict in it by regenerating rather than by merging edits.

| Variable | Effect | Default |
| --- | --- | --- |
| `SCOUT_REGISTRY` | Path to the registry JSON: the roots devscout knows about, and the scopes each was registered with. | `$HOME/.claude/scout/repos.json` |
| `SCOUT_CONTENT_DB` | Path to the shared content-dedup SQLite database. Written only by the agent hooks (`devscout hook read|bash`), which use it to answer a repeated read without walking the tree again. | `$HOME/.claude/scout/content.db` |
| `SCOUT_DEBUG` | `1` turns on hook debug output, the same as creating a `.scout/debug` entry. | unset — no debug output |
| `SCOUT_TELEMETRY` | Opt-in query telemetry. Export `1` in the shell that runs `find`/`refs`/`read`/`impact`/`tests` to append one JSON line per answered invocation to the telemetry log; a usage error, or a seed with no resolved repository or graph, logs nothing. Unset (or any other value) writes nothing. The agent hooks never run these verbs, so `devscout init` does not set this variable for them. | unset — nothing is written |
| `SCOUT_COMPILER_ENGINE` | Path to a built compiler-facts engine executable. Read only by `compiler-facts run`. | unset — no engine is located and nothing is downloaded |
| `SCOUT_EDGE_PROVENANCE` | Path to a diagnostic artifact `map` writes while it resolves `uses-member` edges: one record per edge, naming the emission step that produced it. Written beside the artifacts named under [Storage](storage.md); nothing reads it back. | unset — the diagnostic artifact is not written |
| `HOME` | Locates the registry, the content database and the agent settings file. | the value the process was started with |

## Retired variables

These names were read by earlier releases and are not read any more. Setting one has no effect.

| Variable | Status |
| --- | --- |
| `SCOUT_MTIME_REUSE` | No effect. `map` keys fragment reuse on the content of the bytes it indexes, so there is no mtime mode left to switch to, and the variable is not read. |

## The content store follows `HOME`

The content store follows `HOME` by default. Set `SCOUT_CONTENT_DB` to keep it at a fixed path when `HOME` varies between invocations.

Where each store lives: [Storage](storage.md).
