# G03-E01 交付报告 — R04-RR1-F03 清理未确认被转换为已退出和虚构的 137 退出码

- 执行代理：EXECUTOR-R04-RR1-G03-E01（一次性）
- 基线：da15c4bd9（分支 codex/rust-tauri-migration，含 G01/G02 修复）
- 工作单：G03 / 缺陷 R04-RR1-F03 + 验收用例 C01–C05
- 状态：READY_FOR_REVIEW（不自行宣布独立 PASS）
- 未 commit / 未 push（按禁止事项）；全部改动在工作区

## 1. 当前 HEAD 的 F03 事实重新核实结论

复审报告的事实定位在原基线 773d5a696。在当前 HEAD da15c4bd9（G02 大改 procsupervisor.rs 之后）
逐条重新核实，**四条虚构分支全部仍以原形态存在，另发现一条同族新事实**：

| # | 位置（当前 HEAD） | 基线行为 | 核实结论 |
|---|---|---|---|
| F1 | `exectools.rs` `run_exec_command` timeout 分支（原 735-738 行区域） | terminate 回执不是 `Terminated` 时兜底 `ExitFact::Signal(9)` 并本地构造 `RecordPhase::Terminated`，返回 `ToolRunStatus::Exited{137}` | 仍存在。且 `AlreadyTerminal(Exited{fact})`（真实退出恰在 deadline 到达）的真实 fact 也被丢弃伪造成 137 |
| F2 | `exectools.rs` 结果映射（原 761-765 行区域） | `CleanupTimedOut => ExitFact::Signal(9)`、`_ => ExitFact::Code(-1)` | 仍存在 |
| F3 | `exectools.rs` `run_write_stdin`（原 967 行区域） | `CleanupTimedOut => ToolRunStatus::Exited{code:137}`（文本行 930 却写着 "kill was sent"） | 仍存在；且文本 "kill was sent" 在 G02 的跳发信号（group_signal_skipped）形态下也是不实陈述 |
| F4 | `procsupervisor.rs` `terminal_facts`（原 2387-2395 行区域） | `CleanupTimedOut => ExitFact::Signal(9)`、`_ => ExitFact::Code(-1)`；`await_termination` 的 Some 分支经它把并发终结者的 `CleanupTimedOut` 升级为确认的 `Terminated{Signal(9)}` 回执 | 仍存在（G02 未触及该函数的虚构 arm） |
| F5（新发现，同族） | `procsupervisor.rs` `finalize_exit` | 迟到真实观察落在 `CleanupTimedOut` 记录上时，reason 匹配只认 `Terminating` → 终态降级为无因 `Exited`，**首因丢失** | 与 C03「确认后保留真实退出事实和首因」直接冲突，随本单一并修复 |

G02 改动的影响核实：`child_reaped`/`reclaimed` 语义、`mark_terminating` 的发送时所有权门、
`settle` 恰好一次归还（G01）均与本单修复正交，语义原样保留（见 §6 回归）。
wait 错误路径（两处 reaper 的 `Err(_) => ExitFact::Code(-1)`）在当前 HEAD 仍存在，属 C04 第 4 组。

## 2. 红 → 绿

红测试文件：`rust/crates/lingxi-service/tests/r04_rr1_f03_stop_honesty.rs`（最终 7 个用例 + lib 内
1 个单元用例）。真实 cargo 红运行日志：`artifacts/rust-tauri/R04/RR1-G03-E01/repro/red_run_baseline.log`。

**确定性受控边界（不制造危险进程）**：
- one-shot（C01/C03）：G02 已验证的 reaped-but-undrained 窗口——直接子进程自然退出（reap 已发布），
  普通后台孙进程 `sleep 300 &` 持有管道写端使 drain 吃满 grace（1500ms）；此时 terminate 因所有权
  不可证而**跳发信号**（G02 语义），cleanup 预算 1ms 在观察落地前到期——窗口两侧余量 ≥0.5s/1s。
