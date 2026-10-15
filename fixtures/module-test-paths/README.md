# module-test-paths

Each file imports and calls `total` from `src/lib/money.mjs`. The three
suffix-named files and three files below an exact `__tests__` directory
are test files by path. The case variant, longer directory segment and
`tests/` directory are still referrers, but not test files by path.

The root `__tests__/root.jsx` and nested `src/__tests__/deep/` files
cover both positions of the directory segment.
