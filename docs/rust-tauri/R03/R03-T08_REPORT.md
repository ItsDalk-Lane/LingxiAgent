# R03-T08 报告｜状态机验收与 R04 接口交接（EXECUTOR-R03-T08-E01）

- 状态：**READY_FOR_REVIEW**（执行者口径；独立复核归总控另派）
- TASK_ID：R03-T08（ACCEPTANCE_IDS：R03-A15、R03-A16）
- TASK_BASE_SHA：`dc42a01e37d1110293ec137402fd3239bd5b7d9d`（分支 `codex/rust-tauri-migration`，无 commit/push；工作树候选留给总控冻结）
- 执行时间：2026-09-29（UTC 见 verify-stage-result.json startedAt/finishedAtUnixMs）
- 环境：macOS darwin 27.0.0 arm64；rustup 锁定工具链 **1.98.1**（全部命令经 `~/.cargo/bin` rustup 代理 + `--locked`；证据链统一 `CARGO_TARGET_DIR=/tmp/r03-t08-target`）；SQLite=rusqlite 0.40.2 bundled；无网络外发、无真实供应商；npm/桌面栈除 R02 回归链注册命令外零触碰；**tokio test-util 未引入**（`rust/Cargo.lock` 零变化 = `90111c4b…` R02_HANDOFF 原值）；生产默认入口不变（Node/Electron，R02-A16 定向回归 E1 家族全绿）
- 派单：docs/rust-tauri/R03/dispatches/R03-T08_DISPATCH_E01.md（总控细化 7 项全部执行，见 §3–§9）

---

## 1. 交付物总览

| 交付物 | 位置 |
|---|---|
| 阶段图 R03.json（48 叶镜像自 R00 双账本） | `rust/crates/xtask/src/stage_maps/R03.json`（可复现生成器 `scripts/rust-tauri/r03_t08_generate_stage_map.py`） |
| STAGE_MAPS + runner_identity 注册 | `rust/crates/xtask/src/main.rs` / `runner_identity.rs`（R03.json 入 EMBEDDED+SOURCE_INVENTORY，stale binary 检测覆盖） |
| xtask 阶段中立 kinds | `stage_map.rs`（`stage_share_satisfied`/`deferred_to_later_stage`，语义与 R02 的 r02_share_satisfied/deferred_to_r07 同构，R02 图零改动）+ `verify.rs` roll-up/计数/invariant |
| 组合矩阵+叶案例集成测试 | `rust/crates/lingxi-service/tests/r03_t08_acceptance_matrix.rs`（17 测试；A15 组合矩阵 + 补充叶案例生产者） |
| 矩阵证据脚本 | `scripts/rust-tauri/r03_t08_matrix.sh` |
| A16 seed 机制（永久）+ 变异探针 | `lingxi-adapters/tests/{late_result_fencing_property,run_finalize_property}.rs`（seed 覆写+失败捕获）+ `scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh` |
| T04 R1-F1 收口 | `docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json`（V2/V3/V4 登记）+ `scripts/rust-tauri/r02_t04_storage_tx.sh` S4 按注册表判定 |
| R02 回归链（SUP-02） | R03 图 7 条定向命令 + 全量 `verify-stage R02` 全新证据目录（§8） |
| 阶段交付三件 | `R03_REPORT.md` / `R03_HANDOFF.json` / `R03_ACCEPTANCE_LEDGER.json` |
| 证据根 | `artifacts/rust-tauri/R03/T08-E01/` |

## 2. 验收结论（先行）

| A-ID | 判定 | 一句话依据 |
|---|---|---|
| R03-A15 真实入口驱动状态 | **PASS** | 组合矩阵 12 场景经真实 `SessionStore` admission→`SessionSupervisor`→`RunSupervisor`→kernel 状态机/栅栏→真实 `RunDatabase`/`EventService` 全链产生全部状态/事件/终态；替身（ScriptedProvider/CountingTool/ManualGate）只产出 `ProviderTurn`/`ToolOutcome`/审批决策三类外部响应，不写任何状态（测试文件头边界声明+代码事实）；证据=28 案例 `lingxi.leaf-case-results.v1`（actual==图钉，进程内断言）+ `integration.log` 调用观测（模型/工具/终态逐组合计数）+ 订阅者观测 |
| R03-A16 故障种子可重放 | **PASS（机制证明，如实标注）** | 两个既有属性测试在默认 seed 下从未出现自然异常（P0 基线绿）；按阶段书允许路径以**隔离受控缺陷注入**证明「异常序列→捕获 seed→固定 seed 稳定复现→移除变异后同 seed+全组通过」全机制：捕获 seed=`0xa16d0000000000`（失败行与 seed 文件双记录、两次重放一致、移除后同 seed 绿+全组绿）；变异在 mktemp 临时 cargo crate 中（仓库树零触碰、P4 前删除）；seed 覆写（`R03_T04_PROPERTY_SEED`/`R03_A02_PROPERTY_SEED`）与失败捕获（`*_PROPERTY_FAILURE seed=0x…` 行 + `*_SEED_FILE`）为**永久机制**入长期回归 |

