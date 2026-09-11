# BLOCKED（回退时撤销文件改动任务）

## 置顶（2026-09-11 工具行短标签任务）：全量测试有 5 条与任务书数字不符的红，先于本任务存在

### 1. 任务书「任务 0」数字对不上
任务书称基线 `npm test` = 14098 passed / 0 failed / 7 skipped。本机同一提交 `22410318c` 实测：
`14107 passed / 5 failed / 7 skipped`（1393 文件：3 failed | 1389 passed | 1 skipped），exit 1。
`npm run typecheck` → exit 0，与任务书一致。

### 2. 这 5 条红与本任务无关（已用干净树取证）
把本任务改动的 8 个文件 `git stash` 掉、在干净工作树上单跑这 3 个文件：
`npx vitest run tests/round2-delivery-evidence.test.ts tests/round3-delivery-evidence.test.ts tests/model-observability-e2e-chat.test.ts`
→ **同样 5 failed / 12 passed**。红集合与带改动时逐条一致：

- `round2-delivery-evidence` R10-03 / R10-04：`SOURCE_MANIFEST.json` 的路径集与
  `.sync-audit/verified-source-sha.txt` 指向的 `9c4b11114` 文件清单不一致（manifest 3667 行 vs 当前 3631），
  属"提交级交付证据"对账，任何工作区改动都会影响其判定。
- `round3-delivery-evidence`「源码 manifest 可复算」：同上，同一 manifest 口径。
- `model-observability-e2e-chat` S1 / S2：需要真实 Pi provider 的端到端链路，本机 `expected [] to have a length of 1/2`。

### 3. 待裁决（需要授权才能处理，本任务按白名单不碰）
- 是否重生成 `artifacts/f1-f12-repair/round{2,3}/SOURCE_MANIFEST.json`（或把这两组测试改成比对
  HEAD 而非 `VERIFIED_SOURCE_SHA`）以消掉 3 条 manifest 红。这属于交付证据/门禁口径，不在本任务白名单。
- `model-observability-e2e-chat` 两条是否为环境缺失（无真实模型凭据）；若是，建议在 CI 标记为需要凭据的
  条件用例，而不是让全量测试常态带红。
- 处理：按任务规"对不上→BLOCKED.md 置顶，只做不受影响部分"。本任务改动（工具行标签/图标 + 五语言语言包）
  与这 3 个红文件无交集，故继续执行；收尾以上述实测集合为基线，要求红集合不增、通过数不减。

### 4. 顺带发现
- 跑全量测试会重新生成 `artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch`
  （`tests/round2-delivery-evidence.test.ts` 的 R10-09 调用 `create-delivery-patch.py` 的副作用）。
  收尾已还原该文件；若后续有人跑测试后 `git status` 看到它变脏，属同一已知副作用，不是源码改动。
- 本任务为 MCP/插件家族词在 `messageActivity.labels` 新增了一个键 `_plugin`（扩展/擴充/Extension/拡張/확장）。
  同时保留了 `labels.tool`（工具/Tool）原值供未登记短标签的一方工具兜底。两条都进五语言对账。
  若领导要求"插件家族词复用 labels.tool"，删掉 `_plugin` 并在 `activityLabel` 里把外部工具也指向 `tool` 即可。

## 待裁决：本次提交使审计封印门禁变红（用户已知情并裁决照常提交）

用户指示 commit + push，并选择「如实报告封印红」。因此本次提交之后：

- `tests/post-verification-audit-seal.test.ts` 与 `.sync-audit/verify-post-verification-diff.mjs`
  会报告非审计文件改动（5 个语言包、3 个源码/工具、3 个测试、BLOCKED.md），因为
  `.sync-audit/verified-source-sha.txt` 仍指向 `9c4b11114`，而 HEAD 已前移到本次提交。
- 未做（需另行授权）：推进 `VERIFIED_SOURCE_SHA`、重新生成交付/审计矩阵、把本次提交作为候选
  走封印验证流程。这些属治理动作，不在本任务白名单，也不得为使检查变绿而虚报坐标或放宽 allowlist。
- 影响面：全量测试由「5 条先存红（round2/round3 交付证据 + model-observability-e2e-chat）」
  变为「6 条红（+审计封印）」，其余全绿。

## 顺手活（按任务规不自行处理，待裁决）
- `getToolLabel`（`tool.*` 命名空间）现在只剩"行标签"以外的旧分支在用：`ToolGroupBlock` 的调查聚合卡
  进展措辞 + 语言包对账测试。可考虑整体退役 `tool.*` 70+ 条整句文案，但知识研究回放分支仍依赖，
  按拍板原样保留，不在本任务动。
- `MessageActivity.test.tsx` 里 `tool()` 工厂 + 两处内联语言包桩可合并成一个 helper（本任务已把
  `ToolGroupBlock.test.tsx` 的两处内联桩提成 `useRealLocale()`，`MessageActivity.test.tsx` 未动）。

## 历史：任务 0 基线与当时任务书转述不符（上一轮任务，保留）

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

