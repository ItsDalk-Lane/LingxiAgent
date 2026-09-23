# R00-T04｜盘点入口、身份、数据与权限

状态：**READY_FOR_REVIEW**（R1 修复候选：按 R00-T04_REVIEW_R1.md 的 F01/F02/F03 修订打包与元数据，修复记录见 R00-T04_R1_REPAIR.md；盘点条目、锚点与链路内容未变）。本执行者完成 T04 四个 Steps 并自测 A07/A08 条件成立，不自行判 PASS；独立验收由全新任务执行。本任务只新增盘点文档、JSON 清单与只读扫描/校验脚本；未改任何产品源码、依赖、测试、任务书、总控账本或已冻结的 T01/T02/T03 文件，未提交/推送。

基线：分支 `codex/rust-tauri-migration`，HEAD = Task base = `ffcb85830ffdc75da5fe39b7a8cfddb4617f1aec`（与 T03 已推送 HEAD 一致，已核对）。`docs/rust-tauri/ORCHESTRATOR_PROGRESS.json` 的未提交总控进度修改保持原样未动。环境：macOS darwin 27 arm64，Node v24.16.0，npm 11.13.0，Python 3。

## 交付物

| 文件 | 内容 | SHA-256 |
|---|---|---|
| `docs/rust-tauri/R00/ENTRYPOINTS.json` | 35 个现役入口 + 1 休眠 + 1 残留（合计 37 条），每条含锚点与 principal/绑定/认证/权限域/负责人/资源归属/关闭策略；A08 四条链逐跳锚点 | `f69e658f…4d669` |
| `docs/rust-tauri/R00/STORES.json` | 69 store（authoritative 42 / rebuildable_cache 13 / adjacent_compatible 14；子类 9 种），含 schema 来源、读写方、进程归属、重建语义 | `6067bdd…7b493` |
| `docs/rust-tauri/R00/OWNERSHIP_CURRENT.md` | 现状所有权叙述 + epoch/指纹/拒写核对 + 7 项跨进程风险表 | `9d5d3b7a…bd51a` |
| `docs/rust-tauri/R00/r00_t04_scan.py` | 可复算生成/校验脚本（生成 STORES、锚点校验、入口计数一致性校验、路由面提取、独立持久化 oracle、确定性抽样、负向自检） | `842dfe2d…855c2` |
| `artifacts/rust-tauri/R00/T04/` | scan-output.json（最终校验输出）、final-run-stdout.txt、deliverable-hashes.txt | 见该目录 |

## Steps 完成情况

**Step 1（枚举实际入口）**：10 类入口全部以真实源码锚点登记（desktop_shell 7、renderer_clients 2、http_ws_server 7、cli 3、bridge 3、schedulers 3、channels_dm 4、subagents 1、plugins_mcp 5（现役 3 + 休眠 1 + 残留 1）、tool_boundary 2；合计 37 条登记 = 35 现役 + 1 休眠 + 1 残留，计数块由脚本 A2 检查按 entries 机械校验一致性）。REST 字面路由 392 条（脚本从 50 个路由文件提取 470 个注册位）；WS 面 2 条（`/ws` 经 ws-ticket、`/internal/browser` 原生升级带鉴权）。负空间同样登记：无深链协议注册、无入站 webhook（五平台全部出站长轮询/长连接）、`/api/plugins/dev` 生产路由已随 04f90d2b2 删除（分类残留）。覆盖面修正了一处任务书预设：`lib/sandbox/index.ts` 的 `writeFile` 是转发 shim，真实落盘在已注册的 `lib/resource-io/providers/local-fs-provider.ts`。

**Step 2（每入口七要素）**：ENTRYPOINTS.json 每条目含 principal 来源、agent/session/run 绑定、认证机制、权限域、运行负责人、资源归属、关闭策略。身份模型收口为三 principal（local_user/device/web_session）+ automation/平台用户两类非 HTTP 主体；工具授权唯一实现在会话权限包装器（网关模型面 authorize 显式抛错，engine.ts:4119-4124）。

