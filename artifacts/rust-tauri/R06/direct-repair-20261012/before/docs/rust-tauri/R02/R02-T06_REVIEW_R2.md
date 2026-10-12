# R02-T06｜备份、关闭与启动恢复 — 独立对抗性验收 REVIEW（R2）

- 验收代理：REVIEWER-R02-T06-R2（第 2 轮；非实现者、非修复者，未参与第 1 轮验收，
  不信任任何前代理声明；对 R1 修复做完整 Task 验收而非仅复验旧问题）
- 日期：2026-09-26
- 候选：TASK_BASE_SHA `2aa80f506464e6cba7ec001b987d649e1164faaf`（= 本机 HEAD，未变）
  + 未提交工作树（含 REPAIR-R1 修复面；`git status --porcelain` = 27 条，与报告 §10 一致）
- 工具链：rustup 1.29.1 代理 + `rust-toolchain.toml` 锁定 cargo/rustc 1.98.1（亲核
  `cargo --version`/`rustc --version`）；全部 cargo 命令剥代理 + `--locked` +
  `CARGO_NET_OFFLINE=true` + 专属 `CARGO_TARGET_DIR=/tmp/rust-target-r02-t06-r2`；
  复审专属目录 `/tmp/r02t06-review-r2/`（唯一仓库内写入物 = 本文件）
- 输入：R02 任务书 R02-T06 节 + task-catalog R02-T06 + acceptance-catalog R02-A11/A12 +
  契约 02 §8（备份共同静止点/单写者）+ 05 §1（四层证据）；REVIEW_R1（F01 机理与修复要求）、
  执行报告 §12（REPAIR-R1）；当前源码全量审读（backup.rs / backup_restore.rs /
  backup_faults.rs / queue.rs / run_store.rs / migrations.rs / epoch.rs / shutdown.rs /
  main.rs / lib.rs / config.rs / inspect / 两脚本）；`git diff 2aa80f506..工作树` 全集。

## VERDICT: PASS（0 BLOCKING / 1 MINOR / 3 NON-ISSUE；R1 F01/F02 真实关闭，全 Task 独立复验全绿）

---

## 1. R1 findings 关闭核验（逐条）

### F01（R1，BLOCKING）— 关闭：确认（CLOSED）

**断言语义审读（对抗点：是否被放松为永真）。** `backup_restore.rs` 并发用例重写为
「备份 = 某队列处理前缀的完整快照」，断言集为：

1. run ids 构成 w00..wK 连续前缀（`run_fact_summary` ORDER BY run_id 逐位比对）；
2. 每 run 行 ⇒ start 事件恰好 1 + attempt 行恰好 1（start 三件套同事务，无半 start）；
3. status=completed ⇔ done 事件 1；status≠completed ⇒ done 事件 0（commit 事务对拍，无半 commit）；
4. event 总数 = run_count + completed_runs（每 run 1 start，每 completed 另 1 done）；
5. in-flight ∈ {0,1} 且为 1 时必为前缀最高 id（writer 顺序 await：commit(i) 先于 start(i+1)）；
6. MAX(seq) == COUNT(*)（流内 seq 1..M 连续，撕裂快照会留洞）；
7. 非空化下界：writer 在 w00 完整提交后经 oneshot 信号，备份作业仅在收到信号后提交
   （`queue.submit` FIFO ⇒ w00 两作业必已入队并执行，run_count ≥ 1 是保证不是运气）。

**非永真判定**：备份若丢失已提交前缀 → 断言 1 失败；半 start（行有事件无/attempt 无）→
断言 2 失败；半 commit → 断言 3 失败；多出/缺失事件 → 断言 4 失败；撕裂 seq → 断言 6 失败；
空快照 → 断言 7（1..=20 包含检查）失败。旧断言 `event_count == run_count*2`（把「无半事务」
误写成「无 in-flight run」）已删除；新断言集对**所有**交错成立且严格覆盖旧意图的事务一致性
部分——这是**修正语义，不是放松**。根基亲验：`record_run_started`/`commit_run_outcome`
各为独立 `queue.submit` + `with_write_txn`（run_store.rs:660/778），备份经同一 FIFO
（`RunDatabase::backup_to`）——§12.4 论证与代码事实一致。

