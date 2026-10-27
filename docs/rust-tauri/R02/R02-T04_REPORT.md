# R02-T04｜实现存储 ports 与运行库事务 — 执行报告

- 执行者：ZCode:R02-T04（EXECUTOR-R02-T04，一次性执行代理；不负责独立验收，不提交/推送）
- 状态：**READY_FOR_REVIEW**（PASS/FAIL 判定归总控另派的独立验收）
- 日期：2026-09-26
- 任务书：`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R02_Rust独立服务、存储与事件基础.md` §4 R02-T04
  （场景 R02-A07「提交失败无假成功」、R02-A08「重放迁移幂等」，均 REQUIRED）

## 1. 范围

任务书四步：①新运行/消息库的版本化迁移（run/attempt/invocation/规范化消息/关键事件/迁移收据，
保持目标所有权表）；②有界 DB 工作队列 + 事务 + busy 超时 + WAL/checkpoint 策略 + 磁盘满处理；
③终态与关键事件同事务写入、事件发布在提交后、失败不产生可见成功；④其他现役数据库独立适配
保留、不无依据合并、新持久化点登记指纹/迁移清单。
交付：StoragePort 实现；新库 schema/migrations；事务及失效测试。

设计边界（任务书 + 总控指令）：StoragePort trait 在 lingxi-kernel（domain 无 DB 依赖，DEP-08 机器
执法）；SQLite 实现放新建 lingxi-adapters crate（DEPENDENCY_RULES 计划内 establish_stage=R02，
本任务以 D5 deliberate registry update 落地 planned→exists + 新 DEP-09）；lingxi-service 组合根注入。
T03 的 sessions.rs 内存 run 记录换成存储真身（run/attempt/关键事件落 SQLite），端点形状不变。

不在本任务内（后续 T）：事件流与断线续读（T05）、备份/关闭协调器/损坏库恢复（T06——本任务只
交付 `close()` 的 FIFO 排空 + WAL TRUNCATE checkpoint + worker join）、日志脱敏深化（T07）、xtask
门禁汇总（T08）、旧 JSONL 只读导入（R08 切换演练；本任务不读不写任何现役库）。

## 2. 源码基线与环境

- 开工实测：分支 `codex/rust-tauri-migration`，HEAD = `0ba54e325ab418caa57a8d9605bc492e793bcd5d`
  （= TASK_BASE_SHA = 远端 HEAD，R02-T01/T02/T03 已推送），`git status --short` 为空（干净）。
- tested SHA：`0ba54e325ab418caa57a8d9605bc492e793bcd5d` + 本任务未提交改动（§8 全集）。
- 平台：macOS 27.0 arm64（Darwin 27.0.0）。
- 工具链：rustup 锁定 1.98.1（`rust-toolchain.toml`；经 `~/.cargo/bin`，Homebrew rust 未参与任何
  构建）。全部 cargo `CARGO_NET_OFFLINE=true` + `--locked` + 专属 `CARGO_TARGET_DIR=/tmp/rust-target-r02-t04`。
- 网络：失效代理（127.0.0.1:7890）已从全部命令剥离。
- 测试隔离：全部服务/测试只用合成 home（mktemp / pid+tag 唯一目录）；未触碰真实用户目录、
  真实凭证、任何现役数据库文件。
- 依赖面：`rust/Cargo.lock` 仅新增 lingxi-adapters 成员块（git diff 14 行，无任何第三方版本变化）；
  rusqlite 0.40.2+bundled、tokio 1.53.1、serde/serde_json、sha2 0.11.0、tracing 0.1.44 全部为
  R01 已锁版本；新增 dev-only libc 0.2.189（已锁版本，用于 RLIMIT_FSIZE 故障注入测试）。

## 3. 观察事实（先读再动手，未混写设计）

1. T01–T03 交付面：lingxi-kernel 只有 4-trait 原型 ports（RunStore 无人实现、无消费者）；
   lingxi-service `ServiceState` 持有 auth/tickets/sessions(内存)/limits；sessions.rs 为 T03 最小
   内存面（自述「R02-T04 持久化、T05 流化」）；`run()` 传输纯，bootstrap 在锁后、bind 前。
