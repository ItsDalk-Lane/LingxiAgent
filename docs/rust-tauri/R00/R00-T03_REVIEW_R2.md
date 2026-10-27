# R00-T03 独立验收 R2

**VERDICT: FAIL（任务整体）。R00-A05: PASS；R00-A06: FAIL。** 本轮从任务书、源码、锁文件和冻结候选重新复审，没有沿用 R1 对 A05/A06 的判断。A05 的现役 Pi 能力均能找到迁移归属；A06 漏掉真实的后台执行及终态写入者。另有两条明确的类型专用导出被误记为运行时加载，51 项依赖账本尚不准确。

## 基线、范围与实跑

- Task base：`4b4a1d98f6d2d0e03f9573aa75b7992efed03db6`；审核 HEAD：`60dbe0384e553f0a0aab7d173b2dc1ba43987b82`。已提交差异只有 `.gitignore` 和旧任务书移出跟踪；候选、产品源码、`package.json` 和 `package-lock.json` 未提交。本轮没有改动这些文件。
- 八份冻结候选按文件名排序、每行 `SHA256  文件名\n` 聚合，复算为 `997167baefa82512e60ef667c8db062a90c469f7822b1a6b5475f941b2909e96`；报告 SHA-256 为 `b46de92fa1c5d0eac45240273298fcfd9f8a54de6047269585ca31a40cf12e50`，修复说明为 `b3e20f9c60c8fb25fae3dbc9c67e5e4821cfbfed327e47238e5a20aec8dbcb97`，锁文件为 `e54a16fe14f15b4797069106392040924a5dd616c69a73bd025729090505ac8b`，均与交接一致。
- 按任务书 R00-T03 的四个步骤和 REQUIRED 场景 R00-A05/A06 复核。51 行逐项检查包名、锁版本、直接导入位点、类型标记、加载/调用时点、消费者、迁移负责人、协议对照和退场条件；独立比对 `package.json` 与锁文件得到 51/51 行覆盖、0 个版本差异。重点沿源码重走 D10/11/13 的知识库静态导入，D21 的桌面浏览器 WebSocket，D23 的设备配对和微信二维码，D24 的三类 ZIP，D40/41/50 的服务端调用，D49 的桌面启动及更新排序，以及 React 的静态/类型导入；这些旧问题的具体入口已补进账本。D34/D39 明确没有本仓直接生产导入，没有把锁项自动说成已运行。
- `npx vitest run tests/pi-sdk-import-boundary.test.ts tests/knowledge-source-processors.test.ts tests/tool-schema-validator.test.ts tests/artifact-release-order.test.ts tests/cron-scheduler.test.ts`：5 个文件、33 项通过，退出码 0。另跑 `tests/subagent-tool.test.ts tests/workflow-tool.test.ts tests/loop/loop-controller.test.ts`：3 个文件、107 项通过，退出码 0；这些测试验证现役行为，其中子代理/工作流使用替身，不作为跨进程实跑证明。`node scripts/patch-pi-sdk.cjs`：退出码 0，脚本本身只读。验证器以拦截其末尾证据写入的内存方式重跑：37/37 项和 7 个负例通过；其通过并未识别下述两类错误。没有启动真实服务、模型、外部平台或写入用户目录；也没有重跑会覆盖冻结候选的生成脚本。

## R00-A05｜PASS

服务完整/开放组合入口 `server/main-full.ts`、`server/main-open.ts` 进入 `server/index.ts` 与 `core/session-coordinator.ts`；Electron `desktop/main.cjs` 经 `server/bootstrap.ts` 启动服务、渲染端经 HTTP/WS 提交；CLI `cli/entry.ts` 经本地服务；Hub、Bridge、cron/心跳、压缩旁路分别经注入或直接调用到 Pi session 或 `runAgentLoop`。`core/fresh-import.ts:14` 的用户插件动态路径无法静态闭合，候选将其列为后续运行时拦截责任，而非声称未使用。

重新核对 20 处 Pi 包导入、97 处适配层导入、15 个扩展 hook、16 处非字面量动态加载和 14 项职责：现有生产源码中实际使用的 Pi loop、会话树、压缩、资源/工具、模型、认证、流、用量和 steer/follow-up 均有迁移项；没有找到未归属的 Pi 运行能力。下面 F01 是**误报**两条类型专用导出为运行时边，并不掩盖缺失的 Pi 能力，因此 A05 的“无实际能力漏账”条件仍成立；它不构成对导入图每个运行时标记的认可。

## Findings

### F01｜BLOCKING｜类型专用导出被当作运行时加载，51 行依赖账本仍有假边

**位置：**`r00_t03_import_graph.mjs` 的 `ExportDeclaration` 元素映射；`R00-T03_IMPORT_GRAPH.json` 的 `lib/pi-sdk/index.ts:61,105`；`RUNTIME_DEPENDENCIES.json` 的 D02、D01 相应 `direct_import_sites`。