## 3. 总控细化 1：阶段图注册与正式 Gate

- **R03.json**（以 R02.json 为 schema 模板）：`commands` 14 条（fmt/clippy/全量 test/check-contracts/check-boundaries/a15 矩阵/a16 机制/7 条 R02 定向回归）+ `scenarios` R03-A01..A16 全 REQUIRED（每条绑定真实命令）+ `supplementalLeafScenarios` **48 项**（R00 账本绑定 R03 的全量叶，逐叶全字段镜像 ACCEPTANCE_MAP+FEATURE_STAGE_ACCEPTANCE 双账本——`r03_map_mirrors_match_the_real_r00_ledgers` 单测直接对真账本核对，verify 期 cross_check 再核一遍）。
- **分类**：17 `stage_share_satisfied`（R03 份额=运行/取消/审批等待/子代理/后台/重连/事件转发基底；每叶必带 assertionContract，案例由 `a15_combo_and_leaves` 生产者机器核验 actual==图钉）+ 31 `deferred_to_later_stage`（无 R03 叶专属门禁份额；仍 REQUIRED，验收归 R06/R07/R08；不得绑门禁命令/证据/契约——解析期硬校验）。4 项补充义务（SUP-01..04）落在 `supplementalCoverageNote` + 命令集 + 递延登记：SUP-01=本图+注册+Gate 本身；SUP-02=7 条 R02 定向命令入图；SUP-03=运行层取消语义经基础场景+组合矩阵（终端客户端不提前）；SUP-04=R02.json 9 项 deferred_to_r07 原样未动，本图 48 叶相等性检查成立。
- **注册**：`STAGE_MAPS` 增 `("R03", include_str!(...))`；`runner_identity.rs` EMBEDDED+SOURCE_INVENTORY 同步钉住 R03.json（旧 binary 对新盘上图的 stale 拒绝覆盖新图——现有单测模式自动覆盖）。
- **正式 Gate**：`cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R03 --evidence artifacts/rust-tauri/R03/T08-E01/verify-stage-r03` → **exit 0，overall PASS**：14/14 命令 exit 0、16/16 场景 PASS、48 叶 = 17 pass + 31 deferredToLaterStage + 0 fail + 0 blocked、candidateSourceBinding.stable=true（gate 期间树零变化）、runnerSourceBinding=PASS、testedSha=dc42a01e3（dirty=true 属预期：授权未提交候选）。
- **负向探针（对新图同样硬失败）**：
  - 实跑：`verify-stage R99` → exit 2「unknown stage…registered stages: R02, R03」；`verify-stage R03 --evidence <非空目录>` → exit 1「evidence root … is not empty」。（`negative-probes/` 日志+exit-codes.csv）
  - 单测层（95/95 绿的新增部分）：真实 R03.json 的空场景集/未知 basisKind/递延叶绑门禁命令/缺 stageShare/单阶段叶递延/镜像漂移→cross_check 拒绝，以及 stage_share 叶图钉不成立→FAIL、deferred_to_later 固定输出+deferredToStages 派生、PASS∪DEFERRED 覆盖使 overall PASS 可达。
  - **Attempt-1 如实记录**：首跑 overall FAIL（`verify-stage-r03-attempt1/`，含 FAILURE-ANALYSIS.txt）——a16 命令当时以 `{EVIDENCE}` 为参致脚本 freshness 守卫拒跑（已改为 `{EVIDENCE}/A16`）；r02_legacy_regression 的 E5 全量 npm+封印族分类对未提交候选树分类失败（见 §8 范围拆分）。修复后 attempt-2 全绿；attempt-1 原始字节保留。