2. `lingxi-spike` 的 queue.rs 已证明「单线程独占 Connection + 有界 mpsc」形态可编译运行（WAL 协商
   + backpressure 断言），但其 ops 是 KV 原型，不含迁移/事务/终态语义——本任务按其形态生产化。
3. 协议冻结类型（PROTOCOL_SPEC）：`EventEnvelope`（schemaVersion/eventId/streamId/seq/sessionId/
   runId?/attempt?/eventType/payload，u64 走十进制字符串）、`RunStatus`（8 态，is_terminal）、
   `KnownEventPayload::RunStateChanged/FinalMessageCommitted`、`canon::canonical_string`（不失败）。
   本任务全部复用，零协议 crate 改动。
4. rusqlite 0.40.2 + bundled 已在锁内（spike 用同版本同特性）；`rusqlite::ffi::Error::new(c_int)`
   存在；错误按 `extended_code & 0xff` 主码数值映射（避免枚举面猜测）。
5. ADR-004/DATA_COMPATIBILITY_MATRIX：69 现役存储分五级；11 项 switch-to-new-authority 的只读导入
   属 R08 演练；R02-T04 的「其他现役数据库通过独立存储适配保留」= 不合并、不读写、登记说明。
6. 边界检查器 r01_t01_check_ownership.py：D5 要求 workspace 成员与 registry 双向一致；adapters
   establish 即须改 registry（planned→exists），且 DEP-07 自动对其生效。
7. 故障注入候选的实证淘汰：macOS `chflags uchg` 实测只阻断 open-for-write，**不阻断已打开 fd 的
   write(2)**（python 探针：uchg 后 SQLite 照常 commit 成功）——无法对已建立的连接注入 commit 期
   写错误；plain chmod 同理。被否决，见 §4.4。
8. 审计封印测试族在开工基线即 1/3 红（seal 坐标落后于已授权 R02 提交，R01 同款「预期封印前红」；
   本任务复跑 `npm test -- --run tests/post-verification-audit-seal.test.ts`：1 failed | 2 passed，
  `artifacts/rust-tauri/R02/T04/gates/audit-seal-baseline.log`）。如实记录，未使其变绿，未动白名单。

## 4. 设计决定

1. **StoragePort 在 kernel，错误词汇也在此**（`lingxi-kernel/src/ports.rs`）：R01 的 RunStore 原型
   （无实现无消费者）演进为 `StoragePort`：`record_run_started` / `commit_run_outcome` /
   `load_run`，RPITIT（`-> impl Future + Send`，kernel 零新增依赖）。配套领域类型
   `RunOutcome`（终态 + 关键事件 + 可选 final message，`validate_terminal()` 前置拒绝非终态）、
   `CommittedOutcome`（`newly_committed` + **仅在 Ok 路径返回的** events 信封）、`StorageError`
   （QueueFull/QueueClosed/Busy/DiskFull/Io/Conflict/DatabaseTooNew/SchemaTampered/Corrupted/
   InvalidRequest/Internal + `retryable()` 保守分类）。契约注释固化：发布在提交后；失败无可见成功；
   幂等重放 Ok(false)；冲突诊断不合并。
2. **schema v1 与迁移框架**（`lingxi-adapters/src/storage/migrations.rs`）：7 张表——sessions、runs、
   run_attempts、invocations、messages、key_events、schema_migrations（收据）。每条迁移
   version+name+SQL，指纹 = sha256(SQL)；应用时 `BEGIN IMMEDIATE` + 执行 SQL + 写收据 +
   `PRAGMA user_version` 同事务提交。开库校验：收据连续从 1 起、版本/名称/指纹与编译内一致、
   `user_version` == 最高收据（双向记账，篡改版本号即 `SchemaTampered`）、空收据但存在非系统表
   （收据被抹）即拒、库版本高于本构建即 `DatabaseTooNew`（不降级不重放）。迁移 SQL 刻意不用
   `IF NOT EXISTS`：误重放对既有表响亮失败而非静默成功。所有权映射见表内注释 +
   `docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json`（runs/run_attempts/key_events→
   kernel.run-supervisor 语义；sessions/messages→kernel.session；invocations→kernel.tool-gateway
   （R04 起写入行）；schema_migrations→adapters.storage；物理写者恒为 rust-service 单进程）。