**证据：**源码第 61 行是 `export type { AgentContext, AgentEvent, AgentLoopConfig, AgentMessage, AgentTool, StreamFn } from "@earendil-works/pi-agent-core"`，第 105 行是 `export type { LoadExtensionsResult } from "@earendil-works/pi-coding-agent"`。独立 TypeScript AST 读取表明两节点的 `node.isTypeOnly=true`，元素各自的 `item.isTypeOnly=false`。扫描器只取后者，把 7 个符号和两条导入位点都写成 `type_only:false`；依赖账本沿用错误标记。`r00_t03_validate.py` 再以同一导入图为“预期”比较账本，所以 37 项全绿仍放过了这个负例。

**复现：**用 `typescript.createSourceFile('lib/pi-sdk/index.ts', ..., ts.ScriptKind.JS)` 检查第 61/105 行 `ExportDeclaration.isTypeOnly`，再查询图中对应 `symbols[].type_only` 和 D01/D02 的 `direct_import_sites[].type_only`：源码为 `true`，两份候选均为 `false`。本轮只读探针输出恰好这两处不一致。普通编译会擦除 `export type`，它们不能作为模块运行时求值的证据；Pi 两个包还有其他真实运行时导入，故不应据此宣称包整体未加载。

**违反任务及后果：**第 3、4 步要求区分实际加载与类型/外围依赖，R1 F01 又要求全表准确。错误边使后续清退或打包核查把类型声明算进运行时证据。根因是解析器没有合并 `node.isTypeOnly || item.isTypeOnly`；验证器和生成器共享了这个错误前提。

**同类检查与修复要求：**修正 `export type` 和具名 `type` 的 AST 识别，重生成导入图及 51 行账本；独立按源码复核全部 `export_from`、`import type`、混合具名导入和 CSS/资源导入，新增能让第 61/105 行误标时失败的负例。复跑全表差集、Pi 边界和服务组合入口；保留真实运行时边。

### F02｜BLOCKING｜A06 漏审经注入启动模型循环并写任务终态的现役执行器

**位置：**`PI_REPLACEMENT_MATRIX.json` 的 `worker_counterexamples` 仅有 5 个对象；遗漏 `lib/tools/subagent-tool.ts`、`lib/tools/workflow-tool.ts` 和 `lib/loop/loop-controller.ts`。验证器只检查固定的 `hub/agent-executor.ts`、压缩旁路和 `hub/scheduler.ts`，没有从现役执行链反查候选集合。

**证据与真实入口：**`core/agent.ts:936-944` 实际创建 `subagent` 与 `workflow` 工具，并将 `executeIsolated` 注入；`core/engine.ts:2304` 把调用转给 `core/session-coordinator.ts:8048,8385,8657`，后者创建 Pi session 并 `prompt`。`lib/tools/subagent-tool.ts:466,551-627,802,865-911` 后台派出/续接隔离 Agent，自己对 deferred store、run store、thread store 和 hub 写 `resolved/failed/aborted` 等终态。`lib/tools/workflow-tool.ts:283,304,320-346` 运行脚本节点的隔离 Agent，并自己写 `done/failed`。`core/engine.ts:1435-1447` 还装配 `LoopController`，它在 `lib/loop/loop-controller.ts:179-199` 写持久化 `completed` 并触发后续 turn。这三处都不属于单次无决策文档解析 worker。

**复现：**分别在上述文件搜索 `executeIsolated`、`runWorkflowScript`、`store.resolve/fail/abort`、`finishRun`、`status: "completed"`，再枚举矩阵 `worker_counterexamples[].candidate`；三条现役路径均不在集合中。本轮只读负例探针输出：`subagent-tool` 与 `workflow-tool` 均为 `isolated=true, terminal=true, listed=false`；`loop-controller` 为 `terminal=true, listed=false`。矩阵在 `adapter_import_coverage` 仅把 `subagent-tool.ts:13`、`workflow-tool.ts:3` 的 Pi facade `Type` 归到 PI-14，不能代替其后台执行和终态归属分类。Workflow 默认关闭但可通过工具开关启用，仍是现役生产路径。

**违反任务及后果：**A06 明确要求含模型循环**或**任务终态写入的候选 worker 判为内核迁移，而不是长期保留的外围 worker。当前反例集合只覆盖一部分直接 Pi API/已知注入路径；后续若按该清单迁移，会把子代理/工作流/循环任务的调度与最终状态留给 Node，形成第二位任务负责人。通用 PI-01/PI-11 描述了 Rust 方向，但没有给这三条具体生产链、终态写入点及退出证据，因此不能把缺席视为已核验。

**同类检查与修复要求：**从所有 `executeIsolated` 注入、后台 `runWorkflowScript`、循环 turn 注入及 deferred/thread/task 终态写入反向枚举现役执行器，逐个列入内核迁移或提供足够的单次外围理由；为子代理新建/续接、workflow 节点与恢复、循环完成/取消指定 Rust 唯一负责人、协议对照和退出证据。增加“从源码发现新终态写入者而矩阵未列”会失败的负例，复跑 A06 和相关既有回归。

## 结论

R00-A05 的现役 Pi 能力映射达到本阶段静态验收条件；R00-A06 因实际执行器遗漏而 FAIL，任务交付的依赖账本也因 F01 未满足准确性要求。**R00-T03 不能放行。** 本轮只新增此报告，没有修改候选、产品、锁文件、任务书、总账或用户数据，也没有提交或推送。
