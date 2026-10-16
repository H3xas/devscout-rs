# all-mjs

A repository whose every source file is `.mjs`, so the six query verbs can be
checked on a tree that holds no TypeScript and no configuration file: there is
no `tsconfig.json`, no `jsconfig.json` and no `package.json` here. The names are
invented; nothing here is meant to run.

## Declarations

| File | Declares |
| --- | --- |
| `src/core/money.mjs` | the function `total` and the class `Wallet`, whose `balance` calls `total` |
| `src/core/index.mjs` | nothing: a barrel that re-exports `./money.mjs` with `export *` |
| `src/util/format.mjs` | the function `formatAmount` |
| `src/app.mjs` | `summarize`, `makeWallet`: the cross-file caller |

## References

`src/app.mjs` reaches all three declarations through the three relative
specifier forms:

- **Explicit module specifier.** `total` from `./core/money.mjs`.
- **Directory specifier.** `Wallet` from `./core`, which resolves through
  `src/core/index.mjs` and binds the definition in the file the barrel
  re-exports from.
- **Extensionless specifier.** `formatAmount` from `./util/format`.
- **Bare specifier.** the `node:test` suite imports `node:test` and
  `node:assert` by name.

## Test files

The name rule classifies `.mjs` files with a `*.test.mjs` or `*.spec.mjs`
ending, or below an exact `__tests__` directory segment, case-sensitively.
Neither a runner nor a `package.json` is needed for the classification: the
name alone vouches the file.

Named as test files: `src/core/money.test.mjs` (the `node:test` suite),
`src/core/money.spec.mjs` by suffix, and `src/core/__tests__/check.mjs` by its
`__tests__` directory.

Not test files by name, although each references `total`:
`src/naming/Total.TEST.mjs` (a case variant of `*.test.*`),
`src/core/__tests__x/check.mjs` (`__tests__` as part of a longer segment),
`src/case/__Tests__/check.mjs` (a case variant of the segment) and
`tests/helpers.mjs` (a `tests` directory, not `__tests__`).