3. **有界队列与连接策略**（`queue.rs`）：一条专用 std 线程独占 Connection；调用方经有界 tokio mpsc
   （默认容量 64）提交闭包；`try_submit` 满即 `QueueFull`（显式背压，任务未执行不丢失），
   `submit` 等待容量。开库即协商并验证 pragmas：WAL（协商失败即拒）、busy_timeout 5000ms、
   wal_autocheckpoint 1000 页、foreign_keys=ON、synchronous=FULL（WAL+FULL 使已确认提交跨掉电
   持久，不只进程崩溃）。写入统一 `with_write_txn`（BEGIN IMMEDIATE + 显式 ROLLBACK；busy 错误
   带上配置的超时值）。`close()`：FIFO 排干在队作业 → TRUNCATE checkpoint → join worker，
   checkpoint 失败显式上抛；Drop 路径为崩溃等价（无 checkpoint，靠 WAL 恢复）。工作线程
   catch_unwind 包裹每个作业（panic → `StorageError::Internal`，不毒化通道）。
4. **故障注入方式（A07）与论证**：采用**真实内核级写错误** `setrlimit(RLIMIT_FSIZE)`（仅降 soft
   limit + 忽略 SIGXFSZ）：WAL 追加超限即 write(2) EFBIG，SQLite 在 COMMIT 时刻经已打开的 fd 报
   真实 IO/DiskFull 错误——正是「在 commit 前注入磁盘写入错误」且证据含真实库文件。被否决的
   候选：chflags uchg / chmod（实测无法阻断已打开 fd 的写入，见 §3.7）；hdiutil 小盘（重、沙箱
   风险、清理复杂）。该测试放**独立测试二进制**（tests/disk_full_fault.rs）：RLIMIT_FSIZE 与
   SIGXFSZ 是进程级状态，隔离后不影响并行测试。
5. **终态事务语义**：`commit_run_outcome` 单事务完成：终态校验（kernel 状态机在存储边界二次执法
   run 行当前态→目标态的合法迁移）→ 逐事件分配 per-stream seq（`MAX(seq)+1`，UNIQUE 约束兜底）
   → 写 key_events → （如有）final message 行 + `final_message_committed` 事件 → 更新 run 行。
   任何一步失败整体回滚，调用方收 `Err`、拿不到任何事件。崩溃窗口区分（同事务证明）：
   提交前崩溃=既无终态也无事件；提交后发布前崩溃=两者俱全且可从库恢复（发布可追赶）。
6. **组合根注入**：`ServiceState::bootstrap*` 打开 `{home}/lingxi-service/data/runs.db`（固定名，
   非用户派生；0700 目录内），跑迁移 + 幂等种子合成会话；失败即 `ServiceStartupError::Storage`
   （二进制 exit 2，无内存降级）。sessions.rs 换存储真身：`SessionBackend` trait（RPITIT +
   dyn 擦除 blanket impl），RunDatabase 实现之；`execute_for(port: &impl StoragePort, ...)` 把
   kernel RunContext（认证 Principal 映射 kernel Principal，含 Device 的 user_id 所有权列）经端口
   落库。R02 代表性 run **完成但不编造 final message**（契约 02 §4：完成状态与交付质量分离；
   真实模型回复 R05 才有）。发布=提交返回 Ok 后的 HTTP 200 + 审计日志（T05 接事件流）。
   关闭顺序：停服 → storage.close（排干+checkpoint+join，失败 exit 5）→ 移除自身实例记录（失败
   exit 4）→ 解锁。
7. **D5 deliberate registry update**：DEPENDENCY_RULES.json 中 lingxi-adapters planned→exists
   （established_by 注记）；新增 DEP-09 adapters-no-service（adapters 禁依赖 lingxi-service，
   防 port 实现反噬组合根）；workspace members + 注释更新；OWNERSHIP_TARGET 无需动（adapters.storage
   已登记）。Cargo.lock 仅 +14 行（成员块）。
