# R02-T06｜备份、关闭与启动恢复 — 执行报告

- 执行者：ZCode:EXECUTOR-R02-T06（一次性执行代理；不负责独立验收，不提交/推送；
  PASS/FAIL 判定归总控另派的独立验收）
- 修复代理：ZCode:REPAIR-R02-T06-R1（第 1 轮验收 FAIL 后的修复执行者；未参与原实现，
  未参与验收；见 §12 REPAIR-R1 —— F01/F02 修复与全部复跑证据；本文 §7.1/§7.5 数字已按
  修复后实测更正）
- 状态：**READY_FOR_REVIEW**
- 日期：2026-09-26
- 任务书：`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R02_Rust独立服务、存储与事件基础.md` §4 R02-T06
  （场景 R02-A11「WAL 状态备份可恢复」、R02-A12「损坏库不自动丢弃」，均 REQUIRED）
- tested SHA：`2aa80f506464e6cba7ec001b987d649e1164faaf`（= TASK_BASE_SHA = 远端 HEAD）+ 本任务未提交改动（§7 全集）
- 平台：macOS 27.0 arm64（Darwin 27.0.0）；工具链 rustup 锁定 1.98.1（`rust-toolchain.toml`，
  经 `~/.cargo/bin`；本机 rustup CLI 1.29.1，锁定的 rustc/cargo 1.98.1，已核对）
- 依赖面：`rust/Cargo.lock` 仅 +1 行（lingxi-service 的 **dev-only** rusqlite 引用边；rusqlite
  0.40.2+bundled 为 R01 已锁版本，零新包零版本变化）。lingxi-adapters 的 rusqlite 追加
  `backup` feature（SQLite Online Backup API，仅解锁已锁 crate 内代码）、tokio 追加 `time`
  feature（备份并发测试的定时）——均 feature flag only，无新锁条目。
- 网络与隔离：全部 cargo 命令 `CARGO_NET_OFFLINE=true` + `--locked` + 专属
  `CARGO_TARGET_DIR=/tmp/rust-target-r02-t06`；代理变量从全部命令剥离；全部测试/演练只用
  mktemp 合成 home（/tmp），未触碰真实用户目录、真实凭证、任何现役数据库。

## 1. 范围

任务书四步：①SQLite 在线备份 API/停写者一致性快照（不只复制活跃 db 主文件）；②graceful
shutdown（停收新请求→等待/取消受管任务→flush 关键事件→关闭 DB→移除自己的实例记录）；
③恢复时校验 schema/epoch/完整性与未完成事务记录，无法恢复进入显式降级或拒启、不自动清空库；
④故障注入覆盖 checkpoint、journal、状态文件写入、备份中断、退出超时。
交付：备份与恢复机制；关闭协调器；故障探针。
**本任务责任内的 PROD-DEFECT-1 修复**（RISK_REGISTER `RR-T07-PROD-DEFECT-1`，ADR-004 §5）：
新 Rust 栈的 epoch/启动恢复闸 corrupt 类一律 fail-closed（拒启，无基线软化、无放行边），
可读高位过渡证据时警告/诊断文本必须如实；在 Rust 实现上重跑 4 变体演练证明无一条 fail-open。

不在本任务内：T05 移交的 R2-F01 残留（resume TOCTOU 部分截断残差——未触碰其测试与语义，
回归全绿）；R08 切换演练、R03 运行状态机；Node 生产代码（红线：`core/data-epoch-coordinator.ts`
等零字节改动）。

## 2. 观察事实（先读再动手，未混写设计）

1. **PROD-DEFECT-1 在新栈的复存（修复前基线实测）**：本任务开工时 Rust service 无任何 epoch
   闸——用 HEAD 基线二进制对「缺失印章 + 可读 barrier_raised 过渡日志（1→2）」home 直接
   启动 READY 并接受 execute（HTTP 200），runs.db 落库 1 run + 2 key events。即新栈若无闸
   同样 fail-open，与 Node R10 变体同型。证据：`artifacts/rust-tauri/R02/T06/prod-defect-1/baseline-no-gate/`。
2. 现役闸语义逐行复核（`core/data-epoch-coordinator.ts:547-550`、`server/index.ts:313-335`、
   `shared/data-epoch.cjs`）：corrupt 类 failure 丢弃可读日志 from/toEpoch → mustBlock 恒 false →
   fail-open；警告文本固定输出 "no higher-epoch evidence was found"（R7/R10 与事实相反）。
   印章/日志盘上格式（v2 stamp / v1 journal 七 phase / 校验矩阵）已按原样在 Rust 侧复刻。
3. T02 单写者锁（flock，OS 文件锁为唯一存活权威）在闸之前，与 Node `server/index.ts`
   「同宅互斥 → epoch 闸 → store 打开」顺序一致；T04 已有「版本/指纹/收据篡改拒绝」
   （`SchemaTampered`/`DatabaseTooNew`）基础面，但没有完整性校验与 epoch 校验。
4. T04 关闭链（run() 返回 → storage.close() → 记录移除）无 deadline：排干/checkpoint 若挂起，
   进程无限期不退出——退出超时行为此前不存在。T05 的 WS 会话是长连接受管任务，axum 的
   graceful wait 会等待升级连接关闭——若不在**信号时刻**广播关闭，会与「先等传输退出再处理
   受管任务」形成死锁（实现时以信号时广播消解，见 §4.3）。
5. rusqlite 0.40.2 `backup` feature 在锁内可用（/tmp 探针实测编译运行 + Cargo.lock 零变化）；
   bundled SQLite 对损坏形态的实测行为：garbage 主文件 = NOTADB 拒绝且字节不变；真实崩溃
   后 torn WAL 尾 = **按设计恢复**（不完整尾帧=事务未提交，恢复视图事务一致）；WAL 魔数
   损坏 = 同样按设计恢复（丢无效帧）。T04 的 RLIMIT_FSIZE 故障注入法（独立测试二进制 +
   SIGXFSZ 忽略）可复用于备份中断注入。
6. 审计封印测试族在开工基线即红（seal 坐标落后于已授权 R02 提交，R01 同款预存在红）；
   本任务复跑 `npm test -- --run tests/post-verification-audit-seal.test.ts`：1 failed | 2 passed
   （`gates/audit-seal-baseline.log`）。如实记录，未使其变绿，未动白名单/坐标。

## 3. 修改了什么

新增：
- `rust/crates/lingxi-service/src/epoch.rs`：数据 epoch 启动恢复闸（fail-closed；印章/日志
  盘上格式与 Node 双向兼容；PROD-DEFECT-1 闭环；16 个单元测试）。
