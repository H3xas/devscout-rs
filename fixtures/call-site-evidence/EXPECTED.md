# Expected call sites for `Witness.cs`

Authored by reading `Witness.cs` alone, before any `devscout` command has been run against this
fixture -- the ordering is load-bearing: it is what makes these witnesses independently authored
rather than derived from tool output. `tests/call_site_evidence.rs` asserts the audited surfaces
against this list, not the other way around.

Every site below is a call to `Ledger.Record()` or its overload `Ledger.Record(int)` (both answer
the same bare-member query, `Record`, since devscout's def id is name-keyed, not signature-keyed)
or, for the recursion case, to `Caller.Recurse(int)`.

| Case | File | Line | Calls at that line | Note |
| --- | --- | --- | --- | --- |
| Repeated calls, different lines | `Witness.cs` | 33 | 1 | `Record()` |
| Repeated calls, different lines | `Witness.cs` | 34 | 1 | `Record()` |
| Two calls, one line | `Witness.cs` | 40 | 2 | `Record()` then `Record()`, same line -- the collision case |
| Overload ambiguity | `Witness.cs` | 47 | 1 | `Record()` |
| Overload ambiguity | `Witness.cs` | 48 | 1 | `Record(7)`, the `int` overload -- devscout resolves the member by name only, so this site answers under the same `Record` query as every other row here; which overload bound is not something this crate claims to know (a documented gap, not an error) |
| Recursion | `Witness.cs` | 64 | 1 | `this.Recurse(depth - 1)`, a call to `Recurse` from inside `Recurse`'s own body |
| Awaited sequence | `Witness.cs` | 71 | 1 | `await RecordAsync()` |
| Awaited sequence | `Witness.cs` | 72 | 1 | `await RecordAsync()` |
| Parallel launch/join | `Witness.cs` | 79 | 1 | `RecordAsync()`, launched, not yet awaited |
| Parallel launch/join | `Witness.cs` | 80 | 1 | `RecordAsync()`, launched, not yet awaited |

Totals expected on `refs Record --json`'s `uses-member` inbound table: **6 rows**, all in the
`Record`/`Record(int)` table above (repeated-calls, two-calls-one-line and overload-ambiguity).
`RecordAsync` is a separate member and answers its own `refs RecordAsync` query with its own 4
rows (the awaited-sequence pair plus the parallel-launch-join pair). `Recurse` answers its own
`refs Recurse` query with its own 1 row (line 64).

**Recorded gap, not a witness of the `occurrenceIndex` change:** an EARLIER draft of the recursion
case called `Recurse(depth - 1)` unqualified (no `this.`). Running the audited surfaces against
that draft, before any other change, showed `refs Recurse --json` answering zero rows -- an
unqualified same-type call has no member-access expression for the extractor to build a reference
from at all, so it is invisible to every native surface, not merely uncounted. This is a genuine,
confirmed capability gap (recorded in the capability matrix as an explicit gap on the "caller
identity"/"invocation source range" columns for the unqualified-self-call shape) and is NOT
addressed here: closing it would mean extending `src/extract.rs`'s reference-extraction
rules, which the occurrence-identity work explicitly keeps out of
scope. The fixture's recursion case uses `this.Recurse(...)` so it exercises what native devscout
already establishes, the same as every other case here; the unqualified-call gap is cited in the
matrix from this note, not re-demonstrated by a second, uncaptured case.

The line-40 pair is the one genuine serialization collision this fixture demonstrates: two
`uses-member` edges with identical `(file, line, to, heuristic)` before `occurrenceIndex` existed.
Every other row above is already distinguished by line, so it round-trips correctly on unmodified
`main` -- the audit's own attribution step (`tests/call_site_evidence.rs`) proves this by running
the fixture against unmodified `main` before the `occurrenceIndex` change is applied.

No control-flow, await-ordering or dispatch-target claim is asserted for any site above beyond
"this line calls this member": devscout's native surface models none of that, and the negative
cases in `tests/call_site_evidence.rs` assert those stay absent, not falsely `false`.
