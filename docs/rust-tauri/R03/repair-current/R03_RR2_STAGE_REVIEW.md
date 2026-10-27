# R03 RR2 阶段审查报告（R03_RR2_STAGE_REVIEW）

- 审查者：**STAGE-REVIEWER-R03-RR2**（2026-09-30，一次性全新阶段审查代理；未参与本轮任何实现、修复、执行者自查或 R1 组审）。
- 审查对象：候选 `f4d4b16664827010516bc1f5ec6aee5e14b859d3`（fix(rust-tauri): R03 RR2 repair — canonical request id…，R03-RR2-F05-01）；父提交=RR2 审查基线 `c96f7cc635cf18fa83a81b0e28f33d4fed8baf9c`。
- 输入：RR2 提示词（§4 六项验收、§5.6 阶段审查职责）与 `R03_RR2_定点验收清单.json`；完整 diff；执行者报告 `R03_RR2_F05_E01_REPORT.md` 与证据根 `RR2-F05-E01/`；组审 `R03_RR2_F05_R1_REVIEW.md` 与 `RR2-F05-R1/`；门禁 `RR2-F05-GATE/verify-stage-r03/verify-stage-result.json`；账本 `R03_FIX_ISSUES.json`；`R03_HANDOFF.json` 与 RR1 阶段审查 `R03_FIX_FINAL_STAGE_REVIEW.md`。
- 证据根（本审查者）：`artifacts/rust-tauri/R03/repair-current/STAGE-REVIEW-RR2/`（README 索引）。

```text
STAGE_VERDICT: PASS
```

R03 在 RR2 定点修复候选 f4d4b1666 上可重新接受：唯一阻塞 R03-RR2-F05-01 的修复
（单一 canonical request-id 事实 + 三态跨重启绑定查找 + 旧格式 cause_id 兼容读取与显式歧义拒绝）
在我亲眼所见的全部核验下成立——缺陷被**我自己的测试**在未改动基线 c96f7cc6 上独立复现
（3/3 红，失败模式正是「重启后重试被全新受理=盲重执行」），同一测试在候选上 3/3 绿；
新增套件 7/7、既有 F05 两套件 5/5+5/5、F03 域抽查 13/13 均为本人在候选上的真实运行；
完整 verify-stage R03 门禁 overall PASS 且候选/远程/门禁三方 SHA 绑定一致；pin 三处同步
只增不减；diff 严格限于授权范围，F01–F08 语义未见削弱。账本 C06（OPEN_PENDING_GATE_AND_
STAGE_REVIEW）的两项待决条件（门禁 overall PASS + 阶段复验通过）在本报告后均已满足，
可由总控收口闭合。

## 1. 六项清单 C01–C06 → 本审查者亲眼所见

