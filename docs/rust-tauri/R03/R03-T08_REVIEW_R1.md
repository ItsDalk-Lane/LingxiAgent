# R03-T08 独立验收报告（第 1 轮）

- VERDICT: **PASS**（附 3 项已登记收口义务的裁决确认 + 6 项建议性观察；无未关闭验收阻塞缺陷）
- TASK_ID: R03-T08（状态机验收与 R04 接口交接）；ACCEPTANCE_IDS: R03-A15、R03-A16
- 审查者: REVIEWER-R03-T08-R01（全新一次性独立验收代理；未参与本 Task 的实现/修复/派单；验收期间未修改任何产品源码/测试/配置/阶段图，代码冻结）
- 候选: TASK_BASE_SHA `dc42a01e37d1110293ec137402fd3239bd5b7d9d`（分支 `codex/rust-tauri-migration`，=HEAD，无 commit/push）+ 未提交工作树（修改 10 + 新增 9 路径）
- 审查时间: 2026-09-29；环境: macOS darwin 27.0.0 arm64；rustup 锁定 1.98.1（经 `~/.cargo/bin` + `--locked`）；复测产物仅写 `artifacts/rust-tauri/R03/T08-R01-review/`（rust/target/ 允许）

## 1. 候选摘要前后两次核对（代码冻结证明）

- 验收开始时：`git diff dc42a01e3 | shasum -a 256` 前 16 位 = **`7deab269990f9b43`**（=派单绑定值）。
- 全部复跑与报告撰写前（验收末期）再次计算 = **`7deab269990f9b43`**；`git status --porcelain` 仍为 10 修改 + 9 新增（+审查产物），tracked-diff 零漂移。审查期间代码冻结成立。
- 逐文件清单与派单一致：修改 10（R02-T04_STORAGE_REGISTRY.json、lingxi-adapters 两属性测试、xtask 5 文件、r02_t04_storage_tx.sh、r02_t08_legacy_entry_regression.sh）；新增 9（R03.json、r03_t08_acceptance_matrix.rs、脚本 3、报告/交接 4）+ 证据根与派单文件。

## 2. 阶段图注册与正式 Gate 复核（SUP-01）

**R03.json 逐字段核对（源码级）**：

- `commands` 14 条全部真实可执行（fmt/clippy/全量 test --locked/check-contracts/check-boundaries/a15 矩阵/a16 机制/7 条 R02 定向命令），每条带 timeoutSecs 与 evidencePaths；7 条 R02 定向命令与派单 SUP-02 清单一一对应（auth_matrix/storage_tx/events_matrix/backup_restore/recovery_drill/full_chain/legacy_regression-directed）。
- `scenarios` 恰为 R03-A01..A16、全 REQUIRED、每条绑定真实命令（xtask 单测 `embedded_r03_map_uses_the_taskbook_scenarios` 亦锁死该形状并校验无孤儿命令）。
- `supplementalLeafScenarios` 48 项：我独立对 `docs/rust-tauri/R00/ACCEPTANCE_MAP.json` 计算 execution_stage_ids 含 R03 的叶 = 48，与图内集合**全等**（双向差集为空）；随机抽 3 叶对双账本逐字段镜像核对（featureId/requirement/kind/taskIds/stageIds/ledgerStatus/resultIds/testIds/then/assertions/due 11 字段）全等。分类 17 `stage_share_satisfied` + 31 `deferred_to_later_stage` + 0 + 0（17 份额叶全部带 assertionContract 且 producer=a15_combo_and_leaves；31 递延叶零门禁绑定——递延分类抽查合理：todo_write/notify 工具归 R04、实验 API/cron 路由/压缩/草稿/偏好/UI 投影归 R06/R07/R08，均非 R03 实现面）。
- 生成器 `r03_t08_generate_stage_map.py` 从双账本+分类表可复现生成；分类决策（17/31 及每叶 stageShare/laterShare 文本）落在生成器内可审计。

