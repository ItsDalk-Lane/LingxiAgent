# R02 最终收口修复 — 根因组 8（契约钉住物 vs R17–R21 授权接线漂移）R1

- 子代理：`R02-REPAIR-GROUP-8-R1`
- 仓库：`/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`，HEAD `cdd213078`
- 环境：Node v24.16.0，darwin arm64
- 范围：6 个测试文件红；判定「漂移」（按文档化流程重算/对齐钉住物）或「产品缺陷」（修产品），修后单文件复跑 + 全量回归。未 commit / 未 push。
- 工作区基线：已有并行修复的未提交改动（rust/、docs/rust-tauri/、scripts/rust-tauri/ 等），本组只动了下述 8 个文件（含 1 个 rename）。

## 修复 1 — settings 原语契约（settings-primitives-contract.test.ts）

- **判定：漂移（产品接线写法），修产品代码。**
- 取证：失败断言是
  `expect(source).toMatch(/<SettingsPage tab=\{effectiveActiveTab\}[^>]*>\s*<ActiveTab \/>\s*<\/SettingsPage>/s)`
  —— 契约要求每个 tab 的页体是 `<SettingsPage ...><ActiveTab /></SettingsPage>` 原语形态。R17（cdd213078）把 `<ActiveTab />` 换成了 ternary：
  `{rustSettingsUnavailable && effectiveActiveTab !== 'access' ? <div role=alert>…</div> : <ActiveTab />}`，
  打破了「唯一子节点 = `<ActiveTab />`」的结构契约。同文件其余 4 个断言全绿。
- **修法**：`desktop/src/react/settings/SettingsContent.tsx` 把 ternary 拆成两个分支，各自持有完整 `<SettingsPage tab={effectiveActiveTab} layout=...>` 包装——不可用分支为 `<SettingsPage ...><div role="alert">…</div></SettingsPage>`，正常分支恢复 `<SettingsPage ...><ActiveTab /></SettingsPage>`。产出的 DOM 与拆分前逐字节一致（`div[data-settings-page] > div[role=alert]`，layout 表达式不变；page flow/fill 对单个 alert div 渲染无差别），用户可见行为零变化，契约正则重新命中。
- 验证：`npx vitest run desktop/src/react/settings/__tests__/settings-primitives-contract.test.ts` → 5/5 绿，exit 0（2026-09-28 23:55 UTC+8 前后）。

## 修复 2 — CLI 闭包普查 fail-closed 动态调用（cli-closure-census.test.ts）

- **判定：漂移 + 真实运行时多态 → 机制内登记。**
- 取证：唯一 fail-closed 命中 `cli/rust-service.ts:156 execFileSync(reader)`。`reader` 是 `resolveWindowsRustReader()` 在运行时解出的 Windows Rust reader 二进制（lingxi-service.exe）路径——先验 build.json manifest 字段、PE 头机器位、sha256 全量摘要、toolchain 与 rust 源摘要，再按安装布局（dev `dist-rust-service/win-<arch>/` 或激活后的 server artifacts 树）返回绝对路径。路径真动态、依赖布局，**无法收敛为字面量+映射表**（不满足「优先字面量」的前提）；目标是被打包的原生产物而非仓库源码，按设计永不入模块闭包——与既有 `core/speech-recognition/system-speech-adapter.ts spawn(helper)` 条目同一模式。
- **修法**：
  1. `scripts/compute-cli-closure.mjs` 的 `DYNAMIC_CALL_ALLOWLIST` 追加命名条目（file=cli/rust-service.ts, callee=execFileSync, argText=reader，reason 完整写明验证链与「packaged native artifact, not repo source」理由）。这是机制设计内的正当登记，非遮红：fail-closed 保持原样，调用点文本一变仍会红。
  2. 用 canonical 生成命令 `node scripts/compute-cli-closure.mjs` 重算 `build/cli-runtime-closure.json` + `build/open-boundary-baseline.json`（baseline 字节级未变）。
  - closure diff 审查：仅两类新增——(a) dynamicCallSites 对 allowlist 的回显；(b) 源图新文件 `cli/rust-service.ts`（provenance: cli/entry.ts import-statement）+ stats 8949→8950 / source-graph 835→836。无其他漂移。