8. **其他现役数据库**：`docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json` 登记新持久化点
   runs.db（路径模式、逐表所有权、pragmas、迁移清单+指纹 479b0321…461bb8，与二进制证据里的
   盘上收据一致）；69 现役存储零合并零读写，引用 DATA_COMPATIBILITY_MATRIX 的策略归属。

## 5. 修改了什么

新增：
- `rust/crates/lingxi-adapters/`（Cargo.toml、src/lib.rs、src/storage/{mod,migrations,queue,run_store}.rs、
  src/bin/lingxi-storage-inspect.rs、tests/{migration_idempotency,storage_transactions,disk_full_fault}.rs）
- `rust/crates/lingxi-service/tests/service_persistence.rs`
- `scripts/rust-tauri/r02_t04_storage_tx.sh`
- `docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json`
- `docs/rust-tauri/R02/R02-T04_REPORT.md`（本文件）
- `artifacts/rust-tauri/R02/T04/`（证据，§7）

修改：
- `rust/crates/lingxi-kernel/src/ports.rs`：RunStore 原型 → StoragePort + StorageError/RunOutcome/
  KeyEvent/CommittedOutcome + 契约测试；`lib.rs`：Principal 增加 storage_kind/storage_subject
  词汇（Device 带 user_id）+ LOCAL_OWNER_SUBJECT 常量。
- `rust/crates/lingxi-service/`：Cargo.toml（+kernel/+adapters 依赖）；lib.rs（ServiceState.storage、
  bootstrap 异步化 + 开库、EndpointError::storage 映射 500/503、handlers/WS 读路径 await 化）；
  sessions.rs（存储真身重写，端点形状不变）；main.rs（bootstrap await、storage 关闭链路 + exit 5 +
  usage）；tests/{service_health,instance_lifecycle,auth_matrix}.rs（bootstrap await 三处）。
- `rust/Cargo.toml`（+members+注释）、`rust/Cargo.lock`（+成员块 14 行）。
- `docs/rust-tauri/R01/DEPENDENCY_RULES.json`（D5 update + DEP-09）。
- `scripts/rust-tauri/r02_t01_boundary_negative.sh`（N-C 用例适配：service→adapters 合法边使注入的
  kernel→service 倒挂成为 cargo 级环依赖，拒绝更结构化；定位 grep 改为
  lingxi-service + cyclic package dependency 双形态匹配，注释记录缘由）。

## 6. 关键调用链（端点→kernel port→adapters→SQLite）

```
POST /lingxi/v1/sessions/{id}/execute
 → transport_guard(Origin/Host/限速) → auth_guard(bearer/设备/票据 → Principal)
 → execute_session handler（lib.rs）
 → SessionStore::execute_for(port=&RunDatabase, principal, ...)（sessions.rs；所有权检查先于写）
   → RunContext{kernel Principal, session, run, attempt, generation}（认证身份映射，载荷身份不可信）
   → lingxi_kernel::ports::StoragePort::record_run_started   ── 1 事务：runs 行 + run_attempts 行
   │                                                           + run_state_changed(queued→running)
   │                                                             key_event + last_event_seq
   → lingxi_kernel::ports::StoragePort::commit_run_outcome    ── 1 事务：状态机二次执法 + 终态更新
   │                                                           + run_state_changed(running→completed)
   │                                                             key_event（+ 如有 final message 行与事件）
   → [两实现都在 lingxi-adapters RunDatabase] submit(closure) → 有界 mpsc(64)
   → 专用 db-worker 线程（唯一 Connection；WAL/busy_timeout/FULL）→ BEGIN IMMEDIATE … COMMIT
 ← Ok(CommittedOutcome{events})（仅提交成功才存在）→ 发布（HTTP 200 + post-commit 审计日志）
 ← Err(StorageError)（无任何事件）→ EndpointError::storage → 500/503 + reason（A07）
```
启动：main.rs → 锁 → `ServiceState::bootstrap`（开 runs.db → 迁移/校验收据 → 种子会话；失败 exit 2）
→ 绑定 → READY 行。关闭：SIGTERM → 服停 → `storage.close()`（排干+TRUNCATE checkpoint+join；失败
exit 5）→ 记录清理（exit 4）→ exit 0。