## 4. 总控细化 2：强制修复递延项（T04 R1-F1 + T05/T06 登记）

- **指纹独立重算**：以当前树构建 `lingxi-service` 于临时 home 启动（真实迁移），`lingxi-storage-inspect migrations` 读盘：V1=`479b0321…`（与已发布值逐字节一致，不动）、V2(stale_result_audit)=`64d7edfdab74e13e623a9d8d5f2381dad4803f545fa126d8eda8cc02c3dc889c`、V3(invocation_journal)=`3bd5388f090ab0ae0e137d91d59a8e48f82f409bbd0de657abf29b897a4252c1`、V4(run_lineage)=`371d415462b7e8d698693f50a7b9c9f493fbb4e653ee30e5d3e8930f77e161b5`；receipts==compiledIn、userVersion=4。三值与 T04/T05/T06 报告短前缀一致。
- **登记**：`R02-T04_STORAGE_REGISTRY.json` migrations 数组补 V2/V3/V4（含 registered_by_task 与登记说明；V1 原条目零改动），tables 面补三表所有权映射，generated_by 追加 T08 登记说明。
- **S4 修复**：`r02_t04_storage_tx.sh` S4 由硬编码 `version==1`/单条 receipt 改为**按注册表指纹集判定**：读注册表 → userVersion==supportedVersion==注册表条数 → receipts/compiledIn 与注册表逐条 version/name/fingerprint 全等。任何一边漂移均响亮失败（不静默放宽）。
- **重跑**：脚本全程 S1–S4 全绿（独立探针 `/tmp` + gate 内 `r02_storage_tx` 命令各一次）；S4 dump 对账 3 轮稳定、inspector 只读性维持。

## 5. 总控细化 3：R02 基础链回归（阶段收口）

R03 图内 7 条定向命令（本次 gate 全部 exit 0，证据在 gate 证据根 `R02/` 子树）+ 全量 `verify-stage R02`（§8）：认证与隔离（a05_a06_auth_matrix）、单写者与事务（r02_storage_tx，S4 修复后）、事件快照与续读（a09_a10_events_matrix）、损坏库与备份（a11_backup_restore）、关闭与恢复（a12_recovery_drill）、A15 真实重启链（a15_full_chain）、A16 默认入口与旧栈回归（a16_legacy_regression，定向模式见 §8）。npm 侧按派单拆分：E1–E4.5（入口/绑定/typecheck/contracts/boundary/renderer build）在本 Task 门禁内全绿；**完整 npm 与审计封印状态由总控另记，本 Task 不伪造**（attempt-1 全量 E5 输出已存档供总控账本）。

## 6. 总控细化 6：组合矩阵（A15）

`r03_t08_acceptance_matrix.rs` 17 测试全绿；矩阵组合 ≥ 阶段书清单，全部经真实 service 入口（`SessionStore::execute_for`/`execute_submission_for`/`execute_background_for`/`steer_for`/`cancel_run_for`/`EventService::subscribe`），替身只给外部响应：

| 组合 | 任务/调用/终态计数（combo-counts.json 摘要） |
|---|---|
| 正常 | 1 run、1 model call、0 tool、终态 completed.with_final（含最终消息）；订阅者收到终态事件 |
| 多轮模型 | 1 run、**3 model calls**、1 tool、**唯一终态**（中间模型结束≠任务结束） |
| 多工具 | 1 run、2 model calls、**3 tool calls**（started==completed==3）、completed |
| 超时（turn 预算） | 1 run、3 model calls、`failed.turn_budget_exceeded` 响亮失败、无最终消息（预算不静默完成） |
| 取消-流读取 | 取消→四相取消落 cancelled；门控释放的迟到 turn **0 条 model_call_completed、无最终消息**（内容不落流；audit 行为端口级栅栏契约，由 late_result_fence* 覆盖，此处记录观测值） |
| 取消-审批等待 | durable `waiting_approval` 腿（轮询权威 run 行）→ 取消 → cancelled、**0 工具执行** |
| 重复 | 同 requestId+同内容 → replayed=true 零重执行（runs 仍 1）；同 id+改内容 → `DuplicateRequestConflict`（不复用不新增） |
| 乱序 | 取消后迟到投递：流零污染（0 completed/无 final）；`late_result_audit` 行数如实记录 |
| 跨会话 | alpha/beta 并行 join，双 completed，各 1 run |
| 崩溃恢复 | run 停在模型调用中→驱动 future 丢失（进程丢失边界；SIGKILL 形态由 recovery_crash_points.rs 11 崩溃点覆盖）→ 全新生产形态 bootstrap 真实启动扫描 → `interrupted_needs_attention`、**0 最终消息**、扫描报告 scanned≥1 |

