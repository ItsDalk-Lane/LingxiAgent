# R03 RR2 F05 R1 独立对抗性审查报告 — R03-RR2-F05-01（canonical requestId 全链一致 + 旧 cause_id 兼容读取）

- 审查者：REVIEWER-R03-RR2-F05-R01（全新独立审查者，未参与实现；本报告是本修复能否进入门禁与提交环节的独立判定）。
- 审查日期：2026-09-30。分支 `codex/rust-tauri-migration`。
- 候选：HEAD 固定 `c96f7cc635cf18fa83a81b0e28f33d4fed8baf9c` + 未提交改动（`git diff HEAD`，10 文件，386+/92-）+ 3 个未跟踪路径（新测试 `rust/crates/lingxi-service/tests/request_id_canonicalization.rs`、执行者报告、执行者证据根）。
- 输入：RR2 复审与定点修复提示词（§3/§3.4/§4）、`R03_RR2_定点验收清单.json`（C01–C06）、执行者报告 `R03_RR2_F05_E01_REPORT.md`、完整 diff、证据根 `RR2-F05-E01/`、上轮 F05 语义基线与既有两套件。
- 证据根（本审查者）：`artifacts/rust-tauri/R03/repair-current/RR2-F05-R1/`（全部命令的原始输出、自写复测源码、候选指纹）。

## 0. 最终裁决：**PASS**

R03-RR2-F05-01 的修复（单一 canonical request-id 事实 + 三态跨重启绑定查找 +
旧格式 cause_id 兼容读取与显式歧义拒绝）在我亲眼所见的全部核验下成立：缺陷在
基线上被**我自己的测试**独立复现（3/3 失败、失败模式正是「重启后重试被全新受
理=盲重执行」），同一测试在修复候选上 3/3 通过；新套件 7/7、既有保护 5/5+5/5、
xtask pin 一致性 100/100、workspace 全量 718 passed/0 failed 均为我本人真实
运行；pin 原九行零改动、新增计数与 `--list` 实数一致；候选在审查期间零漂移。
未发现以跳过测试/改断言/删用例消红的形状。发现的问题均为非阻塞观察（§4）。
本修复可以进入门禁与提交环节（完整 verify-stage R03 与受影响 R02 回归按提示词
§5.5/5.6 属总控在候选提交后的门禁腿，非本审查者裁决范围的阻塞项）。

## 1. 候选冻结与红线遵守

- 审查前 `git diff HEAD | shasum -a 256` =
  `7f72a789fbe50ccdeb544c9ed99d4ce1dcba6eb894e88d3173e3a6fa5f21939f`；
  审查结束前复测**完全相同**（`RR2-F05-R1/candidate-fingerprint.txt`）。
  未跟踪测试文件（`e2d5bb35…`）与执行者报告（`d9e084c1…`）sha256 前后一致。
- 审查者未修改任何产品代码、既有测试、脚本、执行者报告；自写复测全程在
  `${TMPDIR}` 的临时树（修复侧 rsync 拷贝）与 detached git worktree（基线侧）
  中进行，结束后已全部清理，`rv1_independent_retest.rs` 从未进入仓库 rust/ 树。

## 2. 逐项核对表（C01–C06 → 审查者亲眼所见）