- PTY（C02）与 Run 取消（C05）：macOS 在会话首领退出时即向 master 交付挂断，无确定性窗口；
  按复审明示许可的「受控边界延迟 reaper 观察」增加验证专用注入 `ReapFaultPoint::
  DelayObservationNext`（真实 killpg 发出后，将终态记录扣持 2s，模拟 OS 迟到回执）。
- wait 错误（C04 第 4 组）：`ReapFaultPoint::WaitErrorNext` 注入走真实 Err 分支。
  两个注入点仅选择分支触发时机、只能由持有 supervisor 的进程内 Rust 代码武装、无产品路径武装
  （与 G01/G02 的 SpawnFaultPoint/PumpFaultPoint 同一先例与文档边界）。

**红运行观察**（对未修复基线）：
- C01（确定性）：工具结果 `Exited(137)`，而 kill 从未发出、退出从未观察——直接命中 F1 虚构。
- C02/C03 首轮（竞态形态，1ms 清理预算）：分别观察到轮询 `Exited(137)` 与首个回执
  `Terminated{fact: Signal(9)}`（无 exit_observed 背书）的虚构输出；改写为确定性构造后
  C02/C03 的最终形态依赖修复后的 API（StopUnconfirmed/注入点），无法再对基线编译——其红证据
  以首轮实测 + C01 确定性红 + lib 单元红形态（terminal_facts 对 terminal-without-fact 必须
  返回 None）共同覆盖，如实记录。
- 过程中发现 tokio 定时器 ~1ms 粒度使「零清理预算」仍存在观察竞争（一次基线运行中 C03 的
  首回执竟是真实观察的 Terminated），故弃用纯零预算构造，全部改为上述大余量窗口。

**绿**：修复后 7/7 用例 + 单元用例全绿；稳定性对抗 3 连跑全绿（每轮 ~2.1s）。

## 3. 源码变化与消费者影响

### rust/crates/lingxi-kernel/src/ports.rs（接口演进）
- `ToolRunStatus` 新增 `StopUnconfirmed { handle, detail }`：终止已请求但退出未获可信回执
  （或 wait 失败）——可查询、引用原句柄、绝非 Exited/Killed/确认失败。
- `ToolSuccess.status` 改为 `Option<Box<ToolRunStatus>>`（装箱原因：冷元数据；内联增长使
  `Result<_, ToolOutcome>` 族越过 clippy result_large_err 的 128 字节界，按 lint 建议装箱）。
- `Exited{code}` 文档收紧：仅真实观察（wait/可信系统回执）可写，观察到的 SIGKILL 是合法 137。

### rust/crates/lingxi-service/src/procsupervisor.rs
- `ExitFact` 新增 `Unobserved`；`status_code()` → `observed_status_code() -> Option<i64>`
  （Unobserved 无诚实数值，类型层面杜绝 -1/137 虚构）。`describe()` 补 Unobserved 文案。
- `exit_fact_of` 既无 code 又无 signal 的兜底 → `Unobserved`（原 Code(-1)）。
- 两个 reaper：wait Err → 审计 `wait_failed` + `ExitFact::Unobserved`（原 Code(-1)）；
  新增 `reap_wait`/`observed_exit_fact`/`hold_observation_for_verification` 与
  `ReapFaultPoint`（None/WaitErrorNext/DelayObservationNext，验证专用）。
- `terminal_facts` → `(Option<ExitFact>, bool, bool)`：terminal-without-fact（CleanupTimedOut）
  与非终态一律 `None`；`await_termination` Some 分支据此分流——有真实 fact 才发 `Terminated`
  回执并 settle；否则审计 `termination_unconfirmed_echo` 并返回 `CleanupTimedOut`
  （**重复终止/并发终结者不再自动升级为确认**；无虚构时才 settle，G01 恰好一次语义不变）。