叶案例（28 项，`leaf-cases.json`）：steer 三态（busy Accepted/idle Miss/drain 进下一模型 input）、cancel 三形（live→Accepted+cancelled/terminal→AlreadyTerminal/unknown→NotFound）、审批等待取消零执行、子代理族（dispatch lineage 四元/Reply 续线程/Close closed 态/child 定点取消不连坐/父取消停 child）、escalation 只读父 write 档→**0 child run**、proactive_delegation 默认关、后台解耦完成+不相关取消下存活、重连只订阅（provider 调用数不变、runs 不变、resume cut 补发终态）。

**如实发现（R03-T08-FINDING-1，MINOR，已入 HANDOFF known_gaps）**：父取消路径的 subagent child run durable 行不就地收口——`spawn_linked` 包装器 biased select 在树取消时先于 child drive 自身取消结算丢弃 drive future，行保持 active，由**下一进程启动扫描**诚实收口为 interrupted_needs_attention（矩阵实测+重启闭环证明）；线程登记 busy 标志同样只在 drive 完成尾清除。child 自身超时路径（timeout_at+drive.await）正常落 cancelled。T06 报告「child 由自身 drive 观测并经唯一 finalize 落 cancelled」在父取消路径不成立。监督层 A06 判定本身成立（cancelled parent + live_children_of 空 + 无关后台存活）。修复归属 R04/R06 触碰该路径时或总控另派（本 Task 不越权改产线）。

## 7. 总控细化 5：A16 种子机制（机制证明，如实标注）

- **永久机制**：`late_result_fencing_property.rs`（T04 fence，默认 seed `0x5eed0000a07df00d`）与 `run_finalize_property.rs`（T01 finalize，默认 seed `0x5eed0000c0ffee01`）新增：`R03_T04/A02_PROPERTY_SEED` 覆写（hex/decimal）；thread-local 感知 panic hook 打印 `*_PROPERTY_FAILURE seed=0x… location=…`（stdout+stderr）并按 `*_SEED_FILE` 落盘——截断的 panic 消息不再丢失失败调度。
- **机制证明**（`r03_t08_a16_seed_mechanism.sh`，gate 内 a16_seed_mechanism 命令）：P0 真实测试默认 seed 绿（基线，无自然异常——如实标注为机制证明而非自然发现）；P1 隔离变异（mktemp 临时 cargo crate 复制真实属性测试+注入受控缺陷：seeded 稀疏子集在快照后经独立 SQLite 连接把终态行改回 queued——正是「终态永不翻转」探测器必须抓的非法 durable 序列；仓库树零触碰）；P2 扫描 seed 至失败并捕获（失败行+seed 文件双记录一致；捕获 seed=`0xa16d0000000000`，扫描 0 号命中）；P3 同 seed 两次重放失败形态一致（稳定复现）；P4 删除变异后同 seed 真实测试绿+双属性测试全组绿。各阶段原始日志+seed 文件在 `A16/`。
- 探测器形态如实记录：变异触及哪个断言取决于 seed 派生的投递序（本轮为 finalize 契约 InvalidRequest 形态；此前的试跑观察到 terminal-flip 形态——均为真实检测路径，脚本对两种形态均收）。

## 8. R02 全量回归（verify-stage R02，全新证据目录）

