# R03 阶段报告｜运行状态机、并发、取消与恢复

## 本轮状态（2026-09-30）：REOPENED_PENDING_REPAIR

对抗性审查（审查基线 `cd3fb19e6`，即本报告当时 HEAD）确认 8 组缺陷 **F01–F08**（34 项新增反例 C-ID）。其中 F08 直接质疑下方「阶段独立验收」的**裁决2**（FINDING-1「两阶段收口」解释：普通父取消后 child durable/busy 残留递延重启收口）及 HANDOFF known_gaps 对应递延；F01/F02 为其运行层根因。2026-09-29 的 STAGE_VERDICT: PASS、626 测试绿与全部历史证据**原文保留、不予否认**；但其接受依据不再视为充分——R03 在本轮修复（工作单 G01–G07）+ 逐项三层检查 + 全新独立阶段审查完成前处于 REOPENED_PENDING_REPAIR，**不放行 R04**。旧 PASS 不能作为反证驳回本轮反例；驳回须以源码/实测反证经全新 Reviewer 裁决。

- 统一问题账（逐 F-ID/C-ID 三层状态，权威索引）：`docs/rust-tauri/R03/repair-current/R03_FIX_ISSUES.json`
- 本轮规格全文：`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_对抗性审查_问题清单与修复总控提示词_2026-09-30.md` 与 `Lingxi_R03_修复验收清单_2026-09-30.json`

### 修复轮进度（2026-09-30，G01–G07 全部执行完毕；本节为追加，不改写上方与下方任何历史原文）

- **G01–G06（F01–F07 运行行为缺陷）已全部修复并通过三层闭环**：执行者普通自查+对抗性自查各一份在档，六位全新独立 Reviewer（G01-R1..G06-R1，均未参与对应候选实现）逐组 **VERDICT: PASS**；总控已按授权提交并推送（`520bb75b9`/`ccb09fde6`/`198e0da1e`/`d56e6883d`/`8883923a5`/`8a6303bcd`，远程包含回执见 `repair-current/R03_FIX_COMMIT_RECEIPTS.json`）。9 个新测试套件 59 个反例用例进入仓库（cancel_link_inheritance 7 / subagent_closeout 8 / cancel_terminal_race 13 / tool_receipt_unknown 6 / admission_dedup_consistency 5 / admission_dedup_adversarial 5 / input_payload_fidelity 5 / input_budget_refusal 2 / background_steering 8），红基线（未修代码上逐组实测失败）与绿证据在 `artifacts/rust-tauri/R03/repair-current/G0{1..6}-E01/`。
- **G07（F08 验收接受缺口）已把上述反例接入正式验收**：R03 阶段图新增 `repair_suites` 门禁命令与 `R03-RP01` 场景（`rust/crates/xtask/src/stage_maps/R03.json`，生成器同步更新；16 原场景/48 叶/7 条 R02 定向链注册零改动），生产者 `scripts/rust-tauri/r03_g07_repair_suites.sh` 逐套件固定计数机器核验（匹配 0/子集/改名=点名缺口），xtask 钉图测试镜像该注册（删除映射在 `cargo test`（门禁命令自身）内变红）。门禁负向测试（删映射/0 匹配过滤器/缺证据文件/陈旧证据根/中途改执行输入→非零退出点名缺口）落为可重跑脚本 `scripts/rust-tauri/r03_g07_gate_negative_tests.sh`（/tmp 隔离副本执行，证据 `artifacts/rust-tauri/R03/repair-current/G07-E01/negative-tests/`）。
- **门禁重验（G07-E01，候选=HEAD 8a6303bcd4+本修复轮工作树）**：`verify-stage R03` **overall PASS**——15/15 命令 exit 0（含新增 repair_suites 20.8s 与 fmt/clippy/workspace/check-contracts/check-boundaries/A15 矩阵/A16 seed/7 条 R02 定向链）；17/17 场景 PASS（16 原 A-ID + R03-RP01）；48 叶=17 份额 PASS+31 递延+0 fail+0 blocked；candidateSourceBinding stable=true（15 个逐命令 checkpoint 全稳定）；testedSha=8a6303bcd4。独立实跑另有 fmt/clippy/workspace(72 suites/709 passed/0 failed)/check-contracts/check-boundaries 全 exit 0（`artifacts/rust-tauri/R03/repair-current/G07-E01/gate-commands/`）。R02 全量回归 `verify-stage R02` 19/20 绿，唯一红 a16_legacy_regression=E5 封印族+patch-too-large 既有治理递延（R03-GOV-01/02）外加 1 条 artifact-core-ustar ENOTEMPTY 临时目录清理环境闪红（隔离重跑 10/10 绿、该文件与 shared/artifact-core 在本轮零触碰，归因记录在案，分类器按设计 fail-closed 不消红）；FINDING-2/3 对应 a07/a14 现为绿。
- **替代裁决2 的依据（明示）**：裁决2 把「普通父取消后 child active/busy 直到重启」判为可递延 MINOR 的前提——「计数清理只可能在被跳过的 note_child_finished 中发生」——已被 G01 产线修复证伪：父取消路径现于**当前进程内**完成 child drive 的合法取消/收尾/终态提交与线程/配额回收（subagent_closeout 套件 8 用例在同进程反复超上限取消后仍可派发），不再依赖重启；startup recovery 仅保留为真正进程崩溃的兜底。因此裁决2 不再作为接受依据；其原文与 2026-09-29 PASS 记录保留于下方历史节，不作篡改。裁决1（E5 治理递延）与裁决3（G01 等价断言）不受本轮影响、继续有效。
- **当前阶段状态**：REOPENED_PENDING_REPAIR（重开→待全新阶段 Reviewer 终审；本轮执行者不自称阶段 PASS）。轮次交付物索引：`docs/rust-tauri/R03/repair-current/`（R03_FIX_NORMAL_SELFCHECK.md / R03_FIX_ADVERSARIAL_SELFCHECK.md / R03_FIX_ACCEPTANCE_RESULTS.json / R03_FIX_HANDOFF.md / R03_FIX_INDEPENDENT_REVIEWS/INDEX.md / G07-E01_REPORT.md）。

