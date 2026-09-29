# R03 阶段终审报告（STAGE-REVIEWER-R03-R01，第 1 轮）

- **STAGE_VERDICT: PASS**
- 阶段：R03（运行状态机、并发、取消与恢复）；分支 `codex/rust-tauri-migration`
- R03_START_SHA `526f7770f1eff6be289b8c34faeccc1b95e181fd` → R03_FINAL_CANDIDATE `1ebb03d9f89af364a42274efc2cd482f298b1130`
- 审查者：STAGE-REVIEWER-R03-R01（全新一次性独立阶段验收代理；未参与本阶段任何实现、修复、Task 验收或派单；审查期间未修改任何产品源码/测试/配置/阶段图）
- 审查时间：2026-09-29；环境：macOS darwin 27.0.0 arm64，rustup 锁定 1.98.1，全部经 `--locked`；git 2.53.0
- 复测产物：`artifacts/rust-tauri/R03/STAGE-REVIEW-R01/`（本报告全部证据落此，含本人独立 verify-stage R03 全新证据目录）

## 0. 结论摘要

八个 Task 的组合结果满足 R03 阶段目标：后续 Task 未破坏前面 Task 的 owner/协议/权限/状态/存储；无第二状态负责人、无伪 Provider 默认、无认证后门；完整 R03 Gate 由本人在最终候选 `1ebb03d9f` 上以全新证据目录**独立重跑 overall PASS**（14/14 命令 exit 0、16/16 场景、48 叶=17 pass+31 deferred+0 fail+0 blocked、candidateSourceBinding.stable=true、testedSha=1ebb03d9f）；六组核心事实经定向测试 + workspace 全量（63 suites / 626 passed / 0 failed）复证；递延义务无删除无倒灌；R04/R05/R07 交接接口源码级核实真实存在、可调用、错误与取消语义齐备。三个裁决点均裁决为不阻塞（详见 §7）。a16 残余红双层定性经独立复现成立，登记治理递延。

## 1. 候选与工作树核对

- `git status`：工作树干净（仅本审查的派单文件与产物目录为未跟踪项）；`git rev-parse HEAD` = `1ebb03d9f89af364a42274efc2cd482f298b1130` = R03_FINAL_CANDIDATE；分支与 origin 同步。
- 阶段提交链：T01..T07（→`0c138456b`）→ T08 `77bf3bad1`（R03.json+xtask 注册+交接文档）→ 阶段修复 `1ebb03d9f`（G01-F01）。
- 阶段修复提交（77bf3bad1..1ebb03d9f）非产物面改动仅 8 文件：4 份文档（T06_REPORT 更正/HANDOFF/REPORT/修复记录/派单/ORCHESTRATOR_PROGRESS）+ 2 个 R02 回归脚本（FINDING-2/3 等价断言）——**零产品源码、零 rust/ 代码改动**，与 G01 修复单声称一致。
- 两份既有 Gate 证据核验：
  - `T08-E01/verify-stage-r03`：overall PASS、14/14 exit 0、testedSha=dc42a01e3（T08 时未提交工作树候选，stable=true）。
  - `STAGE-REPAIR-G01-F03/verify-stage-r03`：overall PASS、14/14 exit 0、testedShaAtEnd=77bf3bad1 + G01 编辑未提交（digest 7b1783b9…，stable=true，finalChangedPaths 空）。
  - 两份证据的命令清单（14 条 argv、evidencePaths）与 R03.json 注册逐条一致；本人在**已提交**最终候选上的重跑（§3）补齐了「绑定最终提交」这一环。

## 2. 六组核心事实独立重跑（本人证据目录，真实退出码）