**Step 3（持久化写入扫描）**：以仓库现成的权威登记（`shared/persistence/store-registry.ts` 69 store + 38 豁免）为基，独立文件级 oracle 复核：146 个含持久化调用 token 的生产文件中 141 个直接被 788 位点的 AST 常谱收录，5 个逐条分类（3 个子串误报、1 个转发面、1 个 DI 接收器真写但目标为 tmpdir 一次性安装脚本）。权威/缓存划分直接引用注册表 epochPolicy，缓存可重建理由引自注册表自身声明的 rebuild/loss 语义（如 knowledge ANN 从保留向量 BLOB 重建、session-checkpoints 丢失仅失回退锚点不丢对话）。

**Step 4（epoch/指纹/拒写/版本）**：DATA_EPOCH=1（`shared/contract-versions.json` 单一来源）；启动闸顺序=同宅互斥（token 探测）→ epoch 事务闸（任何 store 打开前）；旧内核拒写=`epoch-downgrade-blocked` exit 1，唯一逃生门 `LINGXI_ALLOW_DATA_DOWNGRADE=1`，半途迁移一律阻断不自动续跑；epoch=1 基线上损坏元数据降级告警但更高印章证据仍阻断（index.ts:324-347）。schema 指纹 tripwire 实测 170 守卫源无改动。OTA 列车另有 preload 契约闸（ota-core.cjs:1051-1065）。实际发布：package.json 0.1.43 = 最新 tag v0.1.43（2026-09-22），全部历史版本 epoch 恒 1，生产从未触发迁移。跨进程共写风险 7 项列入 OWNERSHIP_CURRENT.md §5（含本任务新发现的 AST 常谱 DI 接收器盲区与 MCP app-tools 绕过网关直调）。

## 验收自测证据

**R00-A07（数据写入点可追踪）**

- 前置：持久化扫描与手工核对完成（上述 Step 3）。
- `npx vitest run tests/persistence-store-registry.test.ts` → 退出码 0，14/14：每个生产持久化位点恰好一个归属（788 位点）。
- `node scripts/check-persistence-schema-fingerprint.mjs` → 退出码 0："170 watched sources, OK"。
- 独立 oracle（`python3 -B docs/rust-tauri/R00/r00_t04_scan.py --validate`）：unclassified=0、stale=0；确定性抽样 8/8 位点 excerpt 在源文件中原样存在且均有归属。
- 负向自检：伪造未覆盖写文件 → 失败触发（true）；注入假 store id → 失败触发（true）；篡改锚点 → 失败触发（true）；篡改入口计数块 → 失败触发（true）。
- 结论（自测）：无未登记的生产权威存储；13 项缓存的可重建理由均引自注册表可核实声明；5 个 oracle 差集逐条人工分类并固化在脚本中（新差集即失败）。

**R00-A08（非桌面入口权限可追踪）**

- CLI 链：`cli/entry.ts:89` → `cli/chat.ts:104`（WS `/ws` + Bearer）→ `server/index.ts:626` principal 解析（`core/server-auth.ts:63-71` 环回 token 仅 local）→ `chat.ts:2894` hub.send → `hub/index.ts:305` owner 路由 → engine.prompt → `session-permission-wrapper.ts:656` 授权 → 工具执行。
- Bridge 链：`telegram-adapter.ts:112` 长轮询 → `bridge-manager.ts:1097` _handleMessage → `:1127` owner 判定（`owner-policy.ts:8-17`）→ `:1995` hub.send → `hub/index.ts:330` executeExternalMessage → `bridge-session-manager.ts:764` owner 会话 → 工具边界。
- cron 链：`cron-scheduler.ts:42` tick → `cron-store.ts:746` markRun CAS 收据 → `scheduler.ts:457` executeIsolated（`:389-402` automation 权限选项）→ `session-coordinator.ts:8048` 隔离会话 → 工具边界。
- 开发调用链：vite dev 渲染端（`cors-policy.ts:1` localhost 白名单）+ preload token（`preload.cjs:34-35`）→ HTTP → hub.send → 同桌面链。
- 结论（自测）：四条链身份/授权来源均有源码锚点；Bridge 平台账号体系列为外部信任风险（R-T04-05）而非已证实安全；MCP app-tools 直调不经网关列为 R-T04-04。链路为静态追踪+既有测试支撑，未跑真实模型/真实平台（见未验证范围）。

