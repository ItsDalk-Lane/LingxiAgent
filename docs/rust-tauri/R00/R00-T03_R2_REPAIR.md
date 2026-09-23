# R00-T03 第二轮修复交接（R2）

状态：**READY_FOR_REVIEW**；按 `R00-T03_ROOT_CAUSE_R2.md` 一次修复 I1-I6 与 W1-W15，不自行判 PASS。Task base `4b4a1d98f6d2d0e03f9573aa75b7992efed03db6`，当前 HEAD `60dbe0384e553f0a0aab7d173b2dc1ba43987b82`。本次未改产品源码、`package.json`/`package-lock.json`、任务书、R1/R2 审核报告、根因报告、R1 修复记录、`ORCHESTRATOR_PROGRESS.json`（总账另有他人未提交改动，保持原样）、R00-T02 冻结文件或用户数据；未提交、未推送、未建 PR。工作树中新增/更新的只有本任务的 8 份候选与本文件。

## 一、F01（类型专用导出误记运行时）修复：I1-I6

- **I1/I2（类型与混合导入）**：`r00_t03_import_graph.mjs` 的 `export_from` 记录改为合并 `node.isTypeOnly || item.isTypeOnly`（R2 F01 根因位），`export * as N`（NamespaceExport）与 `export *` 分别保留本地名；import 侧原有 `clause.isTypeOnly || item.isTypeOnly` 逻辑保留并覆盖 default/namespace/具名/混合/副作用。`lib/pi-sdk/index.ts:61` 六个符号与 `:105` 的 `LoadExtensionsResult` 现为 `type_only:true, load_timing:type_only`，`:60` 的 `runAgentLoop` 保持运行时；`stream-guard.ts:1,2`、`feishu-adapter.ts:8,24`（本地类型导入）经锚点核验。
- **I3（CJS/createRequire/resolve）**：新增 `createRequire` 绑定检测（全量遍历而非仅顶层——修复了函数内绑定如 `scripts/compute-cli-closure.mjs:1064` 的盲点）、直接调用形态 `createRequire(import.meta.url)('js-yaml')`（补入 `shared/safe-fs.ts:37` 漏边）、`require.resolve` 记为 `require_resolve`（只解析不执行）。`usearch-vector-backend.ts:28` 同时记 require（加载）与 require_resolve（解析）。
- **I4（动态/不透明）**：非字面量动态加载 16→17 处，新增一条是 `generate-persistence-schema-fingerprint.mjs:233` 经 createRequire 别名的非字面量 require（别名检测的新发现），全部逐条分类；`anydoc-loader.ts:28` 动态加载、`fresh-import.ts:14` 用户插件未知边维持原判。
- **I5（51 行依赖账本）**：`source_refs` 修复合并顺序——图位点 + 间接调用链 + **初始手工锚点全量保留**（此前图命中会吞掉手工锚点）；每个 `direct_import_sites` 增加 `load_timing` 与 `static_runtime_reachable_from`；`js-yaml` 行补 `shared/safe-fs.ts:37`（call_time、via_create_require）；`katex`/`usearch` 的 `require.resolve` 位点按解析记录；生成器内新增声明-图一致性断言（声称"本仓无直接 import"的行不得存在运行时位点；声称服务组合静态可达的行必须确实从 `server/main-full.ts` 静态运行时可达）。
- **I6（可达性分层）**：`edges`（语法可达，含类型/条件/动态边）与 `staticRuntimeEdges`（仅 `load_timing=module_evaluation` 的 import/export_from/顶层 require）分开 BFS，记录级新增 `static_runtime_reachable_from` 与图级 `root_static_runtime_paths_to_pi_consumers`。81 个 Pi 消费文件在两套口径下一致；`server/main-full.ts` 全量可达 839→808，31 个文件仅经动态/条件/类型边可达。`load_timing` 语义：`module_evaluation`/`call_time`/`type_only`/`resolve_only`。

## 二、F02（执行器遗漏）修复：W1-W15 + 双向枚举闭合