| # | 核心事实 | 命令 | 退出码 | 结果 |
|---|---|---|---|---|
| 1 | 同会话顺序＋跨会话不阻塞 | `cargo test --locked -p lingxi-service --test session_serialization` | 0 | 5 passed / 0 failed |
| 2 | 等待态取消＋子权限/取消传播 | `--test cancellation_tree` ＋ `--test subagent_permission_inheritance` | 0 / 0 | 8/0 ＋ 4/0 |
| 3 | 旧 attempt 迟到＋重复 requestId 冲突 | `--test late_result_fence` ＋ `--test request_dedup` | 0 / 0 | 5/0 ＋ 4/0 |
| 4 | 外部已执行回执未落盘的崩溃恢复 | `--test invocation_journal` ＋ `--test recovery_crash_points` | 0 / 0 | 3/0 ＋ 2/0（11 崩溃点子进程 SIGKILL 族） |
| 5 | shutdown 后拒绝新提交 | `--test exit_race_rejections` | 0 | 2/0 |
| 6 | R02 真实持久化/重启链 | `bash scripts/rust-tauri/r02_t04_storage_tx.sh`（含 S2 kill -9、S4 注册表指纹判定）＋ `r02_t08_full_chain_smoke.sh` | 0 / 0 | S1–S4 全绿；A15 全链 27 PASS 行 ALL GREEN |

以上全部走真实 service 入口（`ServiceState::bootstrap_with_deps` 真实组合根：真实 SQLite/单写者队列/内核状态机/唯一 finalize/取消树/监督/配额），替身仅实现 Provider/Tool port 产生外部响应——无仅手写状态的替身证明核心事实（测试断言直查 SQLite 与外部计数器文件，A11 替身愿意执行 write 而零派发、recovery_crash_points 用独立 rusqlite 句柄直读崩溃前事实）。

补充定向与门禁（均 exit 0，`STAGE-REVIEW-R01/{targeted,gates,r02_chain}/`）：

- `cargo fmt --all -- --check` 无 diff；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`check-contracts`（626 entries 零漂移）；`check-boundaries`（DEP-07/08/D5 PASS）。
- `cargo test --workspace --locked`：**63 test suites 全绿，合计 626 passed / 0 failed / 0 ignored**（T01 482→T02 499→T03 520→T04 539→T05 558→T06 580→T07 601→T08+修复后 626 的单调递增链与各 Task 审查计数一致，无删除/跳过）。

## 3. 完整 R03 Gate 独立重跑（候选绑定最终提交）

```
cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R03 \
  --evidence artifacts/rust-tauri/R03/STAGE-REVIEW-R01/verify-stage-r03
```

- **exit 0，overall PASS**；`verify-stage-result.json`：14/14 命令 exit 0（含 workspace --locked 全量、fmt、clippy、check-contracts、check-boundaries、A15 组合矩阵、A16 seed 机制、7 条 R02 定向链）；16/16 场景 PASS；48 叶 = 17 stage_share_satisfied PASS + 31 deferred_to_later_stage + 0 fail + 0 blocked；runnerSourceBinding PASS（xtask 编译内实现与盘上 R02/R03 图逐字节一致）；`candidateSourceBinding.stable=true`，每命令后 checkpoint 零漂移，**testedShaAtEnd=1ebb03d9f89af364a42274efc2cd482f298b1130**（最终候选提交本身）。
- A15 组合矩阵 28 案例图钉全部 actual==expect（`ok:true`×28、零 non-pass），与 G01-F03 Gate 运行**逐字节一致**（确定性成立）；combo-counts/integration.log 在档。
- A16 seed 机制 P0–P4 全 PASS，捕获 seed=`0xa16d0000000000`（attempt 0 命中、两次重放同位）。
- 7 条 R02 定向链（auth_matrix / storage_tx / events_matrix / backup_restore / recovery_drill / full_chain / legacy_regression-directed E0–E4.5）在 Gate 内全绿。

## 4. 组合阶段完整性检查（后 Task 不破坏前 Task）

