# TypeScript module resolution

Run date: 2026-10-15. Base: `f136d8ba30ad5f4d6f80f2c5c60173349abd599c`. Head: `d55fbaad4119a4945f3a0a70fd3f294cad890686`.

Truth: TypeScript 5.9.3 with declaration files excluded.

| Fixture | Mode | Arm | Agreement | Wrong target | Spurious | Gate |
| --- | --- | --- | ---: | ---: | ---: | --- |
| vite | bundler | devscout-base | 1072/1099 | 1 | 0 | missed |
| vite | bundler | devscout-head | 1087/1099 | 0 | 0 | met |
| nx | node10 | devscout-base | 2920/3020 | 0 | 0 | met |
| nx | node10 | devscout-head | 2976/3020 | 0 | 0 | met |
| pyright | node16 | devscout-base | 2768/2769 | 0 | 0 | met |
| pyright | node16 | devscout-head | 2767/2769 | 0 | 0 | met |
| schematics | nodenext | devscout-base | 147/149 | 0 | 7 | missed |
| schematics | nodenext | devscout-head | 129/149 | 0 | 0 | missed |

## Pooled modes

| Mode | Arm | Agreement | Wrong target | Spurious | Gate |
| --- | --- | ---: | ---: | ---: | --- |
| bundler | devscout-base | 1072/1099 | 1 | 0 | missed |
| bundler | devscout-head | 1087/1099 | 0 | 0 | met |
| node10 | devscout-base | 2920/3020 | 0 | 0 | met |
| node10 | devscout-head | 2976/3020 | 0 | 0 | met |
| node16 | devscout-base | 2768/2769 | 0 | 0 | met |
| node16 | devscout-head | 2767/2769 | 0 | 0 | met |
| nodenext | devscout-base | 147/149 | 0 | 7 | missed |
| nodenext | devscout-head | 129/149 | 0 | 0 | missed |

## Disagreements

