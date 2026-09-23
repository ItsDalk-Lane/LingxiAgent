# R00-T03 独立验收 R1

**VERDICT: FAIL**（任务整体）。**R00-A05: PASS；R00-A06: PASS。**两项基础场景通过，但任务第 3、4 步要求的运行依赖触发条件和归属清单存在阻断错误。验收者未参与候选编制；本报告只依据当前源码、锁文件、候选文件和本轮重跑结果。

## 候选与边界

- Task base：`4b4a1d98f6d2d0e03f9573aa75b7992efed03db6`；审核 HEAD：`60dbe0384e553f0a0aab7d173b2dc1ba43987b82`。两者之间的已提交差异仅为 `.gitignore` 和旧任务书解除跟踪，没有产品源码或依赖锁变化。
- 8 份候选文件按文件名排序、以 `SHA256  文件名\n` 聚合的摘要为 `fa1fd110e93dd1e754c23ebc885db7e07a3635bfe2b2144cb25022683efd0cda`；`R00-T03_REPORT.md` 单文件摘要为 `d08ceb5053aaf48e803edd941ff78ac0067ec80ba60795b043a5363633755f92`，与冻结值一致。`package-lock.json` 未改。
- 本轮未启动真实产品、未调用模型或平台账号、未触碰用户数据。仅新增本审核报告；原有 `ORCHESTRATOR_PROGRESS.json` 改动和 8 份候选保持原样。

## 独立复核

### R00-A05：PASS

从 `server/main-full.ts` / `server/main-open.ts` 到 `server/index.ts`、`core/engine.ts`、`core/session-coordinator.ts`，从 Electron `desktop/main.cjs` 跨进程到 `server/bootstrap.ts`，以及 CLI `cli/entry.ts` 的本地启动/连接链，均能到达旧 Pi 会话。Bridge 在 `server/index.ts:871` 延迟加载管理器；后台在 `hub/scheduler.ts:457` 通过注入的 `engine.executeIsolated` 到 Pi；`lib/llm/cache-preserving-compaction-agent-run.ts:550` 直接调用 `runAgentLoop`。插件目录通过 `core/fresh-import.ts:14` 动态加载，候选明确保留了无法静态证明用户自装插件的限制。

本人独立使用 TypeScript AST 重新扫描 1664 个受控源码文件：20 处 Pi 包导入、16 处非字面量动态加载与图中逐位置相符；另核对 15 处现役 `pi.on` hook、47 个具名/命名空间引用的 facade 符号与 14 项职责的差集。脚本所报 97 处 adapter 导入含相对路径解析；本人单独用字符串形式扫描得到 85 处，未发现候选漏记的相对路径。`lib/pi-sdk/index.ts:25` 的 `AuthStorage` 和 `:52` 的 `prepareCompaction` 是真实深路径导入。`scripts/patch-pi-sdk.cjs` 中没有写文件调用；重新执行它退出码 0，三个已安装 Pi 目标文件前后哈希相同。

此结论是现有仓库源码的生产可达性盘点，不等同于真实进程加载证明；候选已把这个限制说明清楚。运行时用户插件需在后续阶段登记。

### R00-A06：PASS

本人直接读取 `hub/agent-executor.ts:290,343,556,716`：它创建 Pi session 并多轮 `prompt`。将该实际对象的分类在内存中改成 `PERIPHERAL_CANDIDATE` 时，独立负例检查拒绝了分类；原矩阵是 `KERNEL_MIGRATION`。另核对 `hub/scheduler.ts:457,514` 的隔离执行和活动终态、`core/bridge-session-manager.ts:1305` 的会话创建，以及压缩旁路的 `runAgentLoop`，均没有因 hub、Bridge、cron 名称而留作完整 Node 内核。`plugins/office/lib/read-document.ts` 的单次解析列为有条件外围候选，边界方向正确。

### 实际重跑与测试效力

- `node scripts/patch-pi-sdk.cjs`：退出码 0，Pi 目标文件摘要不变。
- `npx vitest run tests/pi-sdk-import-boundary.test.ts tests/hub-plugin-session-agent-capabilities.test.ts tests/cron-scheduler.test.ts tests/office-plugin-tools.test.ts ...`：4 个文件、32 个测试通过，退出码 0。这些既有测试只作相关回归，不代替本任务的依赖分类验收。
- 独立比对 `package.json`、`package-lock.json` 和 51 行清单：50 项顶层生产/可选依赖均在清单，另有 Electron 宿主；锁版本均相符。`R00-T03_VALIDATION.json` 的 27 项检查仅验证清单存在、版本非空及部分差集，**没有验证每行 activation/source_refs/owner 是否与真实使用链一致**；其绿色结果不能覆盖下面的发现。