| C-ID | 要求要点 | 我的独立证据 | 结论 |
|---|---|---|---|
| C01 无空白控制组 | 原 ID 重试恢复绑定或安全拒绝；计数不增；新 key 正常受理；不无条件拒绝 | 套件实跑 7/7 含 `rr2_f05_c01_…`（logs/01，重试 `RequestIdBoundToEarlierRun` 指名旧 run、计数不增、新 key 完成、改内容被拒）；**我的 `srv2_a` 控制臂**：重启后全新 ID `req-fresh-9z` 正常受理并 completed（logs/05）——非无条件拒绝 | 满足 |
| C02 带空格 ID 跨重启重试 | 原样与 canonical 重试识别同一逻辑请求；不二次执行/计数；前后台×确认/未知矩阵 | 套件 `rr2_f05_c02_…` 两用例（含 8 格矩阵）绿（logs/01）；**我的 `srv2_a`**：前台真实受理 " req-9x " → 计数+1 → 同数据根 reboot → 原样与 canonical 重试均 `RequestIdBoundToEarlierRun` 指向同一 run、回显 canonical、runs=1、计数=1、锚点 `request:req-9x`（logs/05）；同一文件在基线 c96f7cc6 上该链红：`FRESHLY accepted … run_count: 2`（logs/06） | 满足 |
| C03 空白/边界变体同一 canonical 契约 | Tab/CRLF/U+3000/U+00A0/长度边界全链一致；非法 ID 副作用前拒；不扩大接受/截断；不依赖 SQLite trim | 套件 `rr2_f05_c03_…` 绿（logs/01）；kernel 单测 2/2 在门禁 workspace 输出中出现 ok；代码核实（diff 亲读）：权威匹配为 Rust 侧 `strip_prefix(REQUEST_CAUSE_ID_PREFIX)`+`canonical_request_id`（`str::trim` 全 Unicode White_Space），**无 SQLite TRIM**；我的复测另用 CRLF 变体贯穿 b 链 | 满足 |
| C04 并发/隔离/失败补偿不回退 | 同 key 单一受理；不同内容冲突；跨域不串用；无幽灵 Replay | 套件 `rr2_f05_c04_…` 绿（logs/01：停驻窗口并发、异形态不同内容冲突、容量拒绝、起始事务失败补偿、主体隔离）；**我的 `srv2_c`**：三命名空间（owner×两会话+device 主体）同逻辑 ID 各自受理、重启后三种拼写各自拒绝指名自己的 run、绝不含他命名空间 run（logs/05；基线侧该链红 logs/06）；既有 `admission_dedup_consistency`/`adversarial` 5/5+5/5（logs/02/03） | 满足 |
| C05 旧 cause_id 兼容 | 旧行仍可关联或显式歧义；不漏查当新任务；不改写冻结行；冲突不挑一条不自动重跑 | 套件 `rr2_f05_c05_…` 两用例绿（logs/01：settled 与异常重启 active 旧行可关联且不改写；三变体冲突→`RequestIdBoundAmbiguous` 列全）；**我的 `srv2_b`**：两条旧格式行（`"request: req-9x "`、`"request:\r\nreq-9x\r\n"`）→ 三种形态重试均显式歧义列两个 run、非单选、计数 0、冻结行逐字不变（logs/05；基线侧红：`FRESHLY accepted … run_count: 3`，logs/06） | 满足 |
| C06 正式门禁、候选绑定与独立反例复验 | 新增测试真实执行且旧保护未删；pin 同步；过滤 0/漏项/旧证据/漂移不能过；Reviewer 独立重跑跨重启带空白变体 | 门禁 verify-stage-result.json：overall **PASS**、**15/15** 命令 exit 0、17/17 场景、48 叶=17 PASS+31 递延；candidateSourceBinding before/after/**15 个逐命令 checkpoint** digest 全一致（60b3e86a…，stable、零变更路径）、testedShaAtEnd=**f4d4b1666**=本地 HEAD=远程（三角核对）；repair-cases.json **十套件**精确计数（request_id_canonicalization=7，原九套件计数原样）；workspace stdout 复算 73 ok 行/**718 passed/0 failed**，7 个新用例名逐一出现（未过滤）；pin 三处（r03_g07_repair_suites.sh / xtask stage_map.rs / R03.json）diff 亲读：原九行零改动、仅追加第十行与增量说明，无删除无降低；xtask 100 passed 行在门禁输出中出现（含 pin 一致性钉测）；**独立反例复验链完整三源**：E01 失败原件（未改动基线 1/4：C01 绿+C02/C03/C05 红，自洽）→ R1 组审（候选 3/3 绿/基线 3/3 红）→ **本审查者**（候选 3/3 绿/基线 3/3 红，自有测试与变体） | **满足**（待决条件已齐，账本 C06 可由总控闭合） |

## 2. R03 核心不变量与范围（D 部分）

- **diff 范围**（`git show --name-only` 亲核）：仅 rust 三个 crate 十文件
  （kernel subagent.rs/ports.rs；adapters run_store.rs；service sessions.rs/dedup.rs/runs.rs/lib.rs；
  新测试 request_id_canonicalization.rs；xtask stage_map.rs/R03.json）+ 生产者脚本
  r03_g07_repair_suites.sh + RR2 证据/报告（RR2-F05-E01、RR2-F05-R1）+ 账本登记。
  **无 R04+ 倒灌**（allowed_next_scope 未被触碰）、无 R02 图/其他阶段图改动、
  **Cargo.lock 零变化**、无任何文件删除（--diff-filter=D 为空）。
- **F01–F08 语义未削弱**：cancel.rs、subagents.rs、task_supervisor.rs、background.rs、limits.rs
  未在本 diff 中；既有九修复套件在门禁 repair-cases.json 中逐套件精确计数全绿；本人重跑
  admission_dedup 两套件 5/5+5/5、cancel_terminal_race 13/13（F03 域）；workspace 718/0
  （RR1 基线 709 + 新套件 7 + kernel 单测 2，增量自洽）。
- **F05 上轮语义保留**：Pending/Committed/Unverified 生命周期、新鲜度、主体/会话隔离、同 key
  不同内容冲突、未知副作用不删绑定、两阶段受理与派发拒绝补偿的代码路径未被本 diff 触及
  （sessions.rs 改动集中在受理面头部规范化、find_request_binding 三态分支与错误变体新增，
  admit_submission 既有逻辑结构保留并亲读核对）。
- **修复形态符合 §3.4 全部要求**：规则本体单一来源在 kernel（canonical_request_id +
  REQUEST_CAUSE_ID_PREFIX，RunLineage 写入方与跨重启读取方共用）；前台/后台两公开受理面
  最前端各规范化一次并以 canonical 形态向下遮蔽传递（内存 DedupKey、DriveAuthorization、
  持久锚点、重启查询、错误回显同源）；原始 ID 仅 tracing 审计字段；runs.rs debug 断言设防；
  无「调用点临时 trim」形状（grep 全调用面：生产调用方仅 HTTP 入口——其本就先 validate——
  与 execute_for 便捷面）；兼容读取不改写 immutable lineage（只读查询 + 测试断言冻结原值）；
  多旧值归一冲突显式歧义（HTTP 409 列全部 run）不挑不重跑。
- **账本与事实一致**：workorder RR2-F05（DONE_PENDING_STAGE_REACCEPTANCE，reviewer PASS）、
  issue R03-RR2-F05-01（P1）、6 个 RR2 C-ID case 的 selfcheck/review 证据路径均真实存在；
  C06 如实登记 OPEN_PENDING_GATE_AND_STAGE_REVIEW 并注明由门禁+阶段复验收口——与事实相符
  （门禁证据 RR2-F05-GATE 为运行后落盘，账本书写于提交前，不自引用候选 SHA 符合提示词
  「不机械要求 SHA 自引用」的约束，无虚报）。
- **R03_HANDOFF.json** 的 reaccepted_after_adversarial_repair 仍指向 RR1 候选 ebcbcad1b——
  属预期：现行接受与交接的更新是总控在阶段审查 PASS 后的收口动作（提示词 §5.6），非本审查
  职责，不构成不一致。

## 3. 亲自重跑数字汇总（全部真实 cargo；rustup 1.98.1，--locked，离线，独立 target dir）

| 运行 | 结果 | exit | 日志 |
|---|---|---|---|
| `--test request_id_canonicalization`（候选） | 7/7 | 0 | logs/01 |
| `--test admission_dedup_consistency`（候选） | 5/5 | 0 | logs/02 |
| `--test admission_dedup_adversarial`（候选） | 5/5 | 0 | logs/03 |
| `--test cancel_terminal_race`（候选，F03 域抽查） | 13/13 | 0 | logs/04 |
| **我的** `srv2_independent_retest`（候选隔离拷贝树） | **3/3** | **0** | logs/05 |
| **我的** `srv2_independent_retest`（基线 c96f7cc6 detached worktree） | **0/3，三条 panic 均为 FRESHLY accepted（盲重执行）** | **101** | logs/06 |

基线红原文示例（logs/06）：`raw padded retry: retry of " req-9x " was FRESHLY accepted
(ExecuteAccepted { run_id: "run_…_000002", run_count: 2, replayed: false })`。
复测首跑曾 3/3 失败——**我的夹具错误**（误用未预置会话名，服务端正确 NotFound 拒绝），
修正我自己的测试文件后全绿；已在 retest-summary.txt 如实记录。

## 4. 诚实性核对（E 部分）

- 门禁证据与实跑一致：JSON 声称的 718/0 与我从其 stdout 复算的 73 ok 行/718 passed 一致，
  且与我在本机重跑的套件结果（7/7、5/5、5/5、13/13）一致；repair-cases 十套件计数与
  pin 表一致。
- 候选无漂移：本地 HEAD=远程 origin=门禁 testedShaAtEnd=f4d4b1666；binding 15 checkpoint
  digest 全一致、worktreeDirty=false。
- 执行者失败原件自洽：基线为 5 用例版本（C04/C05-歧义引用修复后新错误变体无法在基线编译，
  执行者已在报告 §1 如实说明），1 通过（C01 控制组）/4 失败，失败原因正是本缺陷；
  R1 组审与我的复测分别在基线上独立复现红——三源互证。
- 账本无虚报：见 §2；未发现「跳过测试/改断言/删用例/pin 降低/过滤 0」形状。
- **未发现门禁证据与实跑不符、账本虚报或候选漂移。**

## 5. 非阻塞观察（无 P1/P2）

| # | 观察 | 评估 |
|---|---|---|
| 1 | 门禁 rust_clippy 的 evidencePaths 指向 stdout.log（0 字节），实际输出在 stderr.log（6993 字节，完整 Checking 图 + Finished + 0 告警） | cargo 行为使然；命令真实执行且 `-D warnings` exit 0 与日志自洽。属 xtask 证据文件选择的通用小瑕（非本轮引入，fmt/clippy 均如此），建议后续统一收 stderr。不影响裁决 |
| 2 | `find_request_binding` 为命名空间内 LIKE 收窄 + Rust 逐行匹配，非索引精确等值 | 执行者 §7.1 自报；单会话显式 ID 运行数小，最小修复合理取舍，无正确性影响 |
| 3 | canonical debug 断言仅 dev/test 生效 | 执行者 §7.3 自报；结构性保证来自两受理面唯一规范化点+全调用面核实，R07 未来接线已有契约注释设防，届时需回归 |
| 4 | SQLite LIKE 对 ASCII 大小写不敏感使扫描集略宽 | R1 已报；权威匹配为大小写敏感的 Rust 比较，仅扫描宽度，无错误命中 |
| 5 | 服务面非法 ID 校验前移使 `InvalidRequestId` 先于会话 NotFound/Forbidden | HTTP 面对外契约不变（本就先 400）；副作用前拒绝更安全；仅直接调 service 面可察觉 |
| 6 | 账本 RR2-F05 workorder 的 commits/remote_receipt 字段为空 | 防自引用书写于提交前的既定模式；总控收口时应连同 C06 闭合与 HANDOFF RR2 接受记录一并补记（含 f4d4b1666 远程回执） |

## 6. 结论

- 六项清单 C01–C06 全部满足；R03 核心不变量与 RR1 接受基线未回退；受影响 R02 回归在
  本轮 verify-stage R03 门禁内 7 条定向链全绿（r02_auth_matrix/storage_tx/events_matrix/
  backup_restore/recovery_drill/full_chain/legacy_regression，另有 a15/a16）。
- **裁决：PASS——R03 在候选 f4d4b1666 上重新接受。** 建议总控按提示词 §5.6 收口：
  闭合账本 C06（OPEN_PENDING_GATE_AND_STAGE_REVIEW → CLOSED，引用本报告与 RR2-F05-GATE）、
  更新 reopen_status 与 R03_HANDOFF 的现行接受记录（引用真实源码提交 f4d4b1666，不产生
  无限自回填）、补记 workorder commits/remote_receipt。本次止于 R03，不启动 R04。
