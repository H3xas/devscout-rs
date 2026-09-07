# import-edges fixture

Two invented mini-repos and a hand-authored cross-repo edge export, used by
the `import-edges`/`impact` integration tests.

- `storefront/` is the repo `devscout` actually maps: a controller
  (`CheckoutController.cs`) and a test that calls it.
- `ledger/` is never mapped by any test here. It exists only so the export's
  foreign file paths point at something readable; its two files are never
  parsed by this index.
- `export.json` is a `flowtrace-edges` schemaVersion 1 export, hand-authored
  to match the shape the real writer produces: an envelope with `provenance`
  carried once, and per-record ends shaped `{repo, ref, file, line}`. It
  carries two joined edges out of `storefront`:
  - a `calls` record whose `to` end names `CheckoutController.cs` (the
    caller sits in `ledger`);
  - a `publishes`/`consumes` pair sharing the join key `"order-placed"` (the
    consumer sits in `ledger`).
- `export-retracted.json` is the same export, same `provenance.id`, with the
  `calls` record removed -- pins that a re-import drops exactly that row and
  leaves the other one byte-identical.
- `export-different-provenance.json` carries a different `provenance.id` and
  a single unrelated record -- pins that a re-import replaces the prior set
  wholesale rather than merging into it.