- `finalize_exit`：reason 匹配扩展到 `CleanupTimedOut{reason}`——迟到真实观察落在未确认记录上时
  产出 `Terminated{首因, 真实 fact}`（首因穿越未确认窗口保留，不再降级为无因 Exited）。
- 模块/枚举文档同步（终止请求事实与观察事实分离、迟到观察不静默漏掉）。

### rust/crates/lingxi-service/src/exectools.rs
- `run_exec_command` timeout 分支：按回执诚实映射——`Terminated{fact}` 用真实 fact；
  `AlreadyTerminal(phase)` 回显真实相位（真实退出恰在 deadline 到达时报告真实码，不再伪 137）；
  `CleanupTimedOut` 保持未确认；`UnknownProcess` 落入既有「记录消失」的显式 Internal 失败。
- 结果映射：退出码仅来自 `observed_status_code()`；未确认/不可读 → `StopUnconfirmed{原句柄,
  detail}`，文本附 `[stop unconfirmed] …不要假设无进一步副作用；process_id … 仍可查询`；
  超时通告与观察事实解耦（`timeout_fired`），确认 kill 的 137 与非零码照实陈述（不降级 Unknown）；
  非终态到达结果映射属内部不变量破坏，显式 Internal 失败，不编造。
- `run_write_stdin`：`CleanupTimedOut` 文本改为「termination was requested, exit not observed —
  unconfirmed, not exited」（不再谎称 kill was sent；审计携带真实 sent/skip），结构化 status 改
  `StopUnconfirmed`；`Unobserved` fact 同样映射 `StopUnconfirmed`——文本与结构化一致（C02）。

### rust/crates/lingxi-service/src/runs.rs
- `journal_receipt_of` 新增 `StopUnconfirmed` 臂：收据明细如实记录「stop unconfirmed, handle …」，
  不读作观察到的退出。Cancelled→Unknown、unobserved→Unknown 的 R03 语义未动。

### 消费者适配（等价更新，断言不降级）
- `tests/r04_rr1_f02_reaper_cleanup.rs`（8 处）、`tests/r04_t05_process_tools.rs`（3 处）、
  `tests/r04_t05_registry_capacity.rs` / `tests/r04_t08_tool_matrix.rs`（status_of/匹配改
  `as_deref()`）；`status_code()` → `observed_status_code() == Some(n)`（等强度）。
- 协议/生成类型无涉：`ToolResultWire` 本就不序列化 run status；门禁 4 证明生成物零漂移。

## 4. 同族路径检查

- 全库 grep `CleanupTimedOut`：src 内仅 exectools（已修）与 procsupervisor（已修）。
- 全库 grep 虚构源 `ExitFact::Signal(9)` / `ExitFact::Code(-1)` / `code: 137` / `code: -1`：
  src 内仅剩本单新增的注释/文档与单元测试断言，无生产构造点。
- Run/取消链（C05 面）：`journal_receipt_of` 的 Cancelled→Unknown、`unobserved_tool_exit_reason`
  的三分类、迟到结果栅栏均未触碰；取消丢弃 future → `ProcessOwnershipGuard` → `terminate_detached`
  的链路由 C05 主腿 + 丢弃对抗腿实证「已取消控制流」与「外部未确认」两事实分离。
- 不降级核查：普通 0/7、真实观察 SIGKILL 的 137、非零码陈述全部保留（C04 前三组）；
  R03 异常结果误分类修复未回退（全量套件含 admission_dedup/late_result_fence 等通过）。

## 5. 逐 C-ID 自查（普通 / 对抗）

证据日志：`artifacts/rust-tauri/R04/RR1-G03-E01/logs/case_*.log`（逐条单独运行）。

