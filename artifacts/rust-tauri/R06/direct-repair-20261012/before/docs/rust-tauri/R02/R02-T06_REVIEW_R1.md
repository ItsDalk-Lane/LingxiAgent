# R02-T06｜备份、关闭与启动恢复 — 独立对抗性验收 REVIEW（R1）

- 验收代理：REVIEWER-R02-T06-R1（第 1 轮；非实现者，未参与执行，不信任执行者声明）
- 日期：2026-09-26
- 候选：TASK_BASE_SHA `2aa80f506464e6cba7ec001b987d649e1164faaf`（= 本机 HEAD）+ 未提交工作树
- 工具链：rustup 代理锁定 1.98.1（仓库根 `rust-toolchain.toml`，实测 `~/.cargo/bin/cargo --version` = 1.98.1）；
  全部 cargo 命令 `--locked` + `CARGO_NET_OFFLINE=true` + 专属 `CARGO_TARGET_DIR=/tmp/rust-target-r02-t06-review`；
  网络命令一律剥代理。复审专属验证目录 `/tmp/r02t06-review/`（零仓库写入，本文件除外）。
- 方法论说明：本会话的大文件全文读取展示层出现标识符级乱码（同一类型名两种拼写），
  已用三种独立信道否定其为候选缺陷：grep 精确名匹配正常、`cargo clippy -D warnings` 通过（编译器读的是真实字节）、
  测试真实执行。本报告所有源码判断以 grep 定窗 + 编译/测试真实结果为准。

## VERDICT: FAIL（1 BLOCKING / 1 MINOR / 2 NON-ISSUE；根因单一、修复面小，其余全部真实通过）

FAIL 仅由 F01（验收关键测试非确定性失败 + 门禁声明不可复现）触发。A11/A12 的机制、
PROD-DEFECT-1 闭环、关闭协调器、架构接线、回归与 Scope 均经本代理独立复跑为真。

---

## 1. 发现

### F01｜BLOCKING｜A11-4 证据测试 `a11_backup_during_concurrent_commits_is_a_consistent_prefix` 非确定性失败；"cargo test --workspace → 229/0" 门禁声明不可复现

- 位置：`rust/crates/lingxi-adapters/tests/backup_restore.rs:324`（断言行）；对应执行报告 §7.1 A11-4 行与 §7.5「全量测试 exit 0（229 passed / 0 failed）」。
- 证据（本代理实测，锁定 1.98.1、--locked、专属 target dir）：
  - 全量 `cargo test --workspace --locked`：`a11_backup_during_concurrent_commits_is_a_consistent_prefix ... FAILED`，panic 断言
    `left: 7, right: 8`（`event_count == run_count * 2`），`TEST_EXIT:101`，suite 内 8 passed 1 failed，workspace 中止。
  - 单测连跑 10 次：**6 次 FAILED / 4 次 ok**（`/tmp/r02t06-review/` 下 10 连跑记录）。
  - `--no-fail-fast` 全量：37 个 result 块合计 **228 passed + 1 failed = 229**（失败即本测试）——总数与执行者声称的 229 一致，但"0 failed"不可复现。
- 问题（根因在测试不变量，不在产品实现）：
  `record_run_started`（run 行 + attempt 行 + start 事件 + last_event_seq，同一 `with_write_txn`）与
  `commit_run_outcome`（终态事件 + 状态更新，另一事务）是**两次独立的队列作业**；备份作业经同一 FIFO 队列提交
  （`RunDatabase::backup_to` → `queue.submit`）。当备份作业被入队在两次作业之间时，快照**合法地**包含一个
  in-flight run（有 run 行 + start 事件、无终态）——这正是 7=3×2+1 的观测。SQLite 事务一致性（无半事务）由
  同事务写入保证，从不被破坏；被测断言 `event_count == run_count * 2` 把「无半事务」误写成「无 in-flight run」，
  该命题在实现设计上就不成立。
- 为什么违反任务：验收场景 R02-A11 为 REQUIRED，其证据行 A11-4 声称 `退出码 0` 且整套测试为可复现证据；
  `05 验收协议`要求登记的命令/退出码可复核。该测试 60% 失败率使 `cargo test --workspace --locked` 门禁（报告 §7.5、
  也是 R02-T08 xtask 将复用的同一集合）不可作为绿灯声明。