| C-ID | 要求要点 | 审查者亲眼所见的证据 | 结论 |
|---|---|---|---|
| C01 无空白控制组 | 原 ID 重试恢复绑定或安全拒绝；外部计数不增；新 key 正常受理；不无条件拒绝 | 套件真实运行（`logs/01`）：`rr2_f05_c01_…` 通过——重试得 `RequestIdBoundToEarlierRun` 指名旧 run、run 数仍 1、计数仍 1；改内容被拒；新 key `req-43` 真实完成（计数 2）。我的 `rv1_c` 亦含「新命名空间正常受理」控制维度 | 满足 |
| C02 带空格 ID 跨重启重试 | 原样与规范化重试识别同一逻辑请求；不二次执行/计数；前台/后台×确认/未知×原样/规范矩阵 | 套件（`logs/01`）`rr2_f05_c02_…` 两用例通过（8 格矩阵全绿）；**我的独立重跑**（`logs/06`）`rv1_a`：前台真实受理 " req-42 " → 计数 +1 → close+drop+同数据根重 boot → 原样与 canonical 重试均为 `RequestIdBoundToEarlierRun` 指向**同一** run，run 数 1、计数 1、回显 canonical、锚点 `request:req-42` | 满足 |
| C03 空白/边界变体同一 canonical 契约 | Tab/CRLF/U+3000/U+00A0/长度边界全链一致；非法 ID 副作用前拒；不扩大接受/不截断；不依赖 SQLite trim | 套件 `rr2_f05_c03_…` 通过（含 129 字节超长、" "+129 归一后仍超、全空白三类前置拒绝：run 行 0、计数 0）；kernel 单测 2/2（`logs/08`）；代码核实（§3.6）：匹配在 Rust 侧 `strip_prefix`+`canonical_request_id`，**无 SQLite TRIM** | 满足 |
| C04 并发/隔离/失败补偿不回退 | 同 key 单一受理；不同内容冲突；跨域不串用；无幽灵 Replay | 套件 `rr2_f05_c04_…` 通过（停驻窗口并发 in-flight 回显 canonical、`\tpad-a\t` 异形态不同内容冲突、容量拒绝后 canonical 重试全新受理、起始事务失败补偿、U+3000 第三形态结算后 replay、device 主体隔离）；**我的独立重跑** `rv1_c`：三命名空间（owner×两会话 + device 主体）隔离与重启后各自拒绝指向各自 run | 满足 |
| C05 旧 cause_id 兼容 | 旧行仍可关联或显式歧义/待核验；不漏查当新任务；不改写冻结行；冲突不挑一条不自动重跑 | 套件 `rr2_f05_c05_…` 两用例通过（settled 与异常重启 active 旧行均可关联且冻结不改写；三原始变体冲突 → `RequestIdBoundAmbiguous` 列出全部三个 run）；**我的独立重跑** `rv1_b`：两条旧行（`"request: req-42 "`、`"request:\treq-42\t"`）→ 三种形态重试均为显式歧义列出**两个** run、非 `RequestIdBoundToEarlierRun` 单选、计数 0、冻结行逐字不变 | 满足 |
| C06 门禁/候选绑定/独立复验 | 新增测试真实执行且旧保护未删；pin 同步；Reviewer 独立重跑跨重启带空白变体 | 新套件 7/7 真实执行（7 个用例名逐一出现，0 ignored）；`--list` = 7 tests（`logs/02`）= pin 7；脚本与 `R03_REPAIR_PIN_TABLE` 原 9 行**逐字零改动**（HEAD vs 工作区 diff 核实，仅各追加第 10 行）；xtask 100/100 含 `r03_repair_producer_pin_table_matches_the_registered_suites`（`logs/05`）；既有 `admission_dedup_consistency` 5/5、`admission_dedup_adversarial` 5/5（`logs/03/04`）；workspace 全量 73 ok 行/718 passed/0 failed/exit 0（`logs/09`，独立复算与执行者 E5 一致）；候选零漂移（§1）；审查者独立重跑跨重启带空白变体见 C02 行 | 满足（审查者职责范围内）；完整 verify-stage R03 与 R02 定向链属总控提交后门禁腿 |

## 3. 逐层身份一致性（读代码核实，非转述）

链路（行号为当前工作区）：

1. **规则本体单一来源**：`lingxi-kernel/src/subagent.rs:297`
   `REQUEST_CAUSE_ID_PREFIX="request:"` 与 `:309 canonical_request_id(raw)=raw.trim()`
   （Rust `str::trim`，完整 Unicode White_Space）。kernel 同时是锚点写入方
   （RunLineage）与跨重启读取方（adapters）的公共底座，依赖方向合法；原先
   kernel/adapters 两处硬编码前缀现单一来源化。