- 验证：`npx vitest run tests/cli-closure-census.test.ts` → 23/23 绿，exit 0（23:56 UTC+8，62s）。

## 修复 3 — open boundary 棘轮（open-boundary-lint.test.ts）

- **判定：漂移（manifest 未跟上授权新增文件）。**
- 取证：`node scripts/lint-open-boundary.mjs` 输出唯一新边 `cli/entry.ts:10 -> cli/rust-service.ts`（不在 `build/open-boundary-baseline.json`，exit 1）；baseline 由 `classifyRepoPath` 生成、与 manifest 无关且重算后字节未变，故缺口在 hand-maintained 的 `export-manifest.json`。
- **修法**：`export-manifest.json`（头部自述「seed; hand-maintained from this point forward」）按字母序新增 `"cli/rust-service.ts"`。该文件是开放 CLI 核心的 Rust 服务客户端连接逻辑（readRustLocalService / explicitRustConnection / RustCliClient / Windows reader 解析验证），与已列白的 `cli/server-runner.ts`、`cli/client.ts` 同族，无闭源产品面。
- 审查：R17 新增的另两个文件（desktop/src/shared/rust-local-service.cjs、private-server-artifact.cjs）不产生白名单内→外的边，lint 无须登记。
- 验证：`node scripts/lint-open-boundary.mjs` → `lint:boundary ok -- 1 known open->closed edge(s)` exit 0；`npx vitest run tests/open-boundary-lint.test.ts` → 16/16 绿，exit 0（23:59 UTC+8）。

## 修复 4 — 持久化指纹重钉（persistence-schema-tripwire.test.ts）

- **判定：漂移（授权接线改了受护源，指纹未重钉）；无未解释的 schema 语义变化。**
- 取证流程（按要求先核实再重钉）：
  1. 指纹最后钉于 `8d55046d5`；从指纹提取 170 个受护文件（siteMappings[].sourceFile + schemas[].module/extensions/protocolModules），`git diff --name-only 8d55046d5 HEAD` 命中 3 个：`desktop/main.cjs`、`shared/artifact-core/activation.cjs`、`shared/artifact-core/ota-core.cjs`，且全部仅来自 cdd213078（R17–R21 授权提交）。
  2. 逐个审阅 diff：activation.cjs（+10，privateArtifactGuard prepare/verify/seal 钩子与 win32 打包守卫）、ota-core.cjs（+17，guard 透传与 privateRootReady 状态门）、desktop/main.cjs（+192，startRustServer/身份验证/serverNodeKind+Transport 状态/IPC get-server-connection-info/win32 rust 关停分支）。**均无持久化形状、DATA_EPOCH 或写点集合变化**。
  3. 交叉验证：`tests/persistence-store-registry.test.ts` 与 `tests/persistence-startup-receipt.test.ts`（新扫描 vs 提交 inventory/receipt）本来就绿 → R17 未新增持久化写点。
  4. 结构定位：指纹只对 schema/extension/protocol 模块哈希；artifact-core 两文件仅出现在 siteMappings（不带哈希），故 diff 只有 `desktop/main.cjs` 的 sourceHash 变化——漂移完全可解释。
- **修法**：按 guard 指定的重钉流程
  `node scripts/generate-persistence-schema-fingerprint.mjs --classification compatible --compatibility-reason "<完整理由：R17–R21 接线改动三个受护源，无持久化形状变化，inventory/receipt 不变>"`
  payloadFingerprint `sha256:a4e41…` → `sha256:2ed75f…`（与失败信息里的生成值一致），diff 仅 4 处：payloadFingerprint×2、review 理由、desktop/main.cjs sourceHash。
