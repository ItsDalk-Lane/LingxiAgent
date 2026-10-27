# G03 独立复审报告 — R04-RR1-F03（清理未确认被转换为已退出和虚构的 137 退出码）

- Reviewer：**REVIEWER-R04-RR1-G03-R01**（全新代理，未参与 G03 执行或修复，与执行代理无共享上下文）。
- 审查对象：基线 `da15c4bd9`（分支 `codex/rust-tauri-migration`，含 G01/G02 修复）→ 候选 = 当前工作区未提交改动：
  `rust/crates/lingxi-kernel/src/ports.rs`（ToolRunStatus 新增 StopUnconfirmed、ToolSuccess.status 装箱）、
  `rust/crates/lingxi-service/src/procsupervisor.rs`（ExitFact::Unobserved/terminal_facts/finalize_exit/ReapFaultPoint）、
  `rust/crates/lingxi-service/src/exectools.rs`（超时与结果映射）、`rust/crates/lingxi-service/src/runs.rs`（journal StopUnconfirmed 臂）、
  4 个既有测试文件 API 适配、新测试 `rust/crates/lingxi-service/tests/r04_rr1_f03_stop_honesty.rs`。
  工作目录 `/Users/study_superior/Desktop/Code/LingxiAgent`。
- 工具链：`~/.cargo/bin/cargo 1.98.1`（rustc 1.98.1），全部命令 `--manifest-path rust/Cargo.toml --locked`；`rust/Cargo.lock` 零变化（diff 亲核，候选未触碰）。
- 纪律：主树产品/测试源码零修改（`git status` 复核：候选 8 改 1 增原样）；复测输出只写
  `artifacts/rust-tauri/R04/RR1-G03-R1/`（logs/ + 两份 reviewer 自有测试源码）；基线复跑在 `/tmp/r04-g03-baseline`、
  候选对照在 `/tmp/r04-g03-candidate` 两个独立 git worktree（**已用后清理**；我自己一次红跑 panic 路径遗留的孤儿
  `sleep 300`（pid 62495，本测试创建）已按精确 pid 处置并复核无残留）。执行者报告 `G03-E01_REPORT.md` 只作线索，未转抄其任何日志。
- 必读输入已读：总控 §R04-RR1-F03/§5/§6/§7/§8、验收清单 issues[2]、ORCHESTRATOR_PROMPT §4.4 与 §七 R04-T08 第 1 条、
  R03_HANDOFF.json 的 ToolOutcome/Unknown 语义与 journal_contract、R03_RR2_STAGE_REVIEW.md。

```text
VERDICT: PASS
```

## 1. 独立复测：实际命令与退出码

全部为本人真实运行；证据存 `artifacts/rust-tauri/R04/RR1-G03-R1/logs/`（括号内文件名）。cargo test 失败=101、成功=0。