**STAGE_MAPS/runner_identity 注册**：main.rs `STAGE_MAPS` 增 `("R03", include_str!(...))`；runner_identity.rs EMBEDDED 钉 R03.json 字节 + SOURCE_INVENTORY 列名——stale binary 检测对新图生效（EMBEDDED 循环逐文件比对；现有单测模式覆盖，见 §11-O2 观察）。

**xtask 核心改动逐行裁决（R02 门禁语义未弱化）**：

- `stage_map.rs`：新增两个阶段中立 kind（`stage_share_satisfied`/`deferred_to_later_stage`），解析器**按 kind 分支读取自己的份额字段对**（中立 kind 读 stageShare/laterShare，R02 kind 读 r02Share/r07Share）——R02 图解析路径逐行等价（R02.json 本身零字节改动，diff 为空核实）。中立递延 kind 复制了 deferred_to_r07 的全部硬校验（不得绑门禁命令/assertionContract/可过 evidencePaths；新增"递延不得声明可过 evidencePaths"硬校验；单阶段叶禁止递延）。
- `verify.rs`：中立递延叶 roll-up 与 deferred_to_r07 同构（固定状态、early-evidence 失败不改叶状态但计入全命令通过条件）；份额叶走与 r02_share_satisfied **同一条** R14-F01 图钉核对路径（check_leaf_assertion_contract：actual==expect、evidence path 存在、命令通过，任一不满足=FAIL）。overall PASS 条件不变：全场景 PASS ∧ PASS∪DEFERRED 覆盖全部 48 叶且总数==R00 账本 ∧ 全命令通过。输出字段 `deferredToR07` 由"状态计数"改为"deferred_to_r07 声明计数"——对 R02 图两者恒等（该 kind roll-up 固定返回 DEFERRED_TO_R07），无弱化。
- `runner_tests.rs`：**纯新增 371 行、零删除**（diff `-` 行为空核实）；既有 R02 断言全部原样。
- `main.rs`/`runner_identity.rs`：仅注册新增。

**R02 图按原断言执行证明**：执行者全量 `verify-stage R02`（§6）中 R02 图 20 命令仍按原语义运行——3 红恰是原断言未被改写的证据（若门禁被弱化，a07 检查器/a14 风暴不会红）；17 绿含全部原有硬校验。R02.json 零改动 + 两 R02 证据脚本（r02_t04_live_fault_evidence.py / r02_t07_slow_subscriber.sh）零改动（diff 为空核实）。

**负向探针（本人实跑）**：`verify-stage R99` → **exit 2**（"unknown stage…registered stages: R02, R03…never an empty pass"）；`verify-stage R03 --evidence <非空目录>` → **exit 1**（"evidence root … is not empty; preserving its prior evidence"）。与执行者 negative-probes/ 证据一致；单测层新增负向（空场景集/未知 kind/递延绑门禁/镜像漂移/单阶段递延等）随 workspace 全绿。

## 3. R03-A15（真实入口驱动状态）复核 — PASS

- 组合矩阵测试 `r03_t08_acceptance_matrix.rs`（17 个 tokio 测试）逐段读源码核实：替身（ScriptedProvider/CountingTool/ManualGate 形态）只实现 `TurnProviderPort::next_turn`/`ToolExecutorPort::execute`/审批决策三类外部响应，无任何存储/状态写入路径；全部场景经 `SessionStore::execute_submission_for`/`execute_background_for`/`cancel_run_for`/`steer_for`/`EventService::subscribe` 真实入口，状态/事件/终态由真实 SessionSupervisor/RunSupervisor/kernel/RunDatabase/EventService 产生。案例在进程内断言 actual==expect 后落 `lingxi.leaf-case-results.v1` 碎片（脚本仅装配）。
- 覆盖 ≥ 派单 8 组合：正常/多轮模型（3 调用唯一终态）/多工具（3 工具调用）/超时（failed.turn_budget_exceeded 响亮）/取消两形（流读取 0 条 model_call_completed、审批等待 0 工具执行）/重复（replay 零重执行 + DuplicateRequestConflict 不复用不新增）/乱序（取消后迟到投递流零污染，折入 cancel-stream 组合并有专属图钉案例）/跨会话/崩溃恢复（drive 丢失 + 全新生产形态 bootstrap 真实启动扫描 → interrupted_needs_attention、0 final message；SIGKILL 形态由 T07 recovery_crash_points 11 崩溃点覆盖，标注如实）。
- 28 案例图钉：**我的复跑 leaf-cases.json 与执行者逐案例 expect/actual 完全一致**（28/28 ok）；combo-counts 计数与报告表格一致（normal 1/1/0、multi-turn 3、multi-tool 3、timeout failed、duplicate 1 replay+1 conflict、crash interrupted 等）。`integration.log`（测试原始 stdout）+ 订阅者终态事件观测在档。