2. **受理边界唯一 canonical 化点**：`sessions.rs:294-314
   canonicalize_submission_request_id`（内部走 `dedup::validate_request_id`＝
   kernel 规则+非空/≤128 策略），在 `execute_submission_for`（`:713`）与
   `execute_background_for`（`:842`）两个公开受理面**最前端**各调用一次——
   先于输入预算检查之外的会话读取、busy gate、run-id 分配、dedup 预留、任何
   持久写与派发——随后以 canonical 形态重构 `ExecuteSubmission` 向下遮蔽传递。
   原始 ID 仅在 raw≠canonical 时打一条 `tracing::info`（raw_request_id /
   canonical_request_id 审计字段），**不再成为任何身份来源**。
3. **内存 DedupKey**：`admit_submission` `:1031` 对（已是 canonical 的）输入
   再走 `validate_request_id`（幂等，纵深防御），`:1034` DedupKey.request_id
   与 digest 同源。
4. **持久查询**：`:1060 find_request_binding_erased(…, &request_id)` 传
   canonical；`run_store.rs:475 find_request_binding`：SQL 仅以
   `LIKE 'request:%'` 收窄并在 JOIN runs 上限定 (session_id, owner_kind,
   owner_subject) 命名空间；**权威匹配在 Rust 侧**
   `strip_prefix(REQUEST_CAUSE_ID_PREFIX)` + `canonical_request_id(raw)==canonical`
   （`:520-527`），未用 SQLite TRIM（其默认字符集覆盖不了 U+3000/U+00A0）。
   输出三态 Unbound/Bound/Ambiguous（`ports.rs RequestBindingLookup`）。
5. **DriveAuthorization → RunLineage**：`sessions.rs:756/882
   DriveAuthorization::user_submission(submission.request_id)` 用的是遮蔽后的
   canonical submission；`runs.rs:169` 增加 dev/test debug 断言（非 canonical
   输入立即失败）；`subagent.rs:384-389` cause_id＝前缀+canonical。
6. **持久化**：`runs.rs:639 record_run_lineage(ctx, authorization.lineage…)`
   原样落库（此时已是 canonical 锚点）。
7. **错误回显**：`RequestIdBoundToEarlierRun`（`:1081`）、新增
   `RequestIdBoundAmbiguous`（`:1095`，`run_ids` 全列、created_at/run_id 降序
   确定性）、`AdmissionInFlight`/`DuplicateRequestConflict`/`InvalidRequestId`
   的 request_id 字段全部来自 admit_submission 的 canonical 值；HTTP 映射
   `lib.rs:1346-1363` 409 `request_id_bound_ambiguously` 列出全部运行。
8. **全调用面核实**（grep 全仓库）：生产代码中 `execute_submission_for` 仅
   HTTP 入口（`lib.rs:2360` **先** validate——返回值即 canonical——再传入）与
   `execute_for` 便捷面（传 `ExecuteSubmission::plain`，无 id）；
   `execute_background_for` 当前无生产调用方（R07 未来接线由 debug 断言+
   `RunLineage::user_submission` 契约注释设防）；其余调用点全部在
   `#[cfg(test)] mod tests`（sessions.rs:1508 起）。`RunLineage::user_submission`
   生产调用方唯一（runs.rs:179）。**不存在「某调用点临时 trim」形态**——前台/
   后台/重启查询/错误补偿（`:771-781` 驱动失败后的 load_run 三分支补偿）共用
   同一 canonical 事实。
9. **上轮 F05 语义零回退**：Pending/Committed/Unverified 绑定生命周期、
   新鲜度、主体/会话隔离、同 key 不同内容冲突、未知副作用不删绑定、两阶段
   受理与派发拒绝补偿的代码路径未被本 diff 触及；既有两套件 5/5+5/5 通过。
10. **兼容读取不改写历史**：`find_request_binding` 是只读查询；旧行永不 UPDATE
    （`record_run_lineage` 仅 INSERT/同 run 幂等校验）；歧义判定只读不挑。

**结论：内存 key、授权、锚点、持久化、查询、错误回显六层同源于唯一 canonical
事实；原始 ID 仅存审计日志；不依赖 SQLite trim；非调用点临时 trim。**

## 4. 发现的问题（均非阻塞，无 P1/P2）