## 阶段与结论

**READY_FOR_REVIEW**（执行者口径；独立阶段验收归总控另派。READY_FOR_REVIEW 不是 PASS。）

- 阶段 8 任务全部执行完毕；16 基础验收场景 + 48 项 R00 补充叶（17 份额 + 31 递延）经机器门禁消费；阶段图 `verify-stage R03` 真实退出 0（overall PASS）。
- TASK_BASE_SHA（阶段起点）=`526f7770f1eff6be289b8c34faeccc1b95e181fd`；T08 执行基线 = `dc42a01e37d1110293ec137402fd3239bd5b7d9d`（T01–T07 提交后 HEAD）；分支 `codex/rust-tauri-migration`；无 commit/push（T08 候选为未提交工作树，留总控冻结）。

## 范围

仅 R03 任务书授权范围（运行状态机/会话串行化/取消树/迟到栅栏/运行日志与副作用收据/子代理与后台/故障恢复/T08 验收与交接）。R04+ 未提前（工具网关/真实供应商/CLI/UI 均未实现）；生产默认入口不变（Node/Electron，R02-A16 定向 E1 家族全绿）；与基线差异无 ADR（无架构决策变更）。

## 源码

- 阶段内提交：T01 `9e319d648` 之前系列 → T07 `0c138456b` → T08 基线 `dc42a01e3`（TASK_BASE_SHA）；T08 增量为未提交工作树（`git status --porcelain` 可逐条核对：10 修改——注册表 1、属性测试 2、xtask 5、r02 脚本 2；新增 9——R03.json、acceptance_matrix.rs、脚本 3（matrix/a16/generator）、报告 4（T08_REPORT/R03_REPORT/HANDOFF/LEDGER）；另派单文件与证据根 artifacts/rust-tauri/R03/T08-E01/ 为既有未跟踪项/产出）。
- 依赖锁：`rust/Cargo.lock`=`90111c4b…`（R03 全阶段零变化=R02 原值）；`package-lock.json`=`e54a16fe…`；`rust-toolchain.toml`=`eec34104…`（1.98.1）。零新增第三方包/版本；tokio test-util 未引入。

## 环境

macOS darwin 27.0.0 arm64；rustup 锁定 1.98.1（全部经 rustup 代理+`--locked`）；SQLite=rusqlite 0.40.2 bundled；隔离 /tmp 合成数据根；确定性替身（Provider/Tool/ApprovalGate——外部响应 only）；无网络外发、无真实供应商、无真实用户数据。

