# Troubleshooting

## A query prints a staleness warning

Re-run `devscout map .` after edits; it re-parses only what changed and leaves the graph alone
when nothing moved (`... 0 new, 0 removed ...; graph unchanged`). If the index falls behind
`HEAD`, queries print a staleness warning on stderr rather than silently answering from stale
data. Nothing watches the filesystem for you; see [Freshness](../concepts.md#freshness).

## A query answers zero hits

A zero-hit query is reported as a zero hit, not as an error — the tool never guesses a different
symbol on your behalf. When nothing in the graph carries the seed at all, the note on stderr
advises a text-search fallback (`rg` or `grep`) rather than rephrasing. When the seed resolved and
the answer is simply empty, the outcome is `zero-hit`; [Outcomes](../concepts.md#outcomes) tells
the cases apart.

A name in a language or construct the index does not record cannot be found either:
[Limitations](../limitations.md) lists what is not indexed or not resolved.

## A query lists several candidates

An ambiguous name prints every candidate instead of picking one. A member seed carried by more
than one type lists one row per candidate; pass `--pick N` (one-based) to select the nth row. An
out-of-range `N` is a usage error (exit code 2). A Rust path two items share within a file is
ambiguous too; name the second one by its id, which carries a `-1` suffix
([Seeds](../concepts.md#seeds)).

Pass `--pick all` when the sweep wants every candidate rather than one. Each candidate is answered
in its own section, and no section is merged with another. Each section equals the `--pick N` answer
for its number, so a caller can compare them or parse the `sections` array under `--json`
([JSON output](../reference/json-output.md#the-sections-envelope)).

## Paths under `/tmp` print as `/private/tmp`

macOS resolves `/tmp` to `/private/tmp`, so `init` and `map` print the resolved path.

## The hook install printed a snippet instead of installing

`init` writes the agent settings file only when it exists and has a shape it recognises. With no
settings file, invalid JSON or an unexpected shape, it prints the snippet to merge by hand; a
hook failure never fails `init`. See [Agent hooks](agent-hooks.md).

## The read cache or registry moves between runs

The content store follows `HOME` by default. Set `SCOUT_CONTENT_DB` to keep it at a fixed path
when `HOME` varies between invocations, and `SCOUT_REGISTRY` for the registry; see
[Environment variables](../reference/environment.md).

## The compiler-fact lane did not apply

`map` prints a `lane:` line naming why an admitted artifact was not used: a stale artifact, a
moved import, no complete compilation, nothing confirmed, or an unreadable artifact. The
[compiler facts guide](compiler-facts.md#the-lane-line) lists every reason and its detail token.

## After an upgrade, answers or artifacts look wrong

While the version is `0.x`, minor bumps may change artifact layout; delete the artifact directory
and re-run `devscout map`. See [Upgrading](upgrading.md).