- nx devscout-base packages/nx/src/adapter/ngcli-adapter.ts:87 `./compat`: miss; truth `packages/nx/src/adapter/compat.ts`; actual `none`
- nx devscout-base packages/nx/src/adapter/ngcli-adapter.ts:1081 `./compat`: miss; truth `packages/nx/src/adapter/compat.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/add/command-object.ts:44 `./add`: miss; truth `packages/nx/src/command-line/add/add.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/affected/command-object.ts:47 `./affected`: miss; truth `packages/nx/src/command-line/affected/affected.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/affected/command-object.ts:73 `./affected`: miss; truth `packages/nx/src/command-line/affected/affected.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/affected/command-object.ts:99 `./affected`: miss; truth `packages/nx/src/command-line/affected/affected.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/affected/command-object.ts:125 `./affected`: miss; truth `packages/nx/src/command-line/affected/affected.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/affected/command-object.ts:151 `./affected`: miss; truth `packages/nx/src/command-line/affected/affected.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/configure-ai-agents/command-object.ts:69 `./configure-ai-agents`: miss; truth `packages/nx/src/command-line/configure-ai-agents/configure-ai-agents.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/daemon/command-object.ts:17 `./daemon`: miss; truth `packages/nx/src/command-line/daemon/daemon.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/daemon/daemon.ts:8 `../../daemon/client/client`: miss; truth `packages/nx/src/daemon/client/client.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/daemon/daemon.ts:19 `../../daemon/client/client`: miss; truth `packages/nx/src/daemon/client/client.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/exec/command-object.ts:14 `./exec`: miss; truth `packages/nx/src/command-line/exec/exec.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/format/command-object.ts:11 `./format`: miss; truth `packages/nx/src/command-line/format/format.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/format/command-object.ts:23 `./format`: miss; truth `packages/nx/src/command-line/format/format.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/generate/command-object.ts:14 `./generate`: miss; truth `packages/nx/src/command-line/generate/generate.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/generate/generate.ts:410 `../../adapter/compat`: miss; truth `packages/nx/src/adapter/compat.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/generate/generate.ts:411 `../../adapter/ngcli-adapter`: miss; truth `packages/nx/src/adapter/ngcli-adapter.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/graph/command-object.ts:28 `./graph`: miss; truth `packages/nx/src/command-line/graph/graph.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/import/command-object.ts:50 `./import`: miss; truth `packages/nx/src/command-line/import/import.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/init/command-object.ts:26 `./init-v2`: miss; truth `packages/nx/src/command-line/init/init-v2.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/init/command-object.ts:28 `./init-v1`: miss; truth `packages/nx/src/command-line/init/init-v1.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/init/command-object.ts:42 `../../config/nx-json`: miss; truth `packages/nx/src/config/nx-json.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/init/implementation/react/index.ts:96 `./add-vite-commands-to-package-scripts`: miss; truth `packages/nx/src/command-line/init/implementation/react/add-vite-commands-to-package-scripts.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/list/command-object.ts:18 `./list`: miss; truth `packages/nx/src/command-line/list/list.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/mcp/command-object.ts:9 `./mcp`: miss; truth `packages/nx/src/command-line/mcp/mcp.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/mcp/command-object.ts:24 `./mcp`: miss; truth `packages/nx/src/command-line/mcp/mcp.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/migrate/command-object.ts:13 `./migrate`: miss; truth `packages/nx/src/command-line/migrate/migrate.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/migrate/command-object.ts:25 `./migrate`: miss; truth `packages/nx/src/command-line/migrate/migrate.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/migrate/migrate.ts:2056 `../../adapter/ngcli-adapter`: miss; truth `packages/nx/src/adapter/ngcli-adapter.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/migrate/migrate.ts:2057 `../../adapter/compat`: miss; truth `packages/nx/src/adapter/compat.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/new/command-object.ts:11 `./new`: miss; truth `packages/nx/src/command-line/new/new.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/nx-cloud/apply-locally/command-object.ts:15 `./apply-locally`: miss; truth `packages/nx/src/command-line/nx-cloud/apply-locally/apply-locally.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/nx-cloud/complete-run/command-object.ts:16 `./stop-all-agents`: miss; truth `packages/nx/src/command-line/nx-cloud/complete-run/stop-all-agents.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/nx-cloud/connect/command-object.ts:15 `./connect-to-nx-cloud`: miss; truth `packages/nx/src/command-line/nx-cloud/connect/connect-to-nx-cloud.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/nx-cloud/connect/command-object.ts:18 `../../../utils/ab-testing`: miss; truth `packages/nx/src/utils/ab-testing.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/nx-cloud/connect/command-object.ts:41 `./view-logs`: miss; truth `packages/nx/src/command-line/nx-cloud/connect/view-logs.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/nx-cloud/download-cloud-client/command-object.ts:11 `./download-cloud-client`: miss; truth `packages/nx/src/command-line/nx-cloud/download-cloud-client/download-cloud-client.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/nx-cloud/fix-ci/command-object.ts:14 `./fix-ci`: miss; truth `packages/nx/src/command-line/nx-cloud/fix-ci/fix-ci.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/nx-cloud/login/command-object.ts:20 `./login`: miss; truth `packages/nx/src/command-line/nx-cloud/login/login.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/nx-cloud/logout/command-object.ts:14 `./logout`: miss; truth `packages/nx/src/command-line/nx-cloud/logout/logout.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/nx-cloud/record/command-object.ts:17 `./record`: miss; truth `packages/nx/src/command-line/nx-cloud/record/record.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/nx-cloud/start-agent/command-object.ts:14 `./start-agent`: miss; truth `packages/nx/src/command-line/nx-cloud/start-agent/start-agent.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/nx-cloud/start-ci-run/command-object.ts:15 `./start-ci-run`: miss; truth `packages/nx/src/command-line/nx-cloud/start-ci-run/start-ci-run.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/register/command-object.ts:27 `./register`: miss; truth `packages/nx/src/command-line/register/register.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/release/command-object.ts:180 `../../config/nx-json`: miss; truth `packages/nx/src/config/nx-json.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/release/command-object.ts:239 `./release`: miss; truth `packages/nx/src/command-line/release/release.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/release/command-object.ts:279 `./version`: miss; truth `packages/nx/src/command-line/release/version.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/release/command-object.ts:347 `./changelog`: miss; truth `packages/nx/src/command-line/release/changelog.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/release/command-object.ts:387 `./publish`: miss; truth `packages/nx/src/command-line/release/publish.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/release/command-object.ts:429 `./plan`: miss; truth `packages/nx/src/command-line/release/plan.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/release/command-object.ts:445 `./plan-check`: miss; truth `packages/nx/src/command-line/release/plan-check.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/release/utils/git.spec.ts:475 `./exec-command`: miss; truth `packages/nx/src/command-line/release/utils/exec-command.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/release/utils/remote-release-clients/remote-release-client.ts:253 `./github`: miss; truth `packages/nx/src/command-line/release/utils/remote-release-clients/github.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/release/utils/remote-release-clients/remote-release-client.ts:271 `./gitlab`: miss; truth `packages/nx/src/command-line/release/utils/remote-release-clients/gitlab.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/release/version/release-group-processor.ts:290 `@nx/docker/release/version-utils`: miss; truth `packages/docker/src/release/version-utils.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/repair/command-object.ts:19 `./repair`: miss; truth `packages/nx/src/command-line/repair/repair.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/report/command-object.ts:8 `./report`: miss; truth `packages/nx/src/command-line/report/report.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/report/report.ts:603 `../../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-base packages/nx/src/command-line/reset/command-object.ts:40 `./reset`: miss; truth `packages/nx/src/command-line/reset/reset.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/run-many/command-object.ts:31 `./run-many`: miss; truth `packages/nx/src/command-line/run-many/run-many.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/run/command-object.ts:24 `./run-one`: miss; truth `packages/nx/src/command-line/run/run-one.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/run/command-object.ts:49 `./run-one`: miss; truth `packages/nx/src/command-line/run/run-one.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/run/run-one.ts:63 `./run`: miss; truth `packages/nx/src/command-line/run/run.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/run/run.ts:211 `../../adapter/compat`: miss; truth `packages/nx/src/adapter/compat.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/run/run.ts:213 `../../adapter/ngcli-adapter`: miss; truth `packages/nx/src/adapter/ngcli-adapter.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/run/run.ts:225 `../../adapter/rxjs-for-await`: miss; truth `packages/nx/src/adapter/rxjs-for-await.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/show/command-object.ts:128 `./projects`: miss; truth `packages/nx/src/command-line/show/projects.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/show/command-object.ts:185 `./project`: miss; truth `packages/nx/src/command-line/show/project.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/show/project.spec.ts:147 `../../utils/output`: miss; truth `packages/nx/src/utils/output.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/sync/command-object.ts:16 `./sync`: miss; truth `packages/nx/src/command-line/sync/sync.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/sync/command-object.ts:30 `./sync`: miss; truth `packages/nx/src/command-line/sync/sync.ts`; actual `none`
- nx devscout-base packages/nx/src/command-line/watch/command-object.ts:11 `./watch`: miss; truth `packages/nx/src/command-line/watch/watch.ts`; actual `none`
- nx devscout-base packages/nx/src/daemon/client/client.ts:1220 `../../tasks-runner/process-metrics-service`: miss; truth `packages/nx/src/tasks-runner/process-metrics-service.ts`; actual `none`
- nx devscout-base packages/nx/src/daemon/server/watcher.ts:16 `../../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-base packages/nx/src/daemon/server/watcher.ts:46 `../../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-base packages/nx/src/generators/utils/generate-files.spec.ts:2 `nx/src/generators/testing-utils/create-tree`: miss; truth `packages/nx/src/generators/testing-utils/create-tree.ts`; actual `none`
- nx devscout-base packages/nx/src/generators/utils/generate-files.spec.ts:3 `nx/src/generators/tree`: miss; truth `packages/nx/src/generators/tree.ts`; actual `none`
- nx devscout-base packages/nx/src/hasher/file-hasher.ts:3 `../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-base packages/nx/src/hasher/file-hasher.ts:8 `../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-base packages/nx/src/hasher/file-hasher.ts:20 `../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-base packages/nx/src/native/assert-supported-platform.ts:5 `./index.js`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-base packages/nx/src/native/native-bindings.js:336 `./nx.wasi.cjs`: miss; truth `packages/nx/src/native/nx.wasi.cjs`; actual `none`
- nx devscout-base packages/nx/src/plugins/js/project-graph/build-dependencies/explicit-project-dependencies.ts:73 `../../../../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-base packages/nx/src/plugins/js/project-graph/build-dependencies/target-project-locator.spec.ts:600 `nx/src/plugins/js/utils/typescript`: miss; truth `packages/nx/src/plugins/js/utils/typescript.ts`; actual `none`
- nx devscout-base packages/nx/src/plugins/js/project-graph/build-dependencies/target-project-locator.spec.ts:638 `nx/src/plugins/js/utils/typescript`: miss; truth `packages/nx/src/plugins/js/utils/typescript.ts`; actual `none`
- nx devscout-base packages/nx/src/project-graph/plugins/in-process-loader.ts:85 `./load-resolved-plugin`: miss; truth `packages/nx/src/project-graph/plugins/load-resolved-plugin.ts`; actual `none`
- nx devscout-base packages/nx/src/project-graph/plugins/isolation/isolated-plugin.ts:467 `../../../daemon/is-on-daemon`: miss; truth `packages/nx/src/daemon/is-on-daemon.ts`; actual `none`
- nx devscout-base packages/nx/src/project-graph/plugins/isolation/isolated-plugin.ts:469 `../../../tasks-runner/process-metrics-service`: miss; truth `packages/nx/src/tasks-runner/process-metrics-service.ts`; actual `none`
- nx devscout-base packages/nx/src/project-graph/plugins/isolation/plugin-worker.ts:22 `../../../utils/perf-logging`: miss; truth `packages/nx/src/utils/perf-logging.ts`; actual `none`
- nx devscout-base packages/nx/src/project-graph/plugins/isolation/plugin-worker.ts:78 `../load-resolved-plugin`: miss; truth `packages/nx/src/project-graph/plugins/load-resolved-plugin.ts`; actual `none`
- nx devscout-base packages/nx/src/project-graph/plugins/isolation/plugin-worker.ts:86 `../transpiler`: miss; truth `packages/nx/src/project-graph/plugins/transpiler.ts`; actual `none`
- nx devscout-base packages/nx/src/tasks-runner/cache.ts:499 `../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-base packages/nx/src/tasks-runner/cache.ts:513 `../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-base packages/nx/src/tasks-runner/cache.ts:530 `../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-base packages/nx/src/tasks-runner/run-command.ts:134 `../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-base packages/nx/src/tasks-runner/run-command.ts:1132 `../nx-cloud/nx-cloud-tasks-runner-shell`: miss; truth `packages/nx/src/nx-cloud/nx-cloud-tasks-runner-shell.ts`; actual `none`
- nx devscout-base packages/nx/src/utils/handle-errors.ts:69 `../daemon/client/client`: miss; truth `packages/nx/src/daemon/client/client.ts`; actual `none`
- nx devscout-base packages/nx/src/utils/workspace-context.ts:11 `../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-base packages/nx/src/utils/workspace-context.ts:115 `../daemon/server/project-graph-incremental-recomputation`: miss; truth `packages/nx/src/daemon/server/project-graph-incremental-recomputation.ts`; actual `none`
- nx devscout-head packages/nx/src/adapter/ngcli-adapter.ts:87 `./compat`: miss; truth `packages/nx/src/adapter/compat.ts`; actual `none`
- nx devscout-head packages/nx/src/adapter/ngcli-adapter.ts:1081 `./compat`: miss; truth `packages/nx/src/adapter/compat.ts`; actual `none`
- nx devscout-head packages/nx/src/command-line/generate/generate.ts:410 `../../adapter/compat`: miss; truth `packages/nx/src/adapter/compat.ts`; actual `none`
- nx devscout-head packages/nx/src/command-line/init/command-object.ts:26 `./init-v2`: miss; truth `packages/nx/src/command-line/init/init-v2.ts`; actual `none`
- nx devscout-head packages/nx/src/command-line/init/command-object.ts:28 `./init-v1`: miss; truth `packages/nx/src/command-line/init/init-v1.ts`; actual `none`
- nx devscout-head packages/nx/src/command-line/mcp/command-object.ts:9 `./mcp`: miss; truth `packages/nx/src/command-line/mcp/mcp.ts`; actual `none`
- nx devscout-head packages/nx/src/command-line/migrate/migrate.ts:2057 `../../adapter/compat`: miss; truth `packages/nx/src/adapter/compat.ts`; actual `none`
- nx devscout-head packages/nx/src/command-line/release/utils/git.spec.ts:475 `./exec-command`: miss; truth `packages/nx/src/command-line/release/utils/exec-command.ts`; actual `none`
- nx devscout-head packages/nx/src/command-line/release/version.ts:9 `../../executors/run-commands/run-commands.impl`: miss; truth `packages/nx/src/executors/run-commands/run-commands.impl.ts`; actual `none`
- nx devscout-head packages/nx/src/command-line/release/version/release-group-processor.ts:290 `@nx/docker/release/version-utils`: miss; truth `packages/docker/src/release/version-utils.ts`; actual `none`
- nx devscout-head packages/nx/src/command-line/report/report.ts:603 `../../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-head packages/nx/src/command-line/run/run.ts:211 `../../adapter/compat`: miss; truth `packages/nx/src/adapter/compat.ts`; actual `none`
- nx devscout-head packages/nx/src/command-line/show/project.spec.ts:147 `../../utils/output`: miss; truth `packages/nx/src/utils/output.ts`; actual `none`
- nx devscout-head packages/nx/src/executors/run-commands/run-commands.impl.spec.ts:5 `./run-commands.impl`: miss; truth `packages/nx/src/executors/run-commands/run-commands.impl.ts`; actual `none`
- nx devscout-head packages/nx/src/executors/run-commands/running-tasks.ts:19 `./run-commands.impl`: miss; truth `packages/nx/src/executors/run-commands/run-commands.impl.ts`; actual `none`
- nx devscout-head packages/nx/src/hasher/file-hasher.ts:3 `../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-head packages/nx/src/hasher/file-hasher.ts:8 `../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-head packages/nx/src/hasher/file-hasher.ts:20 `../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-head packages/nx/src/native/assert-supported-platform.ts:5 `./index.js`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-head packages/nx/src/native/native-bindings.js:336 `./nx.wasi.cjs`: miss; truth `packages/nx/src/native/nx.wasi.cjs`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/lock-file/bun-parser.spec.ts:3 `./__fixtures__/bun/alias-dependencies.bun.lock`: miss; truth `packages/nx/src/plugins/js/lock-file/__fixtures__/bun/alias-dependencies.bun.lock.ts`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/lock-file/bun-parser.spec.ts:4 `./__fixtures__/bun/auxiliary-packages.bun.lock`: miss; truth `packages/nx/src/plugins/js/lock-file/__fixtures__/bun/auxiliary-packages.bun.lock.ts`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/lock-file/bun-parser.spec.ts:5 `./__fixtures__/bun/basic-dependencies.bun.lock`: miss; truth `packages/nx/src/plugins/js/lock-file/__fixtures__/bun/basic-dependencies.bun.lock.ts`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/lock-file/bun-parser.spec.ts:6 `./__fixtures__/bun/complex-scenarios.bun.lock`: miss; truth `packages/nx/src/plugins/js/lock-file/__fixtures__/bun/complex-scenarios.bun.lock.ts`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/lock-file/bun-parser.spec.ts:7 `./__fixtures__/bun/enhanced-features.bun.lock`: miss; truth `packages/nx/src/plugins/js/lock-file/__fixtures__/bun/enhanced-features.bun.lock.ts`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/lock-file/bun-parser.spec.ts:8 `./__fixtures__/bun/file-dependencies.bun.lock`: miss; truth `packages/nx/src/plugins/js/lock-file/__fixtures__/bun/file-dependencies.bun.lock.ts`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/lock-file/bun-parser.spec.ts:9 `./__fixtures__/bun/git-dependencies.bun.lock`: miss; truth `packages/nx/src/plugins/js/lock-file/__fixtures__/bun/git-dependencies.bun.lock.ts`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/lock-file/bun-parser.spec.ts:10 `./__fixtures__/bun/hoisted-and-nested.bun.lock`: miss; truth `packages/nx/src/plugins/js/lock-file/__fixtures__/bun/hoisted-and-nested.bun.lock.ts`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/lock-file/bun-parser.spec.ts:11 `./__fixtures__/bun/large-project.bun.lock`: miss; truth `packages/nx/src/plugins/js/lock-file/__fixtures__/bun/large-project.bun.lock.ts`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/lock-file/bun-parser.spec.ts:12 `./__fixtures__/bun/nextjs-app.bun.lock`: miss; truth `packages/nx/src/plugins/js/lock-file/__fixtures__/bun/nextjs-app.bun.lock.ts`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/lock-file/bun-parser.spec.ts:13 `./__fixtures__/bun/peer-dependencies-meta.bun.lock`: miss; truth `packages/nx/src/plugins/js/lock-file/__fixtures__/bun/peer-dependencies-meta.bun.lock.ts`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/lock-file/bun-parser.spec.ts:14 `./__fixtures__/bun/scoped-packages.bun.lock`: miss; truth `packages/nx/src/plugins/js/lock-file/__fixtures__/bun/scoped-packages.bun.lock.ts`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/lock-file/bun-parser.spec.ts:15 `./__fixtures__/bun/tarball-dependencies.bun.lock`: miss; truth `packages/nx/src/plugins/js/lock-file/__fixtures__/bun/tarball-dependencies.bun.lock.ts`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/lock-file/bun-parser.spec.ts:16 `./__fixtures__/bun/version-resolution.bun.lock`: miss; truth `packages/nx/src/plugins/js/lock-file/__fixtures__/bun/version-resolution.bun.lock.ts`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/lock-file/bun-parser.spec.ts:17 `./__fixtures__/bun/workspace-dependencies.bun.lock`: miss; truth `packages/nx/src/plugins/js/lock-file/__fixtures__/bun/workspace-dependencies.bun.lock.ts`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/project-graph/build-dependencies/explicit-project-dependencies.ts:73 `../../../../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/project-graph/build-dependencies/target-project-locator.spec.ts:600 `nx/src/plugins/js/utils/typescript`: miss; truth `packages/nx/src/plugins/js/utils/typescript.ts`; actual `none`
- nx devscout-head packages/nx/src/plugins/js/project-graph/build-dependencies/target-project-locator.spec.ts:638 `nx/src/plugins/js/utils/typescript`: miss; truth `packages/nx/src/plugins/js/utils/typescript.ts`; actual `none`
- nx devscout-head packages/nx/src/project-graph/plugins/isolation/plugin-worker.ts:22 `../../../utils/perf-logging`: miss; truth `packages/nx/src/utils/perf-logging.ts`; actual `none`
- nx devscout-head packages/nx/src/tasks-runner/cache.ts:499 `../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-head packages/nx/src/tasks-runner/cache.ts:513 `../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-head packages/nx/src/tasks-runner/cache.ts:530 `../native`: miss; truth `packages/nx/src/native/index.js`; actual `none`
- nx devscout-head packages/nx/src/tasks-runner/run-command.ts:1132 `../nx-cloud/nx-cloud-tasks-runner-shell`: miss; truth `packages/nx/src/nx-cloud/nx-cloud-tasks-runner-shell.ts`; actual `none`
- nx devscout-head packages/nx/src/tasks-runner/task-orchestrator.ts:9 `../executors/run-commands/run-commands.impl`: miss; truth `packages/nx/src/executors/run-commands/run-commands.impl.ts`; actual `none`
- pyright devscout-base packages/pyright-internal/debug.js:2 `./out/packages/pyright-internal/src/pyright`: miss; truth `packages/pyright-internal/src/pyright.ts`; actual `none`
- pyright devscout-head packages/pyright-internal/debug.js:2 `./out/packages/pyright-internal/src/pyright`: miss; truth `packages/pyright-internal/src/pyright.ts`; actual `none`
- pyright devscout-head packages/pyright-internal/src/tests/harness/fourslash/runner.ts:16 `./testState.Consts`: miss; truth `packages/pyright-internal/src/tests/harness/fourslash/testState.Consts.ts`; actual `none`
- schematics devscout-base src/lib/application/files/js/index.js:2 `./src/main`: miss; truth `src/lib/application/files/js/src/main.js`; actual `none`
- schematics devscout-base src/lib/sub-app/files/js/index.js:2 `./src/main`: miss; truth `src/lib/sub-app/files/js/src/main.js`; actual `none`
- schematics devscout-base src/lib/sub-app/files/js/src/app.controller.__specFileSuffix__.js:2 `./app.controller`: spurious; truth ``; actual `src/lib/sub-app/files/js/src/app.controller.js`
- schematics devscout-base src/lib/sub-app/files/js/src/app.controller.__specFileSuffix__.js:3 `./app.service`: spurious; truth ``; actual `src/lib/sub-app/files/js/src/app.service.js`
- schematics devscout-base src/lib/sub-app/files/js/src/app.controller.js:2 `./app.service`: spurious; truth ``; actual `src/lib/sub-app/files/js/src/app.service.js`
- schematics devscout-base src/lib/sub-app/files/js/src/app.module.js:2 `./app.controller`: spurious; truth ``; actual `src/lib/sub-app/files/js/src/app.controller.js`
- schematics devscout-base src/lib/sub-app/files/js/src/app.module.js:3 `./app.service`: spurious; truth ``; actual `src/lib/sub-app/files/js/src/app.service.js`
- schematics devscout-base src/lib/sub-app/files/js/src/main.js:2 `./app.module`: spurious; truth ``; actual `src/lib/sub-app/files/js/src/app.module.js`
- schematics devscout-base src/lib/sub-app/files/js/test/app.e2e-__specFileSuffix__.js:3 `../src/app.module`: spurious; truth ``; actual `src/lib/sub-app/files/js/src/app.module.js`
- schematics devscout-head src/lib/application/files/js/index.js:2 `./src/main`: miss; truth `src/lib/application/files/js/src/main.js`; actual `none`
- schematics devscout-head src/lib/application/files/js/src/app.controller.__specFileSuffix__.js:2 `./app.controller`: miss; truth `src/lib/application/files/js/src/app.controller.js`; actual `none`
- schematics devscout-head src/lib/application/files/js/src/app.controller.__specFileSuffix__.js:3 `./app.service`: miss; truth `src/lib/application/files/js/src/app.service.js`; actual `none`
- schematics devscout-head src/lib/application/files/js/src/app.controller.js:2 `./app.service`: miss; truth `src/lib/application/files/js/src/app.service.js`; actual `none`
- schematics devscout-head src/lib/application/files/js/src/app.module.js:3 `./app.controller`: miss; truth `src/lib/application/files/js/src/app.controller.js`; actual `none`
- schematics devscout-head src/lib/application/files/js/src/app.module.js:4 `./app.service`: miss; truth `src/lib/application/files/js/src/app.service.js`; actual `none`
- schematics devscout-head src/lib/application/files/js/test/app.e2e-__specFileSuffix__.js:3 `../src/app.module`: miss; truth `src/lib/application/files/js/src/app.module.js`; actual `none`
- schematics devscout-head src/lib/application/files/ts-esm/src/app.controller.__specFileSuffix__.ts:2 `./app.controller.js`: miss; truth `src/lib/application/files/ts-esm/src/app.controller.ts`; actual `none`
- schematics devscout-head src/lib/application/files/ts-esm/src/app.controller.__specFileSuffix__.ts:3 `./app.service.js`: miss; truth `src/lib/application/files/ts-esm/src/app.service.ts`; actual `none`
- schematics devscout-head src/lib/application/files/ts-esm/src/app.controller.ts:2 `./app.service.js`: miss; truth `src/lib/application/files/ts-esm/src/app.service.ts`; actual `none`
- schematics devscout-head src/lib/application/files/ts-esm/src/app.module.ts:3 `./app.controller.js`: miss; truth `src/lib/application/files/ts-esm/src/app.controller.ts`; actual `none`
- schematics devscout-head src/lib/application/files/ts-esm/src/app.module.ts:4 `./app.service.js`: miss; truth `src/lib/application/files/ts-esm/src/app.service.ts`; actual `none`
- schematics devscout-head src/lib/application/files/ts-esm/test/app.e2e-__specFileSuffix__.ts:5 `./../src/app.module.js`: miss; truth `src/lib/application/files/ts-esm/src/app.module.ts`; actual `none`
- schematics devscout-head src/lib/application/files/ts/src/app.controller.__specFileSuffix__.ts:2 `./app.controller`: miss; truth `src/lib/application/files/ts/src/app.controller.ts`; actual `none`
- schematics devscout-head src/lib/application/files/ts/src/app.controller.__specFileSuffix__.ts:3 `./app.service`: miss; truth `src/lib/application/files/ts/src/app.service.ts`; actual `none`
- schematics devscout-head src/lib/application/files/ts/src/app.controller.ts:2 `./app.service`: miss; truth `src/lib/application/files/ts/src/app.service.ts`; actual `none`
- schematics devscout-head src/lib/application/files/ts/src/app.module.ts:3 `./app.controller`: miss; truth `src/lib/application/files/ts/src/app.controller.ts`; actual `none`
- schematics devscout-head src/lib/application/files/ts/src/app.module.ts:4 `./app.service`: miss; truth `src/lib/application/files/ts/src/app.service.ts`; actual `none`
- schematics devscout-head src/lib/application/files/ts/test/app.e2e-__specFileSuffix__.ts:5 `./../src/app.module`: miss; truth `src/lib/application/files/ts/src/app.module.ts`; actual `none`
- schematics devscout-head src/lib/sub-app/files/js/index.js:2 `./src/main`: miss; truth `src/lib/sub-app/files/js/src/main.js`; actual `none`
- vite devscout-base packages/vite/src/node/__tests__/fixtures/config/entry/vite.config.ts:1 `@vite/test-config-plugin-module-condition`: miss; truth `packages/vite/src/node/__tests__/fixtures/config/plugin-module-condition/index.mjs`; actual `none`
- vite devscout-base packages/vite/src/node/__tests__/fixtures/config/entry/vite.config.ts:2 `#imports-field`: miss; truth `packages/vite/src/node/__tests__/fixtures/config/entry/imports-field.ts`; actual `none`
- vite devscout-base packages/vite/src/node/__tests__/fixtures/dynamic-import/entry.mjs:2 `./dep.mjs`: miss; truth `packages/vite/src/node/__tests__/fixtures/dynamic-import/dep.mjs`; actual `none`
- vite devscout-base packages/vite/src/node/__tests__/fixtures/runner-import/dynamic-import.ts:1 `./dynamic-import-dep`: miss; truth `packages/vite/src/node/__tests__/fixtures/runner-import/dynamic-import-dep.ts`; actual `none`
- vite devscout-base packages/vite/src/node/__tests__/fixtures/runner-import/vite.config.outside-pkg-import.mts:1 `@vitejs/parent`: miss; truth `packages/vite/src/node/__tests__/packages/parent/index.ts`; actual `none`
- vite devscout-base packages/vite/src/node/__tests__/fixtures/shared-plugins/chunk-import-map/entry.js:1 `./lazy.js`: miss; truth `packages/vite/src/node/__tests__/fixtures/shared-plugins/chunk-import-map/lazy.js`; actual `none`
- vite devscout-base packages/vite/src/node/__tests__/fixtures/test-dep-conditions-app/entry-with-module.js:1 `@vitejs/test-dep-conditions/with-module`: miss; truth `packages/vite/src/node/__tests__/fixtures/test-dep-conditions/dir/index.default.js`; actual `none`
- vite devscout-base packages/vite/src/node/__tests__/fixtures/test-dep-conditions-app/entry.js:1 `@vitejs/test-dep-conditions`: miss; truth `packages/vite/src/node/__tests__/fixtures/test-dep-conditions/index.default.js`; actual `none`
- vite devscout-base packages/vite/src/node/__tests__/fixtures/watch-rebuild-manifest/entry.js:2 `./dep.js`: miss; truth `packages/vite/src/node/__tests__/fixtures/watch-rebuild-manifest/dep.js`; actual `none`
- vite devscout-base packages/vite/src/node/__tests__/fixtures/worker-dynamic/worker.js:2 `./dynamic`: miss; truth `packages/vite/src/node/__tests__/fixtures/worker-dynamic/dynamic.js`; actual `none`
- vite devscout-base packages/vite/src/node/__tests__/packages/parent/index.ts:2 `@vitejs/child`: miss; truth `packages/vite/src/node/__tests__/packages/child/index.js`; actual `none`
- vite devscout-base packages/vite/src/node/__tests__/plugins/fixtures/license/dep-license-mit/index.js:1 `@vitejs/test-dep-nested-license-isc`: miss; truth `packages/vite/src/node/__tests__/plugins/fixtures/license/dep-nested-license-isc/index.js`; actual `none`
- vite devscout-base packages/vite/src/node/cli.ts:227 `./server`: miss; truth `packages/vite/src/node/server/index.ts`; actual `none`
- vite devscout-base packages/vite/src/node/cli.ts:408 `./config`: miss; truth `packages/vite/src/node/config.ts`; actual `none`
- vite devscout-base packages/vite/src/node/cli.ts:409 `./optimizer`: miss; truth `packages/vite/src/node/optimizer/index.ts`; actual `none`
- vite devscout-base packages/vite/src/node/cli.ts:453 `./preview`: miss; truth `packages/vite/src/node/preview.ts`; actual `none`
- vite devscout-base packages/vite/src/node/nodeResolve.ts:1 `#module-sync-enabled`: miss; truth `packages/vite/misc/false.js`; actual `none`
- vite devscout-base packages/vite/src/node/ssr/__tests__/fixtures/errors/syntax-error-dep.js:1 `./syntax-error.js`: wrong-target; truth `packages/vite/src/node/ssr/__tests__/fixtures/errors/syntax-error.ts`; actual `packages/vite/src/node/ssr/__tests__/fixtures/errors/syntax-error.js`
- vite devscout-base packages/vite/src/node/ssr/runnerImport.ts:19 `#module-sync-enabled`: miss; truth `packages/vite/misc/false.js`; actual `none`
- vite devscout-base packages/vite/src/node/ssr/runtime/__tests__/fixtures/cjs-external-existing.js:1 `@vitejs/cjs-external`: miss; truth `packages/vite/src/node/ssr/runtime/__tests__/fixtures/cjs-external/index.cjs`; actual `none`
- vite devscout-base packages/vite/src/node/ssr/runtime/__tests__/fixtures/cjs-external-non-existing.js:1 `@vitejs/cjs-external`: miss; truth `packages/vite/src/node/ssr/runtime/__tests__/fixtures/cjs-external/index.cjs`; actual `none`
- vite devscout-base packages/vite/src/node/ssr/runtime/__tests__/fixtures/cyclic/entry-cyclic.js:2 `./entry.js`: miss; truth `packages/vite/src/node/ssr/runtime/__tests__/fixtures/cyclic/entry.js`; actual `none`
- vite devscout-base packages/vite/src/node/ssr/runtime/__tests__/fixtures/cyclic/entry.js:2 `./entry-cyclic.js`: miss; truth `packages/vite/src/node/ssr/runtime/__tests__/fixtures/cyclic/entry-cyclic.js`; actual `none`
- vite devscout-base packages/vite/src/node/ssr/runtime/__tests__/fixtures/dynamic-import.js:12 `./simple`: miss; truth `packages/vite/src/node/ssr/runtime/__tests__/fixtures/simple.js`; actual `none`
- vite devscout-base packages/vite/src/node/ssr/runtime/__tests__/fixtures/esm-external-existing.js:1 `@vitejs/esm-external`: miss; truth `packages/vite/src/node/ssr/runtime/__tests__/fixtures/esm-external/index.mjs`; actual `none`
- vite devscout-base packages/vite/src/node/ssr/runtime/__tests__/fixtures/esm-external-non-existing.js:1 `@vitejs/esm-external`: miss; truth `packages/vite/src/node/ssr/runtime/__tests__/fixtures/esm-external/index.mjs`; actual `none`
- vite devscout-base packages/vite/src/node/ssr/runtime/__tests__/fixtures/hmr-evaluated-import-race/evaluated.js:5 `./shared.js`: miss; truth `packages/vite/src/node/ssr/runtime/__tests__/fixtures/hmr-evaluated-import-race/shared.js`; actual `none`
- vite devscout-head packages/vite/src/node/index.ts:249 `vite/module-runner`: miss; truth `packages/vite/src/module-runner/index.ts`; actual `none`
- vite devscout-head packages/vite/src/node/server/environment.ts:3 `vite/module-runner`: miss; truth `packages/vite/src/module-runner/index.ts`; actual `none`
- vite devscout-head packages/vite/src/node/server/environments/runnableEnvironment.ts:1 `vite/module-runner`: miss; truth `packages/vite/src/module-runner/index.ts`; actual `none`
- vite devscout-head packages/vite/src/node/server/index.ts:18 `vite/module-runner`: miss; truth `packages/vite/src/module-runner/index.ts`; actual `none`
- vite devscout-head packages/vite/src/node/ssr/fetchModule.ts:2 `vite/module-runner`: miss; truth `packages/vite/src/module-runner/index.ts`; actual `none`
- vite devscout-head packages/vite/src/node/ssr/runtime/__tests__/fixtures/worker.invoke.mjs:5 `vite/module-runner`: miss; truth `packages/vite/src/module-runner/index.ts`; actual `none`
- vite devscout-head packages/vite/src/node/ssr/runtime/__tests__/fixtures/worker.mjs:4 `vite/module-runner`: miss; truth `packages/vite/src/module-runner/index.ts`; actual `none`
- vite devscout-head packages/vite/src/node/ssr/runtime/__tests__/utils.ts:4 `vite/module-runner`: miss; truth `packages/vite/src/module-runner/index.ts`; actual `none`
- vite devscout-head packages/vite/src/node/ssr/runtime/serverModuleRunner.ts:2 `vite/module-runner`: miss; truth `packages/vite/src/module-runner/index.ts`; actual `none`
- vite devscout-head packages/vite/src/node/ssr/runtime/serverModuleRunner.ts:3 `vite/module-runner`: miss; truth `packages/vite/src/module-runner/index.ts`; actual `none`
- vite devscout-head packages/vite/src/node/ssr/ssrModuleLoader.ts:2 `vite/module-runner`: miss; truth `packages/vite/src/module-runner/index.ts`; actual `none`
- vite devscout-head packages/vite/src/node/ssr/ssrModuleLoader.ts:3 `vite/module-runner`: miss; truth `packages/vite/src/module-runner/index.ts`; actual `none`
