# Fact export

`devscout export` writes the whole fact set the index records for the current
repository as one versioned NDJSON stream, so another tool can read one
repository in one pass instead of asking one symbol at a time.

```console
$ devscout export | head -1
{"format":"devscout-facts","schemaVersion":1,"provenance":{...},"coverage":{...},"freshness":{"state":"fresh"}}
$ devscout export --out facts.ndjson
```

The stream is the contract. The private artifacts `map` writes — `graph.json`,
the fragment cache, the freshness record, the digest table — are storage: their
shapes are free to change between releases, and none of them is a promise to a
consumer. See [Storage](storage.md).

## Line grammar

One repository per run, UTF-8, one compact JSON object per line, each terminated
by `\n` and never by `\r`. The first line is the header, the last line is the
trailer, and every line between them is one fact.

Every fact line carries `family` and `evidence` first. `family` names which kind
of fact the line is; `evidence` is one word from the vocabulary below saying how
strongly the index knows it. The remaining keys depend on the family, and a key
the family does not record is omitted rather than written empty.

Keys are camelCase, and the key order within a line is fixed by the release that
wrote it: a later release may append keys, never reorder them.

### Families

| Family | One line per | Keys after `family`, `evidence` |
| --- | --- | --- |
| `definition` | declaration the index records | `id`, `name`, `namespace`, `kind`, `file`, `line`, `endLine` (definitions that carry one), `methods`, `testMethods`, `alsoIn[{file,line}]` |
| `edge` | reference, or a fact derived from recorded references | `file`, `line`, `kind`, then the fields that kind records — see below |
| `publish-site` | message-bus publish or dispatch call in C# | `file`, `line`, `verb`, `message`, `namespace`, `outerTypes`, `enclosingMethod`, `argCount`, `enclosingCall{verb,argPosition,argCount}` |
| `handler-registration` | handler installation call in C# | `file`, `line`, `handler`, `namespace` |
| `di-registration` | two-type-argument service registration in C# | `file`, `line`, `service`, `implementation`, `namespace` |
| `unresolved-import` | module import a declared TypeScript module mode could not resolve | `file`, `line`, `target`, `reason` |
| `test-file` | file the test classification vouches for | `file`, `via` |
| `transport-address` | MassTransit transport configuration fact in C# | `file`, `line`, `kind`, `member`, then the keys that kind states (see [Transport address](#transport-address)) |

`test-file` carries no line: the classification is a fact about the file, not
about a position in it.

### Edge keys by kind

`kind` is the edge's own kind, the same one the graph records. The keys after it
are the recorded fields, renamed to camelCase, in this order:

| `kind` | Keys after `file`, `line`, `kind` |
| --- | --- |
| `inherits`, `uses-type` | `to`, `toFile`, `heuristic` |
| `uses-member` | `to`, `toFile`, `tier` (`ext`/`guess`), `heuristic`, `source` (`semantic`/`semantic-discovered`), `member`, `overloadSignature` |
| `imports` | `target` (a C# `using`: a namespace, resolving to no file) |
| `ambiguous` | `origin`, `raw`, `candidates[{id,file}]`, `candidateCount` |
| `ctor-di` | `iface`, `resolution` (`plain`, `closed`, `open-generic`, `ambiguous`, `infra`, `unresolved`), `args`, `to`, `candidates[{id,file}]` |
| `import` | `target`, `toFile`, `via`, `boundDefs` |
| `call`, `jsx-use`, `dispatch` | `to`, `toFile` |
| `implements`, `overrides` | `to`, `toFile`, `member` |
| `bus-hop` | `message`, `to`, `toFile`, `hopEvidence` |

A `bus-hop` line's own `hopEvidence` is the recorded evidence word for the consumer
shape the hop was established through. It is a different fact from the line's
leading `evidence`, which classifies the hop itself as derived.

Edges loaded by `devscout import-edges` are not exported: one repository per
export, and a cross-repository edge belongs to whoever joined the two.

## Evidence vocabulary

Closed, and documented by the schema's own `enum`:

| Word | Means |
| --- | --- |
| `exact` | The resolver bound this reference to one target under a rule it can justify |
| `semantic` | A same-context compiler fact confirmed the extractor's own reference here |
| `semantic-discovered` | Only an admitted compiler fact named this occurrence |
| `heuristic-ext` | The extension-method tier guessed it |
| `heuristic-guess` | The scored tier guessed it by name |
| `heuristic` | A guess no tier attributed |
| `ambiguous` | The resolver refused to bind it: candidates were, or might have been, more than one |
| `unresolved` | The index records the fact but no rule resolved it |
| `declared` | Read off the source as written — a `using`, a publish call, a registration, a definition, a test attribute or a test project |
| `derived` | Derived from two recorded facts rather than read off either |
| `convention` | This project's own file-naming convention, with nothing in the source saying so |

The word is derived only from fields the index records, never invented: a
`guess`-tier edge is never exported as `exact`, and a compiler-sourced edge names
which of the two compiler populations it belongs to. An unrecognized
`ctor-di` resolution word reads `unresolved`, because a consumer must never be
told an injection bound when it did not.

## Coverage

The header's `coverage` object states, per family and language (`csharp`,
`typescript`, `rust`), whether this release extracts that family: `extracted` or
`not-extracted`. It is a table of what the release can see, not of what the
repository happens to contain — an extracted family with no facts at all reads
`extracted` and simply contributes no lines, which is how a consumer tells "this
repository has none" from "this release cannot see them".

| Family | C# | TypeScript/JavaScript | Rust |
| --- | --- | --- | --- |
| `definition`, `edge`, `test-file` | extracted | extracted | extracted |
| `publish-site`, `handler-registration`, `di-registration`, `transport-address` | extracted | not-extracted | not-extracted |
| `unresolved-import` | not-extracted | extracted | not-extracted |
| `http-route`, `http-client` | not-extracted | not-extracted | not-extracted |

The last row's two names are reserved. This release extracts neither of them, so
they appear in the coverage table with no fact lines anywhere in the stream. A
later release adds them as additive families under `schemaVersion: 1`.

## Provenance

The header's `provenance` keeps three objects apart, because a consumer compares
them against different things.

`source` — the inputs the facts were read from:

| Key | Meaning |
| --- | --- |
| `indexedHead` | The commit the index was built at, or `null` when the map saw none |
| `scope` | The directories the map covered |
| `dirtyAtMap` | The relevant paths already changed when the map read them, each with `path` and the recorded `digest` (`null` for a path whose bytes the index kept no digest of) |
| `fileCount` | How many manifest entries the index holds |
| `fileDigests` | A SHA256 over the digest table as an array of `[path, "sha256:<hex>"]` pairs sorted by path |