- `rust/crates/lingxi-service/src/shutdown.rs`：关闭协调器（三阶段 deadline、超时 marker、
  WsShutdown 受管任务句柄；3 个单元测试）。
- `rust/crates/lingxi-adapters/src/storage/backup.rs`：SQLite Online Backup API 备份 +
  manifest 哈希 + 已验证恢复（integrity gate、partial 清理、防覆盖）。
- `rust/crates/lingxi-adapters/tests/backup_restore.rs`（11 测试（REPAIR-R1 后），R02-A11 主证据）；
  `rust/crates/lingxi-adapters/tests/backup_faults.rs`（2 测试，RLIMIT_FSIZE 备份中断注入，
  独立测试二进制）。
- `rust/crates/lingxi-service/tests/shutdown_coordinator.rs`（5 测试，真实 RunDatabase +
  InstanceGuard + 外部连接持锁注入退出超时）。
- `scripts/rust-tauri/r02_t06_backup_restore.sh`（A11 二进制级证据，S1–S4）；
  `scripts/rust-tauri/r02_t06_recovery_drill.sh`（A12 + 4 变体演练，v1–v8）。
- `docs/rust-tauri/R02/R02-T06_REPORT.md`（本文件）；`artifacts/rust-tauri/R02/T06/`（证据）。

修改：
- `rust/crates/lingxi-adapters/src/storage/queue.rs`：开库后收紧 runs.db 及 -wal/-shm 至 0600
  （T04 REVIEW F04 顺带项；主文件严格失败=开库失败，sidecar 缺失跳过/失败响亮 warn）。
- `rust/crates/lingxi-adapters/src/storage/migrations.rs`：SQLITE_NOTADB(26)→`Corrupted`；
  新增 `verify_integrity`（PRAGMA integrity_check 全 "ok" 才放行）。
- `rust/crates/lingxi-adapters/src/storage/run_store.rs`：`RunDatabase::open` 在收据校验后追加
  `verify_integrity`；新增 `backup_to`（经单写者队列执行备份作业=停写者+在线 API）、
  `logical_dump`/`run_fact_summary`（A11 证据读缝，只 SELECT）、`wal_sidecar_path`。
- `rust/crates/lingxi-adapters/src/storage/backup.rs`（REPAIR-R1 F02）：备份/恢复产物权限收紧
  ——目标目录（新建或已存在）0700、`.partial`/最终 db 副本/manifest 及其 tmp 文件 0600
  （unix；失败响亮中止，见 §12.2）。
- `rust/crates/lingxi-adapters/src/storage/mod.rs`：backup 模块与导出。
- `rust/crates/lingxi-adapters/src/bin/lingxi-storage-inspect.rs`：新增 `backup`/
  `restore-verify` 子命令（走真实 adapters 备份/恢复路径；失败响亮退出非零、零产物）。
- `rust/crates/lingxi-adapters/tests/backup_restore.rs`（REPAIR-R1）：并发用例断言重写为
  事务一致性语义（F01，见 §12.1）；新增确定性用例 `a11_backup_of_a_started_but_
  uncommitted_run_is_a_complete_prefix`（固定交错实证 in-flight run 的快照语义）与
  `backup_and_restore_artifacts_are_owner_only`（F02 权限断言）；全套 9→11 测试。
- `rust/crates/lingxi-service/src/config.rs`：`--shutdown-timeout-ms`（严格解析、正整数校验、
  `InvalidShutdownTimeout`）+ 测试。
- `rust/crates/lingxi-service/src/lib.rs`：epoch/shutdown 模块导出；`ServiceConfig.shutdown_timeout_ms`；
  `ServiceState.ws_shutdown`；`run_ws_session` 受管任务注册（WsSessionGuard）+ shutdown 广播
  select 分支（close 1001）。
- `rust/crates/lingxi-service/src/main.rs`：锁后、任何 store/auth 写入前接 epoch 闸（拒绝 =
  marker + 双语诊断 + exit 2）；关闭链改为协调器（§4.3）；USAGE/退出码表（新增 6）。
- `rust/crates/lingxi-service/Cargo.toml`：dev-only rusqlite（外部连接持锁注入退出超时）。
- 测试文件 5 处 `ServiceConfig{}` 字面量补 `shutdown_timeout_ms`（auth_matrix、
  event_subscription、instance_lifecycle、service_health、service_persistence）。

未触碰：任务书、`contracts/generated/`（门禁实证零漂移）、`.sync-audit/`、PROGRESS.md、
ORCHESTRATOR_PROGRESS.json、Node/Electron 生产入口（**含 core/data-epoch-coordinator.ts，
零字节改动**）、lingxi-protocol、T01–T05 的历史 artifacts（回归证据全部定向写入 T06 目录，
零覆写——`git status` 无 T01–T05 artifacts 变更）。

## 4. 设计决定

### 4.1 备份机制选择（任务书步骤 1，ADR-004 D3 / ROLLBACK_DESIGN §1.4 冻结决策的落地）

- **机制 = SQLite Online Backup API**（`rusqlite::backup`，`sqlite3_backup_*`），永远不是活跃
  主文件拷贝。备份作业经 `RunDatabase::backup_to` 提交到**单写者队列**执行：作业运行期间
  无任何其他 DB 作业可交错（FIFO 串行）=「停写者」，同时拷贝走在线 API=双保险。
- **默认流程 = 冻结静止点**：`wal_checkpoint(TRUNCATE)`（W07）→ Online Backup API 拷贝 →
  目标 `integrity_check` 必须 "ok" → fsync → 原子 rename 到最终名 → 写 manifest
  （schemaVersion/files[].sha256/integrity/walBytesBefore/preCheckpoint，原子写）。
  `BackupOptions.pre_checkpoint=false` 保留纯在线 API 变体（API 直接读已提交 WAL 内容），
  用一条专门测试证明两种形态都完整捕获 WAL-resident 数据（A11 前置的核心风险）。
- **中断不冒充成功**：备份全程先写 `.partial-*` 临时名，integrity 通过后才原子晋升最终名；
  任何失败路径删除 partial（删除失败响亮 error 日志）→ 中断后目标目录不可能存在可被恢复
  流程误信的半份备份；manifest 只在成功路径存在，恢复前强制校验 sha256。
- **恢复 = 拷贝 + 哈希对账 + 完整恢复闸**：`restore_backup` 校验 manifest 哈希后才拷贝，
  拒绝覆盖既有目标，拷贝后再验副本哈希；恢复出的库必须经 `RunDatabase::open` 完整开库
  （迁移收据 + `PRAGMA integrity_check`，与活库同一闸）才算恢复成功。
