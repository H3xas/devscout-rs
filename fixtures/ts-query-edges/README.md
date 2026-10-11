# ts-query-edges

A small TypeScript/React repository whose files between them write every
reference shape the TS extractor records, so `refs`, `read`, `tests` and
`impact` can be checked against the `import`, `call`, `jsx-use` and `dispatch`
edges they produce. The names are invented; nothing here is meant to build.

## Declarations

| File | Declares |
| --- | --- |
| `src/model/cart.ts` | `interface CartItem`, `type CartId`, and `emptyCart` as the default export |
| `src/model/index.ts` | nothing: a barrel that re-exports `./cart` with `export *` |
| `src/store/cartStore.ts` | `class CartStore` |
| `src/actions/cartActions.ts` | the action creators `loadCart` and `clearCart` |
| `src/hooks/useCart.ts` | the hook `useCart` |
| `src/components/CartBadge.tsx` | the component `CartBadge` |
| `src/pages/CartPage.tsx` | the page component `CartPage` |

## References

- **React tree.** `CartPage` renders `<CartBadge />`, `CartBadge` calls
  `useCart()`, and `useCart` builds `new CartStore()`. `src/app/App.tsx` renders
  `<CartPage />`, so it depends on `cartActions.ts` only through the page: two
  hops away.
- **Dispatch.** `CartPage` runs `dispatch(loadCart())`, which the extractor
  records as a `dispatch` edge rather than a call.
- **Import forms.** `CartPage.tsx` imports `CartBadge` and `loadCart` by name,
  `emptyCart` as a default import, and `CartId` and `CartItem` in one statement
  where `CartId` is marked `type`. `useCart.ts` imports `CartItem` with
  `import type` through the `src/model` barrel, which also yields an edge to the
  declaring file carrying the barrel as `via`. `src/services/checkout.ts`
  imports `CartStore` under the alias `Store`.
- **Re-exports.** `src/public/index.ts` re-exports `CartStore` straight from
  its declaring file, and `CartItem` from the barrel, which names it only
  through its own `export *`.
- **Namespace imports.** `checkout.ts` imports `cartActions` as a namespace
  and calls `actions.clearCart()`. `src/services/audit.ts` imports the same
  namespace and never calls through it.
- **Side-effect import.** `src/setup/polyfills.ts` only runs
  `import '../store/cartStore'`.
- **JavaScript caller.** `src/legacy/cartLegacy.js` takes `loadCart` from a
  destructured `require` and calls it.
- **Self-reference.** `src/tree/walk.ts` declares `walkTree`, which calls
  itself inside its own body and is referenced from nowhere else.
- **Many rows on one symbol.** `src/bulk/refill.ts` calls `loadCart()` twenty
  times and dispatches it twelve times, more rows than a default `refs` answer
  shows.

## Test files

Named as test files: `src/hooks/useCart.spec.ts`,
`src/components/CartBadge.spec.tsx`, `src/actions/cartActions.test.ts`,
`src/actions/cartActions.test.js`, `src/pages/CartPage.test.tsx`, and
`src/store/__tests__/deep/total.ts`, which sits below a `__tests__` directory.

Not test files by name, although each references a symbol under test:
`tests/cartStore.ts` (a `tests` directory, not `__tests__`), and
`src/store/cartStore__tests__.ts` (`__tests__` in the file name, not as a
directory).
