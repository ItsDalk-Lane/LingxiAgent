# R00-T03 独立验收 R3

**VERDICT: PASS。R00-A05: PASS；R00-A06: PASS。** 两项 REQUIRED 场景均有本轮独立重走的真实证据；未发现 BLOCKING 问题。两条 LOW 级登记完整性观察（F01/F02）不满足 A06 的"含模型循环或任务终态写入"触发条件，不阻断放行，建议随 R00-T07 账本或 R07 worker 账本吸收。本验收者未参与执行与修复，未沿用 R1/R2 结论、修复报告或验证器绿色作为证明；全部主张按源码、锁文件与重放独立复核。

## 基线、candidate 与环境

- Task base `4b4a1d98f6d2d0e03f9573aa75b7992efed03db6`；tested HEAD `60dbe0384e553f0a0aab7d173b2dc1ba43987b82`；分支 `codex/rust-tauri-migration`。base..HEAD 仅一个用户提交（`.gitignore` 新增通配规则并移除旧任务书目录跟踪，18 个文件删除、无产品源码/依赖变化），按外部提交保留，不计入 T03 改动。R00-T02 依赖已 DONE（总账 R10 PASS，task commit 即本 Task base，A03/A04 PASS）。
- 八份候选文件（`PI_REPLACEMENT_MATRIX.json`、`R00-T03_IMPORT_GRAPH.json`、`R00-T03_REPORT.md`、`R00-T03_VALIDATION.json`、`RUNTIME_DEPENDENCIES.json`、`r00_t03_build_matrices.py`、`r00_t03_import_graph.mjs`、`r00_t03_validate.py`）按文件名排序、每行 `SHA256␣␣文件名\n` 聚合，本人独立重算为 `bc708f4c3015de1246bfd31804ddf24a1ce24ea2c15ac2313f48ce0eada72f9a`，与交接一致；八份单文件摘要亦与 R2 修复交接第五节逐项相同。
- 工具链与平台：Node `v24.16.0`、npm `11.13.0`、Python `3.14.3`、macOS 27.0（Build 26A428）arm64。`package-lock.json` SHA-256 `e54a16fe14f15b4797069106392040924a5dd616c69a73bd025729090505ac8b`（与 R1/R2 轮一致，未改）。
- 本轮实际运行命令（均在本仓库 HEAD，退出码全部为 0）：

```bash
node docs/rust-tauri/R00/r00_t03_import_graph.mjs      # 1664 文件/20 vendor/97 adapter/17 opaque/81 消费
python3 docs/rust-tauri/R00/r00_t03_build_matrices.py  # 14 职责/19 worker/95+5 闭合/51 依赖/gap 空
python3 docs/rust-tauri/R00/r00_t03_validate.py        # 55 项检查、13 负例全部通过
node scripts/patch-pi-sdk.cjs                          # [verify-pi-sdk] all checks passed
npx vitest run tests/pi-sdk-import-boundary.test.ts tests/subagent-tool.test.ts \
  tests/workflow-tool.test.ts tests/loop/loop-controller.test.ts tests/cron-scheduler.test.ts \
  tests/task-registry.test.ts tests/media-poller.test.ts tests/memory-dream-runner.test.ts \
  tests/knowledge-source-processors.test.ts tests/tool-schema-validator.test.ts \
  tests/artifact-release-order.test.ts                 # 11 文件、195 项全部通过
```

重放前将四份 JSON 备份至 `/tmp/r3_backup`，重放后逐字节比对：再生成文件与冻结候选 SHA-256 完全一致，候选未被本轮破坏，输出确定性成立。本轮未启动真实服务/Electron/CLI、未调用模型或平台账号、未读写用户数据目录、未提交/推送；除本报告外未新增或修改任何文件。

## R00-A05｜PASS