## 完成项（逐 T-ID 摘要）

| T-ID | 交付（详细见各 T 报告） |
|---|---|
| R03-T01 | RunStateMachine 转换表+单一 finalize（IdempotentReplay/Conflict/CorruptSettlement）；RunFinish 运行结果契约（NoFinalCause/FailureCause/Cancelled/InterruptedNeedsAttention，不编造最终答案）；身份层 run/attempt/model-call/tool-call 独立；`TurnProviderPort`/`ToolExecutorPort` 注入面（R05/R04 交接）；StoragePort 运行事件/attempt 事务方法。PASS(R1) |
| R03-T02 | SessionSupervisor（每会话单一 owner、busy 409 session_busy 冻结语义、有界 steering 收件箱+leftover 存活）；QuotaManager（global→agent→session 三层、FIFO 有界等待、取消安全零泄漏）。PASS(R1) |
| R03-T03 | CancelScope 取消树（向下传播、取消优先 biased select）；TaskSupervisor（spawn_linked/spawn_detached/drain 有界回收、panic 监督返回）；CancelPolicy 锚定清理预算+StopUnconfirmed 如实报告；ApprovalGate 最小审批等待面。PASS(R1)（D1 于 T06 强制修复闭环） |
| R03-T04 | ResultFence 写前身份核对（模型/工具返回带 fence）；存储三腿拒写（终态/未开 attempt/被超越 attempt）+audit-only `stale_result_audit`（V2）；SubmissionDedup requestId+主体+会话+内容摘要（replay/conflict）；固定 seed 属性测试。PASS(R1)（F-1 登记递延于 T08 收口，见修复轮） |
| R03-T05 | InvocationJournal（prepared→authorized→started→succeeded/failed/unknown，V3；参数摘要+代次绑定）；驱动链写序（意图先落盘、无收据不外发）；`classify_invocation_recovery` 六决策（只读/幂等键/外部核验/NeedsAttention 禁盲重试）；unknown 收据结构。PASS(R1)（V3 登记递延 T08 收口） |
| R03-T06 | 子代理运行适配（dispatch/reply/close→同 RunSupervisor child run+线程登记+父树链接+deliver_retained 回流）；权限衰减（两态档+显式>继承+#1614 衰减拒绝+SUBAGENT_BLOCKED_TARGETS+运行层逐 target 判定）；`execute_background_for` 后台提交（断线≠取消）；RunLineage 四元（V4）。PASS(R1)（O-1 ask 档审批面义务递延 R04，入 HANDOFF；V4 登记递延 T08 收口） |
| R03-T07 | RecoveryCoordinator 启动扫描（list_active_runs+journal 分类+同一 finalize 事务落 interrupted_needs_attention，不虚构回复）；SubmissionIntake 退出闸（先拒新提交再 drain，幂等 replay 仍被回答）；shutdown 相位（后台取消+有界 join+残留如实）。PASS(R1) |
| R03-T08 | 本报告：阶段图 R03.json+注册+Gate 退出 0；组合矩阵+A15；A16 seed 机制证明；T04 F-1/V2-V4 登记+S4 修复；R02 回归链；R03_REPORT/HANDOFF/ACCEPTANCE_LEDGER。READY_FOR_REVIEW |

## 行为变化

对用户可见变化：无（新栈 Rust 服务为显式选择通道，生产默认入口不变；R02-A16 定向回归 E0–E4.5 全绿）。新栈内部为运行状态机/并发/取消/恢复能力的完整落地（见逐 T 报告）。

## 验收（16 A-ID → 命令/对象/预期/实际/退出码/证据）

机器门禁单一事实源：`artifacts/rust-tauri/R03/T08-E01/verify-stage-r03/verify-stage-result.json`（overall PASS；14/14 命令 exit 0；16/16 场景 PASS；48 叶=17 pass+31 deferred+0 fail+0 blocked；candidateSourceBinding.stable=true；testedSha=dc42a01e3）。逐 A-ID 映射（含 T01–T07 原证据与测试对象）见 `R03_ACCEPTANCE_LEDGER.json` scenarios。要点：