1. **owner/状态**：终态唯一 finalize 路径未变——`RunSupervisor::finalize_settlement`（runs.rs:1625）→ `StoragePort::commit_run_outcome`（kernel 裁决在写事务内）；T07 RecoveryCoordinator 的启动扫描结算走**同一** `commit_run_outcome` 事务（recovery.rs:300），是同一 finalize 面的恢复态调用者而非第二写者；`record_run_state_change` 终态目标仍被拒（"belongs to commit_run_outcome"）。六个后续 Task 的 workspace 全量递增链（§2）证明前 Task 套件未被破坏。
2. **协议**：wire/envelope/错误面零改动（check-contracts 626 entries 零漂移）；R03 新事件类型复用 R02 envelope；`lingxi.wire` v1 与 data_epoch 1 未触碰。
3. **权限**：R02 认证/隔离链（auth_matrix 24 案例）在 Gate 内全绿；T06 子代理权限衰减落在 T05 journal authorized 判定步（`RunGrant::Subagent` 逐 target `authorize_child_tool`，拒绝即零派发），未绕过或弱化 R02 授权面；无第二授权面。
4. **存储**：V1 指纹 `479b0321…` 逐字节不动，V2/V3/V4 追加式登记且与注册表/盘上收据对账一致（storage_tx S4 按注册表指纹集判定，全程绿）；runs.db 新根隔离，旧栈数据零接触（回退面成立）。
5. **无第二套负责人/无后门**（重点核查项）：
   - 无第二状态负责人：SessionSupervisor/RunSupervisor/内核状态机/RunDatabase 单写者队列单一链；`SubagentRuntime`/`BackgroundDriveRegistry` 为登记/回流/退出钩子面（无触发/排队/定时逻辑，源码核实）。
   - 无旧 Node/Pi 代执行 Rust 内部循环：Rust 运行循环全在 lingxi-service 内（drive_run 单链）；生产默认入口 Node/Electron（A16 directed E0–E4.5 绿，含 E1 家族断言）；Rust 通道显式 opt-in。
   - 无伪 Provider 默认上线：`ServiceDeps.turn_provider/tool_executor/approval_gate` 生产默认 **None**（lib.rs:519-525）；无 Provider 时显式 `completed.no_final.no_provider_configured`，绝不编造回复。
   - 无认证测试后门进生产默认：生产源码内 `#[cfg(test)]` 均为文件尾测试模块（auth.rs:1753 等 8 处逐一核实）；service 会话/runs/lib 无环境变量授权绕过（唯一 env 读取为 R02 既有 HOME 路径解析）；替身注入仅经 `bootstrap_with_deps` 测试组合根。

## 5. 交接接口核验（R04/R05/R07，源码真实签名对照）

| 接口 | 源码位置（核实存在、pub、签名与 HANDOFF 一致） | 错误/取消语义 |
|---|---|---|
| `ToolExecutorPort::execute`（+`ResultFence`/`ToolOutcome` 含强制 `Unknown`） | lingxi-kernel/src/ports.rs:899 | fence 写前核对；Unknown=外部已执行无回执绝不盲重试/不报成功；`ServiceDeps.tool_executor: Option<Arc<dyn …>>` |
| `TurnProviderPort::{descriptor,next_turn}`（`ProviderTurnResult{fence, turn}`） | ports.rs:878 | Final/ToolRequests/Continue/Empty/Failed(retryable→同 run 新 attempt)；重连不另建用户任务 |
| `SessionStore::execute_submission_for` / `execute_background_for` | sessions.rs:527 / 606 | 同 admission 链（归属→busy 闸→requestId 去重）；`SessionExecuteError` 全枚举（Busy 409 retryable / ShuttingDown 503 / DuplicateRequestConflict 等） |
| `SessionStore::cancel_run_for` / `steer_for` / `run_lineage_for` | sessions.rs:855 / 822 / 934 | `CancelRunOutcome` 四相+AlreadyTerminal+DanglingActive；steer Accepted/Miss；RunLineage 四元 V4 |
| `EventService::subscribe` | events.rs:1155 | SubscribeOutcome 快照/续订阅；重连只订阅（T04 永久测试锁定）；RunOrigin 词表+`execute_background_for` 供 R07 cron/heartbeat/Bridge 复用 |

xtask 交接面：`STAGE_MAPS` 含 ("R02", …)/("R03", …)（main.rs:58-61），runner_identity 钉图字节，stale binary 检测对两图生效。HANDOFF 不写自身 SHA（防自引用）如实。