## 4. R03-A16（故障种子可重放）复核 — PASS（机制证明，如实标注成立）

- 永久机制核实：两属性测试新增 seed 覆写（`R03_T04/A02_PROPERTY_SEED` hex/decimal）与 thread-local 感知 panic hook（stdout+stderr `*_PROPERTY_FAILURE seed=0x… location=…` + `*_SEED_FILE` 落盘）；默认 seed 常量与全部既有断言零改动（diff 核实——仅 seed 来源与打印参数化）。
- `r03_t08_a16_seed_mechanism.sh` 逐行读：P0 真实测试默认 seed 绿 → P1 mktemp 临时 cargo crate 复制真实测试+注入受控缺陷（seeded 稀疏子集经独立 SQLite 连接把终态行改回 queued——正是"终态永不翻转"探测器必抓的非法序列；仓库树零触碰）→ P2 扫描捕获（失败行+seed 文件双记录一致、排除 seed 解析错误、要求真实 FAILED）→ P3 同 seed 两次重放一致 → P4 删除变异后同 seed 真实测试绿+双属性测试全组绿；scratch 目录 trap 清理。
- 证据核实：captured-seed=**0xa16d0000000000**（两次 P3 重放同 seed 同 location=mutation_probe.rs:473）；`seed-mechanism.json` 如实标 `MECHANISM_PROOF_ISOLATED_MUTATION` + honesty_note（不编造自然发现史）——符合派单「没有现成异常时用隔离测试变异证明机制」的明示路径。
- **我的复跑**：a16_seed_mechanism 命令在门禁内 exit 0，捕获 seed 与执行者一致。

## 5. STORAGE_REGISTRY / S4 修复复核（T04 R1-F1 + T05/T06 登记递延收口）

- **指纹独立重算（SQL 文本级，与执行者的服务+inspector 路径互补）**：从 `lingxi-adapters/src/storage/migrations.rs` 提取 V1–V4 SQL 原文逐字节 sha256：V1=`479b0321…`（=已发布值逐字节不动）、V2=`64d7edfd…`、V3=`3bd5388f…`、V4=`371d4154…`——**四值与注册表登记值全等**，且与 T04 审查/T05/T06 报告短前缀一致。
- 注册表修改为**合法增量**：V1 原条目零改动；V2/V3/V4 追加式登记（含 registered_by_task 与登记说明）；tables 面补三表所有权；R02_HANDOFF interfaces[3] 确以该文件为迁移指纹权威（"migrations 版本化（R02-T04_STORAGE_REGISTRY.json 指纹）"），migrations.rs 自身头注亦规定"追加=受审议为、改既有=拒收"——非改写历史。
- S4 修复语义核实：`r02_t04_storage_tx.sh` S4 由硬编码 `version==1`/单条 receipt 改为读注册表 → userVersion==supportedVersion==注册表条数 → receipts 与 compiledIn 逐条 version/name/fingerprint 与注册表全等；任一边漂移响亮失败（无静默放宽路径）。**我的复跑**：r02_storage_tx 命令在门禁内 exit 0（S1–S4 全程绿）。