## 7. 验收场景逐项结果（REQUIRED）

### R02-A07 提交失败无假成功 — PASS（本执行者证据；独立验收归总控）

前置「在 commit 前注入磁盘写入错误」= setrlimit(RLIMIT_FSIZE)（真实内核写错误，§4.4 论证）。

| # | 覆盖 | 命令 | 退出码 | 结果 | 证据 |
|---|---|---|---|---|---|
| A07-1 | commit 阶段失败（终态+完成事件提交遇真实 EFBIG） | `cargo test -p lingxi-adapters --test disk_full_fault -- offline --locked`（CARGO_TARGET_DIR=/tmp/rust-target-r02-t04） | 0（2 passed） | 显式 `Err(DiskFull/Io)`、无事件返回；重开真实库文件查询：run 无终态（running）、`{run}-done` 事件 0 条；故障清除后同一提交成功且两者俱在 | gates/cargo-test-workspace.log；测试体 `commit_fails_for_real_at_commit_time_without_fake_success` |
| A07-2 | WAL/journal 写失败（同一真实机制作用于 start 提交） | 同上 | 0 | start 提交也显式失败，runs 行数 0 | `start_commit_also_fails_under_the_fault` |
| A07-3 | 提交成功但发布前崩溃 vs 提交前崩溃（同事务区分） | `cargo test -p lingxi-adapters --test storage_transactions --offline --locked` | 0（7 passed） | 前者：终态+事件**都在**（可从库恢复）；后者：**都不在**；发布前丢弃信封后事件仍可查询回读 | `a07_crash_windows_distinguish_commit_boundaries` |
| A07-4 | 二进制级（真实进程）：提交后 kill -9 | `bash scripts/rust-tauri/r02_t04_storage_tx.sh artifacts/rust-tauri/R02/T04` | 0（ALL GREEN） | inspect 对真实 runs.db 的查询：completed 行与 -done 事件共存；重启 HTTP runCount=1 | summary.txt、s2-runs-dump.jsonl、s2-counts-after-crash.json |
| A07-5 | 二进制级：真实 IO 故障拒启 | 同上 | 0（场景内 exit=2） | chmod 0555 home → exit 2、显式 Permission denied、无 READY 行 | s3-fault.{out,err} |
| A07-6 | 存储失败映射到端点（HTTP 层无假成功） | service 单测（SessionExecuteError::Storage → EndpointError::storage 500/503） | 0 | `failed_commit_is_a_storage_error_not_a_success`（service 单测）等 | cargo-test-workspace.log |

补充：busy 超时显式（外部连接持锁 → `Busy{timeout_ms}` 等待后失败、释放后重试成功）、有界队列满
显式 `QueueFull`（park 作业 + 容量 1 + 第三次 try_submit）、幂等重放/冲突诊断
（`finalize_is_idempotent_and_conflicts_are_diagnosed`）、优雅关闭 checkpoint 后重启可读
（`graceful_close_checkpoints_and_data_survives_restart`）、WAL 协商验证（`wal_mode_is_negotiated_…`）。

### R02-A08 重放迁移幂等 — PASS（本执行者证据；独立验收归总控）