- **产物权限与活库同纵深（REPAIR-R1 F02）**：备份目标目录（新建或已存在）收紧 0700，
  `.partial`/最终 db 副本/manifest/tmp 收紧 0600（unix，失败响亮中止；非 unix 平台为
  模块文档明示的 no-op）。备份=run 事实的外置副本，沿用 T04-F04 的文件自身兜底论据。
- 备份期间写者行为=**显式选择「停写者（队列静默）+ 在线 API」**并测试（A11 并发写者用例
  证明一致性前缀边界）。

### 4.2 恢复闸 fail-closed 语义（任务书步骤 3 + PROD-DEFECT-1 闭环）

- 位置与顺序（与 T02/T04/T05 的协作）：**T02 单写者锁（含 stale 接管）→ T06 epoch 闸 →
  T03 auth bootstrap → T04 runs.db 打开（迁移收据 → integrity）→ T05 事件服务**。闸在锁后
  mirror Node 的「互斥先于闸」；闸拒绝时 auth 状态/本地 token/任何 store 均未产生（脚本断言
  `local-token.json` 不存在）。
- **零软化**：incumbent 在 DATA_EPOCH=1 的基线软化（`mustBlock` 判定 + fail-open 警告）在本
  栈不存在——`coordinate_data_epoch_startup` 的任何失败都拒绝启动（二进制 exit 2，
  `LINGXI_DATA_EPOCH_BLOCKED` / `LINGXI_DATA_EPOCH_TRANSITION_INCOMPLETE` 机读 marker 与
  Node 同词汇），不存在可被误判的放行边，从结构上消灭 PROD-DEFECT-1 的 mustBlock 误判面。
- **证据如实**：只要过渡日志本身可读（解析+校验通过），corrupt-stamp / corrupt-transition 的
  拒绝都携带 `journal_evidence`（fromEpoch/toEpoch/phase/transitionId），诊断文本明示
  "A READABLE transition journal … epochs 1→2, phase=barrier_raised …"；corrupt-journal 时
  日志不可信，文本只陈述「日志不可读、其 epoch 无法采信」，**绝不声称「无高位证据」**；
  "no higher-epoch evidence was found" 字符串在全 crate 不存在并被单元测试冻结
  （`the_incumbent_lie_string_never_appears_in_rendered_refusals`）。
- 覆盖矩阵：torn stamp / torn journal / 缺失印章+可读高位日志（R7/R9/R10 对应）/ R11 prepared
  日志 / 更高印章 downgrade-blocked / 未盖章 home 含外来数据（unstamped-home-with-data——
  Rust 不自动认领旧数据，认领是显式 R08 切换动作）/ symlink 与 `.tmp-` 半写歧义 / 印章写失败
  （状态文件写入故障，无半张印章）/ committed-tail 清理（Node `committed-tail-cleaned` 镜像）/
  barrier 印章无日志（inconsistent-transition-state）。
- 恢复完整性：`RunDatabase::open` = T04 收据/指纹/版本双向记账 + **新增 integrity_check**；
  A12 的「未完成事务记录」由 SQLite WAL 崩溃恢复按帧语义处理（提交帧才生效），演练 v6
  实测恢复视图事务一致并如实记录。

### 4.3 关闭顺序与退出超时（任务书步骤 2、4）

```
SIGINT/SIGTERM ──► ws_shutdown.request_close()（信号时刻即广播！）
                └► axum with_graceful_shutdown：停止收新请求 + HTTP 在途排干
        受管 WS 任务收到广播 → close(1001) → 会话结束（计数归零）
run() 返回 ──► shutdown::graceful_shutdown（每阶段 deadline=--shutdown-timeout-ms，默认 10s）
        阶段2 ws_drain：等待开连接计数归零（超时→LINGXI_SERVICE_SHUTDOWN_TIMEOUT marker，继续）
        阶段3 storage_close：队列 FIFO 排干（=flush 关键事件，T04 同事务语义）→ TRUNCATE
                checkpoint → worker join（超时→marker，记录「WAL 下次打开自恢复」）
        阶段4 record_cleanup：验记录归属 → 移除自己的 instance.json → 解锁
        退出码：存储失败=5 ＞ 记录失败=4 ＞ 任一超时=6 ＞ 干净=0（usage 表固化）
```

- **信号时刻广播**是死锁消解的关键：axum 的 graceful wait 会等升级（WS）连接结束，若广播
  只在传输退出后才发（协调器阶段 2），双方互等。现在 WS 会话在信号时刻自行 close(1001)，
  协调器阶段 2 只做有界确认。S4 二进制用例证明：开 WS + SIGTERM → 客户端收到 1001、
  服务 exit 0（修复前此形态会无限挂起）。
- 超时显式不静默：每个超时阶段打 `LINGXI_SERVICE_SHUTDOWN_TIMEOUT phase=… deadline_ms=…`
  到 stderr + tracing error；超时后**继续**剩余阶段（记录仍被移除——超时的 DB 关闭不得让
  home 看起来仍被持有），并以独立退出码 6 如实上报。

## 5. 关键调用链

```
启动：main.rs → parse_cli/config（--shutdown-timeout-ms）→ prepare_layout → instance::acquire（T02 锁）
  → epoch::coordinate_data_epoch_startup（读 data-epoch-transition.json 优先 → data-epoch.json；
     corrupt/incomplete/higher/unstamped-with-data 全部拒绝；fresh home 写 v2 epoch-1 印章）
  → ServiceState::bootstrap（auth → runs.db 打开：迁移收据+integrity_check → seed → EventService）
  → bind/publish → READY
运行：POST execute → sessions::execute_for → StoragePort（T04 同事务终态+事件）→ 提交后发布（T05）
备份：RunDatabase::backup_to → 单写者队列作业（[TRUNCATE checkpoint] → Online Backup API →
  目标 integrity_check → fsync → rename 晋升 → manifest 原子写）→ BackupOutcome{sha256,...}
恢复：restore_backup（manifest 哈希对账 → 拷贝 → 副本哈希）→ RunDatabase::open（收据+integrity 全闸）
关闭：SIGTERM → ws 广播 + axum 排干 → graceful_shutdown（ws_drain → storage_close=排干+checkpoint+join
  → record_cleanup）→ exit 0/4/5/6
```

## 6. 测试列表（层级）

1. **单元（lib 内，20 新增）**：`epoch.rs` 16（时间戳 RFC3339 读写、fresh 盖章幂等、R7/R8/R9/R10/R11
   变体拒绝+证据+字节不变、higher stamp、外来数据/符号链接/tmp 半写拒、committed-tail 清理、
   barrier 无日志、印章写失败无残片、**谎报字符串冻结测试**）；`shutdown.rs` 3（退出码优先级、
   WS 排干超时、会话 guard 全路径）；`config.rs` 1（--shutdown-timeout-ms 严格性）。