`analysis` — the inputs the analysis depended on: `devscoutVersion`,
`graphSchemaVersion`, `fragmentGeneration`, `indexStateVersion`, `configInputs`
(each configuration input's recorded digest), `compilerFacts` (the digest of the
admitted compiler-fact artifact as it was read, or `none`), and `semanticLane`
(the run's lane state, or `none`).

`index` — the publishing run's identity: `generation` and `indexedAt`, both
copied from the record. They are the map's own values; the export never reads a
clock of its own.

Every digest in the stream is spelled `sha256:` followed by 64 lowercase hex
characters, whether the index recorded bare hex or the export computed it.

Nothing machine-local is written: no absolute path, no root path, no user or
host name, and no export time. The record's own `owner`, an absolute path in the
publishing worktree's Git directory, is never part of the stream.

## Payload digest

The trailer carries `payloadDigest`: the SHA256 over the exact bytes of the
fact lines, each with its terminating `\n`, in stream order. The header and the
trailer are excluded, so the digest changes when a fact changes and does not
change when only the freshness observation or the publishing generation does.

```json
{"trailer":"devscout-facts","counts":{"definition":19,...},"facts":96,"payloadDigest":"sha256:..."}
```

`counts` carries one key per family, in the family order below, always all eight
of them, and `facts` is the total number of fact lines.

## Canonical order

Facts follow one total order: family rank, then `file` (byte order), then `line`
(numeric, `0` where a family records none), then the whole serialized line (byte
order). The rank is `definition`, `edge`, `publish-site`, `handler-registration`,
`di-registration`, `unresolved-import`, `test-file`, `transport-address`.

Two exports over an unchanged index are therefore byte-identical, and two
identical facts keep both their lines, adjacent, rather than being collapsed.

## Positions, multiplicity and identifiers

Lines are 1-based, as the index records them; a definition also carries
`endLine` when one was recorded. There are no columns.

The export keeps multiplicity: two calls to one target on one line export as two
lines. It carries no durable fact identifier — no hash of a line, no ordinal, no
ID a consumer could store and compare across exports. Positions and digests are
what a consumer has to work with, which is why the payload digest is over the
lines themselves rather than over a table of them.

## Freshness and refusals

The header's `freshness` is the query vocabulary observed at export time:
`fresh`, `stale` (with `indexedHead`, `currentHead` and `changedFiles`), or
`unknown` with a reason (`no-index-state`, `git-unavailable`, `other-worktree`,
`validation-budget`, `index-changed`). A stale index, or one published by another
worktree, still exports and is labelled — the consumer decides what to do with
it.

An export refuses when it cannot state what it read, or when the index moves
under it. One stderr line, exit status 1, no stream on stdout, no `--out` file:

```text
devscout export: refused (<reason>): <detail>; run `devscout map`, then export again
```

| Reason | Cause |
| --- | --- |
| `no-index` | No repository, or a repository with neither a record nor a graph |
| `no-index-state` | A graph with no published record: an index from before provenance was recorded, or a tree outside Git |
| `legacy-index` | A record or digest table this build cannot certify |
| `artifact-missing` | A private artifact the export reads is absent |
| `artifact-unreadable` | A private artifact is present but does not parse |
| `unknown-generation` | A private artifact is of a generation this build does not know |
| `index-changed` | A `map` is running, a `map` ran while the export read, or an artifact is not the one the record certifies |

The export reads one published generation or refuses: it brackets its reads with
the artifact identities the record certifies and compares them again at the end.
`legacy-index`, `no-index` and `no-index-state` are the compatibility story for
older indexes — the recovery is always one `devscout map`.

## Versioning

`schemaVersion` changes only when a key or a family is renamed, removed, or
changes meaning. New families and new keys are additive under the same version:
the trailer's `counts` gains the new family, the coverage table gains its row,
and the schema adds its definition.

Consumers ignore unknown families and unknown keys, and never infer meaning from
a key's position. The committed schema is closed for what one release emits
(`additionalProperties: false`), so a consumer that validates strictly can
detect a stream from a newer release rather than silently accepting it.

The schema lives at `docs/reference/fact-export.schema.json` (JSON Schema
2020-12). It uses a closed keyword subset —
`$schema`, `$id`, `$defs`, `$ref` (local), `title`, `description`, `type`,
`const`, `enum`, `required`, `properties`, `additionalProperties`, `items`,
`oneOf`, `pattern`, `minimum` — so the dependency-free checker in
`tools/fact-export/check.mjs` can validate a stream in any language runtime, and
so a reader knows a validator outside this repository cannot quietly skip a
constraint it does not implement. A schema reaching for another keyword is
refused rather than partly applied.

## Transport address

A `transport-address` line records one MassTransit transport configuration fact
that a C# file states. The leading `evidence` word is always `declared`. The
evidence class of each address and scope value is in that value's own `class`
key, as `hopEvidence` is for a `bus-hop` edge.

devscout recognizes a fact through a closed catalogue of package types and
members, written by fully qualified name. A call is a fact only when its
receiver is one of these:

- a lambda parameter that a catalogue signature types, through nested lambdas
- a field, property, parameter or explicitly typed local whose written type
  qualifies to a catalogue type
- a static catalogue type, or the return value of another catalogue call
- for a chain start (`AddMassTransit`, `AddMassTransitTestHarness`,
  `AddOptions<T>`, `Configure<T>`), any receiver, also a dotted or untyped one

A written name qualifies through the enclosing namespaces, the file's `using`
directives, the project's `global using` directives, the SDK's implicit usings
when `ImplicitUsings` is `enable` or `true`, and the project's `<Using Include>`
items. The name must qualify to exactly one candidate, and that candidate must
be the catalogue type. A repository type in scope with the same name makes the
name ambiguous, and the call gives no fact. A chain start gives no fact when a
repository extension method of the same name is in a namespace that the file
imports, or when its declared receiver is a repository type.

The keys follow `family` and `evidence` in this order. A kind omits every key it
does not state.

| Key | On kinds | Value |
| --- | --- | --- |
| `file`, `line` | all | The repository path and 1-based line of the site. |
| `kind` | all | One of the kinds below. |
| `member` | all | The fully qualified declaring type and member, or the attribute type. A generic declaring type carries its arity after a backtick. |
| `busFile`, `busLine` | facts inside a bus fact | The position of the bus fact that holds this fact. |
| `transport` | `bus` | `rabbitmq`, `amazon-sqs`, `in-memory`, `test-harness` or `not-stated`. |
| `registration` | `registration` | `consumer`, `saga`, `state-machine`, `consumer-namespace`, `saga-namespace`, `state-machine-namespace` or `assembly-scan`. |
| `types` | kinds that state types | An array of `{role, type, resolved, resolvedFile, line}`. `type` is the name as written. `resolved` and `resolvedFile` name the one repository definition that the name qualifies to, and are absent otherwise. `line` is present only on a type configured on a receive endpoint. |
| `formatter` | `configure-endpoints` | `kebab-case`, `snake-case`, `default` or `unknown`. |
| `formatterSource` | `configure-endpoints` | `argument`, `bus` or `default`. |
| `formatterArguments` | `configure-endpoints` | The formatter's stated constructor arguments, as value objects in order. |
| `filtered` | `configure-endpoints` | `true` when the call states a registration filter. |
| `address` | `receive-endpoint`, `endpoint-name`, `send-address`, `request-address`, `endpoint-convention`, `entity-name` | A value object. A request client with no address has no `address` key. |
| `key` | `options-binding` | The bound configuration key, as a value object. A chain that binds no key has no `key` key. |
| `properties` | `options-binding` | An array of `{name, role, value}` for the options properties that hold an address or a scope part. `role` is `host`, `virtual-host` or `region`. |
| `scope` | `bus`, `host` and each kind with `address` | `{host, virtualHost, region, account}`, each part a scope value object. |

The kinds are `bus`, `host`, `receive-endpoint`, `configure-endpoints`,
`registration`, `endpoint-name`, `send-address`, `request-address`,
`endpoint-convention`, `entity-name`, `options-binding` and `outbox`. The type
roles are `message`, `consumer`, `saga`, `state-machine`, `state`, `handler`,
`definition`, `anchor`, `options`, `db-context` and `bus`.

A bus fact is the root call: `AddMassTransit`, `AddMassTransitTestHarness` or
`Bus.Factory.CreateUsing*`. Its `transport` comes from the nested `Using*` call
or from the `CreateUsing*` member. `AddMassTransitTestHarness` gives
`test-harness`, and the file's `test-file` line, when it has one, says that the
file is a test file. A configure-endpoints fact states the formatter in force
and the bus's registrations are their own facts. No queue name is ever derived
from a formatter.

### Value objects

Each value object has exactly one `class`:

| Class | Keys | Meaning |
| --- | --- | --- |
| `literal` | `value`, `scheme` | A string literal. `scheme` is `queue`, `exchange`, `topic`, `rabbitmq`, `rabbitmqs`, `amazonsqs` or `arn` for the URI and AWS resource name forms, and `value` is the entity name that form states. |
| `constant` | `value`, `scheme`, `fields[{field,file,line}]` | Repository `const string` fields, alone or joined with literals by `+`. `fields` lists each field read, constants of constants included, in reading order. |
| `configuration` | `key`, `values[{file,value}]` | A configuration key, and each value that an `appsettings.json` or `appsettings.*.json` file in the binding's project directory states for it, sorted by file. `values` is empty when no file states one. The key object of an options binding never carries values; its properties do. |
| `unknown` | `reason`, `key` | A form the rules do not read. `key` is present when the source states a configuration key. |

A scope value object is a `literal`, `constant` or `unknown` object with no
`scheme`, or `{"class":"not-stated"}`.

Each value must match the grammar of its kind, or it reads `unknown` with the
reason `unread-value`:

| Kind | Grammar |
| --- | --- |
| queue, endpoint and entity names | `[A-Za-z0-9._:/-]+` |
| host | `[A-Za-z0-9.-]+` |
| virtual host | `[A-Za-z0-9._/-]+` |
| region | `[a-z]{2}(-[a-z]+)+-[0-9]+` |
| account | 12 digits |

A URI never keeps its user information, port, query or fragment, in the
export or in any index file. A value joined from string literals and constants
keeps its literals in the fragment cache only as source positions, and the
graph rebuild reads them again, because only the joined value shows which
part is user information. No
configuration value is read except for an options property listed above, and no
credential property is ever listed.

The reason words are a closed list:

| Group | Words |
| --- | --- |
| source forms | `interpolated-string`, `parameter`, `local`, `static-readonly`, `field`, `method-call`, `nameof`, `expression` |
| constants | `package-constant`, `unresolved-constant` |
| URIs and values | `uri-scheme`, `malformed-uri`, `unread-value` |
| configuration | `unreadable-configuration` |
| calls | `overload`, `multiple-hosts` |

A `static readonly` field reads `unknown`, because its value can change at run
time. A value at or after a call's first named argument reads `overload`,
because the catalogue keeps parameter positions, not parameter names. A cycle of constants reads `unresolved-constant`. A settings file that
does not parse, two keys that match one name without regard to ASCII case, and a
value that is no string read `unreadable-configuration` with the key.

### Scope

- A bus fact gets its scope from its own host call. With no host call, every
  part reads `not-stated`. With two or more, each part the transport uses reads
  `unknown` with the reason `multiple-hosts`.
- An address fact gets each part from its own URI or resource name first, and
  then from its bus. With no bus, a part its value does not state reads
  `not-stated`.
- A part the transport does not use reads `not-stated`. RabbitMQ uses `host`
  and `virtualHost`. Amazon SQS uses `region` and `account`.
- For `rabbitmq://host/vhost/name`, two or more path segments make the first one
  the virtual host. For `amazonsqs://region/…`, the host is the region.
- An options binding keeps its properties on its own fact. They never merge into
  a bus scope.

## Known limitations

- `analysis.compilerFacts` digests the artifact present at export time. The
  index does not record which admitted artifact a `map` consumed, so
  re-admitting facts without re-mapping is invisible here — exactly as it is to
  a query. Recording it at map time would change the private record format.
- A TypeScript or JavaScript file the grammar cannot parse contributes no facts.
- The export reads the whole stream into memory before writing it, which is what
  makes a refusal total. The size receipt in
  [benchmarks](../benchmarks/results/) records what that costs on the largest
  corpora this project measures.
- `--out` writes through a temporary file in the target's own directory and then
  renames. A symlink at the target path is replaced, not followed. The target's
  directory is never created, and a failure leaves no file at the path.

## Checking a stream

```console
$ node tools/fact-export/check.mjs fixtures/fact-export/csharp.ndjson
$ node tools/fact-export/check.mjs --selftest
```

The checker streams a file, validates every line against the committed schema,
recomputes the payload digest and compares it with the trailer, and refuses a
`\r`, a missing final newline, a missing trailer and a mismatched digest. It has
no dependencies and holds one line at a time, so a consumer can lift it as a
starting point.