| # | 覆盖 | 命令 | 退出码 | 结果 | 证据 |
|---|---|---|---|---|---|
| A08-1 | 单元层：反复 apply_all 幂等 + 全部负向 | `cargo test -p lingxi-adapters --offline --locked`（lib 10 passed） | 0 | 重复 apply 零迁移；篡改收据版本/user_version/指纹/外来表/未来版本全部响亮拒绝 | cargo-test-workspace.log |
| A08-2 | 集成层（真实文件）：≥3 次重开 | `cargo test -p lingxi-adapters --test migration_idempotency --offline --locked` | 0（2 passed） | 3 轮重开：版本恒 1、schema 指纹逐字节一致、7 表行数稳定；盘上收据指纹 == 编译内指纹；负向：版本改 99 → DatabaseTooNew/SchemaTampered 拒绝；删收据清零版本 → 拒绝（不重放） | 测试体 `repeated_migration_checks_are_idempotent`、`tampered_version_rollback_is_rejected_not_replayed` |
| A08-3 | 二进制层：3 次真实服务重启各跑开库迁移检查 | r02_t04_storage_tx.sh（同上） | 0 | 3 轮：userVersion==supportedVersion==1、收据指纹==编译指纹、行数与基线 diff 为零；inspect 前后 db 文件 sha256 不变（证明查询只读） | s4-migrations-round{1,2,3}.json、s4-hashes-{before,after}.txt、summary.txt |

### 门禁与回归（全部退出码 0）

| 门禁 | 命令要点 | 结果 | 证据 |
|---|---|---|---|
| fmt | `cargo fmt --all -- --check` | CLEAN | gates/fmt-clippy.log |
| clippy | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 error | gates/fmt-clippy.log |
| 全量测试 | `cargo test --workspace --locked` | **165 passed / 0 failed**（32 套件，含既有 protocol/spike/browser-spike/service 全部回归） | gates/cargo-test-workspace.log |
| 边界检查 | `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py --self-test` | OK（正向 + N1–N15 负向全拒） | gates/boundary-selftest.log |
| T01 回归 | r02_t01_boundary_negative.sh、r02_t01_service_smoke.sh | A02/A01 PASS | gates/r02_t01_*（独立证据目录，未覆盖 T01 原始交付） |
| T02 回归 | r02_t02_dual_instance.sh、r02_t02_path_priority.sh、r02_t02_f01_env_token_negative.sh | A03/A04/F01 PASS | gates/r02_t02_* |
| T03 回归 | r02_t03_auth_matrix.sh | ALL CASES PASSED | gates/r02_t03_auth_matrix/ |
| 封印基线 | `npm test -- --run tests/post-verification-audit-seal.test.ts` | 1 failed / 2 passed（开工基线即红的既有状态，如实记录，未触碰） | gates/audit-seal-baseline.log |

## 8. 未验证内容 / 环境限制

1. 跨平台：全部证据仅 macOS arm64。RLIMIT_FSIZE/SIGXFSZ 故障注入是 unix 设施（Windows 等价未做，
   disk_full_fault.rs 内已标注平台边界）；T02 传承的 Windows 目录权限分支仍未在本机验证。
2. 「真实磁盘扇区满」（hdiutil 小盘）未执行：采用内核级 EFBIG 等价（同一 IO 错误路径分类），
   理由见 §4.4；如独立验收要求物理满盘证据，可后续补做。
3. invocations / messages 表 schema 已随 v1 迁移落库并计入指纹，但尚无写入方（分别属 R04/R05+
   的真实工具调用与模型消息）；本任务的运行时写入面是 runs/run_attempts/key_events/sessions。
4. WAL/TRUNCATE checkpoint 在「崩溃后 -wal 残留」场景的恢复由 SQLite 自身完成（A07-3/二进制 S2
   均覆盖）；在线备份 API、备份中断、损坏库拒启细分（A11/A12）属 T06，未在本任务验证。
5. 性能（吞吐/延迟）不在本任务验收面；队列默认容量 64 的负载调优未做（负载上限属 T07/T08）。
6. 二进制脚本对「提交前崩溃」的窗口区分在进程内测试完成（毫秒级窗口无法用外部 kill 确定），
   二进制级覆盖了「提交后崩溃」与「启动期真实 IO 故障」两个外部可观测窗口。

## 9. 已知风险

1. **封印测试族 1/3 红（既有，非本任务造成）**：seal 坐标 ab4f2281 落后于已授权 R02 提交；按总控
   指令如实记录、不修绿、不扩白名单；需总控在相应授权步骤推进封印坐标。