2. **adapters 集成（真实文件，13 新增）**：`backup_restore.rs` 11——A11 主线（WAL 非空前值 →
   在线备份 → 恢复目录全闸重开 → 逻辑 dump 逐行相等）、纯在线 API（无预 checkpoint）变体、
   回滚事务不出现在备份、并发写者下备份=队列处理前缀的完整快照（REPAIR-R1 后的事务一致性
   语义：id 前缀连续、run 行⇒start 事件+attempt 同事务在、completed⇔done 事件对拍、
   event 总数=每 run 1 start+每 completed 1 done、in-flight run 至多 1 个且必为前缀末位、
   流内 seq 1..M 连续无洞——确定性，交错位置不再断言）、**确定性用例：started-but-uncommitted
   run 的备份=完整前缀**（固定交错实证 in-flight 快照语义，REPAIR-R1 新增）、不可写目标
   失败零产物、篡改备份拒绝恢复、拒绝覆盖目标、stem 路径校验、**runs.db/-wal/-shm 0600
   （F04）**、**备份/恢复产物 0700/0600（REPAIR-R1 F02，预建 0777 目录验证显式收紧而非
   umask 运气）**；`backup_faults.rs` 2（RLIMIT_FSIZE：checkpoint 期故障、拷贝期故障，均为真实
   EFBIG，故障后零冒充产物；独立测试二进制，T04 先例）。
3. **service 集成（真实库+真实锁文件，5 新增）**：`shutdown_coordinator.rs`——干净关闭 exit 0
   且记录移除、**退出超时**（外部 SQLite 连接持 WAL 写锁 → deadline 先到、marker、继续记录
   清理）、双关→exit 5、记录损坏→exit 4 且不可验证记录不删、WS 计数排干。
4. **二进制级（真实进程）**：`r02_t06_backup_restore.sh`（S1 A11 全链含 kill -9 后 WAL-resident
   备份/恢复/逻辑对账、S2 优雅停 exit 0、S3 备份中断零产物、S4 开 WS 关停 1001/exit 0）；
   `r02_t06_recovery_drill.sh`（A12 + 4 变体演练 v1–v8，见 §7.2/§7.3）。

## 7. 验收场景逐项结果（REQUIRED）

全部命令：剥代理 + `CARGO_NET_OFFLINE=true` + `--locked` + `CARGO_TARGET_DIR=/tmp/rust-target-r02-t06`；
tested SHA `2aa80f506…` + 未提交改动；macOS 27.0 arm64；合成 /tmp home。

### 7.1 R02-A11 WAL 状态备份可恢复 — PASS（本执行者证据；独立验收归总控）

| # | 覆盖 | 命令 | 退出码 | 结果 | 证据 |
|---|---|---|---|---|---|
| A11-1 | 库内层：WAL 非空前值（10M 页 autocheckpoint 抑制）→ 在线备份 → 恢复目录全闸重开 → 逻辑 dump 相等、3 runs 全 complete、6 事件 | `cargo test -p lingxi-adapters --test backup_restore -- a11_online --locked` | 0（9 passed 整套） | WAL-resident 已提交数据全部进入备份；恢复件与源逐行相等 | `gates/cargo-test-workspace.log`；测试体 `a11_online_backup_captures_wal_resident_committed_data_and_restores` |
| A11-2 | 纯在线 API（无预 checkpoint）同样完整 | 同上 `a11_pure_online` | 0 | 恢复件逻辑内容==源 | 同上 |
| A11-3 | 事务边界：回滚事务不在备份内（真实失败事务） | 同上 `a11_transaction_boundary` | 0 | restored dump 无 rolled-back-run；两个已提交 run 完整 | 同上 |
| A11-4 | 并发写者下备份=队列处理前缀的完整快照（事务边界正确；REPAIR-R1 语义修正，见 §12.1） | 库内：`cargo test -p lingxi-adapters --test backup_restore -- a11_backup_during_concurrent --locked` **×30 连跑全绿**（`repair-r1/f01-test-25x.log`）；固定交错实证：`-- a11_backup_of_a_started_but_uncommitted`（exit 0） | 0（30/30） | 快照含合法 in-flight run 时断言仍全绿：id 前缀连续、run 行⇒start+attempt 同事务在、completed⇔done 对拍、event 总数=每 run 1 start+每 completed 1 done、in-flight≤1 且为前缀末位、seq 1..M 连续；修复前该测试 6 failed/4 ok（10 连跑，REVIEW-R1 F01 实测） | `repair-r1/f01-test-25x.log`；`repair-r1/gates-cargo-test-workspace.log`；测试体 `a11_backup_during_concurrent_commits_is_a_consistent_prefix`、`a11_backup_of_a_started_but_uncommitted_run_is_a_complete_prefix` |
| A11-5 | 备份哈希证据链 | 同上 + manifest 断言 | 0 | manifest sha256==outcome.sha256、integrity=ok | 同上 |
| A11-6 | **二进制级全链**：服务写 3 run → WAL=292,552 B 非空 → kill -9 → inspect backup（真实 Online API）→ restore-verify（恢复件过收据+integrity 开库）→ 源/恢复 dump 逐行 diff 相等、3 run/6 事件全 complete | `bash scripts/rust-tauri/r02_t06_backup_restore.sh artifacts/rust-tauri/R02/T06` | 0（ALL GREEN） | S1–S4 全过；kill -9 已确认生效（脚本断言进程死亡） | `backup-restore/summary.txt`、`s1-wal-size.txt`、`s1-backup-outcome.json`、`s1-restore-outcome.json`、`s1-source-dump.jsonl` vs `s1-restored-dump.jsonl`、`s1-*hash*.txt` |
| A11-7 | 备份中断不冒充成功 | 库内：`backup_into_unwritable…`；RLIMIT：`backup_faults.rs` 2 测试；二进制 S3（chmod 0555 目标父目录） | 0 | 三层全部显式失败 + 零产物（无最终名/无 manifest/无 partial） | `backup-restore/s3-*`；`gates/cargo-test-workspace.log` |

### 7.2 R02-A12 损坏库不自动丢弃 — PASS（本执行者证据；独立验收归总控）