### C01 单次命令清理迟延 — PASS（普通+对抗）
- 普通：真实工具调用 timeout 触发；未观察退出时结果无 Exited/137（负向断言）+
  `StopUnconfirmed{handle==原句柄}`（正向）+ 文本 `[stop unconfirmed]` 含句柄；进程记录
  `CleanupTimedOut{Timeout}` 可查询；audit 时间线 请求（group_signal_skipped）→ 到期
  （cleanup_timed_out）两事实分离、无 exit_observed；child_reaped=true（reap 是真观察，exit
  fact 未定——两类事实如实区分）；live_cap=1 下未确认期间新 spawn 被拒（名额不虚还）；
  迟到真实观察 → `Terminated{Timeout, Code(0)}`（自然退出的真实码，非 137）+ exit_observed
  → settle → 名额归还（新 spawn 成功）。孙进程未被信号波及（信号日志为空），按 pid 精确回收。
- 对抗（请求已发/观察未发分离）：audit 次序断言 + 快照事实断言，见上。

### C02 PTY 清理迟延 — PASS（普通+对抗）
- 普通：合法 PTY 句柄经注入延迟进入 `CleanupTimedOut`（killpg 真实发出，audit/信号日志实证）；
  所有者轮询（空 chars）：文本 "cleanup_timed_out … unconfirmed, not exited" 与结构化
  `StopUnconfirmed{handle}` 一致，引用原句柄；迟到观察后 `Terminated{Close, Signal(9)}`（真实
  137 合法）。无重复确定收据（cleanup_timed_out 恰一条）。
- 对抗：空 chars 轮询 ×3 稳定；多次重复无状态漂移；异会话对照 Forbidden（权限不放松）。

### C03 重复终止与迟到退出 — PASS（普通+对抗）
- 普通：第一次清理超时（回执 CleanupTimedOut）；再次 terminate 得 `AlreadyTerminal(
  CleanupTimedOut)`（不升级为确认；若回显 fact-bearing 相位则必须 exit_observed 背书）；
  未确认期间名额被持（RegistryFull）；迟到真实观察 → `Terminated{Timeout(首因), Code(0)}`；
  确认后第三次 terminate 回显 `Terminated{Timeout, Code(0)}`（真实 fact+首因保留）；
  settle 后名额恰好一次归还（探针 spawn 成功）。
- 对抗（真实退出恰在 deadline 与第二调用间）：C04race 10 轮 `join!(wait_terminal, terminate)`
  竞争全部落在真实观察侧（Terminated 必有 exit_observed；fact ∈ {Code(0), Signal(9)}）；
  并发双终结者的 terminal_facts 虚构升级由 lib 单元用例确定性锁定（terminal-without-fact
  → None）。**局限（如实）**：macOS 无 setsid 工具，无法在集成层确定性构造「组逃逸孙进程使
  kill 已发而观察延迟」的并发双 await 窗口；该 seams 的确定性红由单元级覆盖。

### C04 控制组退出与错误 — PASS（普通+对抗）
- 四组（真实工具调用）：`echo` → Exited{0}+Exited{Code(0)}；`exit 7` → Exited{7}+Code(7)；
  真实 SIGKILL（timeout+正常清理预算，kill 后 wait 观察 Signal(9)）→ Exited{137}+
  `Terminated{Timeout, Signal(9)}`+exit_observed（**真实 137 不降级**）；wait 错误（受控注入）
  → `StopUnconfirmed` + `Exited{fact: Unobserved}` + `wait_failed` 审计（**无 -1/137 编造**）。
- 对抗：取消与自然退出竞争（10 轮，见 C03）；t05 既有 `adversarial_cancel_racing_natural_exit`
  亦随全量回归。

### C05 上层取消不虚报外部静止 — PASS（普通+对抗）
- 普通（完整组合根 harness：真实 run driver/journal/registry/gateway/approvals/进程工具）：
  Run 经真实用户面 `cancel_run` 取消、清理未确认 → Run 侧：settled==run_id、journal 条目
  `Started` 且无收据（R03 Unknown 窗口）；进程侧：`CleanupTimedOut`、killpg_sent <
  cleanup_timed_out、无 exit_observed——两种事实并存可查询，未谎称资源已静止；迟到真实观察
  → `Terminated{CallerDropped(首因), Signal(9)}` → live 清空。