## 6. 递延义务完整性（未删除、未倒灌）

- R03 图 48 叶与 R00 双账本 execution_stage_ids 含 R03 的叶集**全等**（本人双向 diff 为空，48=48）；31 项 `deferred_to_later_stage` 叶保持 REQUIRED_SUPPLEMENTAL、完整 R00 镜像（assertions/then/stageIds 零缺失，抽查+全量 jq 核验零缺失）、laterShare 明确路由 R06/R07/R08、**零门禁命令/零 assertionContract**（未倒灌本阶段）。
- R02 图 `R02.json` 在 526f7770f..1ebb03d9f 区间**零字节改动**；其 9 项 deferred_to_r07 叶（B8A1AD…/32FFEC…/8BC1A0…/3291CF…/2A1C29…/8ED658…/F8935B…/39AD35…/000E6E…）原样保留。
- HANDOFF deferred_registrations 4 项（R07 九叶 / 31 递延叶归属 R06/R07/R08 / T06-O1 ask 档 R04 / Windows R09-R10）在档。

## 7. 三个裁决点

### 裁决 1：a16/E5 残余红双层定性 — 成立，不阻塞 R03 接受（登记治理递延）

独立复现与定性验证（`STAGE-REVIEW-R01/a16_repro/`）：

- **层 1（注册封印族分类态）**：a16-retry-2 的 E5 候选红集 = 恰 3 个文件、全部属于 SEAL_FAMILY（post-verification-audit-seal + round2/round3-delivery-evidence）；audit-seal 失败为 VERIFIED_SOURCE_SHA 滞后（列出的违例文件全是 R02/R03 授权提交的证据/文档）——与 R02 终审「seal-coordinate governance lag，非产品回归」同族同形。文件级覆盖 PASS、no-new-reds PASS（候选红 ⊆ 基线重放红 ∪ 注册族）为分类器机器输出。
- **层 2（round3 patch-too-large 新治理项）**：本人独立复现——`git read-tree 67dee5d2`（round3 脚本真实 BASE）+ `git diff --binary 67dee5d2..HEAD | git apply --cached --binary -` → **exit 128, `error: patch too large`**，与 a16-retry-2 的失败签名逐字一致。机制定位：git apply 硬上限 `MAX_APPLY_SIZE = 1023 MiB`（git apply.c:406，git 2.53.0 自带二进制 strings 证实）——补丁实测 1,903,492,076 B（≈1.90 GB），其中绝大头是**已提交的证据产物**（R03 区间 top 贡献者为 candidate-source-*.json 各 ~9.5 万行等），产品代码占比可忽略。
- **非 R03 产品回归的反证**：补丁在 R03 起点（526f7770，即 R02 验收态）已 1,822,087,928 B——本人重放该提交同样 `patch too large`。R02 验收当时 round3 现场重放**通过了 replay 步骤**（其 E5 红只有 seal-guard 嵌尾形态 `post-verification diff guard failed:`，blocks 中零 patch-too-large 命中），说明该机制当时处于临界之下；R03 新增的 81 MB 证据使 round3 重放彻底越过 git 硬限——性质是「审计重放基线（67dee5d2/89bc0b64）固定古老 + 仓库证据单调增长」的**机制规模耗尽**，随后续阶段只会更差，与产品语义无关（全部产品/运行时门禁绿）。
- **裁决**：不得套旧类别（seal-coordinate-lag 描述的是坐标滞后分类态，不能覆盖 replay 机制本身的规模死亡）；按独立治理项登记，归 seal 工作流在获授权推进重放基线时收口——**不阻塞 R03 阶段接受**。依据：R02 验收对同一条 E5 全量红（当时为 seal 三族）以分类态放行的同一先例；R03 派单明示阶段图内 legacy 回归为 directed E0–E4.5（本人在 Gate 内复跑全绿），E5 全量分类与封印坐标归总控/seal 工作流账本；修复路径唯一且在本阶段红线之外（不修改审计测试、不扩分类器注册类消红——G01-F03 正确地未这么做，分类器按设计 fail-closed 保持 exit 1）。