| # | 形态 | 操作 | 退出码 | 结果 | 证据 |
|---|---|---|---|---|---|
| A12-1 | torn 印章 + 可读高位过渡日志（R7 镜像） | `recovery-drill.sh` v1：启动服务 | 2 | `LINGXI_DATA_EPOCH_TRANSITION_INCOMPLETE reason=corrupt-stamp` + `evidence=transitionJournal(fromEpoch=1,toEpoch=2,phase=barrier_raised…)`；**无**"no higher-epoch evidence"；data-epoch*.json 前后 sha256 一致；未写 local-token.json（闸先于一切写入） | `recovery-drill/summary.txt`、`v1.{out,err}`、`v1-hashes-{before,after}.txt` |
| A12-2 | torn 过渡日志（R9 镜像） | v2 | 2 | reason=corrupt-journal；文件字节不变 | `v2.*`、`v2-hashes-*` |
| A12-3 | 缺失印章 + 可读 barrier_raised 日志（R10 镜像，**Node 实测 fail-open 且写入 55 条**） | v3 | 2 | reason=corrupt-transition + 证据 1→2 如实在文；不发明印章；文件字节不变 | `v3.*`、`v3-hashes-*` |
| A12-4 | garbage 主文件 | v5 | 2 | `not a database`/Corrupted 拒绝；损坏文件前后 sha256 一致；**未创建空库顶替** | `v5.*`、`v5-hashes-*` |
| A12-5 | 篡改迁移收据（指纹） | v7 | 2 | SchemaTampered 拒绝；文件前后 sha256 一致 | `v7.*`、`v7-hashes-*` |
| A12-6 | torn WAL 尾（INFO 行，如实记录） | v6 | 0 | SQLite 按设计恢复：非完整尾帧=事务未提交；恢复视图经 dump 断言事务一致（runs=1/events=1 无半终态）；优雅停 exit 0 | `v6-recovered-dump.jsonl`、`summary.txt` v6 行 |
| A12-7 | WAL 魔数损坏（INFO 行） | v8 | 0 | 观察到按设计恢复（exit 0 ready=1），graceful stop 后仅存 runs.db | `v8-seed.*`、`v8.err`、summary v8 行 |

### 7.3 PROD-DEFECT-1 4 变体演练（Rust 实现重跑）— PASS

| 变体 | Node 实测（ADR-004 §2） | Rust 新栈（本任务） |
|---|---|---|
| v1 torn 印章 + 合法高位日志 | R7：exit 0 fail-open，写入 55 条，谎称无高位证据 | **exit 2 拒启**，corrupt-stamp，证据 1→2 如实在文，文件字节不变 |
| v2 torn 日志 | R9：exit 0 fail-open，写入 55 条 | **exit 2 拒启**，corrupt-journal，文件字节不变 |
| v3 缺失印章 + 合法高位日志 | R10：exit 0 fail-open，写入 55 条，谎称无高位证据 | **exit 2 拒启**，corrupt-transition，证据 1→2 如实在文，不发明印章，文件字节不变 |
| v4 健康对照 | 正常启动 | 正常启动（fresh home 盖 v2 epoch-1 印）+ 优雅停 exit 0 |

修复前基线（HEAD 无闸二进制）对 v3 同型 home 实测 READY + HTTP 200 + 1 run/2 events 落库
——证明缺陷面真实存在于新栈、修复以闸落地并闭环：`artifacts/rust-tauri/R02/T06/prod-defect-1/baseline-no-gate/`。
RISK_REGISTER `RR-T07-PROD-DEFECT-1` 的 R02 截止面在**新实现侧**关闭（Node 生产代码按红线
不改；旧程序侧残余风险仍由 ADR-004 D2 分离根/指针方案覆盖，登记状态更新归总控）。

### 7.4 故障注入矩阵（任务书步骤 4）

| 注入点 | 方法 | 结果 | 证据 |
|---|---|---|---|
| checkpoint | RLIMIT_FSIZE 低于主文件+512B（真实 EFBIG，独立测试二进制） | 备份显式失败（DiskFull/IO），目标目录零产物 | `backup_faults::checkpoint_fault…`；`gates/cargo-test-workspace.log` |
| journal | 过渡日志 torn（盘上损坏形态） | corrupt-journal 拒启，文件不变 | drill v2 + epoch 单测 |
| 状态文件写入 | home 只读（chmod 0555）注入印章写失败 | stamp-write-failed 显式拒启，无半张印章、无 tmp 残片 | epoch 单测 `stamp_write_failure…` |
| 备份中断 | RLIMIT 半尺寸注入拷贝期 EFBIG + 不可写目标 + 二进制 S3 | 三层显式失败，零冒充产物 | `backup_faults::interrupted…`、A11-7 |
| 退出超时 | 外部连接持 WAL 写锁 + deadline 120ms | `LINGXI_SERVICE_SHUTDOWN_TIMEOUT phase=storage_close`、继续记录清理、exit 6、phase 被 deadline 界定（<400ms 实测） | `shutdown_coordinator::storage_close_timeout…` |

### 7.5 门禁与回归（全部亲手执行）