- 对抗：工具 Future 直接丢弃（`driver.abort()`，guard 链同样诚实）；继承取消/迟到结果栅栏由
  R03 套件（cancel_link_inheritance、cancellation_tree、cancel_terminal_race、
  late_result_fence、tool_receipt_unknown、request_dedup、admission_dedup_* 等）在全量中回归。

## 6. 门禁与回归（真实退出码）

| 门禁 | 命令 | 退出码 | 日志 |
|---|---|---|---|
| 1 | `cargo fmt --all -- --check` | 0 | logs/gate1_fmt.log |
| 2 | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | logs/gate2_clippy.log |
| 3 | `cargo test --workspace --locked` | 0（全量含 R02/R03/R04 全套件，含 300s r00 腿） | logs/gate3_workspace_test.log |
| 4 | `cargo run -p xtask -- check-contracts` | 0（生成物零漂移，56 文件 + 626 条矩阵） | logs/gate4_check_contracts.log |
| 5 | `cargo run -p xtask -- check-boundaries` | 0 | logs/gate5_check_boundaries.log |
| 6 | 逐 C-ID 单独运行（C01–C05 + 单元）+ G01/G02 套件复跑 | 全 0/全绿 | logs/case_*.log、logs/g01_*.log、logs/g02_*.log |

- G01 套件（r04_t05_registry_capacity）：9/9 通过；G02 套件（r04_rr1_f02_reaper_cleanup）：
  11/11 通过——两套件语义未被破坏。
- 附加：lingxi-service lib 单元 297/297（终态 fmt 后复跑）；F03 套件 3 连跑稳定。

## 7. 未验证项 / 局限（如实）

1. **并发双 await_termination 的集成级确定性窗口**（C03 对抗）：macOS 无 setsid，无法制造
   组逃逸持有者使 kill 已发而观察确定性延迟；由 lib 单元用例 + C04race 竞争轮覆盖 seams，
   修复分支（terminal_facts→None + unconfirmed_echo）已被两者锁定。
2. **wait 错误与 PTY 观察延迟为受控注入**：真实 OS 层 wait 失败/迟到回执未在本机自然复现
   （复审亦标注 observation_level 为 SOURCE_CONFIRMED_NATIVE_TEST_PENDING）；注入只选择分支
   时机，代码路径与真实失败一致。
3. **红证据形态**：C01 为对基线的确定性红；C02/C03 的最终（确定性）形态依赖修复后 API 无法
   对基线编译，其红证据为首轮实测的竞态红 + 单元级红形态（见 §2），已如实记录。
4. 平台：全部验证在 macOS/darwin 27 arm64 本机；非 Unix 构建路径（UnsupportedPlatform）与
   其他平台未在本轮验证（与既有阶段口径一致）。
5. 桌面 TS/协议生成面无涉（run status 不上线）；未做正式打包验证（本地结果不代替打包/真实
   供应商验证）。

## 8. 改动清单

源码（4 文件）：`rust/crates/lingxi-kernel/src/ports.rs`、`rust/crates/lingxi-service/src/
exectools.rs`、`rust/crates/lingxi-service/src/procsupervisor.rs`、`rust/crates/lingxi-service/
src/runs.rs`。
测试（5 文件改动 + 1 新增）：`tests/r04_rr1_f03_stop_honesty.rs`（新增，7 用例）、
`tests/r04_rr1_f02_reaper_cleanup.rs`、`tests/r04_t05_process_tools.rs`、
`tests/r04_t05_registry_capacity.rs`、`tests/r04_t08_tool_matrix.rs`（均为 API 等价适配，
断言不降级）+ procsupervisor.rs 内单元用例 2 处强化。
未动：总控账本（repair-current/ 既有文件、ORCHESTRATOR_PROGRESS.json）、Cargo.lock、
skills2set/、生成物。
