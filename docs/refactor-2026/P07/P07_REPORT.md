# P07 阶段执行报告｜可测量的启动、运行与界面性能优化

## 结论

**PASS_WITH_BLOCKED_ITEMS。** 12/12 验收场景 PASS；W3/W4 补全（P00 实验受限项，backlog P07-1）完成；同环境配对复测无任何关键指标退步。**本阶段生产代码零改动**——这是测量后的结论而非回避：启动热点在模块编译（产物 bundle 形态已消 -18%）且延后机制既有+有测试锁定；流/历史/UI/取消各面实测远低于用户可感阈值；唯一量化热点（stream-store trim O(n)/append）位于 P05 锁定的 streamId/seq 恢复协议承载面（P06 交接 §9 明令零触碰），影响有界（最差 40µs/append、仅 >5000 事件 turn），按任务书 T01.4 登记延后并给出最小修复方案。BLOCKED 项均为继承（真供应商/真模型：无凭证与费用授权），非本阶段新增。

## 实际输入

- START = END 候选 = `52dbb45f9`（P06 提交后 HEAD；工作区零生产 commit；本阶段改动 = 2 个新测试文件 + artifacts/refactor-2026/P07/（工具/样本/日志） + docs/refactor-2026/P07/，全部未提交，等编排层统一提交）
- 分支 `docs/knowledge-closeout-2026-09-21`（未切换）；Node v24.16.0 / npm 11.13.0 / darwin 27.0.0 arm64；lockfile sha256 `a9735825…`（与 P00 基线完全一致 → 配对比较有效）
- 研究基线 `8037fae7a6f6…` 漂移映射：P00 样本生成于 92c6646c（重构前）；本阶段按协议同环境同输入复测对照，漂移归因 P01–P06 已验收改动（本阶段零生产 diff，git status 可证）。

## 任务逐项结果

