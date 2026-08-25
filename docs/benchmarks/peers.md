# Peers

The other things an agent can install in one command to answer "where is this, who uses it,
what breaks if I change it". Every project below is maintained open source solving a problem it
chose, and several attempt things this index does not attempt at all. This page is coverage of
different problems, not a scoreboard.

**Selection rule, fixed before any tool was tried.** An OSI licence; installs and answers
locally with no hosted account and no API key; a commit inside the last twelve months; and a
CLI or MCP tool an agent would call directly. Every peer is pinned by repository URL plus
version — never by a marketplace name, because at least one of these names refers to several
unrelated projects and the official MCP registry entry is not always the project people mean.

| Peer | Project | Licence | Shape |
| --- | --- | --- | --- |
| `rg` | BurntSushi/ripgrep | MIT / Unlicense | literal and regex text search — the control arm that floors the table |
| `serena` | oraios/serena | MIT | LSP-backed symbol server; `find_symbol`, `find_referencing_symbols`, plus real refactoring verbs |
| `codeindex` | johnhuang316/code-index-mcp | MIT | search index over a local grep backend with per-language extraction strategies |
| `repomap` | Aider-AI/aider (`aider.repomap.RepoMap`) | Apache-2.0 | token-budgeted, PageRank-ranked whole-repo summary from tree-sitter tags |
| `astgrep` | ast-grep/ast-grep | MIT | syntactic AST pattern matcher |
| `vector` | zilliztech/claude-context | MIT | semantic retrieval; fully local via a local embedding server and a self-hosted vector store |
| `devscout` | this tool | MIT OR Apache-2.0 | name-resolved graph: definitions, inbound edges by kind, transitive file reachability |

## Capability matrix

Attempted / not attempted, on each tool's own account of itself. No numbers — those belong in a
dated results document, against a pinned corpus.

| | locate | references | impact | e2e-retrieval | refactor verbs | needs a build |
| --- | --- | --- | --- | --- | --- | --- |
| `rg` | yes | text hits | no | yes | no | no |
| `serena` | yes | resolved | by chaining | yes | yes | workspace load |
| `codeindex` | yes | text hits | not attempted | yes | no | no |
| `repomap` | ranked summary | ranked summary | ranked summary | ranked summary | no | no |
| `astgrep` | not attempted | syntactic | not attempted | not attempted | rewrite only | no |
| `vector` | yes | semantic | not attempted | yes | no | model + store |
| `devscout` | yes | resolved | yes | yes | no | no |

"Not attempted" is quoted from the tool's own documentation and is never counted as a failure:
`astgrep` has no symbol table or cross-file graph, so it cannot distinguish two different
`Foo()`; `codeindex` returns textual and tag hits with no cross-file "all callers" query;
`vector` has no notion of a reference or a dependency edge; `repomap` states outright that it
ranks files rather than resolving anything.

**Considered and excluded.** `universal-ctags` is a flat tags index with no reference graph, so
three of four bands would be `not_attempted`. `scip-dotnet` needs a restorable MSBuild project
and its documented query path is an upload to a hosted instance, failing both the no-build and
the no-hosted-service bars. `mcp-language-server` is a generic LSP bridge whose worked configs
cover other languages; `serena` covers the same shape with C# documented.

## Where peers win

Generated mechanically in every results document — the harness names the leading arm for each
band × metric cell, so this section cannot be quietly trimmed. Ahead of any measurement, these
are the wins to expect on the shape of the tools alone:

- **A skilled `rg` baseline is hard to beat on anything that reduces to one distinctive
  string.** Locate and most reference questions terminate the moment a search returns the right
  identifier, and one `rg` call is very cheap. Against that, an index adds a round trip it does
  not automatically earn back.
- **`astgrep` is fast**, with no index to build and no workspace to load.
- **`codeindex` returns compact payloads**, which matters directly to an agent's context budget.
- **`serena` does things this tool does not do at all** — rename, extract, and other refactoring
  verbs, plus symbol-level editing.
- **`vector` answers a sentence.** It is the only shape here that takes a bug report as prose
  rather than requiring an identifier.
- **`repomap` primes context** rather than answering a question, which is a different and
  legitimate job.

## Gaps we solve

Narrow, and stated no wider than the shape of the tool supports:

- **Transitive reachability has no search string.** "What else must be looked at if this file
  changes" cannot be expressed as a pattern, and it is the question where a prebuilt graph is
  doing work no text search can do. Peers either mark it not attempted or compose it by chaining
  several calls.
- **A reference answer is a resolved edge, not a text hit** — `inherits`, `uses-type`,
  `uses-member` — so an agent can filter by why a file appears, and a reference reached only
  through an inferred receiver type is still an answer.
- **Nothing to build, nothing to host.** No MSBuild restore, no language-server workspace load,
  no container, no embedding model.

## Known gap in our own output shape

`impact` rows aggregate to file granularity and carry no line number, even though the
underlying graph edges do. Under a strict follow-up-reach rule — path, 1-based line, and a
reason on every row — this tool's `impact` answers do not reach, and peers whose rows do carry
a line lead that cell. It is an aggregation step discarding a value it already has, not a
missing capability, and it is disclosed here rather than left for a results table to reveal.
