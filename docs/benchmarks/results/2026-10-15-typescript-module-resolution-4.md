# TypeScript module resolution

Run date: 2026-10-15. Base: `f136d8ba30ad5f4d6f80f2c5c60173349abd599c`. Head: `7625afb5815105255164b0da6ef6d704a5211676`.
First registration: `2026-10-15-typescript-module-resolution.md`. This run scores the sites recorded by the extractor and lists excluded sites.


Truth: TypeScript 5.9.3 with declaration files excluded.

| Fixture | Mode | Arm | Agreement | Wrong target | Spurious | Gate |
| --- | --- | --- | ---: | ---: | ---: | --- |
| vite | bundler | base | 1072/1099 | 1 | 0 | missed |
| vite | bundler | head | 1099/1099 | 0 | 0 | met |
| nx | node10 | base | 2920/2993 | 0 | 0 | met |
| nx | node10 | head | 2993/2993 | 0 | 0 | met |
| pyright | node16 | base | 2768/2768 | 0 | 0 | met |
| pyright | node16 | head | 2768/2768 | 0 | 0 | met |
| schematics | nodenext | base | 129/129 | 0 | 7 | missed |
| schematics | nodenext | head | 129/129 | 0 | 0 | met |

## Pooled modes

| Mode | Arm | Agreement | Wrong target | Spurious | Gate |
| --- | --- | ---: | ---: | ---: | --- |
| bundler | base | 1072/1099 | 1 | 0 | missed |
| bundler | head | 1099/1099 | 0 | 0 | met |
| node10 | base | 2920/2993 | 0 | 0 | met |
| node10 | head | 2993/2993 | 0 | 0 | met |
| node16 | base | 2768/2768 | 0 | 0 | met |
| node16 | head | 2768/2768 | 0 | 0 | met |
| nodenext | base | 129/129 | 0 | 7 | missed |
| nodenext | head | 129/129 | 0 | 0 | met |

## Disagreements

