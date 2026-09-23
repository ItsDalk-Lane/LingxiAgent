# R00-T03｜Pi 与 Node 隐含职责盘点

状态：**READY_FOR_REVIEW**（R2 修复候选）。本任务只新增盘点、重放脚本和静态证据；未改产品运行代码、依赖或用户数据。Task base 为 `4b4a1d98f6d2d0e03f9573aa75b7992efed03db6`，最终重放时共享分支 HEAD 为 `60dbe0384e553f0a0aab7d173b2dc1ba43987b82`，分支 `codex/rust-tauri-migration`。并发新增提交只调整 `.gitignore` 并移除旧任务书跟踪，没有修改本任务扫描的产品源码；本执行者没有提交或推送。任务书、总账和 R00-T02 冻结文件没有由本执行者修改。研究提交 `7d1a0c6...` 只作任务书来源，结论取当前源码。

## 可观察结论

- `lib/pi-sdk/index.ts` 远超 `createAgentSession`：现役职责包括会话树、压缩、资源/扩展、工具工厂、模型目录、凭证、直接摘要、流保护、用量关联、插话/跟进和图片预处理。`PI_REPLACEMENT_MATRIX.json` 用 14 项职责分别给出 Rust 负责人、允许的单次外围处理、对照测试和旧路径退场证据。
- 锁文件明确固定三份 Pi 包均为 `0.86.0`。适配层还从 `node_modules` 深路径取 `AuthStorage` 和 `prepareCompaction`，升级/退出须逐项核对。`scripts/patch-pi-sdk.cjs` 虽保留旧文件名和 postinstall 调用，当前实际**只读验证**包版本、导出标记和生产导入边界；运行前后抽取的三个 Pi 包目标文件哈希一致。
- 受控源码 AST 扫描 1664 个文件，得到 97 处适配层导入、20 处 Pi 包导入、56 个 facade 公开导出和 15 个现役 Pi 扩展 hook 订阅。47 个被源码引用的 facade 符号均有归属；供应商导入、扩展 hook 与适配层子模块差集为空。
- 导入图 v2（R2 修复）：`import type`/`export type`/元素级 `type` 前缀按 TS AST 父子节点合并判定，`lib/pi-sdk/index.ts:61,105` 的七个类型符号不再误记为运行时边，`:60` 的 `runAgentLoop` 保持运行时；CJS `require`、`createRequire` 绑定/直接调用、`require.resolve`（只解析不执行）与动态 `import` 分开记录 `load_timing`；`shared/safe-fs.ts:37` 的 `createRequire(import.meta.url)('js-yaml')` 补入 js-yaml 消费者。可达性分层：语法可达与"顶层静态运行时可达"分别计算，二者对 81 个 Pi 消费文件一致，`server/main-full.ts` 全量可达 839→808（31 个文件仅经动态/条件/类型边可达）。非字面量动态加载 17 处（较 R2 冻结 +1：`generate-persistence-schema-fingerprint.mjs:233` 经 createRequire 别名的非字面量 require，属 I3 别名检测的新发现），全部逐条分类。
- 原生/第三方依赖列为 51 项：26 项重点运行依赖 + 25 项 `package.json` 其余直接依赖。每行分栏 `module_load_activation`（模块求值/加载）与 `operation_activation`（实际功能调用），`direct_import_sites` 全量记录导入种类、类型专用、加载时点、所在进程、组合入口与静态运行时可达性；`source_refs` 并集保留图位点、间接调用链与初始手工锚点（图命中不再吞掉手工锚点）。知识库 `mammoth`/`exceljs`/`jsdom` 随服务组合静态可达并有文件/桌面按需路径；`js-yaml` 含 safe-fs 按需 createRequire 加载；`semver` 由桌面启动/更新与 CLI 产物排序链加载，`require.resolve` 位点（katex CSS、usearch）只记解析不记执行。`koffi` 仍无锁项或生产导入，未列作现役依赖。
- 后台执行清单（R2 修复）：`worker_counterexamples` 从 5 条扩为 W1-1…W15 共 19 条，覆盖 Hub 临时/电话、压缩旁路、Hub 调度器、Bridge 管理器、Office 单次解析（唯一外围正例）、子代理、workflow、循环、桌面封面、desk/studio cron 内层、频道轮次、DM 多轮、延迟结果协调、任务总线公共写入口、命令转后台、回档后台化、媒体轮询、记忆 Dream 和 goal 相邻状态。每条含 evidence/结构化 sink/injection 锚点、激活条件、Rust 唯一负责人与退场证据；锚点按"路径[:行号]（说明）"结构化拆分，`ref` 严格解析并逐条校验行号，`note` 内出现文件样 token 直接报错。双向枚举（8 源头模式 + 12 终态 sink 模式，子串故意过近似）发现 95 个文件，与分类台账双向差集为空，另有 5 条手工相邻补入（goal/活动/run/dream state 等 sink 与 automation 描述构造）。`createAgentSessionAutomationExecutor` 只构造 `{kind:"agent_session"}` 执行描述、`Dreamina` 模型名与记忆 Dream 无关等名称碰撞逐条记录排除理由。

## A05｜生产组合入口与 Pi 能力差集

入口映射位于矩阵的 `production_entrypoint_capabilities`，逐链列出源码定位：