| 门禁 | 命令 | 退出码 | 证据 |
|---|---|---|---|
| fmt | `cargo fmt --all -- --check`（锁定 1.98.1） | 0 | `repair-r1/gates-fmt-clippy.log` |
| clippy | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | `repair-r1/gates-fmt-clippy.log` |
| 全量测试 | `cargo test --workspace --locked --no-fail-fast` | 0（**231 passed / 0 failed，37 套件**；REPAIR-R1 后含 backup_restore 11 测试与 T01–T05 全部既有回归。**更正声明：首轮报告的「229/0」为单次抽样——修复前该集合含 F01 非确定性测试，10 连跑 6 failed/4 ok，229/0 不可复现；修复+2 新测试后为 231/0**） | `repair-r1/gates-cargo-test-workspace.log` |
| 冻结契约 | `scripts/rust-tauri/r01-t02-check-generated.sh` | 0 | `gates/generated-contracts-check.log`；REPAIR-R1 复跑同 0：`repair-r1/gates-generated-contracts-check.log` |
| 依赖/所有权 | `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0（RESULT: OK） | `gates/ownership-check.log`；REPAIR-R1 复跑同 0：`repair-r1/gates-ownership-check.log` |
| T01–T05 回归 8 脚本 | smoke/boundary/dual-instance/path-priority/env-token/auth-matrix/storage-tx/events-matrix（证据定向 `gates/reg-*/`，未覆写 T01–T05 历史 artifacts） | 0 ×8 | `gates/reg-r02_t0*…log` + `gates/reg-*/`；REPAIR-R1 复跑 0 ×8（证据定向 `repair-r1/reg-*/`，`git status` 核验 T01–T05 已提交证据零覆写）：`repair-r1/gates-reg-*.log` |
| 审计封印族 | `npm test -- --run tests/post-verification-audit-seal.test.ts` | 1（**1 failed / 2 passed，预存在红**：seal 坐标落后于已授权 R02 提交；未触碰坐标/白名单） | `gates/audit-seal-baseline.log` |
| Cargo.lock | `git diff -- rust/Cargo.lock` | 仅 +1 行 `"rusqlite",`（lingxi-service dev-dep 引用边） | — |

## 8. 未验证内容 / 环境限制

1. 跨平台：全部证据仅 macOS arm64。RLIMIT_FSIZE/SIGXFSZ、文件 0600、目录 fsync、
   `File::try_lock` 行为等 unix 设施在 Windows 的等价未做（同 T01–T05 平台边界，代码内已标注）。
2. 「torn WAL」「WAL 魔数损坏」两种形态在 bundled SQLite 上实测为**按设计恢复**（v6/v8 INFO
   行如实记录，含恢复视图事务一致性断言）；A12 的「明确失败」面由 v1/v2/v5/v7 四种拒绝形态
   承担。若独立验收要求物理介质级损坏（hdiutil 小盘、块层损坏），需另行补做。
3. 备份机制当前为库 API + 证据 CLI（inspect backup/restore-verify），尚无 HTTP 管理面/定时
   策略（产品化归后续阶段；本任务交付=机制与语义）。
4. 退出超时的二进制级复现依赖外部锁注入（库内测试完成真实注入）；未在二进制脚本中构造
   挂起场景（默认 10s deadline 的正常路径已由 S2/S4 覆盖 exit 0）。
5. `lingxi_ALLOW_DATA_DOWNGRADE` 显式降级逃生口**未**在新栈实现（fail-closed 优先；R02 无
   降级支持，语义=一律拒启，文本明示「升级」）——若产品面需要该维护逃生口，属后续授权决策。
6. 审计封印 1 红为预存在坐标问题；本任务未提交，tested SHA 为「基线 + 未提交改动」。

## 9. 已知风险

1. **unstamped-home-with-data 语义分叉（有意为之，需总控确认归属）**：Node 对未盖章 home
   在 epoch 1 会 `adopted-legacy` 自动盖章；Rust 闸对含外来内容的未盖章 home 一律拒启
   （认领=显式 R08 切换动作）。这是更严格的方向，但意味着同一 home 在 R08 之前只能由
   旧程序（或显式盖章维护流程）接管——R08 切换工具链须提供显式盖章动作。
2. integrity_check 在每次开库全量执行：R02 规模即时完成；大库上属启动时延成本，后续可按
   体积分级（quick_check/interval），须走显式决策不静默降级。
3. 备份作业经单写者队列执行=备份期间 DB 作业背压（QueueFull 上浮 503）；大库备份时长会
   放大该窗口，R03+ 引入在线请求后需按负载复核（与 T07 有界资源衔接）。
4. WS 会话在广播后直接 close(1001) 结束订阅；订阅恢复语义=客户端重连走快照重建（T05
   A10 已证），无事件丢失（关键事件持久于 key_events）。
5. `run_fact_summary`/`logical_dump` 是证据读缝（经队列、只 SELECT）；误用为写路径会绕过
   事务辅助——与 T04 `query_one_text` 同款风险，T08 xtask 门禁可一并加静态检查。
6. **值级位翻转不可检（REVIEW-R1 F03 INFO 登记）**：`PRAGMA integrity_check` 只验结构不验值
   ——SQLite 无页校验和的固有属性，页内值字节/空闲区的单位翻转可被判 ok 并正常开库（REVIEW-R1
   实测）。A12 语料是「可识别的损坏测试库」：全部**结构可识别**形态（页类型翻转、截断、garbage
   主文件、收据篡改）均被拒绝且文件字节不变；值级位腐需端到端哈希清单基线，活库不存在该基线
   （Node 栈同样不覆盖）。登记为已知边界，覆盖它属后续显式决策，不在本任务范围。
7. **备份/恢复产物权限（REPAIR-R1 F02）**：备份=run 事实的外置副本，产物已收紧 0600/0700
   （unix，显式收紧、失败响亮；非 unix 平台无权限位，为模块文档明示的 no-op，保护依赖宿主
   目录约定）。库内权限断言测试用预建 0777 目录证明收紧是显式行为而非 umask 运气，但若运行
   环境 umask=077，该测试对「回归后忘记收紧」的检出力会下降（代码与二进制探针仍为 0600/0700）。

## 10. git status --short 全文（报告时点）

```
 M rust/Cargo.lock
 M rust/crates/lingxi-adapters/Cargo.toml
 M rust/crates/lingxi-adapters/src/bin/lingxi-storage-inspect.rs
 M rust/crates/lingxi-adapters/src/storage/migrations.rs
 M rust/crates/lingxi-adapters/src/storage/mod.rs
 M rust/crates/lingxi-adapters/src/storage/queue.rs
 M rust/crates/lingxi-adapters/src/storage/run_store.rs
 M rust/crates/lingxi-service/Cargo.toml
 M rust/crates/lingxi-service/src/config.rs
 M rust/crates/lingxi-service/src/lib.rs
 M rust/crates/lingxi-service/src/main.rs
 M rust/crates/lingxi-service/tests/auth_matrix.rs
 M rust/crates/lingxi-service/tests/event_subscription.rs
 M rust/crates/lingxi-service/tests/instance_lifecycle.rs
 M rust/crates/lingxi-service/tests/service_health.rs
 M rust/crates/lingxi-service/tests/service_persistence.rs