- 后果：本 Task 的全量测试门禁声明不实（非故意造假——执行者单次运行确实可以通过）；T08 门禁与后续每轮
  CI 式复跑都会随机红；A11-4 证据行不可信。
- 同类路径：同一断言模式的测试（本 suite 内无其他）；T04 `storage_tx` 与 fault 测试不受影响（已复跑全绿）。
- 修复要求（仅测试 + 报告，产品代码零改动）：
  1. 将断言改为事务一致性语义：每 run 事件数 ∈ {1,2}；run 行存在 ⇒ start 事件存在（同事务）；
     status=completed ⇒ done 事件存在；status≠completed ⇒ 不存在 done 事件（对拍同事务保证）；id 前缀连续性保留。
  2. 重跑受影响整组：`cargo test -p lingxi-adapters --test backup_restore --locked`（≥10 连跑全绿）+ 全量
     `cargo test --workspace --locked` + 报告 §7.1 A11-4 与 §7.5 数字更正。
  3. 报告更正处注明「首轮 229/0 为单次抽样，修正前该集合非确定性」。
- 必须重跑的测试：`backup_restore`（含 10 连跑）、`cargo test --workspace --locked`、`r02_t06_backup_restore.sh`。

### F02｜MINOR｜备份产物权限 0644/0755，与 T04-F04 的 0600 纵深不一致，报告未登记

- 位置：`rust/crates/lingxi-adapters/src/storage/backup.rs`（`backup_database`：`create_dir_all` + 默认 umask；
  `restore_backup` 的 `fs::copy` 同理）。
- 证据（本代理实测）：inspect CLI 备份到调用方未预设目录时产物
  `runs.db = 644`、`runs.manifest.json = 644`、目录 `755`（umask 022）；而活库已被 T06 收紧为 0600
  （`queue.rs::tighten_db_file_permissions`，理由是「目录权限回归时文件自身兜底」——该论据同样适用于备份副本）。
- 问题：备份内容含 run 事实/会话 ID/输入文本，等于活库数据的外置副本；产品机制自身创建的产物未沿用同一纵深。
  任务书无此硬性条款，执行报告 §9 已知风险未登记，故只构成 MINOR（当前备份面=证据 CLI，无定时/产品化入口；
  目录权限名义上归调用方，且 drill 中目录为 mktemp 0700 时无暴露）。
- 修复要求：`backup_database`/`restore_backup` 在 rename/落盘前对产物与目录施加 0600/0700（unix），或按红线
  「显式降级并标注」在模块文档 + 报告 §9 登记为调用方责任；二选一后补一条权限断言测试并重跑 `backup_restore` + drill S1。
- 必须重跑的测试：`backup_restore`、`backup_faults`、`r02_t06_backup_restore.sh`。

### F03｜NON-ISSUE（INFO 登记）｜单值位翻转不可检——integrity_check 只验结构不验值（SQLite 无页校验和的固有属性）

- 证据（本代理实测）：对 checkpoint 后的 runs.db 翻转 1 字节（页内空闲区/值字节）→ `integrity_check` = ok，
  服务正常启动，事后 dump 视图 2 runs/4 events 全一致（free-space 翻转本就无损）；页中部 64 字节清零 → 结构上
  同样被 SQLite 容忍（视图一致性经断言复核）。而**结构可识别**的损坏全部被拒且文件字节不变：
  页类型字节翻转（offset SZ-4096）→ exit 2 `integrity_check reported: *** in database main ***`；
  截断半文件 → exit 2 且字节数与哈希不变；garbage 主文件（drill v5）→ `not a database`；
  篡改迁移收据（v7）→ SchemaTampered。
- 结论：A12 语料是「可识别的损坏测试库」，本实现的拒绝面覆盖全部结构可识别形态；值级位腐需端到端哈希清单，
  活库不存在该基线，Node 栈同样不覆盖。登记为已知边界即可，不构成缺陷。

### F04｜NON-ISSUE（方法论）｜本代理会话的文件读取展示层乱码（非候选缺陷）

- 大文件 Read/sed 输出出现标识符级替换（如同一类型名两种拼写、两份互相矛盾的「文件内容」）。
  经 grep 精确名、`cargo clippy -D warnings`（真实字节编译通过）、测试真实执行三信道否定文件损坏。
  所有结论以执行结果与短窗 grep 为准；此条仅作审计透明记录。

---

## 2. 验证命令清单（全部本代理亲手执行；退出码为实测）