**固定交错用例**（`a11_backup_of_a_started_but_uncommitted_run_is_a_complete_prefix`）：
两次完整提交 + 一次仅 start + 备份 ⇒ 恢复件与源 `logical_dump` **逐行相等**（w02 以
'running' 原样保留、start 事件 1、done 事件 0、attempt 1）——真钉住 in-flight 快照语义，
非仅断言 id 前缀。

**确定性亲跑（≥20 轮要求，实测 31 次连续绿）**：

| 项 | 命令（剥代理/`--locked`/专属 target dir） | 结果 |
|---|---|---|
| 单测连跑 | `cargo test -p lingxi-adapters --test backup_restore --locked -- --exact a11_backup_during_concurrent_commits_is_a_consistent_prefix` ×**25** | **25/25 exit 0**（`/tmp/r02t06-review-r2/f01-run-1..25.log`，逐份含 `test result: ok`；FAILED/panicked 全文 0 命中） |
| 全套件复跑 | 同 `--test backup_restore` 整套 ×**6**（workspace 1 + 独立 5） | **6× 11 passed / 0 failed** |
| 全量门禁 | `cargo test --workspace --locked --no-fail-fast` | exit 0，**37 套件 231 passed / 0 failed**（修复声称 231/0 精确复现） |

与 R1 失败形态（10 连跑 6 failed/4 ok、event_count 奇数）对比：修复后 31 次执行零失败、
零 flake 迹象。**oneshot 新引入 flake 排查**：信号在 `commit_run_outcome().await` 返回后发送
（= commit 作业已执行完），测试侧 30s 超时防挂；队列 FIFO + 有界容量下无死锁路径
（备份作业入队后先于后续 writer 作业执行）；25+6 轮实测无挂起/失败。关闭成立。

### F02（R1，MINOR）— 关闭：确认（CLOSED）

**读码**：`backup.rs::tighten_owner_only`（unix 目录 0700/文件 0600，幂等，失败响亮；
非 unix no-op 已在模块文档明示）接入 4 处——`backup_database` 目标目录（任何产物落盘前）、
`.partial`（open 后、首字节前）、manifest tmp（写入前）、`restore_backup` 目标目录 +
恢复 db（copy 后、fsync 前；收紧失败删除副本并响亮报错）。

**二进制亲测（umask 022，预建目录 = 验证显式收紧而非 umask 运气）**：

| 产物 | 权限（实测 `stat -f %Lp`） |
|---|---|
| 预建 0777 备份目录（收紧后） | **700**（umask 022 原生应得 755 → 收紧是显式行为） |
| 备份 `runs.db` / `runs.manifest.json` | **600 / 600** |
| 预建 0777 恢复目录（收紧后） | **700** |
| 恢复 `runs.db` | **600** |
| 活库 `runs.db` / `-wal` / `-shm`（F04 顺带项） | **600 / 600 / 600** |

库内断言测试 `backup_and_restore_artifacts_are_owner_only` 在全量运行中通过。
**收紧失败响亮**：代码级验证（Err → 备份/恢复在发布前中止）；无 root 无法在二进制级
强制 chmod 失败，如实登记为代码级验证 + 相邻路径（不可写目标 create_dir_all 失败）有
专测覆盖（`backup_into_unwritable_destination_fails_without_artifacts` + 二进制 S3）。关闭成立。

### F03 / F04（R1，NON-ISSUE）— 维持原裁定

F03（值级位翻转不可检）本轮 ADV7 亲测再次复现该边界（见 §3 ADV7），定性不变；
F04（会话乱码方法论）不适用于本轮（本轮无乱码现象）。