- `worker_counterexamples` 由人工预选 5 条改为 **W1-1…W15 共 19 条**完整分类：Hub 临时/电话（W1-1）、压缩旁路（W1-2）、Hub 调度器（W1-3）、Bridge 管理器（W1-4）、Office 单次解析（W1-5，唯一 PERIPHERAL 正例）、子代理（W2）、workflow（W3）、循环（W4）、桌面封面（W5）、desk/studio cron 内层（W6）、频道轮次（W7）、DM 多轮（W8）、延迟结果协调（W9）、任务总线公共写入口（W10）、命令转后台（W11）、回档后台化（W12）、媒体轮询（W13）、记忆 Dream（W14）、goal 相邻状态（W15，ADJACENT_SESSION_STATE）。每条含 evidence、结构化 terminal_state_sinks、injection_or_call_source、激活条件、Rust 唯一负责人（rust_owner 以 Rust 组件开头）、协议对照与退场证据。
- **锚点结构化门禁**（按用户新增审查要求落实）：sink/injection 锚点拆为 `{ref, note}`，`ref` 严格匹配 `路径[:行号[-行号][,行号…]]`（行号 1 起始）并逐条校验文件存在与行号在界内；`note` 只允许纯语义说明，内含文件样 token（`*.ts` 等）直接报错。此前"含中文即跳过"的宽松判定已删除，带中文括注的真实路径不会被绕过。
- **双向枚举**：源头 8 模式（`executeIsolated`/`runAgentLoop`/`createAgentSession`/`runAgentPhoneSession`/`executeLoopTurn`/`deliverCustomMessage`/`runWorkflowScript`/`deliverLoopMessage`）+ 终态 sink 12 模式（`finishRun`/`settleTask`/`logRun`/`markRun`/`TaskRegistry`/`DeferredResultStore`/`deferredStore`/`deferred:`/`LoopStore`/`LoopController`/`Dream`/`updateBookmark`），子串匹配故意过近似。发现 95 个文件与分类台账双向差集为空（unclassified=[]、stale=[]）；另有 5 条手工相邻补入（goal、goal-tool、activity-store、subagent-run-store、automation-executors，均给出不经模式命中的 sink 依据）。名称碰撞逐条解释：`createAgentSessionAutomationExecutor` 只构造 `{kind:"agent_session"}` 描述对象（W6）、`Dreamina` 是即梦 CLI 模型名与记忆 Dream 无关（W13/插件）、`win32-runtime-cache`/`model-trace-scope`/`model-manager` 为注释提及、渲染端 `finishRun`/`deferred:` 是 UI 投影、`history-read/*` 为只读投影。
- 生成器侧闭合约束：发现集合未分类即 `SystemExit`；分类条目未被模式发现即 stale 报错；kernel 类文件必须被某条 W 条目引用；全部结构化锚点过严格门禁。

## 三、独立 oracle（生成器与验证器不再共用前提）

1. **独立 Python 源码扫描**（`r00_t03_validate.py` 内实现，与 TS AST 生成器完全不同实现）：注释/正则字面量/字符串区间状态机剥离（保留长度与行号，附"剥离后行数=原文行数"不变量断言，任何漂移立即失败），正则重导出 import/export from/副作用/动态 import/require/require.resolve/createRequire 直接与别名调用的全部位点；对 51 依赖包与全部 Pi 包与导入图**双向**比对，逐位点核 `kind` 与语句级 `type_only`。
2. **TypeScript emit 探针**：`ts.transpileModule` 对 9 个夹具（`import type D`、`import type * as N`、`import {type T,V}`、`import D,{type T,V}`、`import 'x'`、`export type {X} from`、`export {type X,Y} from`、`export * as N`、`export type * as N`）验证类型专用导入编译后不产生运行时模块边、值导入与副作用导入保留——类型判定规则的语义基线。
3. **源码锚点正反例**：`index.ts:60,61,105`、`stream-guard.ts:1,2`、`feishu-adapter.ts:8,24`、`release-order.cjs:10`（顶层 require=module_evaluation）、`safe-fs.ts:37`（函数内 createRequire=call_time）、`usearch:28`（require + require_resolve 并存，resolve 只记 resolve_only）逐条对照源码行文本、独立扫描与图记录；反转任一类型位必须失败。
4. **worker 双向枚举独立复算** + 外围语义约束：PERIPHERAL 候选源码不得含 15 种执行/终态形态 token；注入合成执行器、删除分类、W2/W5/W8/W9/W13 改判外围均须失败。

## 四、重放与结果

