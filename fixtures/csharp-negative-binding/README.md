# csharp-negative-binding fixture

A package-free C# fixture of twenty member-reference sites whose correct
answer is pinned, and whose correct *abstention* is pinned just as hard. The
resolver's precision work is otherwise measured on recovered references, where
a recall gain can quietly trade away a correct refusal; here every case keeps
its place in the denominator on every run, so that trade shows up as a
partition change instead of as an unnoticed regression.

The subject is devscout's syntax-only graph (`init --no-hooks`, then `map`, no
compiler-facts admission), and the witness is the compiler, so neither is ever
both judge and judged.

## Cases

Twenty cases, four in each category:

| Category | What the case turns on |
|---|---|
| `namespace-shadowing` | the type an unqualified name reaches: an inner namespace member, a namespace member over a `using` import, a `using` alias over a `using` directive, a `global::`-qualified name over a `using` import |
| `receiver-overload` | the receiver's static type: a declared base type over a derived `new` member, a virtual call's declaring type, an `as`-cast's narrowed type, an interface parameter |
| `extension-instance` | extension-versus-instance selection: an instance member winning, an inapplicable instance member losing, the closer namespace layer winning, the more specific receiver winning |
| `generic-binding` | type arguments and constraints: an inferred generic over an `object` overload, a derived member group hiding a base generic, two-arity inference, a constraint-satisfying generic over an object overload |
| `negative` | the correct answer is no precise edge: two calls the compiler reports ambiguous, and two reads of a private member from outside its type |

Every case is one member-reference site keyed by file, one-based line and
member name, one case to a line, so the compiler's own occurrence records and
devscout's `uses-member` edges both key it. Type references and inheritance are
outside this fixture.

## Label vocabulary

`labels.json` holds one entry per case:

- `site` — `file`, `line` and `member`, the one reference the case is about.
- `intent` — `resolvable` or `negative`.
- a resolvable case names the `declaringType` the compiler binds it to,
  `overloadParameters` (the parameter types the oracle records for that
  target, for a member whose same-named overloads compete), and
  `plausibleWrong`, the competing type the case exists to refuse.
- a negative case names the `oracleState` the compiler reports (`ambiguous` or
  `inaccessible`), the compiler `diagnostic` on that line, and
  `acceptedOutcomes`, the devscout outcomes that count as a correct abstention
  (`absent` or `tiered`).
- `oracle` — the producer identity the committed document must agree with.

The partition vocabulary the test reports is `precise-correct`, `precise-wrong`
(a precise edge whose declaring type, or recorded overload signature where the
label pins one, differs from the label — and, inside the envelope, every precise
edge at a negative case), `tiered` (`ext` or `guess` edges only) and `absent`.
Recall counts resolvable cases whose site carries an edge to the labelled
target at the precise or `ext` tier; `guess` does not count, and the
precise-only figure is reported beside it.

## Sealing order

The labels are sealed by commit order on the ticket branch, not by assertion:
the first commit that touches this fixture adds the sources and `labels.json`
and no oracle document, observation pin or gate file; a later one adds
`compiler-facts.json` and the oracle identity in `labels.json`; a later one
adds `observed.json`, `gate.json` and the test. `labels.json` does not change
after the commit that adds the document.

Intent labels are written from the C# rules, before the document exists, so the
compiler is never the sole witness to its own expectations. A label corrected
for an authoring error is corrected in the commit that adds the document, with
its cause in the implementation journal; a label the author would maintain
against the compiler stops the work instead.

## Regenerating the oracle document

`compiler-facts.json` is the `--emit compiler-facts` output for this fixture
under .NET SDK 9.0.305, the same pinned install `fixtures/csharp-compiler-facts`
uses. It is produced through the dedicated pinned root, never through whatever
`dotnet` a machine resolves, because the document records the resolved SDK and
MSBuild versions, the MVID of every reference assembly and the hash of every
import (see `tools/scout-semantic/README.md` for obtaining and validating that
root):

```sh
SNAPSHOT_DOTNET_ROOT="$HOME/.dotnet-roots/9.0.305-osx-arm64"
export DOTNET_ROOT="$SNAPSHOT_DOTNET_ROOT"
export PATH="$SNAPSHOT_DOTNET_ROOT:$PATH"
dotnet restore fixtures/csharp-negative-binding/Fixture.csproj
dotnet build tools/scout-semantic -c Release
dotnet run --project tools/scout-semantic --no-build -c Release -- \
  fixtures/csharp-negative-binding/Fixture.csproj --root fixtures/csharp-negative-binding \
  --emit compiler-facts --compiler-facts out/negative-binding.json \
  --tfm net9.0 -p Configuration=Debug -p Platform=AnyCPU --no-git
diff -u fixtures/csharp-negative-binding/compiler-facts.json out/negative-binding.json
```

CI's `semantic-audit` job restores the fixture the same way and diffs the
regenerated document against the committed one, so a drifted install fails the
job rather than refreshing the expected bytes. `--no-git` keeps the document
free of a source-snapshot identity, and two runs over the pinned inputs are
byte-identical.

## Reading the partition

```sh
cargo test --test negative_binding                  # every check, quiet
cargo test --test negative_binding -- --nocapture   # the whole table, every run
```

`observed.json` is the committed partition, regenerated only by
`BLESS=1 cargo test --test negative_binding`; `gate.json` is hand-edited and
`BLESS` never touches it, so a blessed regression still fails the gate. The
gate lists the known deviations by case id and observed outcome, plus the
recall floor. It fails when a case is precision-wrong and unlisted, when a
listed deviation no longer occurs, when recall is below the floor or above it
(a stale floor), when the run differs from `observed.json`, or when the case
count differs from `labels.json`. A listed deviation that a later resolver
change fixes has to be removed by hand, and the floor raised with it.