1. Node 完整/开放组合入口：`server/main-full.ts` / `server/main-open.ts` → `server/index.ts` → `LingxiEngine` / `SessionCoordinator`；两种构建共享 Pi 内核，闭源附加路由另有差异。
2. Electron：`desktop/bootstrap.cjs` 条件加载主进程 → `desktop/main.cjs` 启动 Node 服务 → `server/bootstrap.ts` 动态导入已打包服务入口；preload/React 通过 HTTP/WS 向服务提交，不是直接 import Pi。渲染入口包括主窗口、Mobile、设置、Quick Chat；`desktop/src/index.html` 的主窗口脚本和 `server/routes/chat.ts` → `hub.send` → `promptSession` 是一条实际用户输入链。
3. CLI：`cli/entry.ts` 可启动完整服务或通过 `cli/client.ts` 连服务，任务仍入同一内核。
4. Hub/Bridge/后台：`hub/agent-executor.ts` 有临时与电话两条 `createAgentSession → prompt`；`core/bridge-session-manager.ts` 有 owner Bridge 会话；`hub/scheduler.ts` 的 cron/heartbeat 经注入的 engine 走 `executeIsolated → createAgentSession → prompt`。这些跨对象注入边不能只靠静态 import 图推断。压缩扩展还经 `lib/llm/cache-preserving-compaction-agent-run.ts` **直接**调用 `runAgentLoop`。

扫描图以 17 个入口/组件根开始追踪：81 个从服务入口可达的 Pi 消费文件在语法可达与顶层静态运行时可达两套口径下均为同一集合；`R00-T03_IMPORT_GRAPH.json` 保留每条 import、符号、文件行、根路径、加载时点与两种可达性。关键未知边仍是 `core/fresh-import.ts` 对插件代码的路径加载：用户自装插件不在本仓库静态闭包内，后续 R07/R11 必须按真实加载清单拦截第二内核。

重放命令：

```bash
node docs/rust-tauri/R00/r00_t03_import_graph.mjs
python3 docs/rust-tauri/R00/r00_t03_build_matrices.py
python3 docs/rust-tauri/R00/r00_t03_validate.py
```

验证器不再把导入图同时当"被测值"和"唯一预期"：独立 Python 源码扫描（注释/正则/字符串区间剥离 + 行号不变量）对 51 依赖与全部 Pi 包重导出位点并与图双向比对；TypeScript `transpileModule` emit 探针证明类型专用导入编译后无运行时模块边（9 例正反）；`lib/pi-sdk/index.ts:60,61,105`、`stream-guard.ts:1,2`、`feishu-adapter.ts:8,24`、`release-order.cjs:10`、`safe-fs.ts:37`、`usearch-vector-backend.ts:28` 作为源码锚点正反例逐条核验。人为反转类型位、删除 safe-fs 位点、伪造静态可达声明或注入未分类执行器均使验证失败。

## A06｜执行器反例

矩阵的 `worker_counterexamples` 含 19 条实际对象（W1-1…W15）：含模型循环**或**任务终态写入的现役执行器（Hub 临时/电话、压缩旁路、Hub 调度器、Bridge、子代理、workflow、循环、封面、cron 内层、频道、DM、延迟结果、任务总线、命令后台、回档、媒体、Dream）全部判 **KERNEL_MIGRATION** 并给出 Rust 唯一负责人、协议对照与退场证据；goal 引擎按 ADJACENT_SESSION_STATE 随会话服务交接；Office 单次文档读取是唯一 `PERIPHERAL_CANDIDATE` 正例。验证器从源头（`executeIsolated`/`runAgentLoop`/`createAgentSession`/`runAgentPhoneSession`/`executeLoopTurn`/`deliverCustomMessage`/`runWorkflowScript`/`deliverLoopMessage`）与终态 sink（`finishRun`/`settleTask`/`logRun`/`markRun`/`TaskRegistry`/`DeferredResultStore`/`deferredStore`/`deferred:`/`LoopStore`/`LoopController`/`Dream`/`updateBookmark`）独立双向枚举，发现集合与分类台账（95 文件 + 5 手工相邻）双向差集为空；外围候选的源码不得含任何执行/终态形态 token，W2/W5/W8/W9/W13 改判外围会立即失败。

## 本次验证与界限

| 检查 | 实际结果 |
|---|---|
| 生成导入图、矩阵、依赖清单 | 退出码 0；14 项职责、19 条 worker、95 文件发现闭合、51 项依赖、15 个 hook |
| `r00_t03_validate.py` | 55 项检查通过，含 13 个反例注入；独立 Python 扫描、TS emit 探针、锚点正反例、worker 双向闭合均在内；结果和输入哈希见 `R00-T03_VALIDATION.json` |
| `node scripts/patch-pi-sdk.cjs` | 退出码 0；`[verify-pi-sdk] all checks passed` |
| 定向既有回归 | 根因清单 45 个测试文件、634 项全部通过（Pi 边界/适配、知识来源、工具 schema、产物排序与桌面启动/OTA、Hub/cron/Office、配置、设备接入、子代理三件套、workflow 五件套、循环三件套、延迟结果三件套、任务登记、命令后台、回档两件套、媒体四件套、Dream 两件套、频道/DM、封面路由、goal）；不代替后续真实安装包协议对照 |
| `git diff --check` 与 Python 编译检查 | 退出码 0（仅任务脚本 py_compile） |

当前 Node 为 `v24.16.0`、npm 为 `11.13.0`；`package-lock.json` SHA-256 为 `e54a16fe14f15b4797069106392040924a5dd616c69a73bd025729090505ac8b`。

**未验证：**生产进程实际加载集合、真实模型/Bridge 请求、插件用户安装目录、四平台安装包、Rust 替代实现（本阶段尚不存在）。这些不能从静态差集、只读脚本或既有单测推出 PASS。独立审核宜重点抽查：独立扫描 oracle 与图的一致性、类型锚点、W 表锚点行号与 sink 语义、automation/Dreamina 名称碰撞的排除理由，以及 worker 是否仍掌握任务终态。