| # | 命令 | 结果 | 证据 |
|---|---|---|---|
| 1 | 主树（候选）：`cargo test -p lingxi-service --test r04_rr1_f03_stop_honesty`（全套） | **ok. 7 passed; 0 failed**（2.13s） | `01_candidate_f03_suite.txt` |
| 2 | 基线 worktree（da15c4bd9）：`cargo test -p lingxi-service --test rr1_g03_r01_redgreen_reviewer`（**我自写的基线可编译反例**，3 用例） | **FAILED（exit 101）**：R1+R2 红（R3 该轮未开窗）；4 连跑 R1 4/4 红、R2 4/4 红、R3 2/4 见证红 | `02_red_reviewer_own_baseline.txt` + 终端 4 连跑记录 |
| 3 | 候选对照 worktree（基线 + 复制的候选 9 文件，与主树逐字节 cmp 一致）：同一反例 ×6 | **6/6 全绿（3 passed; 0 failed ×6）** | `03_green_reviewer_own_candidate.txt` |
| 4 | 主树：C01–C05 七用例逐个 `-- --exact` 独立运行 | 各 **ok. 1 passed**（2.05/2.02/2.05/1.04/0.05/2.09/2.03s） | `04_per_cid_exact.txt` |
| 5 | 候选对照树：**我的 6 个对抗变体**（V1–V6，参数与执行者不同） | **ok. 6 passed; 0 failed**（×2 连跑 + 首跑共 3 次） | `05_reviewer_variants_candidate.txt`、源码 `../reviewer_variants_source.rs` |
| 6 | 主树：R03 回归 10 套件（cancel_terminal_race/cancellation_tree/cancel_link_inheritance/late_result_fence/admission_dedup_consistency/admission_dedup_adversarial/request_id_canonicalization/request_dedup/tool_receipt_unknown/r03_t08_acceptance_matrix） | 13/8/7/5/5/5/7/4/6/17 全 **ok，0 failed** | `06_r03_regression_suites.txt` |
| 7 | 主树：G01/G02/R04 套件（registry_capacity/reaper_cleanup/process_tools/t08_tool_matrix/t02/t01/invocation_journal/t03/t04/t06/t07） | 9/11/14/10/13/6/3/11/10/10/19 全 **ok，0 failed**（t08 300.7s 全矩阵） | `06_r03_regression_suites.txt`（续） |
| 8 | 主树：`cargo test -p lingxi-service --lib`；`cargo test -p lingxi-kernel` | **297 passed; 0 failed**；**77 passed; 0 failed** | `07_lib_tests.txt` |
| 9 | 主树：`cargo clippy --workspace --all-targets --locked -- -D warnings` | **exit 0**（无告警） | `08_clippy.txt` |
| 10 | 主树：`cargo fmt --all -- --check` | **exit 0** | `09_fmt.txt` |
| 11 | 主树：`cargo test --workspace --locked` | **exit 0；85 个 ok 结果行、0 FAILED、合计 925 passed** | `10_workspace_full.txt` |
| 12 | 主树：`cargo run -p xtask -- check-contracts`；`-- check-boundaries` | 均 **exit 0**（626 条目 drift-free；ownership+battery OK） | `11_check_contracts.txt`、`12_check_boundaries.txt` |

（`verify-stage R04` 按 §七/总控分工属 G05 门禁阶段，本单最低集合未含，未运行；`check-contracts`/`check-boundaries` 为本人主动加跑。）

## 2. 旧红新绿（独立、自己写的反例，非执行者形态）

我的反例文件 `rr1_g03_r01_redgreen_reviewer.rs`（源码存档 `../reviewer_redgreen_source.rs`）只用**两树共有 API**
（唯一 API 差异 `ToolSuccess.status` 装箱用本地扩展 trait 兼容，断言本体两侧逐字相同）：

- **R1（exec 面，确定性红）**：G02 已验证的 reaped-but-undrained 窗口（孙进程持管道写端，直接子 0.5s 自然退出，
  grace 1500ms），timeout 1s 落窗内，cleanup 1ms。基线红原文：
  `R1 VIOLATED: the tool result claims Exited(137) while the never-signalled grandchild pid … is alive — the exit was never observed`，
  且基线文本只有 timeout 提示、**不含任何退出陈述**——文本与结构化自相矛盾（结构化凭空 Exited(137)，从未发信号、从未观察）。
  同一测试在候选 6/6 绿（StopUnconfirmed + 孙进程存活 + 句柄可查）。
- **R2（terminal_facts 虚构升级，确定性红）**：同窗口，T1（Timeout，cleanup 300ms）与延后 50ms 的 T2（Close）
  并发终结；T1 的 `cleanup_timed_out` 广播唤醒仍停在相位观察上的 T2。基线红原文：
  `R2 VIOLATED (t2): the receipt claims a confirmed kill Terminated{Signal(9)} without an observed exit (record phase CleanupTimedOut { reason: Timeout }, exit_observed in audit: false)`。
  候选绿：T2 得到诚实的 `termination_unconfirmed_echo` → `CleanupTimedOut`，不 settle、不确认。
- **R3（write_stdin 文本/结构化一致性，见证红）**：PTY + 真实 killpg + nanos 清理预算 + 紧相位探针轮询。基线红原文
  （4 连跑中 2 次捕获）：`R3 VIOLATED: the poll text says cleanup_timed_out (exit NOT observed) yet the structured status claims Exited(137). … status: cleanup_timed_out (close; kill was sent)`。
  该窗口在 macOS 上本质不确定（本人独立验证两次：孙进程持 slave 也不能开窗——会话首领退出即向 master 交付挂断；
  close-kill 观察在首测轮询前导内落地），故红为竞态门控（~50% 命中），候选侧同构造永绿（断言只在文本为
  cleanup_timed_out 的轮询上生效，StopUnconfirmed 永不触发）。