?? artifacts/rust-tauri/R02/T06/
?? docs/rust-tauri/R02/R02-T06_REPORT.md
?? docs/rust-tauri/R02/R02-T06_REVIEW_R1.md
?? rust/crates/lingxi-adapters/src/storage/backup.rs
?? rust/crates/lingxi-adapters/tests/backup_faults.rs
?? rust/crates/lingxi-adapters/tests/backup_restore.rs
?? rust/crates/lingxi-service/src/epoch.rs
?? rust/crates/lingxi-service/src/shutdown.rs
?? rust/crates/lingxi-service/tests/shutdown_coordinator.rs
?? scripts/rust-tauri/r02_t06_backup_restore.sh
?? scripts/rust-tauri/r02_t06_recovery_drill.sh
```
（`docs/rust-tauri/R02/R02-T06_REPORT.md` 为本文件，亦为未跟踪新文件。）

## 11. 推荐独立验收重点

1. **4 变体演练复跑**：`bash scripts/rust-tauri/r02_t06_recovery_drill.sh /tmp/任意目录`——
   逐 variant 核对 stderr 的 marker + evidence 行（v1/v3 必须含 `epochs 1→2`），并对 v3 home
   自行以 sqlite3/python 确认「无印章、无 runs.db、日志字节不变」；对照
   `prod-defect-1/baseline-no-gate/` 理解修复前 fail-open 面。
2. **A11 复跑 + 动手**：`bash scripts/rust-tauri/r02_t06_backup_restore.sh /tmp/任意目录`；
   用 `sqlite3` 独立抽查恢复件行数；把 `s1-backup-outcome.json` 的 sha256 与备份文件重算值
   对账；对备份文件翻一个字节后跑 `restore-verify` 应拒绝（哈希对账真实生效）。
3. **备份中断防线**：`cargo test -p lingxi-adapters --test backup_faults`；可临时调大
   RLIMIT 余量验证 "FAULT INJECTION FAILED" 防线会响（证明故障真实生效）。
4. **退出超时**：`cargo test -p lingxi-service --test shutdown_coordinator`；核对
   `LINGXI_SERVICE_SHUTDOWN_TIMEOUT` marker、exit 6、记录仍被移除。
5. **F04**：任一 freshly-opened runs.db/-wal/-shm 的 `stat -f %Lp` 应为 600。
6. **门禁复跑**：fmt/clippy/231 测试/契约/所有权/8 回归脚本；封印 1 红为预存在坐标问题。
7. §9.1 unstamped-home 语义分叉（与 Node adopted-legacy 的差异）是否按 R08 计划接管。

## 12. REPAIR-R1｜第 1 轮验收 FAIL 的修复记录（REPAIR-R02-T06-R1）

输入：`R02-T06_REVIEW_R1.md`（VERDICT FAIL：F01 BLOCKING + F02 MINOR + F03/F04 NON-ISSUE）。
修复面：F01=测试断言语义 + 报告数字；F02=backup.rs 产物权限 + 断言测试 + 登记。
**产品实现零缺陷变更**（与 REVIEW-R1 的裁定一致；§12.3 为修复代理的独立验证结论）。
本轮未触碰：epoch/shutdown/config、Node 生产代码、T01–T05 已提交证据（`git status` 核验
`artifacts/rust-tauri/R02/` 下 T06 之外零 diff）、任务书、审计封印坐标/白名单。

### 12.1 F01（BLOCKING）修复：并发备份测试断言语义

- **根因确认（先重现）**：`a11_backup_during_concurrent_commits_is_a_consistent_prefix`
  10 连跑 6 failed / 4 ok（失败样本 event_count=13/25/29/25，均为奇数=恰好一个未配对的
  start 事件），复现 REVIEW-R1 定频。
- **修复内容**（`tests/backup_restore.rs`，仅测试）：
  1. 重写并发用例断言为「备份=某个队列处理前缀的完整快照」语义（论证见 §12.4），删除
     `event_count == run_count * 2` 的「无 in-flight run」误断言；交错位置不再断言。
  2. 确定性结构化：writer 在 w00 完整提交后经 oneshot 信号，备份作业只在收到信号后提交
     （FIFO 已执行完 w00 两作业 ⇒ run_count ≥ 1 有保证而非碰运气）；30 连跑全绿
     （`repair-r1/f01-test-25x.log`）。
  3. 新增固定交错确定性用例 `a11_backup_of_a_started_but_uncommitted_run_is_a_complete_prefix`：
     两次完整提交 + 一次仅 start + 备份 ⇒ 恢复件逻辑 dump 与源逐行相等（w02 以 'running'
     原样保留、有 start 事件与 attempt、无 done 事件）。该用例同时是 §12.3 独立验证的实证。
- **报告更正**：§7.1 A11-4 行与 §7.5 全量测试行已按修复后实测更正（229/0 → 231/0，37 套件），
  并注明「首轮 229/0 为单次抽样，修正前该集合非确定性」。
- **同类排查**：全仓 grep 计数型一致性断言——`event_subscription.rs` a09 的 `TOTAL_EVENTS`
  在所有 writer `.await` 结束后的收敛态断言（语义正确）；`service_persistence.rs`/T04 顺序
  用例无并发快照断言。本 suite 内无其他同型错误。

### 12.2 F02（MINOR）修复：备份/恢复产物权限

- `backup.rs`：新增 `tighten_owner_only`（目录 0700/文件 0600，幂等、失败响亮；非 unix
  no-op 已在模块文档明示）；接入 4 处——`backup_database` 的目标目录（创建后、任何产物落盘前）
  与 `.partial` 文件（`Connection::open` 后）、manifest tmp 文件（写入前）、`restore_backup`
  的目标目录与恢复出的 db 文件（copy 后、fsync 前；收紧失败删除副本并响亮报错）。
- 测试：`backup_and_restore_artifacts_are_owner_only`（预建 0777 目录 ⇒ 断言的是显式收紧
  而非 umask 运气；backup 目录/db/manifest = 0700/0600/0600，restore 目录/文件 = 0700/0600）。
- 二进制级探针（真实 inspect CLI，umask 022）：备份目录/db/manifest 与恢复目录/文件全部
  0700/0600，预建 0777 目录亦被收紧：`repair-r1/f02-permissions-binary-probe.log`。

### 12.3 修复代理对「产品无缺陷」裁定的独立验证结论：成立

1. **代码级**：`record_run_started` 是单个 `with_write_txn`（runs 行 + run_attempts 行 +
   start 事件 + last_event_seq），`commit_run_outcome` 是另一个事务；备份经
   `RunDatabase::backup_to`→`queue.submit` 与两者同队 FIFO。备份作业落在 start 与 commit
   之间时，快照中的 in-flight run 是**已提交事务的完整产物**（start 事务三件套齐全），
   不是半事务；Online Backup API 在停写者（单写者 worker 执行备份作业）+ 已提交 WAL 帧上
   取快照，事务一致性由 SQLite 保证。
2. **实证级**：修复前失败样本的 event_count 全为奇数（2K+1 形态=恰一个 start-only run），
   且 id 前缀断言在失败样本中仍通过（前缀连续性未被破坏）——与「备份落在 start/commit 之间」
   唯一相符。新增固定交错用例证明该状态的备份/恢复**逐行无损**。
3. **升级核查**：未发现产品侧需要升级的缺陷（备份机制、恢复闸、事件/attempt 同事务完整性
   均与裁定一致）。缺陷面=测试断言语义 + 报告「229/0」不可复现，与 REVIEW-R1 一致。

### 12.4 新断言语义的精确论证

Schema 事实（`migrations.rs` v1 + `run_store.rs`）：start 事务=runs 行('running') +
run_attempts 行 + key_events 行(`event_id='{run}-start'`) + last_event_seq，单事务提交；
commit 事务=终态 UPDATE(runs.status) + key_events 行(`event_id='{run}-done'`)，单事务提交。
单写者 FIFO 意味着任一快照点 = 某个作业前缀的执行结果。因此对恢复件成立且**对所有交错成立**：

- **id 前缀连续**：writer 按序 start/commit w00..w19，快照⊇已执行前缀 ⇒ 恢复件 ids 必为
  w00..wK 连续前缀（`run_fact_summary` 按 run_id 排序核对）。
- **run 行 ⇒ start 事件 + attempt 行同在**：三者在同一 start 事务；快照含行则该事务已提交，
  事件与 attempt 必然同在（无半 start）。
- **completed ⇔ done 事件**：status 更新与 done 事件在同一 commit 事务，二者只能同现或同缺
  （无半 commit）；本测试 writer 只产生 'running'/'completed' 两态。
- **event 总数 = run 数 + completed 数**：每 run 恰 1 个 start 事件，completed 另有恰 1 个
  done 事件（in-flight run 贡献 1 个事件——这就是 7=3×2+1 的正确读法）。
- **in-flight ≤ 1 且必为前缀末位**：writer 在 start(i+1) 前必先 commit(i)（顺序 await），
  任意前缀最多悬着最后一个 start。
- **流内 seq 连续 1..M**：seq 在提交事务内按 MAX(seq)+1 分配（UNIQUE(stream_id, seq) 背书），
  本测试无删除路径，故 COUNT(*)=MAX(seq)=event_count；撕裂快照会留下 seq 洞。

### 12.5 REPAIR-R1 验证命令与退出码（全部本代理亲手执行）

工具链 1.98.1（rustup 锁定，与首轮一致）；全部命令剥代理 + `--locked` +
`CARGO_TARGET_DIR=/tmp/rust-target-r02-t06-repair` + `CARGO_NET_OFFLINE=true`；
tested SHA = `2aa80f506464e6cba7ec001b987d649e1164faaf`（= HEAD，未变）+ 当前未提交改动。

| # | 命令 | 退出码 | 结果/证据 |
|---|---|---|---|
| R1 | 修复前重现：`cargo test -p lingxi-adapters --test backup_restore -- a11_backup_during_concurrent` ×10 | 混合 | **6 failed / 4 ok**（event_count 13/25/29/25 奇数形态）；过程输出未存档（修复前状态，与 REVIEW-R1 V5 同型复现） |
| R2 | 同测试 ×30（修复后，`:a11_backup_during_concurrent` 过滤单测） | 0 ×30 | **30/30 全绿**：`repair-r1/f01-test-25x.log` |
| R3 | `cargo test -p lingxi-adapters --test backup_restore --locked`（全套 11 测试） | 0 | 11 passed / 0 failed（含 2 个新测试 + 9 个既有） |
| R4 | `cargo fmt --all -- --check` | 0 | `repair-r1/gates-fmt-clippy.log` |
| R5 | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 同上 |
| R6 | `cargo test --workspace --locked --no-fail-fast` | 0 | **231 passed / 0 failed，37 套件**：`repair-r1/gates-cargo-test-workspace.log` |
| R7 | `bash scripts/rust-tauri/r02_t06_backup_restore.sh artifacts/rust-tauri/R02/T06/repair-r1` | 0 | S1–S4 ALL GREEN（WAL-resident 备份→恢复→dump 逐行相等；S3 零产物；S4 close 1001/exit 0）：`repair-r1/backup-restore-script.log` + `repair-r1/backup-restore/` |
| R8 | `bash scripts/rust-tauri/r02_t06_recovery_drill.sh artifacts/rust-tauri/R02/T06/repair-r1` | 0 | v1–v8 全绿（v1/v2/v3/v5/v7 exit 2 字节不变；v6/v8 INFO）：`repair-r1/recovery-drill-script.log` + `repair-r1/recovery-drill/` |
| R9 | F02 二进制探针（inspect backup/restore-verify + stat %Lp） | 0 | 全部 0700/0600（含预建 0777 目录被收紧）：`repair-r1/f02-permissions-binary-probe.log` |
| R10 | T01–T05 回归 8 脚本（证据定向 `repair-r1/reg-*/`） | 0 ×8 | smoke/boundary/dual/f01/path/auth/storage-tx/events 全绿：`repair-r1/gates-reg-*.log` |
| R11 | `scripts/rust-tauri/r01-t02-check-generated.sh` | 0 | `repair-r1/gates-generated-contracts-check.log` |
| R12 | `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0（RESULT: OK） | `repair-r1/gates-ownership-check.log` |
| R13 | `git status --porcelain artifacts/rust-tauri/R02/`（滤除 T06） | — | 空 → T01–T05 已提交证据零覆写，无需 git checkout 还原 |
| R14 | `git diff 2aa80f506 --name-only -- core/ server/ desktop/ shared/ tests/` | — | 空 → Node 生产/测试零改动（红线持续遵守） |
| R15 | `git diff --numstat -- rust/Cargo.lock` | — | 仍恰 +1 行 `"rusqlite",`（与首轮声明一致，本轮零变化） |