## 6. R02 回归链与 FINDING-2/3 裁决

**7 条定向命令**（SUP-02）在**我的** verify-stage R03 复跑内全绿：auth_matrix / storage_tx / events_matrix / backup_restore / recovery_drill / full_chain / legacy_regression(directed)。r02_t08 的 npm 定向（E0–E4.5：默认入口/绑定面/typecheck x3/core-contracts/boundary/renderer build）全 PASS。

**全量 verify-stage R02 取证复核（执行者原始日志逐条读取）**：17/20 命令绿、34 叶=25 pass+9 deferred+0 fail+0 blocked、candidate stable、runner PASS；overall FAIL 的 3 红逐一归因核实：

1. `a16_legacy_regression` exit 1：E5 全量 npm+封印族分类对**未提交候选树**闭失败——e5-candidate-causes.txt 显示 round3 块为 `UNRECOGNIZED,seal-coordinate-lag` 混合形态，分类器按设计 fail-closed；pristine-base 重放绿、无新红。与 R02_FINAL_STAGE_REVIEW 对 seal 三族的「seal-coordinate governance lag，非 R02 回归」定性一致；未提交树的完整分类+封印归 seal 工作流/总控账本（AGENTS.md 封印流程）。R02 图该命令零改动（默认 full 模式）。
2. `a07_live_fault_02_check` exit 1（FINDING-2）：红点恰为 `restart.keyEvents mismatch` 单条；`a07_live_fault_01_test`（真实测试）PASS。核实根因链：R03-T07 启动恢复扫描（A13 设计行为）对崩溃残留 active run 诚实 finalize（interrupted_needs_attention 终态+事件）→ keyEvents 1→2。**同根因的库内等价断言已在 T07 更新并经 T07 独立审查确认（execute_concurrency.rs 重启断言 (1,1)→(1,2) 并新增 no-fake-success 状态断言，被评为强化）**；未更新的只剩 R02 期检查器脚本的精确计数。
3. `a14_slow_subscriber` exit 1（FINDING-3）：红点恰为 W1 风暴「every execute must answer 200」单条（1000 并发 24 路，实测 1 个 409 Conflict）；busy 409+session_busy+retryable 为 R03-T02 冻结语义（T02 报告对照现役 Node 源码点位核实的映射，session_busy 409=现役同语义）；S1 其余断言（慢订阅者不 stall 写者/无假成功/健康/有界）未触及。

**r02_t08_legacy_entry_regression.sh 修改审明**：新增可选 `R02_LEGACY_REGRESSION_MODE`，默认 `full`=R02 图原语义零改动（R02.json 命令不带该 env，走 full）；`directed-no-seal-family`=E0–E4.5 后**响亮 SKIP 并记录**（note 明示完整链仍是 R02 图命令与总控账本项）；未知值 exit 1。不重分类、不把任何 E5 红变绿、无静默路径——**不构成门禁弱化**（R02 门禁的 full 模式断言原文零改动；R03 图用 directed 模式是派单明示的范围拆分）。

**FINDING-2/3 裁决：可接受（登记为阶段收口义务），非本轮 FAIL 项**。依据：a) 派单对 R02 回归义务明示「verify-stage R02（全新证据目录）**或等效定向命令重跑**」——定向路径已满足（我的复跑再证）；b) 两红均为 R02 期资产内部计数/形态相对 R03 设计行为（T07 恢复扫描、T02 busy 闸）过期，R03-A07 语义无回退（live_fault_01_test 绿；矩阵 out-of-order 图钉绿）；c) 执行者不改已验收阶段门禁断言而留红+逐条归因，是诚实保守选择（01-约束 §5 允许等价改写但须保留公开保护，该改写权在独立审阅授权）；d) 同根因库内等价改写在 T07 已有获审先例，风险低。**条件：不得永久留红**——收口义务与等价断言方案：