- 红绿两侧命令一致（同 `--locked`），退出码与关键输出存证于 §1 #2/#3。

结论：**F03 核心缺陷族在基线由我自己的测试在三个面上真实复现（exec 结果映射 / 终结者回执升级 / write_stdin 文本-结构化背离），
候选全部关闭，旧红新绿成立。**

## 3. 四条虚构分支逐一核销（代码级，基线行号 → 候选形态）

| # | 基线位置（da15c4bd9） | 基线行为 | 候选核销（亲读 diff + 全树 grep） |
|---|---|---|---|
| F1 | exectools.rs:737 | 超时臂 `terminate` 回执非 Terminated 时兜底 `ExitFact::Signal(9)` 并本地构造 Terminated → `Exited{137}`；`AlreadyTerminal` 的真实 fact 也被丢弃 | 超时臂按回执诚实穷举映射：`Terminated{fact}`→真实 fact；`AlreadyTerminal(phase)`→回显真实相位（真实退出竞速 deadline 时报告真实码）；`CleanupTimedOut`→`RecordPhase::CleanupTimedOut`；`UnknownProcess`→响亮 Internal。全树 grep 无 `Signal(9)` 兜底（仅注释/合法观察断言） |
| F2 | exectools.rs:763-764 | 结果映射 `CleanupTimedOut => ExitFact::Signal(9)`、`_ => ExitFact::Code(-1)` | 映射改为：有 observed fact → `observed_status_code()`；否则按相位给 unconfirmed detail → `StopUnconfirmed`；Running/Terminating 到达终态要求点 → 响亮 Internal（绝不发明码）。`status_code()` 已不存在，改名 `observed_status_code() -> Option<i64>`，类型层面杜绝 -1/137 |
| F3 | exectools.rs:967 | `run_write_stdin`：`CleanupTimedOut => Some(ToolRunStatus::Exited { code: 137 })`，文本行却写 "kill was sent" | 改为 `StopUnconfirmed { handle, detail }`；文本行改为 `cleanup_timed_out (…termination was requested, exit not observed — unconfirmed, not exited)`——文本与结构化同源于同一次相位读取且语义一致；`Exited/Terminated{Unobserved}` → StopUnconfirmed（wait 失败也不报退出） |
| F4 | procsupervisor.rs:2391-2392（terminal_facts）、1433/1669（两个 reaper `Err(_) => ExitFact::Code(-1)`）、2405（`exit_fact_of` 无 code/signal 兜底 Code(-1)） | terminal-without-fact/非终态一律虚构 Signal(9)/Code(-1)；wait 失败编造 -1 | `terminal_facts -> (Option<ExitFact>, …)`：CleanupTimedOut/Running/Terminating → `None`；`await_termination` Some 臂分流——有真实 fact 才 Terminated+settle，否则审计 `termination_unconfirmed_echo` 并返回 CleanupTimedOut（**并发/重复终结者不再自动升级**）；wait Err → 审计 `wait_failed` + `ExitFact::Unobserved`（`observed_status_code()==None`）；`exit_fact_of` 兜底 → Unobserved。全树 grep：`Code(-1)` 0 处、字面 `137`/`Signal(9)` 仅存于注释与合法观察断言 |

**同族第五点（执行者自报 F5，本人独立核验）**：`finalize_exit` 的 reason 匹配从仅 `Terminating` 扩展到
`Terminating | CleanupTimedOut`——迟到真实观察落在未确认记录上时产出 `Terminated{首因, 真实 fact}`，
首因不再丢失。我的 V3 变体（真实退出恰在 deadline 与第二调用之间到达）验证第二调用回显 `Terminated{Timeout, Code(0)}`。

## 4. 合法对照不降级（要求 4，关键）

- 执行者 C04 四组（exit 0 / exit 7 / 真实观察 SIGKILL / wait 错误）逐 `--exact` 独立运行全绿（§1 #4）；其映射链
  亲读核验：`Exited{Code(0)}`→`Exited{0}`、`Code(7)`→`Exited{7}`、**真实观察的** `Signal(9)`→`Exited{137}`（audit
  含 `exit_observed`，137 是合法 128+9），wait 错误→`StopUnconfirmed`+`Unobserved`（观察失败，不编 -1/137）。