- nx base packages/nx/src/command-line/add/command-object.ts:44 `./add`: miss; truth `packages/nx/src/command-line/add/add.ts`; actual `none`
- nx base packages/nx/src/command-line/affected/command-object.ts:47 `./affected`: miss; truth `packages/nx/src/command-line/affected/affected.ts`; actual `none`
- nx base packages/nx/src/command-line/affected/command-object.ts:73 `./affected`: miss; truth `packages/nx/src/command-line/affected/affected.ts`; actual `none`
- nx base packages/nx/src/command-line/affected/command-object.ts:99 `./affected`: miss; truth `packages/nx/src/command-line/affected/affected.ts`; actual `none`
- nx base packages/nx/src/command-line/affected/command-object.ts:125 `./affected`: miss; truth `packages/nx/src/command-line/affected/affected.ts`; actual `none`
- nx base packages/nx/src/command-line/affected/command-object.ts:151 `./affected`: miss; truth `packages/nx/src/command-line/affected/affected.ts`; actual `none`
- nx base packages/nx/src/command-line/configure-ai-agents/command-object.ts:69 `./configure-ai-agents`: miss; truth `packages/nx/src/command-line/configure-ai-agents/configure-ai-agents.ts`; actual `none`
- nx base packages/nx/src/command-line/daemon/command-object.ts:17 `./daemon`: miss; truth `packages/nx/src/command-line/daemon/daemon.ts`; actual `none`
- nx base packages/nx/src/command-line/daemon/daemon.ts:8 `../../daemon/client/client`: miss; truth `packages/nx/src/daemon/client/client.ts`; actual `none`
- nx base packages/nx/src/command-line/daemon/daemon.ts:19 `../../daemon/client/client`: miss; truth `packages/nx/src/daemon/client/client.ts`; actual `none`
- nx base packages/nx/src/command-line/exec/command-object.ts:14 `./exec`: miss; truth `packages/nx/src/command-line/exec/exec.ts`; actual `none`
- nx base packages/nx/src/command-line/format/command-object.ts:11 `./format`: miss; truth `packages/nx/src/command-line/format/format.ts`; actual `none`
- nx base packages/nx/src/command-line/format/command-object.ts:23 `./format`: miss; truth `packages/nx/src/command-line/format/format.ts`; actual `none`
- nx base packages/nx/src/command-line/generate/command-object.ts:14 `./generate`: miss; truth `packages/nx/src/command-line/generate/generate.ts`; actual `none`
- nx base packages/nx/src/command-line/generate/generate.ts:411 `../../adapter/ngcli-adapter`: miss; truth `packages/nx/src/adapter/ngcli-adapter.ts`; actual `none`
- nx base packages/nx/src/command-line/graph/command-object.ts:28 `./graph`: miss; truth `packages/nx/src/command-line/graph/graph.ts`; actual `none`
- nx base packages/nx/src/command-line/import/command-object.ts:50 `./import`: miss; truth `packages/nx/src/command-line/import/import.ts`; actual `none`
- nx base packages/nx/src/command-line/init/command-object.ts:42 `../../config/nx-json`: miss; truth `packages/nx/src/config/nx-json.ts`; actual `none`
- nx base packages/nx/src/command-line/init/implementation/react/index.ts:96 `./add-vite-commands-to-package-scripts`: miss; truth `packages/nx/src/command-line/init/implementation/react/add-vite-commands-to-package-scripts.ts`; actual `none`
- nx base packages/nx/src/command-line/list/command-object.ts:18 `./list`: miss; truth `packages/nx/src/command-line/list/list.ts`; actual `none`
- nx base packages/nx/src/command-line/mcp/command-object.ts:24 `./mcp`: miss; truth `packages/nx/src/command-line/mcp/mcp.ts`; actual `none`
- nx base packages/nx/src/command-line/migrate/command-object.ts:13 `./migrate`: miss; truth `packages/nx/src/command-line/migrate/migrate.ts`; actual `none`
- nx base packages/nx/src/command-line/migrate/command-object.ts:25 `./migrate`: miss; truth `packages/nx/src/command-line/migrate/migrate.ts`; actual `none`
- nx base packages/nx/src/command-line/migrate/migrate.ts:2056 `../../adapter/ngcli-adapter`: miss; truth `packages/nx/src/adapter/ngcli-adapter.ts`; actual `none`
- nx base packages/nx/src/command-line/new/command-object.ts:11 `./new`: miss; truth `packages/nx/src/command-line/new/new.ts`; actual `none`
- nx base packages/nx/src/command-line/nx-cloud/apply-locally/command-object.ts:15 `./apply-locally`: miss; truth `packages/nx/src/command-line/nx-cloud/apply-locally/apply-locally.ts`; actual `none`
- nx base packages/nx/src/command-line/nx-cloud/complete-run/command-object.ts:16 `./stop-all-agents`: miss; truth `packages/nx/src/command-line/nx-cloud/complete-run/stop-all-agents.ts`; actual `none`
- nx base packages/nx/src/command-line/nx-cloud/connect/command-object.ts:15 `./connect-to-nx-cloud`: miss; truth `packages/nx/src/command-line/nx-cloud/connect/connect-to-nx-cloud.ts`; actual `none`
- nx base packages/nx/src/command-line/nx-cloud/connect/command-object.ts:18 `../../../utils/ab-testing`: miss; truth `packages/nx/src/utils/ab-testing.ts`; actual `none`
- nx base packages/nx/src/command-line/nx-cloud/connect/command-object.ts:41 `./view-logs`: miss; truth `packages/nx/src/command-line/nx-cloud/connect/view-logs.ts`; actual `none`
- nx base packages/nx/src/command-line/nx-cloud/download-cloud-client/command-object.ts:11 `./download-cloud-client`: miss; truth `packages/nx/src/command-line/nx-cloud/download-cloud-client/download-cloud-client.ts`; actual `none`
- nx base packages/nx/src/command-line/nx-cloud/fix-ci/command-object.ts:14 `./fix-ci`: miss; truth `packages/nx/src/command-line/nx-cloud/fix-ci/fix-ci.ts`; actual `none`
- nx base packages/nx/src/command-line/nx-cloud/login/command-object.ts:20 `./login`: miss; truth `packages/nx/src/command-line/nx-cloud/login/login.ts`; actual `none`
- nx base packages/nx/src/command-line/nx-cloud/logout/command-object.ts:14 `./logout`: miss; truth `packages/nx/src/command-line/nx-cloud/logout/logout.ts`; actual `none`
- nx base packages/nx/src/command-line/nx-cloud/record/command-object.ts:17 `./record`: miss; truth `packages/nx/src/command-line/nx-cloud/record/record.ts`; actual `none`
- nx base packages/nx/src/command-line/nx-cloud/start-agent/command-object.ts:14 `./start-agent`: miss; truth `packages/nx/src/command-line/nx-cloud/start-agent/start-agent.ts`; actual `none`
- nx base packages/nx/src/command-line/nx-cloud/start-ci-run/command-object.ts:15 `./start-ci-run`: miss; truth `packages/nx/src/command-line/nx-cloud/start-ci-run/start-ci-run.ts`; actual `none`
- nx base packages/nx/src/command-line/register/command-object.ts:27 `./register`: miss; truth `packages/nx/src/command-line/register/register.ts`; actual `none`
- nx base packages/nx/src/command-line/release/command-object.ts:180 `../../config/nx-json`: miss; truth `packages/nx/src/config/nx-json.ts`; actual `none`
- nx base packages/nx/src/command-line/release/command-object.ts:239 `./release`: miss; truth `packages/nx/src/command-line/release/release.ts`; actual `none`
- nx base packages/nx/src/command-line/release/command-object.ts:279 `./version`: miss; truth `packages/nx/src/command-line/release/version.ts`; actual `none`
- nx base packages/nx/src/command-line/release/command-object.ts:347 `./changelog`: miss; truth `packages/nx/src/command-line/release/changelog.ts`; actual `none`
- nx base packages/nx/src/command-line/release/command-object.ts:387 `./publish`: miss; truth `packages/nx/src/command-line/release/publish.ts`; actual `none`
- nx base packages/nx/src/command-line/release/command-object.ts:429 `./plan`: miss; truth `packages/nx/src/command-line/release/plan.ts`; actual `none`
- nx base packages/nx/src/command-line/release/command-object.ts:445 `./plan-check`: miss; truth `packages/nx/src/command-line/release/plan-check.ts`; actual `none`
- nx base packages/nx/src/command-line/release/utils/remote-release-clients/remote-release-client.ts:253 `./github`: miss; truth `packages/nx/src/command-line/release/utils/remote-release-clients/github.ts`; actual `none`
- nx base packages/nx/src/command-line/release/utils/remote-release-clients/remote-release-client.ts:271 `./gitlab`: miss; truth `packages/nx/src/command-line/release/utils/remote-release-clients/gitlab.ts`; actual `none`
- nx base packages/nx/src/command-line/repair/command-object.ts:19 `./repair`: miss; truth `packages/nx/src/command-line/repair/repair.ts`; actual `none`
- nx base packages/nx/src/command-line/report/command-object.ts:8 `./report`: miss; truth `packages/nx/src/command-line/report/report.ts`; actual `none`
- nx base packages/nx/src/command-line/reset/command-object.ts:40 `./reset`: miss; truth `packages/nx/src/command-line/reset/reset.ts`; actual `none`
- nx base packages/nx/src/command-line/run-many/command-object.ts:31 `./run-many`: miss; truth `packages/nx/src/command-line/run-many/run-many.ts`; actual `none`
- nx base packages/nx/src/command-line/run/command-object.ts:24 `./run-one`: miss; truth `packages/nx/src/command-line/run/run-one.ts`; actual `none`
- nx base packages/nx/src/command-line/run/command-object.ts:49 `./run-one`: miss; truth `packages/nx/src/command-line/run/run-one.ts`; actual `none`
- nx base packages/nx/src/command-line/run/run-one.ts:63 `./run`: miss; truth `packages/nx/src/command-line/run/run.ts`; actual `none`
- nx base packages/nx/src/command-line/run/run.ts:213 `../../adapter/ngcli-adapter`: miss; truth `packages/nx/src/adapter/ngcli-adapter.ts`; actual `none`
- nx base packages/nx/src/command-line/run/run.ts:225 `../../adapter/rxjs-for-await`: miss; truth `packages/nx/src/adapter/rxjs-for-await.ts`; actual `none`
- nx base packages/nx/src/command-line/show/command-object.ts:128 `./projects`: miss; truth `packages/nx/src/command-line/show/projects.ts`; actual `none`
- nx base packages/nx/src/command-line/show/command-object.ts:185 `./project`: miss; truth `packages/nx/src/command-line/show/project.ts`; actual `none`
- nx base packages/nx/src/command-line/sync/command-object.ts:16 `./sync`: miss; truth `packages/nx/src/command-line/sync/sync.ts`; actual `none`
- nx base packages/nx/src/command-line/sync/command-object.ts:30 `./sync`: miss; truth `packages/nx/src/command-line/sync/sync.ts`; actual `none`
- nx base packages/nx/src/command-line/watch/command-object.ts:11 `./watch`: miss; truth `packages/nx/src/command-line/watch/watch.ts`; actual `none`
- nx base packages/nx/src/daemon/client/client.ts:1220 `../../tasks-runner/process-metrics-service`: miss; truth `packages/nx/src/tasks-runner/process-metrics-service.ts`; actual `none`
- nx base packages/nx/src/daemon/server/watcher.ts:16 `../../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx base packages/nx/src/daemon/server/watcher.ts:46 `../../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx base packages/nx/src/generators/utils/generate-files.spec.ts:2 `nx/src/generators/testing-utils/create-tree`: miss; truth `packages/nx/src/generators/testing-utils/create-tree.ts`; actual `none`
- nx base packages/nx/src/generators/utils/generate-files.spec.ts:3 `nx/src/generators/tree`: miss; truth `packages/nx/src/generators/tree.ts`; actual `none`
- nx base packages/nx/src/project-graph/plugins/in-process-loader.ts:85 `./load-resolved-plugin`: miss; truth `packages/nx/src/project-graph/plugins/load-resolved-plugin.ts`; actual `none`
- nx base packages/nx/src/project-graph/plugins/isolation/isolated-plugin.ts:467 `../../../daemon/is-on-daemon`: miss; truth `packages/nx/src/daemon/is-on-daemon.ts`; actual `none`
- nx base packages/nx/src/project-graph/plugins/isolation/isolated-plugin.ts:469 `../../../tasks-runner/process-metrics-service`: miss; truth `packages/nx/src/tasks-runner/process-metrics-service.ts`; actual `none`
- nx base packages/nx/src/project-graph/plugins/isolation/plugin-worker.ts:78 `../load-resolved-plugin`: miss; truth `packages/nx/src/project-graph/plugins/load-resolved-plugin.ts`; actual `none`
- nx base packages/nx/src/tasks-runner/run-command.ts:134 `../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx base packages/nx/src/utils/handle-errors.ts:69 `../daemon/client/client`: miss; truth `packages/nx/src/daemon/client/client.ts`; actual `none`
- nx base packages/nx/src/utils/workspace-context.ts:115 `../daemon/server/project-graph-incremental-recomputation`: miss; truth `packages/nx/src/daemon/server/project-graph-incremental-recomputation.ts`; actual `none`
- schematics base src/lib/sub-app/files/js/src/app.controller.__specFileSuffix__.js:2 `./app.controller`: spurious; truth ``; actual `src/lib/sub-app/files/js/src/app.controller.js`
- schematics base src/lib/sub-app/files/js/src/app.controller.__specFileSuffix__.js:3 `./app.service`: spurious; truth ``; actual `src/lib/sub-app/files/js/src/app.service.js`
- schematics base src/lib/sub-app/files/js/src/app.controller.js:2 `./app.service`: spurious; truth ``; actual `src/lib/sub-app/files/js/src/app.service.js`
- schematics base src/lib/sub-app/files/js/src/app.module.js:2 `./app.controller`: spurious; truth ``; actual `src/lib/sub-app/files/js/src/app.controller.js`
- schematics base src/lib/sub-app/files/js/src/app.module.js:3 `./app.service`: spurious; truth ``; actual `src/lib/sub-app/files/js/src/app.service.js`
- schematics base src/lib/sub-app/files/js/src/main.js:2 `./app.module`: spurious; truth ``; actual `src/lib/sub-app/files/js/src/app.module.js`
- schematics base src/lib/sub-app/files/js/test/app.e2e-__specFileSuffix__.js:3 `../src/app.module`: spurious; truth ``; actual `src/lib/sub-app/files/js/src/app.module.js`
- vite base packages/vite/src/node/__tests__/fixtures/config/entry/vite.config.ts:1 `@vite/test-config-plugin-module-condition`: miss; truth `packages/vite/src/node/__tests__/fixtures/config/plugin-module-condition/index.mjs`; actual `none`
- vite base packages/vite/src/node/__tests__/fixtures/config/entry/vite.config.ts:2 `#imports-field`: miss; truth `packages/vite/src/node/__tests__/fixtures/config/entry/imports-field.ts`; actual `none`
- vite base packages/vite/src/node/__tests__/fixtures/dynamic-import/entry.mjs:2 `./dep.mjs`: miss; truth `packages/vite/src/node/__tests__/fixtures/dynamic-import/dep.mjs`; actual `none`
- vite base packages/vite/src/node/__tests__/fixtures/runner-import/dynamic-import.ts:1 `./dynamic-import-dep`: miss; truth `packages/vite/src/node/__tests__/fixtures/runner-import/dynamic-import-dep.ts`; actual `none`
- vite base packages/vite/src/node/__tests__/fixtures/runner-import/vite.config.outside-pkg-import.mts:1 `@vitejs/parent`: miss; truth `packages/vite/src/node/__tests__/packages/parent/index.ts`; actual `none`
- vite base packages/vite/src/node/__tests__/fixtures/shared-plugins/chunk-import-map/entry.js:1 `./lazy.js`: miss; truth `packages/vite/src/node/__tests__/fixtures/shared-plugins/chunk-import-map/lazy.js`; actual `none`
- vite base packages/vite/src/node/__tests__/fixtures/test-dep-conditions-app/entry-with-module.js:1 `@vitejs/test-dep-conditions/with-module`: miss; truth `packages/vite/src/node/__tests__/fixtures/test-dep-conditions/dir/index.default.js`; actual `none`
- vite base packages/vite/src/node/__tests__/fixtures/test-dep-conditions-app/entry.js:1 `@vitejs/test-dep-conditions`: miss; truth `packages/vite/src/node/__tests__/fixtures/test-dep-conditions/index.default.js`; actual `none`
- vite base packages/vite/src/node/__tests__/fixtures/watch-rebuild-manifest/entry.js:2 `./dep.js`: miss; truth `packages/vite/src/node/__tests__/fixtures/watch-rebuild-manifest/dep.js`; actual `none`
- vite base packages/vite/src/node/__tests__/fixtures/worker-dynamic/worker.js:2 `./dynamic`: miss; truth `packages/vite/src/node/__tests__/fixtures/worker-dynamic/dynamic.js`; actual `none`
- vite base packages/vite/src/node/__tests__/packages/parent/index.ts:2 `@vitejs/child`: miss; truth `packages/vite/src/node/__tests__/packages/child/index.js`; actual `none`
- vite base packages/vite/src/node/__tests__/plugins/fixtures/license/dep-license-mit/index.js:1 `@vitejs/test-dep-nested-license-isc`: miss; truth `packages/vite/src/node/__tests__/plugins/fixtures/license/dep-nested-license-isc/index.js`; actual `none`
- vite base packages/vite/src/node/cli.ts:227 `./server`: miss; truth `packages/vite/src/node/server/index.ts`; actual `none`
- vite base packages/vite/src/node/cli.ts:408 `./config`: miss; truth `packages/vite/src/node/config.ts`; actual `none`
- vite base packages/vite/src/node/cli.ts:409 `./optimizer`: miss; truth `packages/vite/src/node/optimizer/index.ts`; actual `none`
- vite base packages/vite/src/node/cli.ts:453 `./preview`: miss; truth `packages/vite/src/node/preview.ts`; actual `none`
- vite base packages/vite/src/node/nodeResolve.ts:1 `#module-sync-enabled`: miss; truth `packages/vite/misc/false.js`; actual `none`
- vite base packages/vite/src/node/ssr/__tests__/fixtures/errors/syntax-error-dep.js:1 `./syntax-error.js`: wrong-target; truth `packages/vite/src/node/ssr/__tests__/fixtures/errors/syntax-error.ts`; actual `packages/vite/src/node/ssr/__tests__/fixtures/errors/syntax-error.js`
- vite base packages/vite/src/node/ssr/runnerImport.ts:19 `#module-sync-enabled`: miss; truth `packages/vite/misc/false.js`; actual `none`
- vite base packages/vite/src/node/ssr/runtime/__tests__/fixtures/cjs-external-existing.js:1 `@vitejs/cjs-external`: miss; truth `packages/vite/src/node/ssr/runtime/__tests__/fixtures/cjs-external/index.cjs`; actual `none`
- vite base packages/vite/src/node/ssr/runtime/__tests__/fixtures/cjs-external-non-existing.js:1 `@vitejs/cjs-external`: miss; truth `packages/vite/src/node/ssr/runtime/__tests__/fixtures/cjs-external/index.cjs`; actual `none`
- vite base packages/vite/src/node/ssr/runtime/__tests__/fixtures/cyclic/entry-cyclic.js:2 `./entry.js`: miss; truth `packages/vite/src/node/ssr/runtime/__tests__/fixtures/cyclic/entry.js`; actual `none`
- vite base packages/vite/src/node/ssr/runtime/__tests__/fixtures/cyclic/entry.js:2 `./entry-cyclic.js`: miss; truth `packages/vite/src/node/ssr/runtime/__tests__/fixtures/cyclic/entry-cyclic.js`; actual `none`
- vite base packages/vite/src/node/ssr/runtime/__tests__/fixtures/dynamic-import.js:12 `./simple`: miss; truth `packages/vite/src/node/ssr/runtime/__tests__/fixtures/simple.js`; actual `none`
- vite base packages/vite/src/node/ssr/runtime/__tests__/fixtures/esm-external-existing.js:1 `@vitejs/esm-external`: miss; truth `packages/vite/src/node/ssr/runtime/__tests__/fixtures/esm-external/index.mjs`; actual `none`
- vite base packages/vite/src/node/ssr/runtime/__tests__/fixtures/esm-external-non-existing.js:1 `@vitejs/esm-external`: miss; truth `packages/vite/src/node/ssr/runtime/__tests__/fixtures/esm-external/index.mjs`; actual `none`
- vite base packages/vite/src/node/ssr/runtime/__tests__/fixtures/hmr-evaluated-import-race/evaluated.js:5 `./shared.js`: miss; truth `packages/vite/src/node/ssr/runtime/__tests__/fixtures/hmr-evaluated-import-race/shared.js`; actual `none`