| # | 命令（剥代理/`--locked`/专属 target dir） | 退出码 | 结果 |
|---|---|---|---|
| V1 | `cargo fmt --all -- --check` | 0 | 通过 |
| V2 | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 通过 |
| V3 | `cargo test --workspace --locked` | 101 | **1 failed（F01）**，其余通过 |
| V4 | `cargo test --workspace --locked --no-fail-fast` | 101 | 228 passed / 1 failed / 总 229 |
| V5 | `cargo test -p lingxi-adapters --test backup_restore --locked a11_backup_during_concurrent` ×10 | 混合 | **6 failed / 4 ok（F01 定频）** |
| V6 | `cargo test -p lingxi-service --test shutdown_coordinator --locked` | 0 | 5 passed（含外部连接持锁注入 exit 6、记录清理继续） |
| V7 | `cargo test -p lingxi-service --lib epoch:: --locked` | 0 | 16 passed（含谎报字符串钉住测试） |
| V8 | `cargo test -p lingxi-service --lib --locked config` | 0 | 25 passed（`--shutdown-timeout-ms` 严格解析） |
| V9 | `bash scripts/rust-tauri/r02_t06_backup_restore.sh /tmp/r02t06-review/ev` | 0 | S1–S4 ALL GREEN（kill -9 后 WAL=292552B 备份→恢复→逐行相等；S2 exit 0；S3 零产物；S4 close 1001/exit 0） |
| V10 | `bash scripts/rust-tauri/r02_t06_recovery_drill.sh /tmp/r02t06-review/drill` | 0 | v1–v8 全绿（v1/v2/v3/v5/v7 exit 2 + 哈希前后一致；v4 exit 0；v6/v8 INFO 按设计恢复且恢复视图经断言事务一致） |
| V11 | A11 对抗脚本（自建，/tmp）：篡改备份中段 1 字节 / 伪造 manifest sha / 截断备份 / 仅 `.partial` 伪装 / 自洽伪造 manifest 的半库 | 全部拒绝 rc=1 | 无一被接受为恢复成功；完好对照恢复 rc=0 |
| V12 | RLIMIT 真实性（真实二进制路径）：`ulimit -f 100`（<主库 77824B）备份 | rc=1 | 显式失败、目标目录零产物 |
| V13 | 同上 `ulimit -f 500` | 0 | 同一备份成功——注入真实性成立（故障随限值消失） |
| V14 | 独立对账：python3 sqlite3 只读打开备份与源库 | — | runs/events 逐项 MATCH，备份 integrity_check=ok |
| V15 | A12 对抗脚本（自建）：页类型字节翻转 / 截断（checkpoint 后） | exit 2 | 拒绝且文件字节一致（d1/d2）；free-space 翻转容忍=F03 INFO |
| V16 | epoch 闸对抗脚本（自建 8 变体，见 §4） | 8/8 正确 | a1–a8 全过 |
| V17 | 锁序实测：活实例持锁 + 第二实例对同一 home（撕毁印章）启动 | exit 3 | SINGLE_WRITER marker，0 条 epoch 行 → 锁先于闸 |
| V18 | T01–T05 回归 8 脚本（证据定向 /tmp） | 0 ×8 | smoke/boundary/dual/f01/path/auth/storage-tx/events 全绿 |
| V19 | `npm test -- --run tests/post-verification-audit-seal.test.ts` | 1 | 1 failed / 2 passed：失败清单全为 T01–T05 已提交交付+`.gitignore`（封印坐标落后于已授权 R02 提交），**零 T06 文件**；预存在红，如实记录 |
| V20 | `bash scripts/rust-tauri/r01-t02-check-generated.sh` | 0 | 冻结契约零漂移 |
| V21 | `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0 | RESULT: OK |
| V22 | `git diff 2aa80f506 --name-only -- core/ server/ desktop/ shared/ tests/` | — | 空 → Node 生产/测试零改动（红线遵守） |
| V23 | `git diff 2aa80f506 --numstat -- rust/Cargo.lock` | — | 恰 +1 行 `"rusqlite",`（lingxi-service dev 引用边），与声称一致 |
| V24 | `git status --porcelain artifacts/rust-tauri/R02/`（滤除 T06） | — | 空 → T01–T05 已提交证据零覆写 |
| V25 | 残留进程 `pgrep -fl lingxi-service` | 0 | 无遗留 |

## 3. Acceptance 独立判定

### R02-A11｜WAL 状态备份可恢复 — PASS（机制与证据真实；受 F01 牵连的仅为 A11-4 证据行的可复现性）

- 前置真实：S1 kill -9 后 `s1-wal-size.txt` = 292552 B（与执行者证据同值），WAL-resident 已提交数据确认存在。
- 机制亲自核验：备份经单写者队列（`backup_to`→`queue.submit`）+ Online Backup API（`backup::backup_database`），
  非活跃主文件拷贝；`.partial-*` → integrity gate → 原子 rename → manifest 原子写；恢复=manifest sha256 对账→
  拒绝覆盖→拷贝后副本哈希→必须过 `RunDatabase::open`（收据+integrity）全闸。
- 对抗全绿：篡改 1 字节拒绝（哈希对账真实生效）；伪造 manifest 拒绝；截断拒绝；仅 `.partial` 目录拒绝；
  自洽伪造 manifest 的半库被**恢复开库闸**拒绝（双层防线真实）；完好对照恢复成功（防"永远拒绝"假阳性）。
- 独立对账：不经 Rust dump 路径，python3 sqlite3 只读比对备份=源（MATCH）。
- 中断注入真实性：V12/V13 证明 RLIMIT 故障随限值消失（真实注入，非恒假故障）。
- 结论：A11 核心命题（在线备份可恢复、事务边界正确、中断不冒充成功）在二进制级+库内级均成立。

### R02-A12｜损坏库不自动丢弃 — PASS

- 执行者 drill v1–v8 独立复跑全绿；v1/v3 stderr 携带 `evidence=transitionJournal(fromEpoch=1,toEpoch=2,…)` 与
  `epochs 1→2` 人读证据；三个 `.err` 中谎报字符串 0 命中；v1/v3/v5/v7 损坏输入前后 sha256 逐对 diff 为空（字节不变）；
  v5 stderr 命名 `not a database`；v6 的「恢复视图经断言事务一致」验证为真实断言（completed 行⇔done 事件互斥校验，
  不一致即 fail），v6/v8 如实标 INFO 而非谎报拒绝——「按设计恢复」未被误报为损坏成功。
- 本代理自构形态（执行者清单之外）：页类型字节翻转→exit 2 字节不变；checkpoint 后截断→exit 2 字节不变；
  值级位腐容忍已按 F03 如实定性。原文件保留（前后哈希）与「不创建空库冒充」在全部形态成立。

## 4. PROD-DEFECT-1 闭环 — 独立结论：成立（新栈侧）

1. **4 变体演练复跑（V10）**：v1 撕印章+合法高位日志 / v2 撕日志 / v3 缺印章+合法高位日志 → 全部 exit 2 拒启，
   v1/v3 证据 1→2 如实在文（marker 行先于人读块，实测行序 4<5），v2 明示日志不可读、不引用其 epoch；v4 健康对照
   正常启动+优雅停 exit 0。文件字节不变经哈希对核实证。
2. **修复前基线证据真实**：`prod-defect-1/baseline-no-gate/` 显示 HEAD 无闸二进制对 v3 同型 home READY +
   execute 200，`v3-post-write-counts.json` = 1 run + 2 key_events —— 缺陷面在新栈真实存在过，修复以闸落地。
3. **本代理自构 8 个对抗变体（执行者清单之外，V16）**，全部行为正确：
   - a1 journal `fromEpoch` 可读但 `toEpoch` 缺失 → corrupt-journal 拒启，**不发明**任何 epoch 证据，文件不变；
   - a2 journal phase=committed 与印章 1/1 矛盾 → corrupt-transition 拒启且**不清理**日志（tail 清理条件 own_epoch≥to 未满足）；
   - a3 legacy v1 印章 `{epoch:1}` + barrier_raised 日志 1→2 → 拒启+证据；
   - a4 prepared 日志目标 1→999999 → incomplete-transition 拒启+证据 999999、不发明印章；
   - a5 印章已提交 2/2 + committed 日志 1→2 而本核为 epoch 1 → 拒启、日志保留、无 local-token.json；
   - a6 legacy epoch-1 印章单独存在 → 刷新为 v2 后启动（adopted-legacy 等价，分叉已按 §9.1 如实登记挂 R08）；
   - a7 空日志文件 → 拒启；a8 非法 phase → corrupt-journal 拒启且不引用其 epoch。
4. **结构性无 fail-open**：`coordinate_data_epoch_startup` 任何 Err → 二进制 exit 2；Ok 仅四条 Proceed 路径
   （fresh 盖章/稳定/刷新/committed-tail 清理，后者要求 target_committed 且 own_epoch≥to——a2/a5 实测不满足即拒）。
   全栈 0 处 `BASELINE_WARNING`/`ordinary startup will continue`；`LINGXI_ALLOW_DATA_DOWNGRADE` 0 引用（如实声明未实现）。
5. **谎报字符串钉住**：`"no higher-epoch evidence was found"` 在全 crate 仅存在于文档注释与
   `assert!(!contains)` 反向断言（`the_incumbent_lie_string_never_appears_in_rendered_refusals` 等 5 处），单测全过。
6. **Node 零改动**（V22）+ 印章/日志盘上格式双向兼容按 `shared/data-epoch.cjs` 校验矩阵逐条比对（七 phase、
   checkpoint 收据、recoveryModes 覆盖、v1 回退），Rust 闸不比 Node 松。

## 5. 其余审查项结论（全部核过）

- **关闭协调器真实**：5 测试含真实注入——外部 `rusqlite::Connection` 持库（busy_timeout 10s）对短 deadline →
  `LINGXI_SERVICE_SHUTDOWN_TIMEOUT phase=storage_close`、继续记录清理、exit 6；exit 5/4 映射；WS 排干 guard 全路径。
  S4 二进制级：开 WS + SIGTERM → 客户端收 close(1001)、服务 exit 0（信号时刻广播消解死锁的设计经实测成立）。
- **接线顺序**：锁后→闸→auth→开库，V17 实证（持锁时 corrupt home 得 exit 3、零 epoch 行）；拒启 home 无
  local-token.json/runs.db（V16 a5 + drill v1 断言）；USAGE 含 `--shutdown-timeout-ms` 与退出码 0/1/2/3/4/5/6 全表。
- **T05 移交（R2-F01）未破坏**：events_matrix 回归绿；5 个测试文件 diff 仅 +`shutdown_timeout_ms` 字面量。
- **F04 顺带项**：runs.db/-wal/-shm 0600 有专测且通过。
- **回归零覆写**：T01–T05 已提交 artifacts 零 diff（V24）；8 回归脚本全绿（V18）；contracts/ownership 0（V20/V21）。
- **Cargo.lock**：恰 +1 行 dev 引用边（V23）。
- **Scope 干净**：无 T07 脱敏/T08 xtask 提前实现；16 个跟踪文件 +671/−42，无隐藏删除；真实用户目录零触碰
  （全部 /tmp 合成 home）。
- **报告一致性**：除 F01 的 229/0 与 F02 未登记权限外，其余声称与证据一一相符（WAL 字节数、基线计数、229 总数、
  已知风险 §8/§9、unstamped 分叉挂 R08、LINGXI_ALLOW_DATA_DOWNGRADE 未实现声明均属实）。
- **封印预存在红**：V19 失败清单全为 T01–T05 已提交交付（坐标 ab4f2281 落后于已授权 R02 提交），零 T06 文件；
  未提议虚报坐标/扩白名单（按红线仅记录）。

## 6. PASS 标准核对

| 标准 | 结论 |
|---|---|
| 1. A11/A12 真实 PASS + PROD-DEFECT-1 闭环 | A11 机制 PASS 但 A11-4 证据行不可复现（F01）；A12 PASS；闭环成立 |
| 2. 生产路径真实接通 | PASS（V9/V10/V17 实测二进制链路） |
| 3. 无 BLOCKING | **不满足（F01）** |
| 4. 无测试篡改 | 满足（F01 属测试自身缺陷，非篡改；断言文本与实现语义可公开对质） |
| 5. 无未解释数据/关闭/恢复缺口 | 满足（F03 值级位腐为固有边界，已解释登记） |
| 6. 回归绿（封印预存在红如实记录） | 满足（V18/V19） |
| 7. 证据与候选一致 | 满足（除 F01 声明的可复现性、F02 未登记） |

**VERDICT: FAIL** — 唯一阻断项 F01：将 `a11_backup_during_concurrent_commits_is_a_consistent_prefix` 的断言改为
事务一致性语义（本报告 §1 F01 修复要求 1-3），重跑指定测试并更正报告两处数字后，本代理的其余全部绿灯结论
无需重验即可升级为 PASS 建议。

— REVIEWER-R02-T06-R1；本报告为唯一仓库内写入物。