| # | 级别 | 观察 | 评估 |
|---|---|---|---|
| 1 | 低（性能，执行者已自报 §7.1） | `find_request_binding` 对命名空间内全部 `request:%` lineage 行做 LIKE 收窄+Rust 逐行匹配，非索引精确等值 | 单会话显式 ID 运行数通常极小；最小修复的合理取舍；无正确性影响 |
| 2 | 低（执行者已自报 §7.3） | canonical debug 断言仅 dev/test 生效，release 剔除 | 结构性保证来自两受理面唯一规范化点+全调用面核实（§3.8）；HTTP 面本就先 validate；可接受 |
| 3 | 信息 | SQLite `LIKE` 默认对 ASCII 大小写不敏感，`Request:…` 等大小写变体行会进入扫描集 | 权威匹配是 Rust 侧大小写敏感的 strip_prefix+比较，这些行被跳过；无错误命中，仅扫描略宽 |
| 4 | 低（服务面错误优先级微变） | `execute_submission_for` 头部 canonical 化使非法 ID 的 `InvalidRequestId` 先于会话读取的 NotFound/Forbidden | HTTP 面本来就先 400 校验 ID，对外可观察契约不变；校验前移更安全（副作用前拒绝）；仅直接调 service 面的自写测试可察觉 |
| 5 | 信息（执行者已自报 §7.4/7.5） | C02「未知副作用」格以停驻工具计数作反证信号，未模拟旧进程驱动复活交错；R07 后台入口尚无生产调用方 | 存储已 close 后写队列拒绝写，该交错在本架构不可达；R07 接线已有断言+契约设防，届时需回归 |

未发现任何「跳过测试/改断言预期/删用例/pin 降低」的消红形状；pin 只增不减。

## 5. 审查者亲自重跑汇总（全部真实 cargo 运行；rustup 1.98.1，离线，独立 target dir）

| 运行 | 用例数 | 通过 | 失败 | exit |
|---|---|---|---|---|
| 新套件 `request_id_canonicalization`（`--test-threads=4`） | 7 | 7 | 0 | 0 |
| 同上 `--list` | 7 tests | — | — | 0 |
| `admission_dedup_consistency` | 5 | 5 | 0 | 0 |
| `admission_dedup_adversarial` | 5 | 5 | 0 | 0 |
| `-p xtask`（含 pin 一致性） | 100 | 100 | 0 | 0 |
| kernel 新单测（canonical/prefix 过滤） | 2 | 2 | 0 | 0 |
| **我的** `rv1_independent_retest` @ 修复候选（临时拷贝树） | 3 | **3** | 0 | **0** |
| **我的** `rv1_independent_retest` @ 基线 c96f7cc6（detached worktree，产品代码零改动） | 3 | 0 | **3** | **101** |
| workspace 全量 | — | **718**（73 个 ok 结果行） | 0 | 0 |

基线侧 3 个失败的全部失败原因（`logs/07`）：重试被 FRESHLY accepted、run_count
增长——即 RR2 §3.1 源码链推出的盲重执行行为。这同时构成对执行者
`failure-originals/baseline-run.txt` 的独立验证：该原件内容自洽（基线为 5 用例
版本、panic 行号与现行 7 用例文件不同、C01 控制组通过、失败原因正是本缺陷，
非编译错误等无关红），且缺陷本身被我的独立测试在同一提交上复现。

## 6. 结论

- R03-RR2-F05-01 缺陷真实（基线独立复现）、修复真实生效（同一测试修复侧全绿）、
  修复方式符合 §3.4 全部要求（单一 canonical 边界、原始 ID 仅审计、无临时 trim、
  无 SQLite trim 依赖、旧行兼容读取+显式歧义、不改写 immutable lineage、
  保留全部上轮 F05 语义、未越界 R04）。
- 六项验收 C01–C06 在审查者亲眼所见范围内全部满足；pin 增量诚实（原九行零改动、
  新增计数=实际用例数）；候选审查期间零漂移。
- **裁决：PASS**——本修复可进入门禁与提交环节。完整 verify-stage R03、受影响
  R02 定向回归与候选提交绑定由总控按提示词 §5.5/5.6 继续执行，不因本裁决豁免。