- 命令：`cargo run --locked -p xtask -- verify-stage R02 --evidence artifacts/rust-tauri/R03/T08-E01/verify-stage-r02-regression`（在 R03 gate 完成后串行执行，避免候选快照互扰）。**实际结果：overall FAIL（17/20 命令 exit 0；34 叶=25 pass+9 deferred+0 fail+0 blocked；candidate stable；runner PASS）**，3 条命令红，逐条归因（不改写任何断言、不伪造绿）：
  1. `a16_legacy_regression` exit 1——E5 全量 npm+封印族分类对未提交候选树闭失败（attempt-1 同因，§3/本节下文）：属审计封印/完整 npm 状态，派单明示归总控另记。R02 图该命令语义零改动（full 模式默认）。
  2. `a07_live_fault_02_check` exit 1——`restart.keyEvents mismatch`：R02 期检查器期望重启后 keyEvents=1；R03-T07 启动恢复扫描（A13 设计行为）现会把崩溃时的 active run 诚实 finalize（interrupted_needs_attention 终态+事件），事件数随之增加。**R03-A07 语义本身未回退**（live_fault_01_test 测试命令 PASS；同事务终态/无假成功断言维持）——R02 期精确计数期望相对 R03 行为层过期（**R03-T08-FINDING-2**，R02 资产更新归独立审阅授权，本 Task 不改已验收阶段门禁断言）。
  3. `a14_slow_subscriber` exit 1——1000 并发（24 路）同会话 execute 风暴中出现 1 个 409：R03-T02 冻结语义（busy 会话 409 session_busy retryable，= 现役 Node 栈同语义）下并发同会话提交的合法拒绝；R02 期「全部 200」断言先于运行层 busy 闸存在（**R03-T08-FINDING-3**，同上归属）。无 stall/无假成功/订阅者健康断言不受影响（红在 200-only 单条）。
- 判定：派单的 R02 回归义务由 **7 条定向命令（R03 gate 内全绿）+ r02_t04 全程（修复后）** 满足（「或等效定向命令重跑」路径）；全量 R02 gate 作为额外取证如实记录 FAIL 与归因，其 3 红均已解释且不涉及 R03 验收场景回退。完整 npm/封印结论归总控。
- **legacy 回归范围拆分（如实）**：`r02_t08_legacy_entry_regression.sh` 新增可选 `R02_LEGACY_REGRESSION_MODE`（默认 `full` = R02 图原语义零改动；`directed-no-seal-family` = E0–E4.5 后响亮 SKIP 并记录；未知值 exit 1）。R03 图以 directed 模式注册（E0–E4.5 全绿：默认入口/绑定面/typecheck x3/core-contracts/boundary gates/renderer build）；E5 全量 npm+封印族分类归总控账本——attempt-1 对当前未提交候选树的全量 E5 输出（3 红均封印族：seal-coordinate-lag/uncommitted-source-rejection 混合，round3 块含 UNRECOGNIZED 混合形态被分类器按设计闭失败）已原样存档于 attempt-1 证据根，供总控另记；本 Task 不伪造其结论。

## 9. 环境红线核验

tokio test-util 未引入（Cargo.lock 零变化）；npm/桌面栈仅 R02 注册命令按惯例触碰（定向模式）；生产默认入口不变（E1 家族断言全绿）；不 commit/push、不改 DONE、不外发；替身边界=外部响应 only（测试文件头声明+实现事实：替身无存储/状态写入路径）。

## 10. 验证命令（全部实跑退出码）

| 命令 | 退出码 | 备注 |
|---|---|---|
| `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0 | gate 内 rust_fmt + 独立实跑 |
| `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 0 | gate 内 rust_clippy |
| `cargo test --manifest-path rust/Cargo.toml --workspace --locked` | 0 | gate 内 rust_test_workspace（63 个 test target 全 ok，0 failed；含新增 17 矩阵测试与 95 xtask 测试） |
| `cargo run --locked -p xtask -- check-contracts` | 0 | API_COMPAT_MATRIX 626 entries 零漂移 |
| `cargo run --locked -p xtask -- check-boundaries` | 0 | O1–O8+D1–D5 |
| `cargo run --locked -p xtask -- verify-stage R03 --evidence artifacts/rust-tauri/R03/T08-E01/verify-stage-r03` | **0（overall PASS）** | §3 |
| R02 回归链（verify-stage R02 全新证据目录） | §8 回填 | 7 条定向命令已在 R03 gate 内绿 |

## 11. 边界与未验证项（如实）

- 崩溃恢复组合用「驱动 future 丢失+真实启动扫描」形态（T05 同先例）；进程级 SIGKILL 形态由 T07 recovery_crash_points.rs 11 崩溃点覆盖（本次全量 test 一并绿）。
- FINDING-1（§6）为如实登记的实现差距，未在本 Task 修复（超派单范围）。
- 完整 npm 与审计封印状态、阶段封印、commit/push 归总控；本报告不预写。
- A16 为机制证明（无自然异常历史，不编造发现史）；属性测试的 seed 扫描证明「随机属性测试发现异常」的通道，不声称产品缺陷。