## Findings

### F01｜BLOCKING｜多项运行依赖被误写为按需前端或文件操作触发

**位置：**`RUNTIME_DEPENDENCIES.json` 的 D10、D11、D13、D40、D41、D49、D50；生成根因在 `r00_t03_build_matrices.py` 的 D10/D11/D13 固定描述及 `EXTRA_DIRECT` 统一套用的 activation/通用归属。

**证据与反例：**

- D10 `mammoth` 写“Office/read 或文件路由按需加载”，D11 `exceljs` 写“表格文件操作时动态加载”，D13 `jsdom` 写“知识来源或 Office 操作触发”。然而 `lib/knowledge/source-processors.ts:4-6` 顶层静态导入三者，`lib/knowledge/knowledge-manager.ts:47` 顶层引用该模块；`lib/knowledge/source-adapters.ts:1` 也静态导入 `jsdom`。候选自身的导入图把这些包标为从 `server/main-full.ts` 和 `server/main-open.ts` 静态可达。一次普通服务组合加载就包含这些依赖，不能以“读取 Office 文件时才动态加载”描述打包/启动依赖。
- D41 `js-yaml` 和 D50 `typebox` 的 activation 同写“相关前端视图/辅助操作按需加载”。`core/first-run.ts:10`、`core/agent-manager.ts:10`、`core/provider-catalog.ts:3` 是服务端配置读取；`lib/tools/invocation/schema-validator.ts:2` 直接导入 `typebox/value`，`core/engine.ts:4133,4186,4390` 用它建立工具执行参数校验。它们不是只供前端视图的库。D50 的 Rust ToolRegistry 归属方向虽正确，触发描述仍误导边界。
- D40 `diff` 的前三条 source_refs 全是 React 文件，漏掉 `lib/resource-io/file-change-presentation.ts:2` 的服务端使用。D49 `semver` 归为泛用“文本差异/配置解析/版本比较辅助库”，写成前端按需触发，但 `desktop/src/shared/artifact-boot.cjs:59` 与 `shared/artifact-core/ota-core.cjs:167` 经 `shared/artifact-core/release-order.cjs:10` 在桌面启动/更新产物先后判断中使用。这里的旧产物兼容和升级排序需要具体负责人及协议测试，不能留作“Rust 对应服务逻辑或 React 展示按实际调用方分工”。

**复现：**运行 `rg -n '^(import|const).*?(exceljs|mammoth|jsdom|js-yaml|typebox|diff|semver)' lib/knowledge/source-processors.ts lib/knowledge/source-adapters.ts core/first-run.ts core/agent-manager.ts core/provider-catalog.ts lib/tools/invocation/schema-validator.ts lib/resource-io/file-change-presentation.ts shared/artifact-core/release-order.cjs`，再查看 `R00-T03_IMPORT_GRAPH.json` 中相应 `external_imports[].reachable_from`。D49 的桌面调用链由上述两处 `require("release-order.cjs")` 复核。

**为什么阻断：**R00-T03 明确要求区分实际使用、可选触发和 Rust/外围归属。当前表会让后续阶段误以为办公库无需随服务启动打包、服务端 schema/配置处理只属于前端，或把旧更新排序行为从迁移验收中漏掉。根因是依赖清单生成时先按包名套统一文案、只取前几个引用，没有按真实调用者和加载时点逐项复核。相同问题须遍历其余 51 行，而非只改这七行。

**修复要求：**按完整调用链把每项的“模块加载时点”和“实际功能调用时点”分开；补齐服务端/桌面/前端的真实消费者、明确 Rust 负责人或保留外围的边界，并为产物版本排序、配置解析、工具 schema、知识文档处理分别给出可执行对照检查。重生成候选后，重跑导入图和差集，独立抽查全部 51 行的 source_refs/activation/owner；至少复测服务组合入口、工具校验和桌面产物排序。不能仅修改验证器预期让 27 项仍为绿色。

## 结论与后续验收

Pi 职责与第二内核这两项基础场景在当前静态范围内可判 PASS；运行依赖账本未满足任务交付要求，故 **R00-T03 仍为 FAIL**。修复后需由新的独立验收者从源码与锁文件重新完整审核，不沿用本轮 PASS 作为下一候选的自动结论。