- 验证：`npx vitest run tests/persistence-schema-tripwire.test.ts` → 15/15 绿，exit 0（00:01 UTC+8）。

## 修复 5 — server 启动诊断契约（server-startup-diagnostics-contract.test.ts）

- **判定：R17 接线改写了关停结构 → 对齐契约测试定位方式，保持并增强断言强度；产品代码无缺陷。**
- 取证：
  - 字符串断言红：测试期望 win32 关停分支字面量 `if (process.platform === "win32") {\n await requestServerShutdown(...)`；R17 把四处条件改为 `process.platform === "win32" && serverNodeKind !== 'lingxi-service'`（Rust 服务不经 Node 原生 Job guardian，直杀 + 只验自有句柄退出）——产品语义正确且必要。
  - `ReferenceError: serverNodeKind is not defined`：三个 vm-exec 用例把抽取的 `shutdownServer` 源注入沙箱，沙箱未定义该模块级变量。是**测试脚手架漂移**，不是产品引用了不存在的变量。
- **修法**（tests/server-startup-diagnostics-contract.test.ts）：
  1. 字符串断言更新为含 gate 的精确分支形态（token-auth grace 仍是 Node 路径第一步）＋原有「requestServerShutdown 先于 requestWindowsServerGuardianStop」顺序断言保持。
  2. 三个既有 vm 用例 prologue 补 `let serverNodeKind = null;`，继续真实验证 Node/guardian 语义（未弱化：unconfirmed guardian 保留、reused 不裸杀、125 收敛失败各分支原样通过）。
  3. **新增** vm 用例「shuts down an owned Windows Rust service directly without the Node guardian pipeline」：`serverNodeKind='lingxi-service'` + win32 下断言 shutdownRequests=0、controlStops=0、confirmChecks=0、kill 序列 = `["SIGTERM", undefined]`（先 TERM 后强杀）——把新 Rust 直杀语义也钉住，强度净增。
  4. 过程中发现自身一次小错：`killCalls` 未传入 `vm.createContext` 导致 ReferenceError 被产品 try/catch 吞掉；补 sandbox 字段后通过。
- 验证：`npx vitest run tests/server-startup-diagnostics-contract.test.ts` → 39/39 绿，exit 0（00:03 UTC+8）。

## 修复 6 — desktop-rust-local-service 套件级失败（原 .test.cjs）

- **判定：不是加载错误被吞——是 runner 形态不匹配（取证结论与任务书假设不同，如实修正）。**
- 取证：
  - `node --test tests/desktop-rust-local-service.test.cjs` → 9/9 全过、exit 0（62ms）。require 链、模块路径、导出全部健康。
  - vitest 收集该文件时，node:test 在 import 期立即执行用例（stdout 出现 ✔ 行）但 vitest 注册不到任何 suite → 「No test suite found in file」套件级失败。该文件是仓库唯一 `tests/*.test.cjs`，cdd213078 新增、从未绿过；R17 的 build.yml 改动与运行该文件无关，npm test（vitest run）也无 node --test 步骤——即该门禁下此文件必然红。
- **修法**：转为仓库现行 runner 形态，`git mv` 为 `tests/desktop-rust-local-service.test.mjs`（保留历史）：
  - `require('node:test')` → `import { test } from 'vitest'`（vitest 的 CJS 入口显式拒绝 require，故必须 ESM）；
  - `node:assert/strict` 与全部 9 个用例体、断言逐字保留；
  - `t.after` → `nodeTestContextAdapter(ctx)`（`ctx.onTestFinished`），保持 `stagedBinary(t)` 调用形态；
  - ESM 下补 `__dirname`（fileURLToPath(import.meta.url)），SUT 两个 .cjs 的 `module.exports = {...}` 命名导入经 interop 正常解析；
  - `.gitignore:99 *.log` 与本问题无关（确认）。