| 任务 | 实现 | 生产入口 | 测试/日志 | 旧路径去向 | 状态 |
|---|---|---|---|---|---|
| T01 热点重测与选择 | UNCHANGED_VERIFIED + 新测量（W1/W2/W5 复测、W3 两层、W4 三规格、cpu-prof、trim 微基准） | server/main-full.ts；chat.ts:776；session-stream-store.ts:144（定位未改） | P07-T01-* 命令族 | 无改动 | PASS（HOTSPOT_REPORT.md） |
| T02 启动关键路径 | UNCHANGED_VERIFIED + A03 行为测试新增 | server/index.ts:406/445/860/1277/1312 | P07-T02-lazy-init（3 例）+ 既有结构测试:491 | 无改动 | PASS（STARTUP_CRITICAL_PATH.md） |
| T03 历史读取与重复投影 | UNCHANGED_VERIFIED（P05 锁定面零触碰） | server/history-read/*；routes/sessions.ts:316 | phase A/B + 定向 3 套件 | 无改动 | PASS（HISTORY_COST_REPORT.md） |
| T04 流式 UI | UNCHANGED_VERIFIED + wall-clock 测试新增 | desktop/.../use-stream-buffer.ts | p07-stream-buffer-wallclock 2 例 | 无改动 | PASS（UI_PROFILE_REPORT.md） |
| T05 CPU 隔离决策 | NOT_APPLICABLE（无热点证据）+ 既有 worker 设施复核 | ptc-runtime.ts:284 等 | cpu-prof/W3/W4/soak 证据 | 无改动 | PASS（CPU_OFFLOAD_DECISION.md） |
| T06 长运行资源 | UNCHANGED_VERIFIED + soak 基准（×2 全量） | session-stream-store/history cache/terminal manager | P07-T06-bench-soak(-r2) | 无改动 | PASS（SOAK_RESOURCE_REPORT.md） |
| T07 配对基准与总回归 | 新增对比报告 + 全量门禁 | — | P07-T07-* 命令族 | 无改动 | PASS（P07_PERFORMANCE_COMPARISON.md） |

## 场景逐项结果

见 `docs/refactor-2026/P07/ACCEPTANCE_MAP.json`（A01–A12 逐条：测试路径/用例名/fixture 摘要/生产入口/预期/实际/命令 ID/证据/状态；全部 PASS）。

## 接线和旧路径

本阶段零生产改动 → 无新消费者/无双写/无隐式回退。新增资产的真实消费者：
- `tests/p07-startup-lazy-init.test.ts` → 锁定 server/index.ts startBridgeManager single-flight（A03）。
- `desktop/.../p07-stream-buffer-wallclock.test.ts` → 锁定 StreamBufferManager 合并/终态契约（A06/A07 回归锚点）。
- `artifacts/refactor-2026/P07/tools/bench-*.mjs` → P08 产物验收的性能工作负载（P07_PERFORMANCE_COMPARISON §7 清单）。

## 验证（命令、exit code、日志；首次失败与重跑都记录）

| 命令 | exit | 备注 |
|---|---|---|
| P07-T07-typecheck → -r2 | 1 → 0 | 首跑 1 处新测试类型错（class 表达式提升），修复重跑绿 |
| P07-T07-lint → -r2 | 1 → 0 | 首跑 P07 工具 18 err（Node 全局显式导入/duplicate key），修复重跑全绿 |
| P07-T07-build-renderer | 0 | vite build renderer |
| P07-T07-bench-compare-selftest | 0 | 假优化判 INVALID（A11） |
| P07-T07-directed-suites | 0 | 15 文件 113 例（启动契约/流store/历史目录×3/UI性能/取消边界/p06×3） |
| P07-T07-core-contracts | 0 | core-contracts-strict + tool-invocation-boundaries + dependency-boundaries + lint-open-boundary |
| P07-T07-full-test | 1（预期） | 全量：**4 红 = F1 已知基线同一组**（seal 旧坐标/round3 manifest/R10-03/R10-04，与 P05/P06 逐名一致）；14797 绿 / 1 expected fail / 15 skipped（较 P06 +5 = 本阶段新增 5 例）；审计封印红因旧坐标属已知，不影响本阶段（AGENTS.md 审计封印节） |
| P07-T07-restore-patch | 0 | f1-f12 patch 还原，哈希复核 25fb315f… |
| P07-T01-bench-memory → -r2 | 1 → 0 | 首跑 Bearer token 与隔离 HOME 令牌不一致 401，修复重跑 |
| P07-T01-bench-w3l2-w4 → -r2 → -r3 | 0 → 1 → 0 | r2 因 node:abort-controller 内建不存在失败（lint 修复引入），改 globalThis 后绿 |
| P07-T02-build-server | 1（部分） | bundle/index.js 产出可用；seed 签名需 LINGXI_SIGN_KEY（发布凭据，P08 范畴） |

环境受阻单列：真供应商冒烟与真模型行为评测维持 BLOCKED（无凭证/费用授权）；四平台 CI 证据取回属 P08。

## 数据、权限与平台

- 零 schema 变更、零迁移（P05 DATA_COMPATIBILITY 全表继续有效）；指纹守卫未触碰。
- 测试与基准全部合成数据/隔离 HOME/本地 HTTP 替身；未读写真实用户 HOME、会话、记忆、凭证。一次启动日志采集初犯未设隔离 LINGXI_HOME：被外置 server 探测保护立即拒绝（零副作用，保护行为符合设计，已如实登记并全部改用隔离 HOME 重跑）。
- 平台：darwin arm64 单机（协议声明比较须同环境成对——已满足）；Windows/Linux 产物验收属 P08。

## 差异与限制

1. **soak 未纳入真实浏览器开合循环**：BrowserManager 需真实浏览器宿主（Electron 会话），本环境无 GUI 授权；已用进程级句柄/内存轨迹+缩略图定时器自停代码核对替代，登记为限制（非虚报覆盖）。
2. W4-S2（流中取消）6 样本、S3 单样本（进程树实测），低于 30 连续样本口径——场景时长所限，如实标注。
3. 桌面 Electron 窗口级启动（window.show→可交互）未测：W1 协议的计时终点为 server-info.json（引擎与安全策略就绪后写入），桌面端轮询该文件为可用性权威；GUI 自动化未获授权。
4. 遗留登记（未触碰）：P04 NEXT_STAGE_HANDOFF §2 措辞项、P06 fix_rounds 计数瑕疵（本阶段未合法触碰对应文件，按编排约束保留原样，留给后续合法触碰者）；R10-09 顺序依赖波动（P04 起已知，登记在案）；H1 热点延后（见 P07_RESULT.json hotspots）。
5. 范围外建议（不做）：见 HOTSPOT_REPORT §7（compile cache、双事件发布收敛等）。

## 回退与下一阶段

- 回退：生产代码零改动 → 无代码回退项；新增测试/工具/文档独立可删（删除 `tests/p07-startup-lazy-init.test.ts`、`desktop/.../p07-stream-buffer-wallclock.test.ts`、`artifacts|docs/refactor-2026/P07/` 即完全恢复原状）；dist-server 为 gitignore 的构建产物目录（可整体删除重建）。
- 数据保护：无任何用户数据/缓存变更；bench 工具临时目录用后即删；soak 工具原版 `process.exit()` 在 try 内跳过 finally 清理、每次运行（含成功）泄漏 ≈12MB 临时目录（验收 F-A），**P07-FIXR1 已修复并实测复跑确认用后即删**（见下方 P07-FIXR1 节；该句为 P07-FIXR1 更正，原文「soak/bench 全部临时目录用后即删」与验收实测不符）。
- 下一阶段交接：`docs/refactor-2026/P07/NEXT_STAGE_HANDOFF.md`（P08：旧路径退出、全产品回归与发布准备；含本阶段可复用工作负载与 H1 解锁建议）。

## 独立验收修复轮（P07-FIXR1，2026-09-22，两项）

来源 = 独立验收 [P07_ACCEPTANCE_REVIEW.md](P07_ACCEPTANCE_REVIEW.md) §4 低严重度发现 F-A/F-C（F-B 的 run-id 后缀与 samples/ 纳入清单属结构性建议，验收已建议归 P08，本轮不做）。零生产代码、零测试断言改动（`git diff HEAD` 复核为空；2 个新测试文件与全部交付物为未跟踪/已交付状态）。本轮只触碰：`artifacts/refactor-2026/P07/tools/bench-soak-resources.mjs`、SOAK_RESOURCE_REPORT.md、HISTORY_COST_REPORT.md §4、ACCEPTANCE_MAP.json A05、P07_RESULT.json、本报告，及 logs/P07-FIXR1-* 新日志。

**F-A：soak 工具 `process.exit()` 跳过 finally 清理（工具缺陷 + 本报告回退节一句不实）**

- 事实：`bench-soak-resources.mjs` 主流程 try 块末尾的 `process.exit(code)` 立即终止进程，finally 的 `rmSync(lingxiHome)` 从不执行——每次运行（含成功）泄漏 ≈12MB 的 `hana-p07-soak-*` 临时目录（验收 CE-1 实测累积 8 个目录）。
- 修复：改 `process.exitCode = …` 赋值 + 事件循环自然排空（finally 执行 `cache.dispose()` + `rmSync` 后按同一语义退出；成功 0 / 失败非 0）。
- 验证：`P07-FIXR1-tmpdir-before` / `-after` / `-final`（运行前后 `hana-p07-soak-*` 计数均 0——两次真实运行 + 一次负向注入运行后零残留）；`P07-FIXR1-soak-minimal`（6 批 exit 0）与 `P07-FIXR1-soak-original`（12 批原始规格 exit 0：`components_ok=true`、`NO_REPRODUCIBLE_UNBOUNDED_GROWTH`、rss_growth 0.3%、逐批 rebuilds/hits/evictions 与执行轮样本逐位一致）——判定逻辑不受影响；失败路径退出码由负向注入运行证实（exit 1）。

**F-C：soak 断言空转（`!== "fresh"` 恒真）+ 「追加必失效→下批重建」机制叙述不准确**

- 事实：`HistoryProbeVerdict` 判定集 = `valid` / `append_candidate` / `branch_view_stale` / InvalidationReason（`server/history-read/types.ts`），不存在 `"fresh"`——旧计数 `probe_invalidated=1/批` 不构成证明。验收 CE-2 实测（真实 `HistoryDirectoryCache`）：外部追加判定为 `append_candidate`（目录保留、非 invalidate；外部追加未走插桩写入、mutation epoch 不变），读路径（`server/history-read/index.ts` tryDirectoryOnce）仅在 `verdict === "valid"` 直接命中，其余走增量/全量重建；每批 rebuilds=12 实由 12 会话>8 槽 LRU 颠簸驱动，与追加无关。
- 修复：(1) 断言改为「外部追加后 probe 不得判 `valid`，否则 `allOk=false`」，样本新增 `probe_verdict_after_append`（实际判定留档，本次复跑全部 `append_candidate`）与 `probe_append_detected` 字段，废弃恒真的 `probe_invalidated`；(2) 机制叙述更正落点：SOAK_RESOURCE_REPORT §1 组件 3 行 + 新增 §7 更正节、HISTORY_COST_REPORT §4 追加条目、ACCEPTANCE_MAP.json A05 三字段。
- 断言有效性证明（负向注入，做完零残留于工具本体）：`logs/P07-FIXR1-negprobe-inject.mjs`（修复版副本仅禁用外部追加——文件未变时 probe 判 `valid`）→ `P07-FIXR1-negprobe-inject-run` exit 1，`P07-FIXR1-negprobe-verify`（exit 0）核验红因：全批 `probe_verdict_after_append="valid"`、`probe_append_detected=false`、`components_ok=false`，而稳态判定仍 `NO_REPRODUCIBLE_UNBOUNDED_GROWTH`——变红归因于新断言本身，非其他路径。

**复核（run-logged，exit code 见 command-log.jsonl）**：`P07-FIXR1-tmpdir-before`(0) / `P07-FIXR1-soak-minimal`(0) / `P07-FIXR1-soak-original`(0) / `P07-FIXR1-tmpdir-after`(0) / `P07-FIXR1-negprobe-inject-run`(1，预期) / `P07-FIXR1-negprobe-verify`(0) / `P07-FIXR1-tmpdir-final`(0) / `P07-FIXR1-lint`(0) / `P07-FIXR1-json-validate`(0) / `P07-FIXR1-manifest-check`(0)（终态全量校验另存档 logs/P07-FIXR1-manifest-check-final.out，exit 0、118 条全 OK；manifest-check.out/.err 与 -final.out 按惯例不入清单——自指不可能，清单头注已注明，P06-FIXR 同款）。原始规格复跑说明：全量 12 批仅 ≈3.5s，故最小（6 批）与原始规格均真实执行；样本经 `--out` 落 logs/，不覆盖执行轮 `samples/soak-resources.json`。

交付后停止，等待用户指定下一阶段。
