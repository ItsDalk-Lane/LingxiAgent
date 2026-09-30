# R03 修复轮 G04-E01 普通自查（F05：去重条目在真正受理前固化，失败重试返回不存在的运行）

- 执行代理：EXECUTOR-REPAIR-R03-G04-E01。日期：2026-09-30。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`，候选起点 HEAD `198e0da1e`（含已通过独立审查的 G01/G02/G03——本轮未回退、未破坏）。
- 证据根：`artifacts/rust-tauri/R03/repair-current/G04-E01/normal-selfcheck/`；工具链 `~/.cargo/bin/cargo`（1.98.1），全部 `--locked`，隔离 /tmp 数据根。
- 结论：**5/5 C-ID 普通自查 PASS**（红基线在隔离 worktree＝HEAD 旧代码上 4 红 1 钉；修复后全绿，3 次复跑稳定）。

## 方法

普通自查 = 每个 C-ID 的给定/操作/通过条件按验收清单字面执行一遍，通过真实组合根（真实 SQLite 存储、真实受理链/去重、真实 RunSupervisor/TaskSupervisor、真实单次 finalize）。Provider/Tool 替身只产生外部响应与受控外部副作用（跨"重启"存活的追加式计数文件）；存储装饰器（FailingStartPort/ParkingStartPort）把每一次写都委托给真实数据库，只在审查点名的边界注入受控故障——与已接受的 recovery_crash_points 套件的 GatedPort 同一模式，未 mock 被测链路。

## 逐 C-ID

### R03-FIX-F05-C01｜后台 spawn 拒绝后无幽灵 Replay — PASS

- 给定：`CancelPolicy.supervised_task_cap = 4`（同一真实语义、极小占用），4 个真实受监督任务停 在受控外部等待上占满容量；新 requestId `c01-k`。
- 操作：`execute_background_for` 提交 → 显式拒绝；释放容量（等待 fillers 退出）→ 同 key 同内容重试。
- 观测：首次 = `Err(SessionExecuteError::BackgroundRegistryFull { cap: 4 })`（不是成功响应）；runs 表 0 行、外部计数 0；重试 = `Ok`，**返回的 run id 有 durable 行**（`SELECT status FROM runs WHERE run_id=?1` = running），驱动真实执行并结算，会话最终恰好 1 个运行。
- 红基线（旧代码）：重试直接 `Ok(replayed=true)` 返回 `run_000001a0dcba6c00_000001`，该 id **无 durable 行** —— 正是审查的最小反例（`adversarial-selfcheck/red-baseline-admission_dedup_consistency.log`："no ghost replay: the accepted run id must have a durable row"）。
- 命令/退出码：`~/.cargo/bin/cargo test -p lingxi-service --locked --test admission_dedup_consistency r03_f05_c01 -- --test-threads=1` → exit 0（全文件 5 passed / 0 failed，`normal-selfcheck/green-admission_dedup_consistency.log`）。
- 证据：上述日志 + `normal-selfcheck/stability-3x-suites.log`（3 次复跑 5/0 ×3）。

### R03-FIX-F05-C02｜运行起始持久化失败补偿 — PASS

- 给定：真实存储起始事务提交前注入故障（FailingStartPort 在委托真实 `record_run_started` 前返回 `StorageError::Io`；读路径与后续写全部委托真实库）；前台带 key `c02-k`。
- 操作：首次提交失败 → 故障一次性恢复 → 同 key 同内容重试。
- 观测：首次 = `Err(Storage(Io))`（detail 含 "injected"），runs 0 行、外部 0；重试 = `Ok`，run id 有 durable 行，**最终恰好 1 个有效执行**、外部计数 1 —— 无假受理残留。
- 同族单元级（sessions.rs，真实 store 形状、测试端口注入）：`start_failure_under_request_id_re_admits_the_same_key_fresh`（失败→同 key 重试全新受理→结算后 replay 同一运行）。队列满的同族面：去重注册表 cap 满在 admission 闭包内 loud 拒绝、**不记录任何条目**（dedup 单元 `rejected_admission_records_nothing_and_cap_is_loud`）；存储写队列满/IO 失败同属"起始事务 Err"类，与注入故障走同一补偿链。
- 红基线（旧代码）：重试返回幽灵 replay（run 无行）——"no ghost replay: the retried run id must have a durable row"。
- 命令/退出码：同上（`r03_f05_c02` 过滤）→ exit 0；单元 `cargo test -p lingxi-service --locked --lib sessions::` → 10 passed / 0 failed。
- 证据：`normal-selfcheck/green-admission_dedup_consistency.log`、`normal-selfcheck/green-lib-units.log`。

### R03-FIX-F05-C03｜并发同 key 和异内容 — PASS

- 给定：ParkingStartPort 把首个提交停在预留与持久受理之间（真实 `record_run_started` 前）；同主体会话 alpha、key `c03-k`。
- 操作：同内容并发重试；不同内容并发；释放窗口后结算再重试。
- 观测：窗口内同内容 = 显式可重试拒绝（新错误 `AdmissionInFlight`，**不返回任何 run id**——不暴露半提交假结果）；不同内容 = `DuplicateRequestConflict`（无论受理处于哪个相位）；窗口释放后首个提交真实受理（run 行存在、外部 1）；结算后同 key 重试 = 幂等 replay 指向同一真实运行，会话恰好 1 个运行。
- 单元级确定性交错（current_thread + yield 停驻）：`same_key_duplicate_in_the_start_window_is_in_flight_not_a_ghost`。
- 红基线（旧代码）：窗口内同内容并发拿到 `Ok(replayed=true)` 指向无行 run——"no half-committed fake result: run_… has no durable row"。
- 命令/退出码：同上（`r03_f05_c03`）→ exit 0。
- 证据：同上两日志。

### R03-FIX-F05-C04｜副作用之后响应失败不删绑定 — PASS（钉）

- 给定：Tool 替身外部动作 +1（追加计数文件）后，响应在两态丢失——变体 A：journal started 已提交、receipt 未定，前台请求 future 被 abort；变体 B：run 已完整结算（journal receipt 已提交），响应被客户端丢弃。
- 操作：同 key 重试。
- 观测：两态都返回**既有（可核实的）运行**：`replayed=true`、run id 与首次相同且行存在；外部计数不增（A 态 1、B 态 2 各自保持）；runs 数不增。修复引入的错误路径撤销逻辑被钉住：durable start 之后任何失败都不再触碰绑定（dedup 单元 `committed_binding_is_never_released_or_unmarked`：release/mark/drop 三种"失败形态"对 Committed 绑定全部 no-op）。
- 说明：本用例在旧代码上也通过（旧代码从不删除绑定）——它是修复的**红线钉**，防止"遇错撤销"修过头；其红价值由 C01/C02/C03/C05 承担。
- 命令/退出码：同上（`r03_f05_c04`）→ exit 0；单元 `cargo test -p lingxi-service --locked --lib dedup::` → 10 passed / 0 failed。
- 证据：`normal-selfcheck/green-admission_dedup_consistency.log`、`normal-selfcheck/green-unit-dedup-sessions.log`。

### R03-FIX-F05-C05｜重启重试的安全契约 — PASS

- 给定：进程 A 带 key `c05-k` 提交，外部 +1、运行 completed；关闭存储模拟进程死亡；进程 B 在**同一数据根**上全新 bootstrap（内存去重表为空、恢复扫描运行）。
- 操作：同 key 同内容重试；新 key；同 key 改内容。
- 观测：同 key = **显式拒绝** `RequestIdBoundToEarlierRun { request_id, run_id }`（指向既有运行；不盲重做：runs 保持 1、外部保持 1）；新 key = 正常受理（真实新运行）；同 key 改内容 = 同样显式拒绝（不静默重做）。错误文案明确"query that run's durable outcome or resubmit under a new requestId"——**不声称对任意外部系统 exactly-once**。
- 书面契约（代码内冻结）：`dedup.rs` 模块文档（跨重启契约指向受理面）+ `sessions.rs` 错误变体文档 + `RunDatabase::find_run_id_by_request`（持久锚点 = 既有 `run_lineage.cause_id = "request:{id}"`，无新迁移、无新调度器）。
- 红基线（旧代码）：同 key 重试被静默当作全新请求执行（`a post-restart binding recovery must be an explicit replay` 断言失败——盲重做）。
- 命令/退出码：同上（`r03_f05_c05`）→ exit 0。
- 证据：同上。

## 回归与门禁（普通层）

- workspace：`cargo test --workspace --locked` = **69 suites / 689 passed / 0 failed**（底线 ≥67/673/0；+2 套件 +16 用例，无删除无跳过）——`logs/workspace-test-final.log`。
- `cargo fmt --all -- --check` 零 diff；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`rust/Cargo.lock` sha1 `3b659f41eb262eaf42efc2f91ec93f9989d42934` 与 HEAD 相同。
- G01–G03 套件逐套复跑绿（`normal-selfcheck/adjacent-suites.log`）：cancel_link_inheritance 7/0、subagent_closeout 8/0、cancel_terminal_race 13/0、cancellation_tree 8/0、tool_receipt_unknown 6/0；相邻面 request_dedup 4/0、background_disconnect_recovery 3/0。
- 额外门禁：`xtask check-contracts`（API_COMPAT_MATRIX 626 entries 零漂移）exit 0；`xtask check-boundaries` exit 0。
