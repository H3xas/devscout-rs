# Cross-repo impact

`impact` walks one repository's graph. When another tool has recorded edges that cross into other
repositories — a call from a client in another repository, or a message one repository publishes
and another consumes — `import-edges` loads them so `impact` can name the files on the far side.

## Load an export

```sh
devscout import-edges <file> --repo <id>
```

`import-edges <file> --repo <id>` loads a versioned cross-repo edge export into an auxiliary
artifact beside `graph.json`, never touching the graph schema. `--repo` names this repository in
the export, is required and is never inferred.

The export is a `flowtrace-edges` file at `schemaVersion` 1: a `provenance` object carried once,
and an `edges` list whose records each have a `kind`, a `from` end and a `to` end shaped
`{repo, ref, file, line}`, plus an optional join `key`. The fixture under
[`fixtures/import-edges/`](../../fixtures/import-edges/README.md) holds a small example.

An invalid export is refused with exit 1 naming the offending value, leaving any pre-existing
artifact untouched: a format or schema-version mismatch, a record missing `kind`, `from` or `to`,
or a kind outside the fixed set. A successful import replaces the prior set wholesale; there is no
merge.

## Read the answer

`impact` then names files reached only through an imported edge -- directly, or composed across a
fileless message node -- each row carrying `why: "imported-edge"`, the foreign repo id, and the
export's provenance id. Imported rows are capped and counted apart from the native ones, so an
import can never evict a native row. An imported edge reaches a file and stops there; it never
widens the native walk.

`imported-edge` is the weakest evidence the `why` vocabulary names: a foreign, producer-asserted
fact never outranks anything this crate resolved for itself. See
[JSON output](../reference/json-output.md#why).

## Skip or remove an import

`impact --no-imports` skips it, and a repo with no import configured answers exactly as it did
before. To replace an import, run `import-edges` again with the new export.