- **我自己的控制组 V4**（参数独立）：exit 0 → `Exited{0}`；exit 7 → `Exited{7}` 且文本含 "Command exited with code 7"、
  无 "[stop unconfirmed]"；观察 SIGKILL → `Exited{137}`、audit 有 `exit_observed`、文本无 unconfirmed 行。**没有任何非零
  结果被一律改成 Unknown/StopUnconfirmed**；`observed_code.filter(|c| *c != 0)` 只吞 0 码不吞非零。
- C04 对抗（取消×自然退出竞争 10 轮）绿：每个 Terminated 回执都有 `exit_observed` 背书，AlreadyTerminal 回显真实 Code(0)。
- R03 Unknown 语义未动：`ToolOutcome::Unknown{reason}`、取消→journal Started 无回执（Unknown 窗口）、禁盲重试链
  均不在本 diff 内且 R03 十套件全绿（§1 #6）。

## 5. C01–C05 逐项独立结论

| C-ID | 独立证据（执行者用例逐个实跑 + 我的变体） | 结论 |
|---|---|---|
| C01 单命令清理迟延 | 逐 exact 绿 + **V1**（grace 2500ms/child 0.8s/timeout 2s/cleanup 2ms 另一几何）：StopUnconfirmed 引用原句柄、文本含 unconfirmed 无退出陈述、孙进程存活、slot 保持（RegistryFull）、迟到真实退出 `Terminated{Timeout, Code(0)}` 带 `exit_observed`、slot 恰好一次归还 | 满足 |
| C02 PTY 清理迟延 | 逐 exact 绿 + **V2**（cleanup 100ms/grace 400ms；3 次空 chars + 1 次带 chars 轮询 + 外来会话）：空轮询 StopUnconfirmed 且文本含 `cleanup_timed_out`/`unconfirmed`/原句柄；带 chars 写入在未确认句柄上以 `Failed{Conflict}` 拒绝并指名真实相位（诚实，非编造退出）；外来会话 Forbidden；`cleanup_timed_out` 审计恰好 1 条、无 `exit_observed` | 满足 |
| C03 重复终止与迟到退出 | 逐 exact 绿 + **V3**（真实退出恰在 deadline 与第二调用间到达）：确认前重复 terminate 得 `AlreadyTerminal(CleanupTimedOut)` 回显不升级；确认后回显 `Terminated{Timeout(首因), Code(0)(真实 fact)}`；live_cap=1 名额未确认期间保持、确认后恰好一次归还（G01 settle 语义无恙） | 满足 |
| C04 控制组 | §4 全部（执行者四组逐 exact + 我 V4 + 竞争对抗） | 满足 |
| C05 上层取消不虚报静止 | 逐 exact 绿（两用例）+ **V5**：Run 取消后 journal `Started` 无回执（R03 Unknown 窗口）；进程侧 killpg 真发、`CleanupTimedOut`、两事实同时间线分离；**我的新增检查——迟到真实观察落地后重载 journal，仍 Started 无回据，未被改判成功**。R03 Cancelled/Unknown 语义十套件绿 | 满足 |

## 6. 消费者完整性、注入边界与 journal 一致性

- **ToolRunStatus 全部消费点**（全仓 grep）：`exectools.rs`（run_exec_command 结果映射、run_write_stdin 映射、
  tty 启动 Running——三个 match 全穷举）、`runs.rs:2542`（journal detail，三变体+None 穷举，新臂
  `"stop unconfirmed, handle …: …"`，**不读作 exit**）。`ToolResultWire`（runs.rs:2593）不携带 run status，wire 面零影响；
  toolgateway 只审计 resource_refs；filetools/mcpbridge/workerrpc 构造非进程工具的 `status: None`。枚举无 serde 派生，
  无协议序列化面需要新变体。clippy `-D warnings` + 全 workspace 编译证明无漏 match。
- **ReapFaultPoint 注入边界**：全仓 grep `ReapFaultPoint|arm_reap_fault_for_verification` 仅
  procsupervisor.rs（定义/消费）与 r04_rr1_f03_stop_honesty.rs（测试武装）两处，**零产品调用点**；仅选择分支触发时机
  （WaitErrorNext 走真实 Err 分支；DelayObservationNext 在 wait 已解析、reap 事实已发布后扣持终态记录 2s），
  与 G01/G02 的 SpawnFaultPoint/PumpFaultPoint 同一先例；持有 supervisor 的进程内 Rust 代码才能武装，模型/工具输入不可达。