- FINDING-2 等价断言：`r02_t04_live_fault_evidence.py` restart 段改为 `runs==1`（不产生新 run，保留）、`keyEvents==2`（与 T07 已改的库内断言对齐；或 `>=1` 且新增"重启后该 run 状态∈{interrupted_needs_attention} 且无 final message"断言——比原计数更强地钉住 no-fake-success）；busy/terminal/disk 段与 run 身份一致性断言原文保留。修后必须重跑：a07_live_fault_01_test + a07_live_fault_02_check + verify-stage R02 全量。
- FINDING-3 等价断言：W1 断言改为「每个 execute 均有应答且应答∈{200, 409}，409 必须 body 含 session_busy+retryable:true，零 5xx/零超时/零悬挂」（409=被服务的合法拒绝而非 stall，no-stall 语义保留；no-fake-success 语义保留）；W2 顺序段「全部 200」原文保留；S1 其余断言不动。修后必须重跑：a14_slow_subscriber + verify-stage R02 全量。
- 两项归属：总控另派 R02 资产更新（独立审阅授权），或并入下一触碰 R02 脚本的授权任务；R03-T08 报告/HANDOFF 已如实登记，不需回改。

## 7. FINDING-1 裁决（父取消路径 subagent child durable 行不就地收口）

**裁决：PASS-with-registration（MINOR 定级成立，非阻塞，不需本轮修）**。核实链条：

- a) **子任务确实停止（监督层证据）**：`task_supervisor.rs` spawn_linked 为 `biased select`——树取消分支先就绪时丢弃 child future 并 `record_exit(TaskExit::Aborted)`；矩阵 leaf_parent_cancel_stops_child 实测断言 `live_children_of(parent)` 为空（A06 监督信号）+ child 永不 completed + 无 final message；child 自身超时路径（timeout_at+drive.await）正常经自身四相取消落 cancelled（leaf_child_cancel 场景绿）。
- b) **两阶段收口解释成立**：T07 已把「durable 行保持 active、由下一进程启动扫描诚实收口 interrupted_needs_attention」确立为**文档化闭环模型**（T07 报告退出序明示 background_unconfirmed 残留行同路径收口；HANDOFF cancellation 诚实边界如实记载 + `DanglingActive` 诊断式结果）。矩阵同时证明了闭环第二阶段（全新 bootstrap 真实扫描 → interrupted_needs_attention + 0 final message）。行永不伪造终态、永不 completed——不属 §5「必须为零」清单任一项（无假成功/无重复终态/无取消后污染）。R03-T03 契约「受管工作退出后回收资源并写最终状态」在监督任务层（tokio task 退出、future 于 await 点释放=与断连 HTTP 请求同构）成立；durable 行级终态延至恢复扫描，属该文档化模型的诚实偏离，非验收场景（A05/A06）失败。
- c) **T06 报告表述失实**：「child 由自身 drive 观测并经唯一 finalize 落 cancelled」在父取消路径不成立（drive future 被 biased select 丢弃）——已由 T08 在三份阶段级文档（T08 报告 §6、R03_REPORT 已知缺陷、HANDOFF known_gaps）如实更正并定修复归属。**文档级义务登记足够**：T06 的 A11/A12 验收判定不受影响（权限继承/断线语义与父取消 finalize 无关），改已过审报告正文留给下次触碰该报告时更正（与 T03 D2/T06 D-1 同处置模式），不构成本轮 FAIL。
- **残留影响（如实入账，供修复轮）**：长驻进程内该 child 行持续 active、线程登记 busy 标志不清（该线程 reply/close 被拒直至重启）——预览通道的精度缺口，MINOR 合理。**修复归属已绑定**（HANDOFF known_gaps：R04/R06 触碰 task_supervisor/subagents 时按根因收口或总控另派）。**修后必须重跑**：cancellation_tree.rs（r03_a05/a06）、r03_t08_acceptance_matrix.rs（leaf_parent_cancel_stops_child/leaf_subagent_dispatch_reply_close/leaf_child_cancel_stops_only_the_child）、subagent_*/background_* 全组、recovery_startup_scan.rs、lingxi-service 全量 + verify-stage R03 整链。

