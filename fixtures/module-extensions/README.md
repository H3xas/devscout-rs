# module-extensions

A small repository whose files are `.mjs`, `.cjs`, `.mts` and `.cts`, so the
four module suffixes and the JavaScript test-path rule can be checked against
the definitions and edges they produce. The names are invented; nothing here is
meant to run.

## Declarations

| File | Declares |
| --- | --- |
| `src/core/money.mjs` | the function `total` and the class `Wallet` |
| `src/core/index.mjs` | nothing: a barrel that re-exports `./money.mjs` with `export *` |
| `src/util/format.mjs` | the function `formatAmount` |
| `src/legacy/totals.cjs` | the function `grandTotal`, exported with `module.exports` |
| `src/legacy/report.cjs` | the function `report` |
| `src/typed/schema.mts` | the interface `Row` and the function `parseRow` |
| `src/typed/settings.cts` | the function `readSettings` |
| `src/build/tool.mts` | the function `tool`, whose built twin is `src/build/tool.mjs` |
| `src/dual/thing.mjs` | the function `thing`, whose TypeScript source is `src/dual/thing.mts` |
| `src/dual/emitted.mts` | the function `emitted`, with no `.mjs` of its own |
| `src/dual/flagged.cts` | the function `flagged`, with no `.cjs` of its own |

## References

- **Explicit module specifier.** `src/app.mjs` imports `total` from
  `./core/money.mjs`.
- **Directory specifier.** `src/app.mjs` imports `Wallet` from `./core`, which
  resolves through `src/core/index.mjs` and binds the definition in the file the
  barrel re-exports from.
- **Extensionless specifier.** `src/app.mjs` imports `formatAmount` from
  `./util/format`, and `src/legacy/report.cjs` imports `grandTotal` from
  `./totals`.
- **Source before its built output.** `src/app.mjs` imports `tool` from
  `./build/tool`, where both `tool.mts` and `tool.mjs` exist.
- **Literal file before the compiled-from mapping.** `src/dual/pick.mjs`
  imports `thing` from `./thing.mjs`, where both `thing.mjs` and `thing.mts`
  exist; it imports `emitted` from `./emitted.mjs` and `flagged` from
  `./flagged.cjs`, where only the TypeScript sources exist.
- **Unresolvable specifier.** `src/broken/missing.mjs` imports from
  `./nowhere.mjs`, which names no file in the repository.
- **Bare specifier.** the `node:test` suite imports `node:test` and
  `node:assert` by name.

## Not walked

`src/skip/upper.MJS` differs from a listed suffix only in case, and
`src/skip/notes.txt` names no supported suffix. Neither contributes a
definition.

## Test files

Named as test files: `src/core/money.test.mjs` (the `node:test` suite),
`src/core/money.test.js`, `src/core/money.spec.js`, `src/core/money.spec.mjs`,
`src/core/money.test.cjs` and `src/core/money.spec.cjs` by suffix, and
`src/core/__tests__/deep/check.mjs`, `check.js` and `check.cjs` by their
`__tests__` directory.

Not test files by name, although each references a symbol under test:
`src/core/money.test.jsx` and `src/core/money.test.mts` (suffixes the rule does
not name), `src/naming/Total.TEST.mjs` (a case variant of `*.test.*`),
`src/core/__tests__x/check.mjs` (`__tests__` as part of a longer segment),
`src/case/__Tests__/check.mjs` (a case variant of the segment) and
`tests/helpers.js` (a `tests` directory, not `__tests__`).