- **生产入口→Pi 链独立重走**：`server/main-full.ts`/`main-open.ts` 静态导入 `server/index.ts` 的 `startServer`；Electron `desktop/bootstrap.cjs` → `desktop/main.cjs` → `server/bootstrap.ts:62` 动态 `await import(serverEntry)`；CLI `cli/entry.ts` 本地启动或 `client.ts` 连服务；`core/session-coordinator.ts:11`（`createAgentSession` 等）与 `hub/agent-executor.ts:290,556`（建 session 后 `session.prompt`，:343,:716）、`lib/llm/cache-preserving-compaction-agent-run.ts:550`（直接 `runAgentLoop`）逐条到源码行确认。`core/fresh-import.ts:14` 用户插件路径列为未知边并移交 R07/R11，未冒称可静态闭合。
- **导入集合独立复算**：本人 rg 全文扫描生产目录得到与图一致的 Pi vendor 语句位点；`core/provider-compat/qwen.ts:16`、`shared/persistence/store-registry.ts:598` 两处 `@earendil` 字符串分别为注释与 `packageName` 字面量，图不计入正确。`lib/pi-sdk/index.ts:25`（AuthStorage）、`:52`（prepareCompaction）确为 `node_modules` 深路径导入。`scripts/patch-pi-sdk.cjs` 全文仅 `readFileSync`，无写调用；实跑退出 0。
- **类型位修复（R2-F01）源码级核验**：`index.ts:60` 为 `export { runAgentLoop }`（图记 `type_only:false, module_evaluation`），`:61`/`:105` 为 `export type {...}`（图记 `type_only:true, load_timing:type_only`）——与源码逐行一致。
- **hook 与符号**：15 处 `pi.on(...)` 订阅（agent-loop-guard 5、compaction-guard 3、deferred-result 2、model-call-observer 2、engine.ts 3）本人 rg 独立重扫逐条吻合；47 个被引用 facade 符号、56 个公开导出、14 项职责（PI-01…PI-14）结构完整，rust_owner/permitted_peripheral/protocol_test/retirement_evidence 逐项非空且 PI-01 锚点（coordinator/bridge/hub/compaction 四链）真实。
- **51 项依赖对账**：`package.json` 生产+可选依赖 50 项全部在列、第 51 行为 Electron 宿主，锁版本 51/51 与 `package-lock.json` 零差异。R1-F01 点名的 D10/D11/D13（知识库 `source-processors.ts:4-6`/`source-adapters.ts:1` 顶层静态导入）、D40（`file-change-presentation.ts:2` 服务端 diff）、D41（含 `shared/safe-fs.ts:37` 函数内 `createRequire` 补边）、D49（`release-order.cjs:10` 顶层 require semver + 桌面 `artifact-boot.cjs:59`/`ota-core.cjs:167` + Tauri updater/Rust ArtifactService 负责人）、D50（`schema-validator.ts:1-2` + engine 三处工具参数校验）全部行级核实，加载时点（module_evaluation/call_time/type_only/resolve_only）与源码吻合。`koffi` 无锁项、`codemirror` meta 包与 `@codemirror/*` 子包、`@tiptap/pm` 传递依赖的边界表述准确。17 处非字面量动态加载逐条有分类。数字口径说明：REPORT 的"97 处适配层导入"含 `lib/pi-sdk/index.ts` 自身 11 条，VALIDATION 的"adapter imports=86"为排除自身后的消费者口径，两者不矛盾。
- **独立 oracle 与负例审查**：`r00_t03_validate.py` 的 `independent_scan()` 为真正独立实现（注释/字符串区间剥离 + 剥离后长度/行数不变量 + 正则重扫，不读图作预期），对 51 依赖与全部 Pi 包重导出位点与图**双向**比对；TS `transpileModule` 探针、类型位翻转、safe-fs 删除、静态依赖伪装动态、未分类执行器注入、W2/W5/W8/W9/W13 改判外围等 13 个负例均为真实语义负例（改外围负例以 `FORBIDDEN_PERIPHERAL_TOKENS` 源码语义检查实现，非同源自证）。
- 判定：受控源码范围内每项实际使用的 Pi 能力均有迁移项；未发现无归属的 Pi 运行能力。**A05 PASS**（静态范围界限见文末）。