## 8. 交接文档质量（R03_REPORT / R03_HANDOFF / R03_ACCEPTANCE_LEDGER）

- **接口为真实可调用名称+签名（逐条对照源码核实）**：`ToolExecutorPort::execute`/`TurnProviderPort::{descriptor,next_turn}`（lingxi-kernel/src/ports.rs，签名逐字一致）；`ServiceDeps.turn_provider/tool_executor: Option<Arc<dyn …>>`（lib.rs，生产默认 None + 响亮行为记载一致）；`SessionStore::execute_submission_for/execute_background_for/cancel_run_for/steer_for/run_lineage_for`（sessions.rs 签名一致）；EventService::subscribe 存在（见 O-1 精度观察）；错误/取消语义（SessionExecuteError 全枚举、CancelRunOutcome 四相/AlreadyTerminal/DanglingActive）、journal 写序（prepared→authorized→started→receipt）、T06 O-1 ask 档义务绑定 R04、递延登记（R07 九叶 ID 与 R02 图 deferred 集**逐一全等**核对；31 递延叶；O-1 ask 档；Windows R09/R10）均在档。**不写自身 SHA** ✓（tested_sha=基线 SHA + 防自引用说明）。
- **LEDGER 16 A-ID 证据映射回溯（超 6 项抽查，实查全部 16）**：每 A-ID 的 command_refs 均回溯到执行者 gate 证据目录内真实存在且内容相符的命令产物（rust_test_workspace 941 行 63×"0 failed"；r02_storage_tx/full_chain/backup_restore/recovery_drill/auth_matrix/events_matrix/legacy_regression 各 stdout 尾行 ALL GREEN；A15 三件证据；A16 四件证据）；退出码与 gate result.json 一致；我的复跑再证 14/14 exit 0 + 16/16 PASS + 48=17+31。
- attempt-1 如实性：attempt1/ 目录存在，FAILURE-ANALYSIS 与我独立复现的失败形态一致（a16 参数 bug 已修；E5 分类属上述第 1 红）。

## 9. 实际复跑（全部本人本机真实执行；产物在 T08-R01-review/）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo run --locked -p xtask -- verify-stage R03 --evidence artifacts/rust-tauri/R03/T08-R01-review/verify-stage-r03`（全新目录） | **0（overall PASS）** | 14/14 命令 exit 0（含 workspace --locked 全量、fmt、clippy -D warnings、check-contracts、check-boundaries、A15 矩阵、A16 机制、7 条 R02 定向）；16/16 场景 PASS；48 叶=17+31+0+0；candidateSourceBinding.stable=true；runnerSourceBinding=PASS；testedSha=dc42a01e3（dirty 属预期）；工具链 1.98.1 |
| 负向探针（自跑） | 2 / 1 | 未知阶段 R99 exit 2；脏证据根 exit 1（§2） |
| 迁移指纹独立重算（SQL 文本 sha256） | — | V1–V4 与注册表全等（§5） |
| verify-stage R02 抽验 | — | 接受执行者取证并逐条复核其原始日志（§6：20 命令退出码、3 红日志、E5 分类文件、双账本 deferred 集）；另我的 gate 复跑重放 7 条定向全绿 |

A15 逐案例对比：我的 28 案例 expect/actual 与执行者完全一致（确定性成立）；A16 捕获 seed 一致。

## 10. 范围核对