- 验证：`npx vitest run tests/desktop-rust-local-service.test.mjs` → 9/9 绿，exit 0（00:07 UTC+8）。

## 集中复跑

- 2026-09-28 16:07:40Z（00:07 UTC+8）六文件合并复跑：
  `npx vitest run desktop/…/settings-primitives-contract.test.ts tests/cli-closure-census.test.ts tests/open-boundary-lint.test.ts tests/persistence-schema-tripwire.test.ts tests/server-startup-diagnostics-contract.test.ts tests/desktop-rust-local-service.test.mjs`
  → **Test Files 6 passed (6)，Tests 107 passed (107)**，exit 0。
- `npm run typecheck`（tsc×3）→ exit 0（16:08Z 前后）。
- 全量 `npm test`：见下节。

## 全量 npm test 结果

- 2026-09-28 16:11–16:13Z：`npm test`（全量，两次运行结论一致）
  → **Test Files 3 failed | 1472 passed | 3 skipped (1478)；Tests 6 failed | 15037 passed | 15 skipped (15058)**，vitest exit 1。
- 红清单（恰好 = 任务书预期的封印三件套，均为「已验证坐标滞后于未提交工作区」的封印工作流项，不属于本组 6 文件，也未出现其他红）：
  1. `tests/post-verification-audit-seal.test.ts`（1 failed）—「changes since VERIFIED_SOURCE_SHA are audit-only (allowlist enforced)」：`.sync-audit/verified-source-sha.txt` 钉住的坐标 vs 当前未提交工作区（含本组 8 文件与并行代理改动）。
  2. `tests/round2-delivery-evidence.test.ts`（3 failed）— R10-03/R10-04 等：manifestSourceRef，同一坐标滞后根因。
  3. `tests/round3-delivery-evidence.test.ts`（2 failed）— 同族（round3 patch/manifest vs 工作区漂移）。
- 6 个修复文件在全量运行中全部绿（107/107）；除上述三件套外无任何其他红，**无需停下扩登**。按 AGENTS.md 封印流程约定，坐标重钉留待获授权的提交/封印步骤，不在本子代理范围。

## 本组改动清单（工作区，未提交）

| 文件 | 改动 |
| --- | --- |
| `desktop/src/react/settings/SettingsContent.tsx` | ternary 拆双 SettingsPage 分支（DOM 等价） |
| `scripts/compute-cli-closure.mjs` | DYNAMIC_CALL_ALLOWLIST 登记 cli/rust-service.ts execFileSync(reader) |
| `build/cli-runtime-closure.json` | canonical 脚本重算（+reader 回显、+rust-service 源图条目） |
| `build/open-boundary-baseline.json` | 重算后字节未变（无需改动） |
| `export-manifest.json` | 新增 `cli/rust-service.ts` 白名单条目 |
| `build/persistence-schema-fingerprint.json` | compatible 重钉（a4e41…→2ed75f…，review 理由随附） |
| `tests/server-startup-diagnostics-contract.test.ts` | gate 对齐 + serverNodeKind 沙箱补齐 + 新增 Rust 直杀 vm 用例 |
| `tests/desktop-rust-local-service.test.cjs → .test.mjs` | node:test/CJS → vitest/ESM（用例与断言逐字保留） |

## 红线自查

- 未删除任何测试、未把断言改永真、未扩大豁免名单遮红：allowlist 条目是机制设计内的命名登记（每条带完整理由），export-manifest 是其文档化 hand-maintained 语义内的正常补录，指纹重钉走了 guard 自己输出的 canonical 流程且 classification/reason 如实。
- 未 commit / 未 push；未动并行代理负责的 rust/、docs/rust-tauri/、scripts/rust-tauri/ 文件。
- 本地结果不代替其他平台 / 正式打包 / 真实供应商验证。