- **journal 一致性**：`CancelPhase::StopUnconfirmed`（runs.rs:2286）为 R03 既有取消链相位（基线同款，本 diff 未触碰），
  与 kernel `ToolRunStatus::StopUnconfirmed` 是不同结构、不同账本（取消时间线 vs 工具回执），不混淆。
  **V6（成功方向）**：完成的工具调用收据 = `Succeeded` + detail `stop unconfirmed, handle …: …`（工具调用本身成功返回，
  进程终态未确认——两事实分层如实）；迟到真实观察落地后重载 journal，**收据 detail 逐字不变，未被改写为 exit**。
  与 Unknown（Started 无回据）/Cancelled（wire Cancelled）三种形态互不复活跃。
  迟到结果把 run 改判成功：V5（取消方向）与 V6（成功方向）双反例均证伪。

## 7. G01/G02/R03 无同族回归

- G01（容量）：registry_capacity 9/9；我的 V1/V3 slot 保持/恰好一次归还语义直接复测。
- G02（泵归属/重发门/回收）：reaper_cleanup 11/11、process_tools 14/14；R2 变体路径即 G02 的
  `group_signal_skipped` 门（child_reaped 后不发信号）——候选下孙进程存活与信号日志为空再次亲证。
- R03：十套件 + tool_receipt_unknown（Unknown 回据 6/6）+ r03_t08_acceptance_matrix 17/17。
- 全 workspace 925/0、clippy 0、fmt 0、check-contracts/check-boundaries 0。

## 8. 问题清单（全部非阻塞观察项，无 P1/P2）

| ID | 严重度 | 定位 | 内容 | 评估 |
|---|---|---|---|---|
| O-1 | 观察 | exectools.rs `timed_out = timeout_fired` | 超时提示语现在绑定看门狗触发事实而非终态成因：真实退出恰与 deadline 竞速时，文本可能同时含 "Command timed out after N seconds" 与 "Command exited with code 0" | 两个陈述各自为真（看门狗确实触发；退出码是真实观察），非虚构；F03 范围内可接受。后续可在 AlreadyTerminal 竞速臂考虑措辞去歧义 |
| O-2 | 观察 | kernel ports.rs `ToolRunStatus::StopUnconfirmed` 与 service `CancelPhase::StopUnconfirmed` | 同名不同层概念（工具运行状态 vs 取消清理相位），均有诚实语义 | 无行为混淆（不同结构/账本，V5/V6 证）；仅命名相似性，供后续阅读注意 |
| O-3 | 观察 | procsupervisor.rs `hold_observation_for_verification` | 注入扣持固定 2s 使依赖它的测试（C02/C05）单用例下限 ~2s | 仅测试时长，无正确性影响 |
| O-4 | 观察 | runs.rs journal receipt | `Success{StopUnconfirmed}` 回据 outcome 为 `Succeeded`（detail 指明 unconfirmed） | 工具调用确实成功返回（带部分输出与可查句柄），进程终态另列 detail——分层如实；若后续要求把 StopUnconfirmed 收据单列 ReceiptOutcome 档，属新契约演进而非本单缺陷 |

无阻塞问题；无未修复同族回归；未发现「过滤 0/删旧测试/降门禁/pin 降低」形态——4 个既有测试文件仅为 API 适配
（`status_code`→`observed_status_code`、`Option<Box>` 装箱解引），断言语义未削弱（diff 亲读）。

## 9. 结论

- 四条虚构分支（超时兜底 Signal(9)、CleanupTimedOut→Signal(9)/Code(-1)、write_stdin Exited{137}、
  terminal_facts 虚构升级）+ 同族 wait-Err→Code(-1) 与 finalize 首因丢失，全部在候选中核销，且由我自己的
  基线可编译反例独立复现红（R1/R2 确定性、R3 见证）、候选全绿。
- C01–C05 全部由我逐用例独立运行通过，且各配一个我自己的对抗变体（V1–V6，参数与执行者不同）通过；
  合法对照（0/7/真实 137）精确不降级，wait 错误=观察失败。
- 消费者穷尽、注入零产品调用点、journal 三形态（Succeeded-with-detail/Unknown/Cancelled）不混淆且迟到观察不改判
  （V5/V6 双向反例）；R03 十套件、G01/G02 套件、全 workspace 925/0、clippy/fmt/check-contracts/check-boundaries 全绿。

**VERDICT: PASS**