- A01–A14：`rust_test_workspace`（63 test target 全绿：run_lifecycle/session_serialization/cancellation_tree/late_result_fence*/request_dedup/invocation_journal*/subagent_*/background_*/recovery_*/*property 等）+ 相应叶案例；A02 另绑定 r02_storage_tx（同事务终态基底回归）；A13 另绑定 r02_full_chain；A14 另绑定 r02_recovery_drill。
- **A15**：`a15_combo_and_leaves` exit 0——12 组合矩阵经真实链路（替身只给外部响应），28 案例 actual==图钉（进程内断言+`lingxi.leaf-case-results.v1`），`integration.log` 调用观测+`combo-counts.json` 任务/调用/终态计数；17 份额叶图钉全过。
- **A16**：`a16_seed_mechanism` exit 0——P0 基线绿（无自然异常，如实标注）→P1 隔离变异（mktemp crate，仓库零触碰）→P2 捕获 seed=`0xa16d0000000000`（失败行+seed 文件一致）→P3 同 seed 两次稳定复现→P4 移除变异后同 seed+全组绿；seed 覆写+失败捕获为永久机制入两个属性测试。
- R02 基础链回归（SUP-02）：**7 条定向命令在 R03 gate 内全绿**（认证隔离/单写者事务/事件快照续读/损坏库备份/关闭恢复/真实重启链/默认入口 E0–E4.5）+ 全量 `verify-stage R02`（全新证据目录 `verify-stage-r02-regression`，17/20 命令绿、overall FAIL 伴 3 红逐条归因：E5 封印族分类=总控账本项；a07 检查器精确计数与 a14 风暴 200-only 断言为 R02 期资产相对 R03 设计行为（恢复扫描 finalize/冻结 busy 闸）过期——R03 语义本身无回退，登记 FINDING-2/3，资产更新归独立审阅授权）。

## 安全与数据

权限负向：子代理 escalation 只读父 write 档→统一拒绝且 0 child run（叶案例机器核验）；审批等待取消→0 工具执行；跨主体/伪造会话语义由 R02 回归链维持。真实进程：崩溃恢复组合+T07 11 个 SIGKILL 崩溃点（全量 test 绿）；r02_t04 S2 kill -9 同事务语义回归绿。单写者：storage_tx S1–S4 全绿（S4 按注册表指纹集判定）。迁移/回滚：无生产数据迁移（新栈隔离根）；A11 备份恢复回归绿。审计封印/完整 npm：总控另记（T08 不伪造；attempt-1 全量 E5 输出已存档）。

## 完整映射（F-ID→T-ID→A-ID→测试→结果）

见 `R03_SCOPE_MATRIX.json`（8 任务/16 验收全 REQUIRED）与 `R03_ACCEPTANCE_LEDGER.json`（16 场景+48 补充叶+additional gates）。未覆盖集合：31 项 deferred_to_later_stage 叶（仍 REQUIRED，验收归 R06/R07/R08，登记于 R03 图与 HANDOFF）+ R02 图 9 项 DEFERRED_TO_R07 叶（R07 消费）。

## 阶段独立验收（2026-09-29）

STAGE-REVIEWER-R03-R01（全新代理，未参与本阶段实现/修复/Task 验收）：**STAGE_VERDICT: PASS**。
- 候选 1ebb03d9f89af364a42274efc2cd482f298b1130；verify-stage R03 独立重跑 exit 0，testedShaAtEnd=1ebb03d9f；16/16 场景；48 叶 17 share PASS + 31 DEFERRED + 0 fail/0 blocked。
- 六组核心事实独立重跑全绿；workspace 626/0（63 suites）；fmt/clippy/check-contracts/check-boundaries exit 0。
- 报告：docs/rust-tauri/R03/R03_FINAL_STAGE_REVIEW_R1.md；证据：artifacts/rust-tauri/R03/STAGE-REVIEW-R01/。
- 治理递延（不阻塞）：a16/E5 封印族分类态 + round3 patch-too-large（git MAX_APPLY_SIZE 1023MiB 硬限，1.90GB 补丁；审计重放规模耗尽）——归 seal 工作流，见 R03_HANDOFF stage_governance_deferrals。

## 已知缺陷

- **R03-T08-FINDING-1（MINOR，T08 组合矩阵发现，未修复——如实登记）**：父取消路径 subagent child run durable 行不就地收口（spawn_linked 包装器 biased select 先丢弃 drive future），由下一进程启动扫描诚实收口为 interrupted_needs_attention（矩阵实测+重启闭环已证）；child 自身超时路径正常落 cancelled。T06 报告对应表述在父取消路径不成立。影响域：子代理父取消场景的行级即时终态（监督层 A06 判定成立）；修复归属 R04/R06 触碰或总控另派。关联：R03-A06（判定成立不受影响）。**【G01-F01 文档更正执行记录 2026-09-29】**T06 报告该表述已由 STAGE-REPAIR-R03-G01-F01 按本裁决语义更正（原句删除线保留，见 repairs/R03_STAGE_REPAIR_G01_F01.md）；产线修复归属不变（R04/R06 或总控另派），本条不因此自标关闭。**【G07 修复轮更新 2026-09-30】**产线修复已由本轮 G01 落地（提交 `520bb75b9`，F01+F02）：父取消路径在当前进程内完成 child 取消收尾/终态/线程与配额回收（`subagent_closeout` 8 用例+`cancel_link_inheritance` 7 用例，同进程反复超上限后仍可派发），G01-R1 独立审查 PASS；本条作为历史 FINDING 记录关闭依据，原文保留。FINDING-2/3 的资产面在候选 `1ebb03d9f` 前已收口（阶段终审裁决3），本轮 G07 全量 R02 回归中 a07/a14 实测绿。
- **R03-T08-FINDING-2（MINOR，R02 资产过期，非 R03 语义回退）**：`r02_t04_live_fault_evidence.py` 重启 keyEvents 精确计数（=1）未计入 R03-T07 恢复扫描对 active run 的诚实 finalize 事件；全量 verify-stage R02 中 a07_live_fault_02_check 红、live_fault_01_test 绿。R02 资产断言更新归独立审阅授权（本 Task 不改已验收阶段门禁断言）。
- **R03-T08-FINDING-3（MINOR，R02 资产过期，非 R03 语义回退）**：`r02_t07_slow_subscriber.sh` 并发同会话风暴「全部 200」断言先于 R03-T02 冻结 busy 闸（409 session_busy retryable=现役 Node 同语义）存在；1000 并发出现 1 个 409 触发该单条断言。同上归属。
- T03 D1/D2（MINOR）：D1 已于 T06 强制修复闭环；D2 为报告文档项，随下次触碰修正。
- T06 D-1/O-1：D-1 报告文档项；O-1（ask 档审批面）为 R04 义务（HANDOFF 已绑）。
- 审计封印坐标滞后族（seal-coordinate-lag）非 R03 技术门禁项，归 seal 工作流/总控。

## 未执行/BLOCKED

- 完整 npm 与审计封印状态：归总控另记（派单明示；本阶段不伪造——legacy 回归 E5 以 directed 模式在门禁内跑 E0–E4.5，全量 E5 attempt-1 输出存档）。不阻止阶段 READY_FOR_REVIEW（属总控账本项，非 R03 验收场景）。
- Windows/正式打包：R09/R10（继承 R02 登记项）。
- 无其他 BLOCKED 项。

## 回退

停用 Rust 预览调度器 = 不选择 Rust 通道（生产默认入口未变，无需动作）；R03 全部改动在新栈隔离面（runs.db 新根/新表 V2–V4），旧栈数据零接触；隔离运行日志保留于 artifacts/rust-tauri/R03/。不丢失本次后新增数据（无生产数据迁移）。

## 独立审查

- T01–T07：各 `R03-T0*_REVIEW_R1.md` 独立评审 PASS（T03 附 D1/D2、T04 附 F-1、T06 附 D-1/O-1，均为非阻塞；F-1/登记递延已由 T08 收口）。
- T08（本 Task）：待总控另派独立复核（本报告为执行者口径）。

## 下一阶段

允许范围：R04 任务书（统一工具网关、四基础工具与沙盒）。必须输入：`R03_HANDOFF.json`（ToolExecutorPort+调用上下文+journal 写序+授权判定点+T06 O-1 ask 档审批策略面义务；TurnProviderPort；execute_background_for/cancel/事件订阅面；48+9 递延叶登记）。不允许开始：R05+ 任务、生产入口切换、真实数据迁移。

## 远程/发布

未获准、未执行：无 commit/push/tag/release；无外发。发布授权条件项独立处理，未授权不伪造其结果。