### 裁决 2：T08 FINDING-1 两阶段收口解释 — 满足 A06/T03 契约（PASS-with-registration 维持）

- 契约句「受管工作退出后回收资源并写最终状态」：监督任务层成立——spawn_linked 包装器 biased select 在树取消时丢弃 child drive future 并 `record_exit(Aborted)`，`live_children_of(parent)` 清空、child 永不 completed、无 final message（A06 监督断言实测，A15 图钉 `ok:true` 复证）；资源（future/tokio task/配额/许可）就地回收。
- durable 行级终态延迟至下一进程启动扫描收口为 `interrupted_needs_attention`——这不是为本缺陷发明的特例，而是 T07 已确立的**文档化闭环模型**（关停残留行/崩溃残留行走同一 `DanglingActive`→扫描→同一 finalize 事务路径）；A15 崩溃恢复案例实测了闭环第二阶段（全新 bootstrap 真实扫描 → interrupted_needs_attention + 0 final message）。行永不伪造终态、永不 completed——不属 05 契约 §5「必须为零」任一项。
- G01-F01 文档更正已核实落盘：T06_REPORT 原句删除线保留 + 裁决语义更正、REPORT/HANDOFF 同步、产线修复归属不变（R04/R06 触碰或总控另派）且必跑矩阵在 T08 审查列明。残留影响（长驻进程内行 active、线程 busy 标志不清致 reply/close 被拒直至重启）为预览通道精度缺口，MINOR 定级合理。
- 裁决：**「登记的已知缺陷+文档已更正」满足 A06/T03 契约**，不阻塞阶段接受；产线修复按登记归属递延，修后按既列矩阵重跑。

### 裁决 3：G01 等价断言 — 未降低 R02 保护

- **FINDING-2（a07 restart 段）**：`runs==1` 保留（无新 run）、`keyEvents 1→2`（R03-T07 恢复扫描经同一 finalize 诚实补一终态事件）+ **新增** `status==interrupted_needs_attention` 钉住。严格强于原裸计数：伪造 completed、空白行、多跑一个 run、少/多事件现在都会失败；与 T07 已获审的库内等价断言（execute_concurrency (1,1)→(1,2)+no-fake-success）先例一致。busy/terminal/disk 段原文未动。修后 a07_live_fault_02_check 实测 PASS（verify-stage R02 19/20，唯一红=a16）。
- **FINDING-3（a14 风暴 W1 段）**：「全部 200」→「每个 execute 均有应答且 ∈{200,409}，409 必须 body 含 session_busy+retryable:true，零 5xx/零超时/零悬挂，全部请求记账（len==EXECUTES）」。409 是 R03-T02 冻结的现役 Node 同语义 busy 闸拒绝（T02 审查对 desktop-session-submit.ts:455/sessions.ts:1539 逐点位核实）——被服务的合法拒绝而非 stall；no-stall/no-fake-success 语义保留且应答记账更强；W2 顺序段「全部 200」**原文保留**；S2 慢订阅者显式 detach 断言未动。修后 a14_slow_subscriber 实测 PASS。备注（cosmetic，非缺陷）：storm-results.json 的 `zero_5xx_zero_timeout_zero_hang: true` 为断言守护下的常量字段，真实保护在 probe 内 asserts。
- 裁决：**两处等价断言均为收紧或等价，未降低 R02 保护**；R02 图命令与断言原文零改动（R02.json 零字节 diff 复核）。

## 8. 逐 Task 与 A-ID 当前证据状态