---

## 2. 完整 Task 验收（全部本代理亲手执行，退出码实测）

| # | 命令（剥代理/`--locked`/专属 target dir） | 退出码 | 结果 |
|---|---|---|---|
| W1 | `cargo fmt --all -- --check` | 0 | 通过 |
| W2 | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 通过 |
| W3 | `cargo test --workspace --locked --no-fail-fast` | 0 | **37 套件 231/0**（修复声称精确复现；含 backup_restore 11、backup_faults 2、shutdown_coordinator 5、epoch 16 单测、谎报字符串钉住测试） |
| W4 | `cargo test -p lingxi-adapters --test backup_faults --locked` | 0 | 2 passed（RLIMIT_FSIZE checkpoint 期 + 拷贝期真实 EFBIG） |
| W5 | `cargo test -p lingxi-service --test shutdown_coordinator --locked` | 0 | 5 passed（含外部连接持 WAL 写锁 → deadline marker + exit 6 + 记录清理继续） |
| W6 | `bash scripts/rust-tauri/r02_t06_backup_restore.sh /tmp/r02t06-review-r2/ev` | 0 | S1–S4 ALL GREEN：S1 kill -9 后 WAL=292552 B（与 R1 观测同值）→ Online API 备份 → restore-verify 过收据+integrity → dump 逐行相等 3 runs/6 events 全 complete；S2 exit 0；S3 零产物；S4 WS close(1001)/exit 0 |
| W7 | `bash scripts/rust-tauri/r02_t06_recovery_drill.sh /tmp/r02t06-review-r2/drill` | 0 | v1–v8 全绿（v1/v2/v3/v5/v7 exit 2 + 文件字节不变 + 无 local-token.json；v4 健康对照 exit 0；v6/v8 INFO 如实记录且 v6 恢复视图经事务一致性断言） |
| W8 | `bash scripts/rust-tauri/r01-t02-check-generated.sh` | 0 | 冻结契约零漂移 |
| W9 | `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0 | RESULT: OK |
| W10 | `bash scripts/rust-tauri/r02_t04_storage_tx.sh /tmp/…`（证据定向 /tmp） | 0 | T04 回归 ALL GREEN |
| W11 | `bash scripts/rust-tauri/r02_t05_events_matrix.sh /tmp/…`（证据定向 /tmp） | 0 | T05 回归 ALL GREEN（R2-F01 移交面未破坏） |
| W12 | `npm test -- --run tests/post-verification-audit-seal.test.ts` | 1 | 1 failed / 2 passed：失败 = allowlist 坐标检查（expected ['.gitignore',…209] vs []），**失败清单零 T06 文件**——预存在红（seal 坐标落后于已授权 R02 提交），如实记录，未触碰坐标/白名单 |
| W13 | `git diff 2aa80f506 --name-only -- core/ server/ desktop/ shared/ tests/` | — | 空 → Node 生产/测试零改动（红线持续遵守，含 core/data-epoch-coordinator.ts） |
| W14 | `git diff --numstat 2aa80f506 -- rust/Cargo.lock` | — | 恰 +1 行 `+ "rusqlite",`（lingxi-service dev-dep 引用边；rusqlite 0.40.2 已锁，零新包） |
| W15 | `git status --porcelain artifacts/rust-tauri/R02/`（滤除 T06） | — | 空 → T01–T05 已提交证据零覆写 |
| W16 | 5 个测试文件 diff 全文 | — | 恰 7 行 `+ shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,`，与 §12 声明一致（auth_matrix/event_subscription/instance_lifecycle/service_health/service_persistence） |
| W17 | `pgrep -fl lingxi-service\|lingxi-storage-inspect` | — | 无遗留进程 |

**Scope 核验**：16 个跟踪文件 +671/−42 + 12 个未跟踪新文件，无隐藏删除；无 T07/T08 提前实现；
全部测试/演练用 /tmp 合成 home，真实用户目录零触碰。

**mtime 取证（修复面限定）**：epoch.rs/shutdown.rs/main.rs/lib.rs/config.rs/queue.rs/
run_store.rs/migrations.rs/mod.rs/backup_faults.rs/shutdown_coordinator.rs/两脚本 mtime 全部
在原执行窗口（22:08–23:07）；仅 backup.rs（00:01）与 backup_restore.rs（00:04）落在修复窗口
——与 §12「修复只触 backup.rs 权限 + 测试断言 + 报告」一致，产品实现零缺陷变更声明成立。

---

## 3. 对抗结果（本轮自构）

| # | 对抗 | 结果 |
|---|---|---|
| ADV1 | 备份文件中段翻 1 字节（尺寸不变）后 restore-verify | exit 1，`backup file hash mismatch` 拒绝；目标目录零残留 ✓ |
| ADV2 | 截断备份半尺寸后 restore-verify | exit 1，哈希+字节数双失配拒绝 ✓ |
| ADV3 | **自洽伪造 manifest**（对篡改文件重算 sha256 写回 manifest，攻击者同时控制文件+manifest） | 哈希对账通过 → **恢复开库闸拒绝**（页类型字节翻转 → `database disk image is malformed`，exit 1）——双层防线真实生效 ✓ |
| ADV4 | 完好对照恢复 | exit 0（防「永远拒绝」假阳性）✓ |
| ADV5 | 独立对账：不经 Rust 路径，python3 sqlite3 只读打开备份与源库 | runs/事件逐项一致，两侧 integrity_check 均 ok ✓ |
| ADV6 | 篡改前后哈希对账（drill v1/v2/v3/v5/v7 的 before/after 文件） | 脚本内 diff 断言全过：损坏输入字节不变、无空库顶替、无印章发明 ✓ |
| ADV7 | 自洽伪造 + **值级**位翻转（中段单字节，落无损页） | restore-verify 成功且恢复件事实无损（1 run/2 events 与源一致）——即 R1 F03 已裁定的 SQLite 无页校验和固有边界，修复未改变该机制，维持 NON-ISSUE |
| ADV8 | epoch 闸自构变体 a2：journal phase=committed 与印章 1/1 矛盾 | exit 2 corrupt-transition；**日志保留未清理**（tail 清理条件不满足）；无 auth 状态写入 ✓ |
| ADV9 | epoch 闸自构变体 a4：prepared 日志目标 1→999999 | exit 2 incomplete-transition；证据如实命名 toEpoch=999999；**不发明印章** ✓ |
| ADV10 | 谎报字符串钉住 | `"no higher-epoch evidence was found"` 全 crate 仅存在于文档注释与 `assert!(!contains)` 反向断言（源码 grep 4 处断言位）；`the_incumbent_lie_string_never_appears_in_rendered_refusals` 等 16 个 epoch 单测全绿；drill v1/v3 stderr 0 命中 ✓ |

**修复引入新问题专项审查**：

- **tighten_owner_only 恢复失败路径误删面**：读码确认 `remove_file(&target_path)` 仅在
  copy/tighten 失败路径执行，且此前 `target_path.exists()` 已拒绝一切预存文件（含符号链接
  ——exists() 跟随链接返回真 → 拒绝）→ 顺序执行下删除的只可能是恢复自己刚创建的副本，
  不可能命中用户已有文件。残存理论 TOCTOU（exists 检查与 copy 之间外部进程建文件）为
  预存在行为（R1 已在场、修复未改变，copy 本身即覆盖语义），登记 INFO 不构成缺陷（F06）。
- **新并发测试 oneshot 信号**：见 §1 F01——31 次执行零 flake，无死锁路径，无新 flake。
- **备份 tighten 收紧调用方预存目录**：对预存 0777 目录收紧为 0700 是模块文档 + 报告 §12.2
  明示的设计行为（不删任何用户文件），测试以预建 0777 钉住语义。认可。

---

## 4. 发现（R2 新增编号，接续 R1）

### F05｜MINOR｜repair-r1 的 30 连跑证据为手写摘要，非原始命令输出

- 位置：`artifacts/rust-tauri/R02/T06/repair-r1/f01-test-25x.log`；对应报告 §7.1 A11-4、§12.5 R2 行。
- 证据（本代理亲读）：该文件内容为 30 行 `run N: ok` + `TOTAL: pass=30 fail=0`，无命令行、
  无 cargo 原始输出（对照同目录 `gates-cargo-test-workspace.log` 为完整原始日志）。
- 问题：作为独立证据不自足——无法从该 artifact 复核其确由 `cargo test` 产生。
- 后果：仅证据形式弱点；**非阻断**——本代理以同命令 25 连跑 + 全套件 6 轮 + 全量 231/0
  独立复现了其声称的结论（31 次连续绿 ≥ R1 修复要求 10 连跑），数字与结论均成立。
- 根因：修复代理存档摘要而非原始输出。
- 修复要求（可选，不阻断本轮）：后续修复/门禁证据统一存档原始命令输出（或摘要 + 命令行 +
  原始尾行），避免同类自证缺失。
- 必须重跑的测试：无（结论已由本代理独立复现）。

### F06｜NON-ISSUE（INFO 登记）｜restore 失败路径删除面的理论 TOCTOU 窗口

- `restore_backup` 的 exists() 拒绝检查与 `fs::copy` 之间存在理论窗口：外部进程在该窗口内
  于目标路径创建文件时，copy 会覆盖它且失败路径会删除该路径。顺序执行下不存在此问题
  （见 §3 修复引入新问题审查第 1 条）。预存在行为，REPAIR-R1 未改变，单机产品形态下
  无实际攻击面。登记备查，不构成缺陷。

### F07｜NON-ISSUE（INFO 登记）｜值级位翻转 + 自洽伪造 manifest 可通过恢复（恢复件事实无损）

- ADV7 实测：对备份翻一个**值级**字节并用重算哈希伪造 manifest → 哈希对账与
  integrity_check 均通过、恢复成功且事实无损。这是 R1 F03 已裁定的固有边界（SQLite 无页
  校验和、integrity_check 只验结构；活库无端到端哈希基线，Node 栈同样不覆盖）。
  REPAIR-R1 未触碰该机制面，维持 NON-ISSUE；自洽伪造者本就等价于合法重新备份。

### F08｜NON-ISSUE（INFO 登记）｜ADV3 形态被开库闸拒绝后，inspect CLI 不清理已拷贝副本

- 自洽伪造的结构损坏库：copy+哈希成功 → 开库闸拒绝（exit 1）→ `adv6-out/runs.db` 残留在
  目标目录。该副本永不被信任（契约要求经 `RunDatabase::open` 才算恢复成功），且再次恢复
  同目录会被「拒绝覆盖」挡住；与 R1 V11 观察行为一致，非修复引入。登记备查。

---

## 5. Acceptance 独立判定

### R02-A11｜WAL 状态备份可恢复 — PASS（独立确认）

- 前置真实：S1 kill -9 后 WAL=292552 B（亲测，与执行者/R1 同值），WAL-resident 已提交数据确认存在。
- 机制亲核：备份经单写者队列（`backup_to`→`queue.submit`）+ Online Backup API
  （`rusqlite::backup`），非活跃主文件拷贝；`.partial-*` → integrity gate → 原子 rename →
  manifest 原子写；恢复 = manifest sha256 对账 → 拒绝覆盖 → 拷贝后副本哈希 → 必须过
  `RunDatabase::open`（收据 + integrity_check）全闸。
- 库内级 + 二进制级全绿（W3/W6）；A11-4 证据行修复后可复现（31 次连续绿）。
- 对抗 ADV1–ADV5 全部符合预期；中断不冒充成功三层验证（库内不可写目标 / RLIMIT_FSIZE /
  二进制 S3）。
- 判定：**PASS**。

### R02-A12｜损坏库不自动丢弃 — PASS（独立确认）

- drill v1–v8 亲跑全绿（W7）：5 种拒绝形态（torn 印章/torn 日志/缺印+高位日志/garbage 主文件/
  篡改收据）全部 exit 2 + 文件字节不变 + 不创建空库顶替 + 闸先于 auth（无 local-token.json）；
  v1/v3 证据 1→2 如实在文（marker 行 + journal_evidence + 人读块）；v6/v8「按设计恢复」
  如实标 INFO 且 v6 恢复视图经事务一致性断言（completed ⇔ done 事件互斥校验）。
- 本代理自构 ADV8/ADV9（执行者清单之外）行为正确；结构级自洽伪造被开库闸拒绝（ADV3）。
- 判定：**PASS**。

### PROD-DEFECT-1 闭环（新栈侧）— 独立结论：成立

1. 4 变体演练亲跑（W7）：v1/v2/v3 → exit 2 拒启 + 证据如实 + 字节不变 + 无 auth 写入；
   v4 健康对照正常。与 Node 侧 R7/R9/R10 fail-open（写 55 条 + 谎报）形成闭环对照。
2. 修复前基线证据真实：`prod-defect-1/baseline-no-gate/v3-post-write-counts.json` =
   1 run + 2 key_events（无闸二进制对同型 home READY + 落库）——缺陷面真实存在过，闸修复落地。
3. 谎报字符串结构排除：全 crate grep + 反向断言单测 + drill stderr 0 命中（ADV10）。
4. 本代理自构 a2/a4（ADV8/ADV9）+ R1 的 8 变体先例：无一条 fail-open；`coordinate_data_epoch_startup`
   任何 Err → 二进制 exit 2，Ok 仅四条 Proceed 路径，结构上无软化边；`LINGXI_ALLOW_DATA_DOWNGRADE`
   0 引用（如实声明未实现）。
5. Node 零改动（W13）+ 印章/日志盘上格式与 `shared/data-epoch.cjs` 兼容（R1 已逐条比对，
   本轮源码复核 epoch.rs 校验矩阵同构）。
- 判定：**闭环成立**。

---

## 6. PASS 标准核对

| 标准 | 结论 |
|---|---|
| 1. A11/A12 真实 PASS | 满足（§5，库内+二进制+对抗三层独立确认） |
| 2. PROD-DEFECT-1 闭环成立 | 满足（§5） |
| 3. 无 BLOCKING（F01 真实关闭且无新问题） | 满足（F01 CLOSED：语义修正非放松，31 次连续绿；无新 BLOCKING） |
| 4. 无测试篡改 | 满足（新断言=修正语义，覆盖严格不弱于正确意图；固定交错用例钉住；§12.4 论证与代码事实一致） |
| 5. 无未解释缺口 | 满足（F06/F07/F08 均 INFO 登记；F03 边界维持已解释） |
| 6. 回归绿（封印预存在红如实记录） | 满足（W3/W10/W11 全绿；W12 预存在红如实记录，失败清单零 T06 文件） |
| 7. 证据与候选一致 | 满足（231/0、WAL 292552、基线计数、mtime 修复面限定均与声称一致；唯一弱点 F05 MINOR 已登记，结论经独立复现兜住） |

**VERDICT: PASS** — R1 唯一阻断项 F01 已真实关闭（断言语义修正为可对质的「前缀完整快照」
命题 + 31 次连续确定性亲跑 + 报告数字更正与实测一致），F02 已关闭（显式收紧 + 预建 0777
亲测），修复未引入新问题（F05–F08 均 MINOR 以下）。全部验收面由本代理独立复跑确认。

— REVIEWER-R02-T06-R2；本报告为唯一仓库内写入物；未 commit/push。