## Outside the scoring set

| Fixture | Arm | Sub-class | Sites |
| --- | --- | --- | ---: |
| nx | base | json-module | 4 |
| nx | base | non-literal-import | 7 |
| nx | head | json-module | 4 |
| nx | head | non-literal-import | 7 |
| pyright | base | json-module | 16 |
| pyright | head | json-module | 16 |
| schematics | base | refused-configuration | 48 |
| schematics | head | refused-configuration | 48 |
| vite | base | json-module | 10 |
| vite | base | non-literal-import | 20 |
| vite | head | json-module | 10 |
| vite | head | non-literal-import | 20 |

- nx base packages/nx/migrations.spec.ts:1 `./migrations.json`: json-module; truth ``; actual `none`
- nx base packages/nx/src/ai/set-up-ai-agents/set-up-ai-agents.ts:121 `(non-literal)`: non-literal-import; truth ``; actual `none`
- nx base packages/nx/src/command-line/configure-ai-agents/configure-ai-agents.ts:59 `(non-literal)`: non-literal-import; truth ``; actual `none`
- nx base packages/nx/src/command-line/init/init-v2.ts:65 `(non-literal)`: non-literal-import; truth ``; actual `none`
- nx base packages/nx/src/command-line/repair/repair.ts:2 `../../../migrations.json`: json-module; truth ``; actual `none`
- nx base packages/nx/src/daemon/server/nx-console-operations.ts:56 `(non-literal)`: non-literal-import; truth ``; actual `none`
- nx base packages/nx/src/daemon/server/nx-console-operations.ts:113 `(non-literal)`: non-literal-import; truth ``; actual `none`
- nx base packages/nx/src/generators/utils/project-configuration.spec.ts:16 `../../../schemas/project-schema.json`: json-module; truth ``; actual `none`
- nx base packages/nx/src/project-graph/plugins/load-resolved-plugin.ts:17 `(non-literal)`: non-literal-import; truth ``; actual `none`
- nx base packages/nx/src/tasks-runner/cache.ts:292 `(non-literal)`: non-literal-import; truth ``; actual `none`
- nx base packages/nx/src/utils/db-connection.ts:3 `../../package.json`: json-module; truth ``; actual `none`
- nx head packages/nx/migrations.spec.ts:1 `./migrations.json`: json-module; truth ``; actual `none`
- nx head packages/nx/src/ai/set-up-ai-agents/set-up-ai-agents.ts:121 `(non-literal)`: non-literal-import; truth ``; actual `none`
- nx head packages/nx/src/command-line/configure-ai-agents/configure-ai-agents.ts:59 `(non-literal)`: non-literal-import; truth ``; actual `none`
- nx head packages/nx/src/command-line/init/init-v2.ts:65 `(non-literal)`: non-literal-import; truth ``; actual `none`
- nx head packages/nx/src/command-line/repair/repair.ts:2 `../../../migrations.json`: json-module; truth ``; actual `none`
- nx head packages/nx/src/daemon/server/nx-console-operations.ts:56 `(non-literal)`: non-literal-import; truth ``; actual `none`
- nx head packages/nx/src/daemon/server/nx-console-operations.ts:113 `(non-literal)`: non-literal-import; truth ``; actual `none`
- nx head packages/nx/src/generators/utils/project-configuration.spec.ts:16 `../../../schemas/project-schema.json`: json-module; truth ``; actual `none`
- nx head packages/nx/src/project-graph/plugins/load-resolved-plugin.ts:17 `(non-literal)`: non-literal-import; truth ``; actual `none`
- nx head packages/nx/src/tasks-runner/cache.ts:292 `(non-literal)`: non-literal-import; truth ``; actual `none`
- nx head packages/nx/src/utils/db-connection.ts:3 `../../package.json`: json-module; truth ``; actual `none`
- pyright base packages/pyright-internal/src/common/windowsProcessTree.ts:9 `./windowsProcessTree.json`: json-module; truth ``; actual `none`
- pyright base packages/pyright-internal/src/localization/localize.ts:12 `./package.nls.cs.json`: json-module; truth ``; actual `none`
- pyright base packages/pyright-internal/src/localization/localize.ts:13 `./package.nls.de.json`: json-module; truth ``; actual `none`
- pyright base packages/pyright-internal/src/localization/localize.ts:14 `./package.nls.en-us.json`: json-module; truth ``; actual `none`
- pyright base packages/pyright-internal/src/localization/localize.ts:15 `./package.nls.es.json`: json-module; truth ``; actual `none`
- pyright base packages/pyright-internal/src/localization/localize.ts:16 `./package.nls.fr.json`: json-module; truth ``; actual `none`
- pyright base packages/pyright-internal/src/localization/localize.ts:17 `./package.nls.it.json`: json-module; truth ``; actual `none`
- pyright base packages/pyright-internal/src/localization/localize.ts:18 `./package.nls.ja.json`: json-module; truth ``; actual `none`
- pyright base packages/pyright-internal/src/localization/localize.ts:19 `./package.nls.ko.json`: json-module; truth ``; actual `none`
- pyright base packages/pyright-internal/src/localization/localize.ts:20 `./package.nls.pl.json`: json-module; truth ``; actual `none`
- pyright base packages/pyright-internal/src/localization/localize.ts:21 `./package.nls.pt-br.json`: json-module; truth ``; actual `none`
- pyright base packages/pyright-internal/src/localization/localize.ts:22 `./package.nls.qps-ploc.json`: json-module; truth ``; actual `none`
- pyright base packages/pyright-internal/src/localization/localize.ts:23 `./package.nls.ru.json`: json-module; truth ``; actual `none`
- pyright base packages/pyright-internal/src/localization/localize.ts:24 `./package.nls.tr.json`: json-module; truth ``; actual `none`
- pyright base packages/pyright-internal/src/localization/localize.ts:25 `./package.nls.zh-cn.json`: json-module; truth ``; actual `none`
- pyright base packages/pyright-internal/src/localization/localize.ts:26 `./package.nls.zh-tw.json`: json-module; truth ``; actual `none`
- pyright head packages/pyright-internal/src/common/windowsProcessTree.ts:9 `./windowsProcessTree.json`: json-module; truth ``; actual `none`
- pyright head packages/pyright-internal/src/localization/localize.ts:12 `./package.nls.cs.json`: json-module; truth ``; actual `none`
- pyright head packages/pyright-internal/src/localization/localize.ts:13 `./package.nls.de.json`: json-module; truth ``; actual `none`
- pyright head packages/pyright-internal/src/localization/localize.ts:14 `./package.nls.en-us.json`: json-module; truth ``; actual `none`
- pyright head packages/pyright-internal/src/localization/localize.ts:15 `./package.nls.es.json`: json-module; truth ``; actual `none`
- pyright head packages/pyright-internal/src/localization/localize.ts:16 `./package.nls.fr.json`: json-module; truth ``; actual `none`
- pyright head packages/pyright-internal/src/localization/localize.ts:17 `./package.nls.it.json`: json-module; truth ``; actual `none`
- pyright head packages/pyright-internal/src/localization/localize.ts:18 `./package.nls.ja.json`: json-module; truth ``; actual `none`
- pyright head packages/pyright-internal/src/localization/localize.ts:19 `./package.nls.ko.json`: json-module; truth ``; actual `none`
- pyright head packages/pyright-internal/src/localization/localize.ts:20 `./package.nls.pl.json`: json-module; truth ``; actual `none`
- pyright head packages/pyright-internal/src/localization/localize.ts:21 `./package.nls.pt-br.json`: json-module; truth ``; actual `none`
- pyright head packages/pyright-internal/src/localization/localize.ts:22 `./package.nls.qps-ploc.json`: json-module; truth ``; actual `none`
- pyright head packages/pyright-internal/src/localization/localize.ts:23 `./package.nls.ru.json`: json-module; truth ``; actual `none`
- pyright head packages/pyright-internal/src/localization/localize.ts:24 `./package.nls.tr.json`: json-module; truth ``; actual `none`
- pyright head packages/pyright-internal/src/localization/localize.ts:25 `./package.nls.zh-cn.json`: json-module; truth ``; actual `none`
- pyright head packages/pyright-internal/src/localization/localize.ts:26 `./package.nls.zh-tw.json`: json-module; truth ``; actual `none`
- schematics base src/lib/application/files/js/src/app.controller.__specFileSuffix__.js:1 `@nestjs/testing`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/js/src/app.controller.__specFileSuffix__.js:2 `./app.controller`: refused-configuration; truth `src/lib/application/files/js/src/app.controller.js`; actual `src/lib/application/files/js/src/app.controller.js`
- schematics base src/lib/application/files/js/src/app.controller.__specFileSuffix__.js:3 `./app.service`: refused-configuration; truth `src/lib/application/files/js/src/app.service.js`; actual `src/lib/application/files/js/src/app.service.js`
- schematics base src/lib/application/files/js/src/app.controller.js:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/js/src/app.controller.js:2 `./app.service`: refused-configuration; truth `src/lib/application/files/js/src/app.service.js`; actual `src/lib/application/files/js/src/app.service.js`
- schematics base src/lib/application/files/js/src/app.module.js:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/js/src/app.module.js:3 `./app.controller`: refused-configuration; truth `src/lib/application/files/js/src/app.controller.js`; actual `src/lib/application/files/js/src/app.controller.js`
- schematics base src/lib/application/files/js/src/app.module.js:4 `./app.service`: refused-configuration; truth `src/lib/application/files/js/src/app.service.js`; actual `src/lib/application/files/js/src/app.service.js`
- schematics base src/lib/application/files/js/src/app.service.js:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/js/src/main.js:1 `@nestjs/core`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/js/test/app.e2e-__specFileSuffix__.js:1 `@nestjs/testing`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/js/test/app.e2e-__specFileSuffix__.js:2 `supertest`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/js/test/app.e2e-__specFileSuffix__.js:3 `../src/app.module`: refused-configuration; truth `src/lib/application/files/js/src/app.module.js`; actual `src/lib/application/files/js/src/app.module.js`
- schematics base src/lib/application/files/ts-esm/src/app.controller.__specFileSuffix__.ts:1 `@nestjs/testing`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts-esm/src/app.controller.__specFileSuffix__.ts:2 `./app.controller.js`: refused-configuration; truth `src/lib/application/files/ts-esm/src/app.controller.ts`; actual `src/lib/application/files/ts-esm/src/app.controller.ts`
- schematics base src/lib/application/files/ts-esm/src/app.controller.__specFileSuffix__.ts:3 `./app.service.js`: refused-configuration; truth `src/lib/application/files/ts-esm/src/app.service.ts`; actual `src/lib/application/files/ts-esm/src/app.service.ts`
- schematics base src/lib/application/files/ts-esm/src/app.controller.ts:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts-esm/src/app.controller.ts:2 `./app.service.js`: refused-configuration; truth `src/lib/application/files/ts-esm/src/app.service.ts`; actual `src/lib/application/files/ts-esm/src/app.service.ts`
- schematics base src/lib/application/files/ts-esm/src/app.module.ts:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts-esm/src/app.module.ts:3 `./app.controller.js`: refused-configuration; truth `src/lib/application/files/ts-esm/src/app.controller.ts`; actual `src/lib/application/files/ts-esm/src/app.controller.ts`
- schematics base src/lib/application/files/ts-esm/src/app.module.ts:4 `./app.service.js`: refused-configuration; truth `src/lib/application/files/ts-esm/src/app.service.ts`; actual `src/lib/application/files/ts-esm/src/app.service.ts`
- schematics base src/lib/application/files/ts-esm/src/app.service.ts:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts-esm/src/main.ts:1 `@nestjs/core`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts-esm/test/app.e2e-__specFileSuffix__.ts:1 `@nestjs/testing`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts-esm/test/app.e2e-__specFileSuffix__.ts:2 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts-esm/test/app.e2e-__specFileSuffix__.ts:3 `supertest`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts-esm/test/app.e2e-__specFileSuffix__.ts:4 `supertest/types.js`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts-esm/test/app.e2e-__specFileSuffix__.ts:5 `./../src/app.module.js`: refused-configuration; truth `src/lib/application/files/ts-esm/src/app.module.ts`; actual `src/lib/application/files/ts-esm/src/app.module.ts`
- schematics base src/lib/application/files/ts-esm/vitest.config.e2e.ts:1 `vitest/config`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts-esm/vitest.config.ts:1 `vitest/config`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts/jest.config.ts:1 `jest`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts/jest.config.ts:2 `ts-jest`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts/jest.config.ts:3 `typescript`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts/src/app.controller.__specFileSuffix__.ts:1 `@nestjs/testing`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts/src/app.controller.__specFileSuffix__.ts:2 `./app.controller`: refused-configuration; truth `src/lib/application/files/ts/src/app.controller.ts`; actual `src/lib/application/files/ts/src/app.controller.ts`
- schematics base src/lib/application/files/ts/src/app.controller.__specFileSuffix__.ts:3 `./app.service`: refused-configuration; truth `src/lib/application/files/ts/src/app.service.ts`; actual `src/lib/application/files/ts/src/app.service.ts`
- schematics base src/lib/application/files/ts/src/app.controller.ts:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts/src/app.controller.ts:2 `./app.service`: refused-configuration; truth `src/lib/application/files/ts/src/app.service.ts`; actual `src/lib/application/files/ts/src/app.service.ts`
- schematics base src/lib/application/files/ts/src/app.module.ts:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts/src/app.module.ts:3 `./app.controller`: refused-configuration; truth `src/lib/application/files/ts/src/app.controller.ts`; actual `src/lib/application/files/ts/src/app.controller.ts`
- schematics base src/lib/application/files/ts/src/app.module.ts:4 `./app.service`: refused-configuration; truth `src/lib/application/files/ts/src/app.service.ts`; actual `src/lib/application/files/ts/src/app.service.ts`
- schematics base src/lib/application/files/ts/src/app.service.ts:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts/src/main.ts:1 `@nestjs/core`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts/test/app.e2e-__specFileSuffix__.ts:1 `@nestjs/testing`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts/test/app.e2e-__specFileSuffix__.ts:2 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts/test/app.e2e-__specFileSuffix__.ts:3 `supertest`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts/test/app.e2e-__specFileSuffix__.ts:4 `supertest/types`: refused-configuration; truth ``; actual `none`
- schematics base src/lib/application/files/ts/test/app.e2e-__specFileSuffix__.ts:5 `./../src/app.module`: refused-configuration; truth `src/lib/application/files/ts/src/app.module.ts`; actual `src/lib/application/files/ts/src/app.module.ts`
- schematics head src/lib/application/files/js/src/app.controller.__specFileSuffix__.js:1 `@nestjs/testing`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/js/src/app.controller.__specFileSuffix__.js:2 `./app.controller`: refused-configuration; truth `src/lib/application/files/js/src/app.controller.js`; actual `none`
- schematics head src/lib/application/files/js/src/app.controller.__specFileSuffix__.js:3 `./app.service`: refused-configuration; truth `src/lib/application/files/js/src/app.service.js`; actual `none`
- schematics head src/lib/application/files/js/src/app.controller.js:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/js/src/app.controller.js:2 `./app.service`: refused-configuration; truth `src/lib/application/files/js/src/app.service.js`; actual `none`
- schematics head src/lib/application/files/js/src/app.module.js:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/js/src/app.module.js:3 `./app.controller`: refused-configuration; truth `src/lib/application/files/js/src/app.controller.js`; actual `none`
- schematics head src/lib/application/files/js/src/app.module.js:4 `./app.service`: refused-configuration; truth `src/lib/application/files/js/src/app.service.js`; actual `none`
- schematics head src/lib/application/files/js/src/app.service.js:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/js/src/main.js:1 `@nestjs/core`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/js/test/app.e2e-__specFileSuffix__.js:1 `@nestjs/testing`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/js/test/app.e2e-__specFileSuffix__.js:2 `supertest`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/js/test/app.e2e-__specFileSuffix__.js:3 `../src/app.module`: refused-configuration; truth `src/lib/application/files/js/src/app.module.js`; actual `none`
- schematics head src/lib/application/files/ts-esm/src/app.controller.__specFileSuffix__.ts:1 `@nestjs/testing`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts-esm/src/app.controller.__specFileSuffix__.ts:2 `./app.controller.js`: refused-configuration; truth `src/lib/application/files/ts-esm/src/app.controller.ts`; actual `none`
- schematics head src/lib/application/files/ts-esm/src/app.controller.__specFileSuffix__.ts:3 `./app.service.js`: refused-configuration; truth `src/lib/application/files/ts-esm/src/app.service.ts`; actual `none`
- schematics head src/lib/application/files/ts-esm/src/app.controller.ts:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts-esm/src/app.controller.ts:2 `./app.service.js`: refused-configuration; truth `src/lib/application/files/ts-esm/src/app.service.ts`; actual `none`
- schematics head src/lib/application/files/ts-esm/src/app.module.ts:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts-esm/src/app.module.ts:3 `./app.controller.js`: refused-configuration; truth `src/lib/application/files/ts-esm/src/app.controller.ts`; actual `none`
- schematics head src/lib/application/files/ts-esm/src/app.module.ts:4 `./app.service.js`: refused-configuration; truth `src/lib/application/files/ts-esm/src/app.service.ts`; actual `none`
- schematics head src/lib/application/files/ts-esm/src/app.service.ts:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts-esm/src/main.ts:1 `@nestjs/core`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts-esm/test/app.e2e-__specFileSuffix__.ts:1 `@nestjs/testing`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts-esm/test/app.e2e-__specFileSuffix__.ts:2 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts-esm/test/app.e2e-__specFileSuffix__.ts:3 `supertest`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts-esm/test/app.e2e-__specFileSuffix__.ts:4 `supertest/types.js`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts-esm/test/app.e2e-__specFileSuffix__.ts:5 `./../src/app.module.js`: refused-configuration; truth `src/lib/application/files/ts-esm/src/app.module.ts`; actual `none`
- schematics head src/lib/application/files/ts-esm/vitest.config.e2e.ts:1 `vitest/config`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts-esm/vitest.config.ts:1 `vitest/config`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts/jest.config.ts:1 `jest`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts/jest.config.ts:2 `ts-jest`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts/jest.config.ts:3 `typescript`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts/src/app.controller.__specFileSuffix__.ts:1 `@nestjs/testing`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts/src/app.controller.__specFileSuffix__.ts:2 `./app.controller`: refused-configuration; truth `src/lib/application/files/ts/src/app.controller.ts`; actual `none`
- schematics head src/lib/application/files/ts/src/app.controller.__specFileSuffix__.ts:3 `./app.service`: refused-configuration; truth `src/lib/application/files/ts/src/app.service.ts`; actual `none`
- schematics head src/lib/application/files/ts/src/app.controller.ts:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts/src/app.controller.ts:2 `./app.service`: refused-configuration; truth `src/lib/application/files/ts/src/app.service.ts`; actual `none`
- schematics head src/lib/application/files/ts/src/app.module.ts:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts/src/app.module.ts:3 `./app.controller`: refused-configuration; truth `src/lib/application/files/ts/src/app.controller.ts`; actual `none`
- schematics head src/lib/application/files/ts/src/app.module.ts:4 `./app.service`: refused-configuration; truth `src/lib/application/files/ts/src/app.service.ts`; actual `none`
- schematics head src/lib/application/files/ts/src/app.service.ts:1 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts/src/main.ts:1 `@nestjs/core`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts/test/app.e2e-__specFileSuffix__.ts:1 `@nestjs/testing`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts/test/app.e2e-__specFileSuffix__.ts:2 `@nestjs/common`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts/test/app.e2e-__specFileSuffix__.ts:3 `supertest`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts/test/app.e2e-__specFileSuffix__.ts:4 `supertest/types`: refused-configuration; truth ``; actual `none`
- schematics head src/lib/application/files/ts/test/app.e2e-__specFileSuffix__.ts:5 `./../src/app.module`: refused-configuration; truth `src/lib/application/files/ts/src/app.module.ts`; actual `none`
- vite base packages/vite/rolldown.config.ts:7 `./package.json`: json-module; truth ``; actual `none`
- vite base packages/vite/rolldown.dts.config.ts:15 `./package.json`: json-module; truth ``; actual `none`
- vite base packages/vite/src/client/bundledDevHmrClient.ts:324 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/client/client.ts:161 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/module-runner/esmEvaluator.ts:50 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/node/__tests__/environment.spec.ts:173 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/node/__tests__/fixtures/config/entry/vite.config.import-attributes.ts:2 `vite/package.json`: json-module; truth ``; actual `none`
- vite base packages/vite/src/node/__tests__/fixtures/config/native-compat/json-named-import-bare/vite.config.js:1 `some-pkg/package.json`: json-module; truth ``; actual `none`
- vite base packages/vite/src/node/__tests__/fixtures/config/native-compat/json-named-import-ok/vite.config.js:1 `some-pkg/package.json`: json-module; truth ``; actual `none`
- vite base packages/vite/src/node/__tests__/fixtures/config/native-compat/json-named-import-ok/vite.config.js:2 `./data.json`: json-module; truth ``; actual `none`
- vite base packages/vite/src/node/__tests__/fixtures/config/native-compat/json-named-import/vite.config.js:1 `./data.json`: json-module; truth ``; actual `none`
- vite base packages/vite/src/node/__tests__/fixtures/config/native-compat/json-ok/vite.config.js:1 `./data.json`: json-module; truth ``; actual `none`
- vite base packages/vite/src/node/__tests__/fixtures/config/native-compat/json/vite.config.js:1 `./data.json`: json-module; truth ``; actual `none`
- vite base packages/vite/src/node/__tests__/resolve.spec.ts:294 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/node/__tests__/resolve.spec.ts:309 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/node/config.ts:2483 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/node/config.ts:2784 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/node/constants.ts:3 `../../package.json`: json-module; truth ``; actual `none`
- vite base packages/vite/src/node/plugins/css.ts:2555 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/node/plugins/css.ts:2608 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/node/plugins/css.ts:2715 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/node/plugins/css.ts:2966 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/node/plugins/css.ts:3079 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/node/plugins/terser.ts:49 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/node/ssr/runtime/__tests__/fixtures/cyclic/entry.js:7 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/node/ssr/runtime/__tests__/fixtures/dynamic-import.js:13 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/node/ssr/runtime/__tests__/fixtures/dynamic-import.js:14 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/node/ssr/runtime/__tests__/fixtures/dynamic-import.js:15 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/node/ssr/runtime/__tests__/fixtures/dynamic-import.js:16 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite base packages/vite/src/node/ssr/runtime/__tests__/fixtures/dynamic-import.js:19 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/rolldown.config.ts:7 `./package.json`: json-module; truth ``; actual `none`
- vite head packages/vite/rolldown.dts.config.ts:15 `./package.json`: json-module; truth ``; actual `none`
- vite head packages/vite/src/client/bundledDevHmrClient.ts:324 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/client/client.ts:161 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/module-runner/esmEvaluator.ts:50 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/node/__tests__/environment.spec.ts:173 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/node/__tests__/fixtures/config/entry/vite.config.import-attributes.ts:2 `vite/package.json`: json-module; truth ``; actual `none`
- vite head packages/vite/src/node/__tests__/fixtures/config/native-compat/json-named-import-bare/vite.config.js:1 `some-pkg/package.json`: json-module; truth ``; actual `none`
- vite head packages/vite/src/node/__tests__/fixtures/config/native-compat/json-named-import-ok/vite.config.js:1 `some-pkg/package.json`: json-module; truth ``; actual `none`
- vite head packages/vite/src/node/__tests__/fixtures/config/native-compat/json-named-import-ok/vite.config.js:2 `./data.json`: json-module; truth ``; actual `none`
- vite head packages/vite/src/node/__tests__/fixtures/config/native-compat/json-named-import/vite.config.js:1 `./data.json`: json-module; truth ``; actual `none`
- vite head packages/vite/src/node/__tests__/fixtures/config/native-compat/json-ok/vite.config.js:1 `./data.json`: json-module; truth ``; actual `none`
- vite head packages/vite/src/node/__tests__/fixtures/config/native-compat/json/vite.config.js:1 `./data.json`: json-module; truth ``; actual `none`
- vite head packages/vite/src/node/__tests__/resolve.spec.ts:294 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/node/__tests__/resolve.spec.ts:309 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/node/config.ts:2483 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/node/config.ts:2784 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/node/constants.ts:3 `../../package.json`: json-module; truth ``; actual `none`
- vite head packages/vite/src/node/plugins/css.ts:2555 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/node/plugins/css.ts:2608 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/node/plugins/css.ts:2715 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/node/plugins/css.ts:2966 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/node/plugins/css.ts:3079 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/node/plugins/terser.ts:49 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/node/ssr/runtime/__tests__/fixtures/cyclic/entry.js:7 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/node/ssr/runtime/__tests__/fixtures/dynamic-import.js:13 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/node/ssr/runtime/__tests__/fixtures/dynamic-import.js:14 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/node/ssr/runtime/__tests__/fixtures/dynamic-import.js:15 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/node/ssr/runtime/__tests__/fixtures/dynamic-import.js:16 `(non-literal)`: non-literal-import; truth ``; actual `none`
- vite head packages/vite/src/node/ssr/runtime/__tests__/fixtures/dynamic-import.js:19 `(non-literal)`: non-literal-import; truth ``; actual `none`