```bash
node docs/rust-tauri/R00/r00_t03_import_graph.mjs    # 1664 文件；20 Pi 包导入；97 适配层导入；17 不透明；可达 81/81（语法/静态运行时）
python3 docs/rust-tauri/R00/r00_t03_build_matrices.py # 14 职责；19 worker；95 发现文件闭合 + 5 手工相邻；51 依赖；gap 全空
python3 docs/rust-tauri/R00/r00_t03_validate.py        # 55 项检查通过，13 个负例；退出码 0
node scripts/patch-pi-sdk.cjs                          # 退出码 0；[verify-pi-sdk] all checks passed
```

- 13 个负例：未登记 Pi 能力、pi-sdk:61 类型位反转、静态依赖写成动态、截断 diff 引用、泛化 semver 负责人、mermaid 类型导入误标、codemirror 无证据激活、**safe-fs 位点删除**、**pi-agent-core 类型边误标**、**合成未分类执行器**、**删除分类暴露 stale**、**W2/W5/W8/W9/W13 改判外围**、agent-executor 宽松分类。
- 既有回归（根因 §四.4 清单全集）：45 个测试文件、634 项全部通过，退出码 0（Pi 边界与适配、知识来源、工具 schema、产物排序、桌面启动/OTA、Hub/cron/Office、配置、设备接入、子代理三件套、workflow 五件套、循环三件套、延迟结果三件套、任务登记、命令后台、回档两件套、媒体四件套、Dream 两件套、频道/DM、封面路由、goal）。
- `git diff --check` 退出码 0；`python3 -m py_compile`（两个任务脚本）通过；四份 JSON 可解析。Node `v24.16.0`、npm `11.13.0`；`package-lock.json` SHA-256 `e54a16fe14f15b4797069106392040924a5dd616c69a73bd025729090505ac8b`（未改）。

## 五、冻结核对

八份候选（`PI_REPLACEMENT_MATRIX.json`、`R00-T03_IMPORT_GRAPH.json`、`R00-T03_REPORT.md`、`R00-T03_VALIDATION.json`、`RUNTIME_DEPENDENCIES.json`、三个 `r00_t03_*` 脚本）按文件名排序、每行 `SHA256  文件名\n` 聚合 SHA-256：

```
bc708f4c3015de1246bfd31804ddf24a1ce24ea2c15ac2313f48ce0eada72f9a
```

单文件摘要：REPORT `3f020296c7e51f39b4914a0e557c3a738d14f5f0c5b0888883d63507e2d1cbda`；IMPORT_GRAPH `638d0663acb77b43dcda5844e11175af1914a8aba6464515d93f821ec9a61312`；MATRIX `c16b01b7bc038522696a3d74a9b8bf46f5e00581455d97ddeb77ddd217ab6d56`；RUNTIME_DEPENDENCIES `68d98d48740356b3f683242b7c1985ad019564ac612e14d2fa1ad202e86d3c71`；VALIDATION `f1b0a228afb50a01bc7655da851a542e291482a20ef6ab9420fcd18aebcffda1`；import_graph.mjs `fedfe47879eeaf071b335a961a137733d4456f9c50c0c5f10ac6492affaa0800`；build_matrices.py `b7ef3b76dfa246a67cda05420427418e603fc020a859c59aaf3c11f6f63bd43a`；validate.py `c305210dc04a5ba1945addb8ff6a1c7216e5e0532d05924115e15ef3b30ccb80`。

## 六、剩余风险与未验证范围

- **静态范围限制**：未启动真实服务/Electron/CLI、未调用真实模型或平台、未读用户插件目录、未做安装包验证。静态可达与静态运行时可达都是源码推演；`fresh-import.ts` 用户插件边仍需 R07/R11 运行时拦截。
- **独立扫描的已知近似**：Python 扫描的正则字面量启发式（前字符/关键字判别）在病态代码上可能误判，但行数不变量会先失败；from-正则不支持花括号前换行的 default 绑定换行排版（当前受控源码未出现）。
- **require 的 load_timing** 只在锚点位点逐条独立核验（顶层/函数内），未对全部 require 位点做 AST 级嵌套复算；图生成器的 TS AST 判定与锚点正反例覆盖了根因点名场景。
- **W 表行号锚点**绑定当前源码；后续产品源码演进会使锚点门禁失败（按设计应失败并重新盘点，不是缺陷）。
- 自动化门禁的通过不等于 A05/A06 验收；需全新独立验收者从任务书、源码、锁文件与全部新候选完整复审，不沿用旧轮 PASS，也不以根因报告或本文件替代验收。