## R00-A06｜PASS

- **双向枚举独立复算**：按生成器同口径（git 跟踪 ∩ 9 目录 ∩ 源码扩展名，排除 `__tests__`/`dist*`/`*.test.*`；共 1664 文件）以相同 20 个源头/终态子串独立全文重扫，得到 95 个发现文件，与矩阵台账**双向差集为空**（unclassified=[]、stale=[]），另有 5 条手工相邻（goal/工具、activity-store、subagent-run-store、automation-executors）各有不经模式命中的 sink 依据。
- **W 表实质核查**：19 条（W1-1…W15）中 17 条 KERNEL_MIGRATION + W15 ADJACENT_SESSION_STATE + W1-5 唯一 PERIPHERAL 正例（Office 单次解析，`terminal_state_sinks` 明确为空，`plugins/office/lib/read-document.ts` 无 session/终态/多轮）。83 个结构化 sink/injection 锚点逐一校验：文件全部存在、行号全部在界；语义抽查吻合（`lib/subagent-thread-store.ts:248` 确为 `finishRun(threadId,...)`；`core/media/poller.ts:154` 确为 `_settle(..., {status:"cancelled"})`；`lib/memory/dream/state-store.ts` 确为 dream `state.json` 持久化）。每条内核条目 rust_owner 以 Rust 组件开头、protocol_test 与 retirement_evidence 具体可执行。
- **R2-F02 三执行器复测**：`lib/tools/subagent-tool.ts`（W2，deferred/run/thread/hub 终态）、`lib/tools/workflow-tool.ts`（W3，节点 done/failed）、`lib/loop/loop-controller.ts`（W4，stopped/completed/paused + 后续 turn）均以 `core/agent.ts:936-944` 注入、`core/engine.ts:2304` 转发、`core/session-coordinator.ts:8048,8385,8657` 建隔离 Pi session 的真实调用链列入 KERNEL_MIGRATION，不再是五条人工预选的缺口。
- **pattern 之外的反漏项搜索**（本轮新增，非旧问题复测）：
  - 绕过 Pi 的直接模型调用链：`core/llm-client.ts` `callText` 为直连 HTTP POST（源码注释明确不走 Pi completeSimple）；其 33 个调用者中未入台账者（autolearn、diary、deep-memory、session-summary、appearance-summary、rc-summary、approval-gateway、vision-bridge、git-environment 路由等）均为请求内单次辅助调用或由已列作业驱动，模型边界由 PI-08 覆盖（source_ref 含 `core/llm-client.ts:520` 即 `callText` 定义，retirement_evidence 明确"本地 direct HTTP 旁路也收归同一网关"）。
  - 长驻定时器面：全仓 `setInterval` 注册文件 20 个，逐一对台账——poller(W13)、bridge-manager(W1-4)、deferred coordinator/store(W9)、cron-scheduler(W6)、exec background(W11)、memory-ticker(W14)、server/index(W4/W9/W10/W15 装配)均在册；其余为平台 SDK 适配器（W1-4 allowed_peripheral 范围）、browser-manager（外围宿主）、file-history/knowledge/observability-persistence/ota/auth 轮询/cli spinner 等无模型循环且无任务终态的基础设施。
  - `session.prompt` 直接调用点全集（7 处）均在已列对象内（agent-executor W1-1、session-coordinator 主链、engine 转发、pi-sdk hook 安装）。
  - 结论：未发现"含模型循环或任务终态写入却被判外围或遗漏"的对象。**A06 PASS**。两条边界观察见 F01/F02。

## R1/R2 findings 与旧结论处置