## 实际命令与结果

| 命令 | 退出码 | 结果 |
|---|---|---|
| `python3 -B docs/rust-tauri/R00/r00_t04_scan.py`（生成+校验） | 0 | STORES 69 项写入；A=101 锚点 0 错、A2 入口计数一致（35 现役 + 1 休眠 + 1 残留 = 37）、B=392 路由、C=69↔69、D=0 未分类 0 过期、E=8/8、负向 4/4；`R00_T04_SCAN_OK` |
| `npx vitest run tests/persistence-store-registry.test.ts tests/persistence-schema-tripwire.test.ts tests/http-route-security.test.ts tests/server-auth.test.ts tests/ws-scope.test.ts tests/device-registry.test.ts` | 0 | 6 文件 74/74 通过 |
| `node scripts/check-persistence-schema-fingerprint.mjs` | 0 | 170 守卫源无改动 |
| `python3 -m py_compile docs/rust-tauri/R00/r00_t04_scan.py` | 0 | — |

## 改动与原因

仅新增 5 个文件 + artifacts/T04 目录，原因：T04 交付要求 ENTRYPOINTS.json / STORES.json / OWNERSHIP_CURRENT.md；扫描脚本是为满足"可复算证据"并把 5 个 oracle 差集的判定固化为可再验证的护栏（新差集即失败，防止把本次人工分类变成一次性口头结论）。无产品行为变化，无需扩大回归。

## 未验证范围

- 未运行真实模型、真实平台账号、真实外部连接器；A08 四链为源码级追踪+既有测试（含 T01 启动探针、路由安全、认证、ws-scope、设备注册表 74 项）支撑，不等于 LIVE 验证（任务书允许真实账号验证延后 R10）。
- 未实测桌面 GUI dev 窗口、未做睡眠唤醒注入、未验证四平台安装包。
- `build/persistence-store-inventory.json` 为已提交版本（Sep 21）；本任务以运行时重扫（测试内重扫并比对一致）+ 独立 oracle 交叉验证，未重新提交该文件（不在本任务权限内）。
- 历史发布版本 epoch=1 结论基于 contract-versions 的版本规则与 git tag/release-digest 核对，未逐一签出旧 tag 重跑。

## 风险与移交

- R-T04-03（AST 常谱 DI 接收器盲区）与 R-T04-04（MCP app-tools 绕过网关）是本任务新发现，建议 R02/R04 设计时显式接收；R-T04-01（双内核冷启动竞态）为上游已自认残余。
- 休眠开发面（gateway local-developer）三处残留已登记（EP-DEV-01/02），R04 统一网关时裁决。

## 工作区状态与独立验收重点

工作区：仅新增上述文件；`docs/rust-tauri/ORCHESTRATOR_PROGRESS.json` 保持进入时未提交修改，未被本执行者触碰。未 commit/push/PR/tag/release。

建议独立验收重点：① 抽 5-8 个 ENTRYPOINTS 锚点到源码复核行号与语义（脚本可一键复核全部 101 个）；② 重跑 `python3 -B docs/rust-tauri/R00/r00_t04_scan.py` 与上表命令核对退出码；③ 对 STORES.json 抽 3 个 regenerable store 核对其 rebuild_or_loss_semantics 与注册表原文一致；④ 复核 5 个 oracle 分类的人工判断是否成立（尤其 mac-self-install 的 tmpdir 判定）；⑤ 抽一条 A08 链（建议 cron）从入口走查到 session-permission-wrapper 的 deny_on_prompt 语义。