2. **r02_t01_boundary_negative.sh N-C 用例语义变化（本任务引发的受控适配）**：service→adapters
   合法边使注入的 kernel→service 倒挂在 cargo 解析层即成环被拒（比 DEP-08 更早的结构性拒绝）；
   脚本定位断言已改为双形态匹配。若未来 adapters→kernel 边被移除，该用例将回到 DEP-08 形态，
   断言仍兼容。
3. `query_one_text` 是证据/测试读缝（经同一 worker，仅 SELECT），若后续误用于写路径会绕过事务
   辅助函数——T08 的 xtask 门禁可考虑加「该函数无写 SQL」的静态检查。
4. run_id 由 `时间戳+全局序` 生成：同一毫秒内并发 execute 依赖队列串行化保证序号递增；R03 的
   RunSupervisor 接管后应改为内核侧统一 ID 策略。
5. seed 会话的 `INSERT OR IGNORE` 幂等仅按主键：若未来改 seed 内容（如标题），旧库不会更新——
   届时须走新迁移版本而非改 v1 SQL（改 v1 会触发指纹不匹配拒启，这是设计使然）。
6. `lingxi-service --version`/协议生成物未动，但 `rust/Cargo.lock` 变化会使 R01_HANDOFF 里记录的
   lockfile sha256 过期（该 sha 是 R01 候选时点快照，属正常阶段推进；提交时总控可按惯例补登）。

## 10. git status --short 全文（报告时点）

```
 M docs/rust-tauri/R01/DEPENDENCY_RULES.json
 M rust/Cargo.lock
 M rust/Cargo.toml
 M rust/crates/lingxi-kernel/src/lib.rs
 M rust/crates/lingxi-kernel/src/ports.rs
 M rust/crates/lingxi-service/Cargo.toml
 M rust/crates/lingxi-service/src/lib.rs
 M rust/crates/lingxi-service/src/main.rs
 M rust/crates/lingxi-service/src/sessions.rs
 M rust/crates/lingxi-service/tests/auth_matrix.rs
 M rust/crates/lingxi-service/tests/instance_lifecycle.rs
 M rust/crates/lingxi-service/tests/service_health.rs
 M scripts/rust-tauri/r02_t01_boundary_negative.sh
?? artifacts/rust-tauri/R02/T04/
?? docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json
?? rust/crates/lingxi-adapters/
?? rust/crates/lingxi-service/tests/service_persistence.rs
?? scripts/rust-tauri/r02_t04_storage_tx.sh
```
（docs/rust-tauri/R02/R02-T04_REPORT.md 为本文件，亦为未跟踪新文件。）

## 11. 推荐独立验收重点

1. **复跑 A07-1/A07-3**：`cargo test -p lingxi-adapters --test disk_full_fault --test
   storage_transactions`——核对失败路径断言真实（错误分类 + 真实库查询「无终态或无事件」二选一），
   而非仅测钩子；可临时把 `SoftFsizeLimit::lower` 的余量调大验证「FAULT INJECTION FAILED」
   防线会响（证明故障真的在生效）。
2. **复跑二进制脚本**：`bash scripts/rust-tauri/r02_t04_storage_tx.sh /tmp/任意目录`——用 sqlite3
   CLI 独立抽查 runs.db 行（不信任 inspect 单一工具）；比对 s4 三轮指纹一致。
3. **A08 负向动手**：对任一 runs.db 直接 `UPDATE schema_migrations SET version=99`（或删除收据行）
   后启动服务——应 exit 2 拒启而非重放。
4. **边界**：`r01_t01_check_ownership.py --self-test`（DEP-09 生效性：给 adapters manifest 加
   `lingxi-service` 依赖应被 D1 拒——N 电池外可手动一试）。
5. **端点形状不变**：对照 T03 报告的响应样例抽查 GET session / execute 响应字段
  （runCount/last_runs/runId）。
6. 核对 `rust/Cargo.lock` diff 仅 +14 行成员块；核对 69 现役存储零接触（注册 JSON 的声明 vs
   DATA_COMPATIBILITY_MATRIX）。

— 执行者声明：以上命令、退出码、证据路径均来自本机真实执行；未执行项已如实列入 §8。
READY_FOR_REVIEW。
