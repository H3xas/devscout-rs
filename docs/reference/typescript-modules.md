# TypeScript and JavaScript module resolution

`devscout map` extracts module names from source syntax, then resolves them using the importing
file's tsconfig and package configuration. A declared `moduleResolution` of `node` or `node10`,
`node16`, `nodenext`, or `bundler` selects the corresponding rules. When `moduleResolution` is
absent, `module: node16`, `node18` or `node20` selects node16, `nodenext` selects nodenext, and
`preserve` selects bundler. An explicit `classic` is reported as unsupported and uses the previous
resolution. A scope with no declared mode also keeps the previous resolution.

| Mode | Relative imports | Package entry points |
| --- | --- | --- |
| `node` / `node10` | Extensionless and directory index imports are allowed; emitted `.js` names can reach `.ts` or `.tsx` source. | Reads `main`, then a directory index. Ignores `exports` and `imports`. |
| `node16` | The importing file's CommonJS or ESM format matters. ESM requires an explicit extension and forbids directory imports; `.js`, `.mjs` and `.cjs` names can reach their TypeScript source extensions. | Reads `exports` and `#` `imports` under the selected `import` or `require`, `node`, `default` and custom conditions. |
| `nodenext` | Applies the same format-sensitive extension and directory rules as node16. | Reads conditional `exports` and `imports` for the usage mode. |
| `bundler` | Extensionless relative imports and directory indexes are allowed. | Reads conditional `exports` and `imports` without node16's ESM extension requirement. |

Format comes from `.mts`/`.mjs` (ESM), `.cts`/`.cjs` (CommonJS), or the nearest accepted
`package.json` `type` for `.ts`, `.tsx`, `.js` and `.jsx`. A package's `exports` map can be a root
target, subpath map, pattern or nested condition map; a present map that does not match blocks a
directory fallback. `null` excludes a target. The `types` condition, `types`/`typings` fields and
the package `module` field do not produce an implementation edge.

The nearest enclosing npm or yarn `workspaces` declaration, or pnpm `packages` declaration, names
the repository's workspace members. `*`, `**` and `!` patterns select member directories.
Workspace packages can resolve by name and allowed subpath, after the mode's `paths` and `baseUrl`
rules. Duplicate member names remain unresolved. External dependencies stay external; devscout
does not index `node_modules` contents. A package target under its declared `outDir` maps to source
only when its tsconfig also declares `rootDir` or `composite`. The corresponding source wins over
a built output file. No sibling `src/` path is guessed when that mapping is absent.

The extractor records an import statement, export-from statement, import-equals, a literal dynamic
`import()`, and `require('name')` only when that call directly initializes a variable in a top-level,
non-exported declaration. It records `export =` and namespace re-exports so their bindings can reach
the named module. `import * as ns` and `export * as ns` allow member lookup. A non-literal dynamic
import contributes a count but no edge. A bare or nested `require()`, `require.resolve`, a binding
through dynamic import, an ambient module name, and an anonymous default export have no module
binding here.

## Unresolved imports

For a declared-mode scope, `graph.json` appends `unresolved_imports` when a recorded import cannot
name an indexed implementation file. Each record has `from_file`, `from_line`, `target` (the source
specifier) and one `reason`. `stats.ts.unresolved_import_reasons` counts those reasons. These keys
are omitted when empty.

| Reason | Meaning |
| --- | --- |
| `bare-package-outside-workspace` | No workspace member or local alias names the package. |
| `esm-extension-required` | A node16/nodenext ESM relative import omits the required extension. |
| `directory-import-not-allowed` | A node16/nodenext ESM import names a directory. |
| `exports-subpath-not-matched` | A present `exports` map has no matching subpath. |
| `imports-specifier-not-matched` | The package's `imports` map has no matching `#` name. |
| `no-matching-condition` | A map entry has no target for the active conditions. |
| `build-output-without-source-mapping` | A named output has no declared mapping back to indexed source. |
| `declaration-only-target` | Resolution reaches a declaration file, which devscout does not index as implementation. |
| `target-not-indexed` | The selected implementation target is absent from the graph's indexed files. |
| `ambiguous-workspace-name` | More than one selected member declares the same package name. |
| `configuration-refused` | A configuration input needed for this import is malformed or uses an unsupported shape. |

`stats.ts` also appends `unsupported_mode_scopes`, `refused_config` (file and failing field), and
`non_literal_dynamic_import_count` when present. A refused manifest or workspace file contributes
no package facts; a refused module setting declares no mode for that config chain. A tsconfig that
does not parse contributes no option and is reported as refused. The fallback never probes a target
that contradicts a declared mode.

`map` must run again after changing `tsconfig.json`, `package.json` or `pnpm-workspace.yaml` alone;
a configuration edit does not re-resolve the existing graph during a query. Type-level resolution,
declaration files, JSON modules, project references, `rootDirs`, `moduleSuffixes`, `jsconfig.json`,
Yarn Plug'n'Play and `node_modules` internals are outside this resolver. See
[How names resolve](../explanation/resolution.md#typescript-imports) for the graph/query boundary.