- 不实现 R04+：rust/ 改动仅 xtask + 两个属性测试文件 + 一个集成测试 + R03.json——**零产品源码改动**；无工具网关/真实供应商/CLI/UI。
- tokio test-util：Cargo.lock 全文无 test-util；rust/Cargo.toml 零 diff。
- `rust/Cargo.toml`/`rust/Cargo.lock`/各 crate Cargo.toml 零变化；三个锁文件 sha256 与 HANDOFF 登记值全等（Cargo.lock=90111c4b…、package-lock=e54a16fe…、rust-toolchain=eec34104…/1.98.1）。
- npm/桌面栈：工作树零触碰（git status 无 npm/桌面路径）；npm 侧仅注册命令在 /tmp 候选副本上按惯例运行（directed E0–E4.5）。
- 生产默认入口不变（E1 家族断言在我的 gate 复跑内全 PASS）。
- skills2set/、共享契约、审计封印白名单：零触碰。

## 11. 建议性观察（不构成验收缺口）

- O-1（HANDOFF 精度）：`EventService::subscribe` 实际签名为 `async fn subscribe(...) -> Result<SubscribeOutcome, SubscribeReject>`；HANDOFF 只写了 SubscribeOutcome 两形态，未提 Result 包装与 Reject 变体（StreamNotFound/Forbidden 等）。建议 R04/R07 消费时以 events.rs 为准，或下次触碰 HANDOFF 时补全。
- O-2（stale 检测对称性）：runner_identity 单测的 stale-rejection 变异只打 R02.json；EMBEDDED 循环机制对 R03.json 同样生效，但补一个 R03.json 变异用例更对称。
- O-3（LEDGER 预期字段）：16 场景的 `expected` 均为「阶段书通过条件（任务书 §4 对应验收原文）」引用式占位而非逐字通过条件；权威仍在任务书+阶段图，建议后续账本直接内联原文。
- O-4（表述计数）：T08 报告称「组合矩阵 12 场景」；combo-counts.json 实为 11 个 combo 计数器（乱序折入 cancel-stream-read 且有专属图钉案例）+测试函数 17 个。口径不影响覆盖判定（≥8 必需组合全在），建议下次触碰统一口径。
- O-5（A02 案例粒度）：A02/A03/A04/A09/A10 仅绑 rust_test_workspace 全量套件（与 R02 图同模式）；如后续需要单场景失败定位，可在图中细化定向命令。
- O-6（findings 台账汇总）：T03 D2、T06 D-1 两个报告级 MINOR 文档项仍开（登记为随下次触碰更正）；加上本次 FINDING-1 的 T06 表述更正，共 3 个「下次触碰时更正」项，建议总控在阶段收口清单里合并追踪，避免散失。

## 12. 结论

- **R03-A15：PASS**（真实链路驱动成立、替身边界干净、28 案例图钉 my-run 复现一致）。
- **R03-A16：PASS**（机制证明如实标注；seed=0xa16d0000000000 两次稳定复现+移除后同 seed/全组绿；永久机制入两属性测试；my-run 复现）。
- 阶段 Gate：verify-stage R03 exit 0（overall PASS）由我独立复跑证实；负向探针 exit 2/1 自跑证实；R02 门禁语义经逐行 diff 与全量 R02 取证复核未弱化。
- FINDING-1：PASS-with-registration（两阶段收口模型成立、T06 表述更正与产线修复登记归属明确）；FINDING-2/3：R02 期资产过期确认、留红可接受，**登记为阶段收口义务**并附等价断言方案与必跑矩阵（§6）；E5/封印族归总控账本（派单明示）。
- 到期义务（阶段书 T08 怎么做 1–4、必须交付三件、A15/A16、总控细化 1–7、T04 F-1 收口）均有有效证据且真实接线成立；无未关闭验收阻塞缺陷。

**VERDICT: PASS**

（收口义务移交：①FINDING-2/3 R02 资产等价断言更新+verify-stage R02 全量重跑；②FINDING-1 产线修复（R04/R06 或总控另派）+其必跑矩阵；③T06 报告表述更正（与 T03 D2/T06 D-1 合并追踪）。以上均不阻塞本 Task 验收与阶段 READY_FOR_REVIEW 之后的独立阶段验收判定。）
