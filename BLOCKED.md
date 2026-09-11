# BLOCKED（回退时撤销文件改动任务）

## 置顶：任务 0 基线与任务书转述不符（不阻塞本任务）

任务书「现状与任务 0」称 2026-09-11 实测 `npm test` 为 14047 passed / 11 failed / 7 skipped，
红集合为 persistence-schema-tripwire(4)、persistence-store-registry(3)、open-boundary-lint(2)、
persistence-startup-receipt(1)、model-slice.test.ts(文件级)。
本机复核实测（分支 `feat/tool-activity-presentation`，HEAD `0a098595bc9ef30b9d7d8aabce2a184e43d9abe0`）：

- `npm test` → **14050 passed / 12 failed / 7 skipped**（1389 文件：5 failed | 1383 passed | 1 skipped），exit 1。
- 12 红：`model-observability-e2e-chat`（S1+S2，2）、`open-boundary-lint`（2）、
  `persistence-schema-tripwire`（4）、`persistence-startup-receipt`（1）、`persistence-store-registry`（3）。
- `npm run typecheck` → exit 0。
- 差异：`model-slice.test.ts` 已不在红集合；新增 `model-observability-e2e-chat` 2 项；总红 12 而非 11。

处理：按任务规「对不上→BLOCKED.md 置顶，只做不受影响部分」。本任务改动的文件
（core/ 快照与 turn actions、server/routes/sessions.ts、lib/checkpoint-*、desktop chat/settings/locales）
与上述 5 个红文件无交集，故继续执行；收尾以本实测集合为基线，要求失败集合只减不增。

## 其它阻塞

无。

## 越界说明（已按守卫自带 repin 处理，非阻塞）

- `build/cli-runtime-closure.json` 不在任务白名单内，但它是 `tests/cli-closure-census.test.ts`
  自带的「in-place 重写」守卫产物。新增 `core/workspace-snapshots.ts` 后，其
  `matches the committed deterministic closure` 用例必然新增一条 source-graph 条目与
  `lib/checkpoint-store.ts` 的一条 provenance。按任务规「改到守卫哈希文件时…repin 并注明」，
  运行该守卫用例让官方 writer 重新生成该文件（生成器读取当前源码树，保留在途同任务源码改动，
  无内容丢失），随后该文件 22/22 通过。`build/open-boundary-baseline.json` 未被改变。
- 为让「重新生成入口改小菜单」这一需求落地，同步更新了 3 个既有交互测试
  （`AssistantMessageCompletionActions` / `ChatTranscript.turn-time` / `ProcessFoldBlock`）
  的操作路径（点击入口后再点默认项）与其模块 mock；断言本身未放宽（原调用形状逐字保留）。
- 全量复跑中 `desktop/.../GitChangesModal.test.tsx`（在途他人改动文件）偶发一次
  内容时序失败（`findByTestId` 命中占位 `…`）；单独复跑 3/3 通过，与本任务改动无交集。