- R1-F01（依赖行误写）：七行 + 同类全表已按真实消费者链修复，本轮行级复核通过。
- R2-F01（类型位误标）：`:61/:105` 已按 `node.isTypeOnly||item.isTypeOnly` 修正，`:60` 保持运行时；`stream-guard.ts:1` 类型专用、`feishu-adapter.ts:8/24` 正反例与源码一致。
- R2-F02（三执行器漏列）：已列且锚点/终态/注入链完整；同类路径（W5-W15 全域 + 本轮 pattern 外搜索）无再现。
- R1 曾判 A05/A06 PASS、R2 判 A05 PASS/A06 FAIL：本轮不沿用任何旧判定，上述结论均由本轮独立证据重建。

## Findings

### F01｜LOW｜heartbeat 机制库未行级入账（执行链已被 W1-3 覆盖）

**位置：**`lib/desk/heartbeat.ts`；`PI_REPLACEMENT_MATRIX.json` 的 `worker_discovery` 台账与 W 表。**证据：**该文件实现笺扫描/工作台巡检（指纹去重、prompt 构造、`jian.md` exec-log 区块写入 `in_progress/completed/skipped/failed` 巡检状态），不含 20 个枚举子串，故不在 95 文件台账；其执行经 `hub/scheduler.ts:13,163,166` 注入的 `onBeat/onJianBeat → _executeActivityForAgent → executeIsolated` 落在 W1-3（KERNEL_MIGRATION，activation 已点名 heartbeat）。**影响：**分类语义正确、无漏判；但笺/巡检活动状态的行级 sink 锚点与指纹去重职责未在 W 表显式登记，源码演进时该机制不会触发 stale 差集告警。**修复要求：**在 W1-3（或手工相邻区）补 `lib/desk/heartbeat.ts` 的 exec-log 状态写入锚点与一句职责说明；无需改判分类。**重跑：**`r00_t03_build_matrices.py` + `r00_t03_validate.py`；`tests/cron-scheduler.test.ts` 已绿可作佐证。

### F02｜LOW｜autolearn 后台模型作业未按 W15 先例登记相邻形态

**位置：**`lib/autolearn/autolearn-service.ts`；W 表/台账。**证据：**turn 观察异步触发 → `callText`（summarize 槽提炼 + guard 槽安全审查，`lib/autolearn/autolearn-service.ts:27,99`）→ 经 `lib/tools/install-skill.ts` 安装技能并通知用户；无 Pi loop、无 Run/Task 终态、防抖状态明确不落盘，故不满足 A06"含模型循环或任务终态写入"的触发条件，模型边界已由 PI-08 网关化覆盖、技能写入属 D21 Rust Skill/Config 权威。**影响：**不构成 A06 漏判；但矩阵对 W15 goal（仅会话状态）主动补入"相邻状态"先例下，"后台异步模型作业 + 技能库变更 + 用户通知"形态零登记，R07 worker 账本若仅从 W 表展开会缺这一行。**同类路径：**`lib/diary/diary-writer.ts`（PI-08 source_ref 已点名）与其形态相近，一并登记更完整。**修复要求：**补一条 ADJACENT/SUPPORTING 登记行（模型调用走 PI-08 网关、技能安装走 D21、无终态），不改任何 KERNEL/PERIPHERAL 判定。**重跑：**生成器+验证器闭合两差集仍为空即可。

## 未验证范围（静态界限，与候选声明一致）

生产进程实际加载集合（需运行时拦截 `fresh-import.ts` 用户插件边）、真实模型/Bridge/五平台请求、四平台安装包、Rust 替代实现（尚不存在）。这些不能从静态差集、只读脚本或单测推出，候选报告已如实声明；本轮同样未执行，不据此扣减上述 PASS——A05/A06 的验收定义即静态可达性盘点与职责分类。R00-T07 建账与 R07/R11 实施须承接 F01/F02 与 `fresh-import.ts` 运行时拦截责任。

## 结论

两项 REQUIRED 均有本轮独立重走的源码级证据，候选可复现（重放字节级一致）、负例真实有效、R1/R2 全部 finding 已核实修复且同类路径无再现；仅存两条 LOW 级登记完整性观察，不构成 BLOCKING。**R00-A05: PASS；R00-A06: PASS；VERDICT: PASS。**