未执行项：REVIEW-R1 的 V11/V12/V13/V15/V16/V17 对抗自构脚本未逐条重放（首轮已证、本轮改动
不触及其机制面——备份哈希/拒绝路径与 epoch 闸零字节改动；R7/R8 脚本内含同型断言全绿）；
审计封印族未复跑（预存在红与本轮无关，首轮 V19 结论仍有效）。

### 12.6 证据清单（artifacts/rust-tauri/R02/T06/repair-r1/）

`f01-test-25x.log`（30 连跑）、`gates-cargo-test-workspace.log`（231/0 全量）、
`gates-fmt-clippy.log`、`backup-restore-script.log`、`recovery-drill-script.log`、
`f02-permissions-binary-probe.log`、`gates-reg-*.log` ×8 + `reg-*/`（回归证据）、
`gates-generated-contracts-check.log`、`gates-ownership-check.log`、
`backup-restore/` 与 `recovery-drill/`（两个脚本的产物证据）。

— 执行者声明：以上命令、退出码、证据路径均来自本机真实执行；未执行项已如实列入 §8。
READY_FOR_REVIEW。

— REPAIR-R1 声明（REPAIR-R02-T06-R1）：§12 的命令、退出码、证据均来自本代理本机真实执行；
F01/F02 已修复，F03 已按 INFO 登记（§9.6），产品实现零缺陷变更，未 commit/push。
READY_FOR_REVIEW。