| Task | A-ID | 执行者/独立审查 | 本人复证 |
|---|---|---|---|
| R03-T01 状态机 | A01/A02 | PASS / REVIEW-R1 PASS | run_lifecycle+run_finalize_property 在 626/0 内；A01 终态唯一+三模型调用；A02 属性计数 200/350/1450/0 |
| R03-T02 串行化/限流 | A03/A04 | PASS / PASS | session_serialization 5/0 复跑；busy 409 冻结语义（FINDING-3 等价面） |
| R03-T03 取消树/监督 | A05/A06 | PASS（D1 于 T06 修复闭环）/ PASS | cancellation_tree 8/0 复跑；D1 修复（AbortHandle+Drop 探针）在 626/0 内 |
| R03-T04 迟到栅栏 | A07/A08 | PASS（F-1 递延 T08 收口）/ PASS | late_result_fence 5/0 + request_dedup 4/0 复跑 |
| R03-T05 journal/收据 | A09/A10 | PASS / PASS | invocation_journal 3/0 + recovery_crash_points 2/0 复跑；V3 指纹对账绿 |
| R03-T06 子代理/后台/权限 | A11/A12 | PASS / PASS（O-1 ask 档 R04 绑定） | subagent_permission_inheritance 4/0 复跑；REPORT 更正落盘核实 |
| R03-T07 恢复/退出 | A13/A14 | PASS / PASS | recovery 套件在 626/0 内；exit_race_rejections 2/0；r02_recovery_drill/r02_full_chain 绿 |
| R03-T08 验收/交接 | A15/A16 | READY_FOR_REVIEW / REVIEW-R1 PASS（3 收口义务） | A15 28/28 图钉与 G01-F03 逐字节一致；A16 seed=0xa16d0000000000 复现 |

16/16 场景在本人 Gate 重跑内全 PASS；`R03_ACCEPTANCE_LEDGER.json` command_refs 回溯至真实命令产物（抽查+全量结构核验）。三份阶段收口义务（FINDING-1 产线修复、FINDING-2/3 等价断言+verify-stage R02 全量重跑、T06 表述更正）中第 2/3 项已由 G01 修复完成并验证，第 1 项维持登记递延——状态与 R03_REPORT/R03_HANDOFF 记载一致（REPORT §已知缺陷如实保留未自标关闭）。

## 9. 发现

**无阻塞发现（BLOCKING=0、MAJOR=0）。** 本审查未制造与原合同无关的新必改需求。非阻塞观察（供总控账本，均已有归属登记）：

1. O-1（延续登记）：FINDING-1 产线修复（父取消路径 child durable 行就地收口）+ 其必跑矩阵——归属 R04/R06 或总控另派，HANDOFF known_gaps 已绑定。
2. O-2（治理递延）：a16/E5 双层红（封印族分类态 + round3 patch-too-large 规模耗尽）——归 seal 工作流推进重放基线时收口；在基线推进前每阶段 E5 全量将保持红（fail-closed），directed E0–E4.5 为阶段图内合法通道。
3. O-3（cosmetic）：r02_t07 storm-results.json 的 `zero_5xx_zero_timeout_zero_hang: true` 常量字段建议随下次触碰接真实分支或移除，防误读。

## 10. 范围限制

- 本机 arm64 macOS 单平台；无真实供应商/真实外部系统（R03 规格允许确定性替身）；Windows/打包归 R09/R10；完整 npm/审计封印状态归总控账本（本阶段未伪造）。
- a16 层 2 复现基于 git 2.53.0（与 G01-F03 运行环境同机同版本）；git apply 1023 MiB 硬限由 git 官方源码 apply.c 证实。
- R02 全量 verify-stage R02 未由本人整跑（G01-F03 已跑：19/20，唯一红 a16）；本人在 R03 Gate 内复跑其 7 条定向链全绿，满足派单「或等效定向命令重跑」路径。

## 11. 最终判定

R03 阶段目标（运行状态机/会话串行化/取消树/迟到栅栏/journal 与副作用收据/子代理与后台/故障恢复/验收交接）经真实调度器与真实服务链路证实：唯一终态、取消传播、迟到栅栏、unknown 副作用恢复、权限继承、断线≠取消、退出拒新、诚实重启呈现全部有机器证据且由本人独立复跑；阶段图 48 叶（17+31）与 R00 账本全等、递延未丢失；R04/R05/R07 交接接口真实可调用；三个裁决点均不阻塞；递延治理项（a16 双层红、FINDING-1 产线修复、T06-O1、Windows）登记在册、归属明确。

**STAGE_VERDICT: PASS**