## Synthetic cases

- base: 21/65 matching
  - forms-cjs src/main.ts:1 `./target`: expected `src/target.ts`; actual `none`
  - forms-cjs src/main.ts:2 `./missing`: expected `unresolved:target-not-indexed`; actual `none`
  - forms-cjs src/main.ts:5 `./target`: expected `src/target.ts`; actual `none`
  - forms-esm src/barrel.ts:4 `./missing`: expected `unresolved:target-not-indexed`; actual `none`
  - forms-esm src/main.ts:4 `./target`: expected `src/target.ts`; actual `none`
  - forms-esm src/main.ts:5 `./missing`: expected `unresolved:target-not-indexed`; actual `none`
  - implied-node20 src/main.ts:2 `./target`: expected `unresolved:esm-extension-required`; actual `src/target.ts`
  - implied-preserve src/main.ts:2 `./missing`: expected `unresolved:target-not-indexed`; actual `none`
  - bundler src/main.ts:3 `./absent`: expected `unresolved:target-not-indexed`; actual `none`
  - node10 src/main.ts:3 `./absent`: expected `unresolved:target-not-indexed`; actual `none`
  - node10 src/main.ts:4 `./target.jsx`: expected `src/target.ts`; actual `none`
  - node16-cjs src/main.ts:2 `./target`: expected `src/target.ts`; actual `none`
  - node16-cjs src/main.ts:3 `./target.js`: expected `src/target.ts`; actual `none`
  - node16-cjs src/main.ts:4 `./missing.js`: expected `unresolved:target-not-indexed`; actual `none`
  - node16-esm src/main.ts:2 `./target`: expected `unresolved:esm-extension-required`; actual `src/target.ts`
  - node16-esm src/main.ts:3 `./folder`: expected `unresolved:directory-import-not-allowed`; actual `src/folder/index.ts`
  - node16-esm src/main.ts:4 `./missing.js`: expected `unresolved:target-not-indexed`; actual `none`
  - nodenext-cjs src/main.ts:3 `./missing`: expected `unresolved:target-not-indexed`; actual `none`
  - nodenext-esm src/main.ts:2 `./target`: expected `unresolved:esm-extension-required`; actual `src/target.ts`
  - workspace-duplicate apps/web.ts:1 `@sample/duplicate`: expected `unresolved:ambiguous-workspace-name`; actual `none`
  - workspace-exports apps/web.ts:1 `@sample/map`: expected `packages/map/src/index.ts`; actual `none`
  - workspace-exports apps/web.ts:2 `@sample/map/util/one`: expected `packages/map/src/util/one.ts`; actual `none`
  - workspace-exports apps/web.ts:3 `@sample/map/hidden`: expected `unresolved:no-matching-condition`; actual `none`
  - workspace-exports apps/web.ts:4 `@sample/map/missing`: expected `unresolved:exports-subpath-not-matched`; actual `none`
  - workspace-exports apps/web.ts:5 `@sample/map/array`: expected `packages/map/src/util/one.ts`; actual `none`
  - workspace-exports apps/web.ts:6 `@sample/map/folder/one`: expected `unresolved:exports-subpath-not-matched`; actual `none`
  - workspace-exports apps/web.ts:7 `@sample/map/custom`: expected `packages/map/src/util/one.ts`; actual `none`
  - workspace-exports apps/web.ts:8 `@sample/map/declaration`: expected `unresolved:declaration-only-target`; actual `none`
  - workspace-exports apps/web.ts:9 `@sample/map/unmatched`: expected `unresolved:no-matching-condition`; actual `none`
  - workspace-exports packages/map/src/self.ts:1 `#one`: expected `packages/map/src/util/one.ts`; actual `none`
  - workspace-exports packages/map/src/self.ts:2 `#absent`: expected `unresolved:imports-specifier-not-matched`; actual `none`
  - workspace-npm apps/web.ts:1 `@sample/lib`: expected `packages/lib/src/index.ts`; actual `none`
  - workspace-npm apps/web.ts:2 `@sample/lib/src/extra`: expected `packages/lib/src/extra.ts`; actual `none`
  - workspace-npm apps/web.ts:3 `@sample/other`: expected `unresolved:bare-package-outside-workspace`; actual `none`
  - workspace-npm apps/web.ts:4 `@sample/fallback`: expected `packages/fallback/index.ts`; actual `none`
  - workspace-output apps/web.ts:1 `@sample/mapped`: expected `packages/mapped/src/index.ts`; actual `none`
  - workspace-output apps/web.ts:2 `@sample/built`: expected `packages/built/src/index.ts`; actual `none`
  - workspace-output apps/web.ts:3 `@sample/unmapped`: expected `unresolved:build-output-without-source-mapping`; actual `none`
  - workspace-output apps/web.ts:4 `@sample/composite`: expected `packages/composite/index.ts`; actual `none`
  - workspace-paths apps/web.ts:2 `@sample/miss`: expected `packages/miss/index.ts`; actual `src/@sample/miss.ts`
  - workspace-pnpm apps/web.ts:1 `@sample/deep`: expected `packages/nested/deep/src/index.ts`; actual `none`
  - workspace-pnpm apps/web.ts:2 `@sample/ignored`: expected `unresolved:bare-package-outside-workspace`; actual `none`
  - workspace-yarn apps/web.ts:1 `@sample/yarn`: expected `packages/lib/src/index.ts`; actual `none`
  - workspace-yarn apps/web.ts:2 `@sample/absent`: expected `unresolved:bare-package-outside-workspace`; actual `none`
- head: 65/65 matching
