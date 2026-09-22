# NEXT_STAGE_HANDOFF — P02 输入（P01 → P02）

日期：2026-09-21｜P01 验收基线：见 P01_RESULT.json。

## 1. 已验收坐标与环境

- 工作区 START = END = `92c6646c581883436e026c53898e8bfc34ce4d27`（零生产 commit；20 个跟踪文件修改 + 新增文件均为工作区状态，等待用户授权提交——含 build/cli-runtime-closure.json、build/open-boundary-baseline.json、export-manifest.json 三个再生成基线）
- 分支：`docs/knowledge-closeout-2026-09-21`（未切换）
- Node v24.16.0；npm 11.13.0；darwin 27.0 arm64；lockfile sha256 `a9735825cea1d018…`（未变）
- 全量 npm test 终态：**预期红 = F1 治理 4 例（与 P00 基线一致，未扩大）**；其余 14699+ 用例绿

## 2. P02 开工前必读

| 文件 | 用途 |
|---|---|
| docs/refactor-2026/P01/BOUNDARIES.md | 六类职责边界 + 7 个公共接口 + Pi↔产品状态映射方向（§3 是 P02 身份/生命周期工作的锚） |
| docs/refactor-2026/P01/PI_ADAPTER_INVENTORY.md | Pi 适配面库存 + 深路径例外 + 升级边界规则 |
| docs/refactor-2026/P01/HOST_CAPABILITY_MAP.md | 宿主能力语义（含通知"sent=已广播"语义澄清，P05 引用） |
| docs/refactor-2026/P01/STRICT_SCOPE.json | strict 核心范围（6 文件）+ 加入规则 + 例外账本（1 条 Math.random 登记） |
| docs/refactor-2026/P01/ACCEPTANCE_MAP.json | 12 场景的生产入口与证据坐标 |

## 3. 本阶段建立的门禁（P02 不得削弱）

1. `npm run typecheck:core-contracts`（CI 已接）：6 文件 strict+noUncheckedIndexedAccess+exactOptionalPropertyTypes；负例 fixtures 在 tests/fixtures/core-contracts-negative/（tsconfig.test.json exclude 保护）。
2. `npm run check:dependency-boundaries`（CI 已接）：sdk-direct-import/sdk-deep-path/host-into-core/adapter-reverse-dep/sdk-dynamic-unproven/host-dynamic-unproven 六规则（验收修复轮加固：常量拼接折叠+片段提及，拼接构造的包名不再绕过；运行时拼接为登记的固有静态边界）；例外仅 1 个（schema-validator 的 typebox/value）。**P02 新增核心契约文件若要进 strict 范围，改 tsconfig.core-contracts.json include 并保证零诊断。**
3. tests/pi-sdk-import-boundary.test.ts 扫描面已含 .ts 与全部生产根——P02 若新增 SDK 消费必须走 lib/pi-sdk。
4. 生产基线三件套（export-manifest.json / build/cli-runtime-closure.json / build/open-boundary-baseline.json）由 scripts/compute-cli-closure.mjs 再生成——**P02 改动 cli/shared 导入图后必须重跑该脚本**，否则 cli-closure-census/open-boundary-lint 契约测试红。
5. tests/quality-gates-contract.test.ts 钉住 tsconfig.json 的 packages/*/src include glob（即使目录不存在）——退役该契约需用户授权（P08）。

## 4. P02 唯一主责输入（P00 所有权图 🔴 归 P02）

- taskId 无统一铸造厂（5 种自铸格式 + 媒体任务双记账，OWNERSHIP_MAP taskId 行）——P02 统一铸造与 attempt/generation 语义。
- runId 终态/取消/恢复链（chat.ts:868/910、abortSession 三分支）已在 P00 核对收敛，P02 补 attempt/generation 区分。
- 身份品牌模块 shared/identity-brands.ts 已就绪（sess_/mc_/ma_/mt_ + require 守卫）；P02 接入 taskId 品牌时可复用该模式（同文件追加，勿建第二套）。

## 5. 已执行验证与遗留

- 已绿：typecheck 三腿、core-contracts、dependency/tool-invocation boundaries、pi-sdk 族 41 例、composition+纵向链 9 例、gateway 17 例（含 A08）、cli 8 例、contract 45 例、全量（F1 4 例外）。
- BLOCKED（环境）：`npm run build:server:open` / `smoke:server:open`——其依赖安装需 npm registry，本机代理 127.0.0.1:7890 不通且直连被拒（curl 000，F3 同源）。**等价覆盖**：server-composition-boundary Part 3/4 在开发树 spawn 真实全量组合并完成真实 HTTP/WS 请求（含纵向链）；未覆盖的仅打包 open 树变体，归 P08 产物验收时在网络恢复后执行。日志：P01-T07-build-server-open.out（终止于依赖安装）。
- BLOCKED（环境）：四平台 CI 运行证据（F3 不变，P08 取回）。
- F2（lint 3 生产 error）主责仍 P03；F1（seal 4 例）治理流程不变。

## 6. 必保留兼容（P02 不可改变）

- 观测分组口径：user_turn 会话粒度复用 mt_、后台强制新根、traceId≠sessionId（品牌类型已固化该纪律）。
- 四常驻工具+按需目录；Pi 适配层零修改纪律（升级边界见 PI_ADAPTER_INVENTORY §5）。
- strictJson 三件套目前只在 /sessions/new——P02 扩展写入口时复用 server/hono-helpers.ts 的 `strictJson` + `ensureJsonObjectBody` + `rejectWrongFieldTypes`（不要新建第二套）；字段校验只做类型层，值域/枚举归各契约 normalizer。

## 7. 工作区卫生提醒

- 全量 npm test 后检查 `git status`：`artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch` 会被 R10-09 重写（F5 既有行为），取证后 `git checkout --` 还原。本轮已还原两次。
- **全量验证的 stdout 不要重定向到仓库扫描范围内的文件**（如 artifacts/**）：R10-09 校验对工作树做两次 `git add -A` 快照，运行期并发写入会撕裂 manifest 偶发红（验收修复轮实测并登记，单独复跑/输出落 /tmp 即绿）。
- 本轮新增未跟踪目录：docs/refactor-2026/P01/、artifacts/refactor-2026/P01/、tests/fixtures/core-contracts-negative/（+新增测试/脚本/配置文件）。
- 验收修复轮补充：P01 命令级 JSONL 由 `artifacts/refactor-2026/P01/tools/rebuild-command-log.mjs` 重建（42 条 rebuilt 标注，原索引保留 log-file-index.jsonl）；后续阶段命令记录用同目录 `run-logged.mjs`（与 P00 同构）。
