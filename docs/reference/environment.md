# Environment variables

Hand-written for now; this page will be generated from the variables the binary reads.

| Variable | Effect |
| --- | --- |
| `SCOUT_REGISTRY` | Path to the registry JSON. Default `$HOME/.claude/scout/repos.json`. |
| `SCOUT_CONTENT_DB` | Path to the shared content-dedup SQLite database. Default `$HOME/.claude/scout/content.db`. |
| `SCOUT_MTIME_REUSE` | Accepted and ignored. `map` keys fragment reuse on the content of the bytes it indexes, so there is no mtime mode left to switch to; the variable is read only so an existing script does not break. |
| `SCOUT_DEBUG` | `1` turns on hook debug output. Equivalent to creating a `.scout/debug` file. |
| `SCOUT_TELEMETRY` | Opt-in query telemetry. Export `1` in the shell that runs `find`/`refs`/`read`/`impact`/`tests` to append one JSON line per answered invocation to `scout/log/queries.jsonl`; a usage error or a seed with no resolved repository or graph logs nothing. Unset (or any other value) writes nothing. The agent hooks never run these verbs, so `devscout init` does not set this variable for them. |
| `SCOUT_COMPILER_ENGINE` | Path to a built compiler-facts engine executable. Read only by `compiler-facts run`; no default and nothing is downloaded. Unset (or empty) refuses with one line and touches nothing. |
| `HOME` | Used to locate the registry, content database, and agent settings file. |

## The content store follows `HOME`

The content store follows `HOME` by default. Set `SCOUT_CONTENT_DB` to keep it at a fixed path
when `HOME` varies between invocations.

Where each store lives: [Storage](storage.md).
