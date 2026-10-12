# R02-T05 独立验收报告（REVIEW-R1）

- **Reviewer**: REVIEWER-R02-T05-R1（round 1，独立对抗验收；未参与实现，不相信执行者声明）
- **Task**: R02-T05「事件顺序、快照和断线续读」
- **Candidate**: HEAD(e80c4b5d0) + 未提交 T05 改动（`git diff e80c4b5d0..工作树`，8 modified + 7 untracked）
- **Base**: TASK_BASE_SHA `3c7d05ca41c37019fc7a1a94beac5413c701466b`；回填提交 `e80c4b5d04b7697ae0277d74cbc661f777e65a39`（T04 DEPENDENCY_RULES.json D5 更新，内容已核对：`adapters planned→exists` + 新增 DEP-09，established_by 注明 T04）
- **审查日期**: 2026-09-26

## 0. 结论（VERDICT: FAIL）

**FAIL**。唯一 BLOCKING：F01 —— hold-join 在 durable 读完成与 `release_hold` 之间存在并发窗口时，**静默丢失 seq > cut 的已缓冲关键事件**（`events.rs:543` hold 分支推进 `last_enqueued_seq`，`events.rs:530` flush 时再去重命中）。R02-A09「关键事件不漏不重」的保证在代码层面不成立；随任务交付的两条 A09 race 测试均为 `current_thread` 运行时，结构上无法命中该窗口（窗口内无调度点），因此官方测试全绿与缺陷存在并不矛盾。我用字节级一致的 vendored 逻辑写了确定性反例（探针 A/A2，均复现），并用真实生产代码（multi_thread 运行时 + 真实 SQLite + 真实 HTTP/WS 路径）施压验证窗口在调度压力下可达（page-capped 区间 63/500 命中，均因 `next_cursor` 可分页恢复；uncapped 区间 0/200，窗口真实但窄）。

R02-A10 独立判定 **PASS**（409 cursor_expired + WS 控制帧保连接 + rebuild==authority + 全部负例亲手探测通过）。

回归（T01–T04 脚本、Cargo.lock 零 diff、Node/Electron 零 diff、契约零漂移）全绿；审计封印族 pre-existing 红，归因已核实（坐标 `ab4f2281` 滞后于已授权 R02 提交，127 个已提交文件差异与 T05 未提交集合零交集），如实记录，不属本 Task 新失败。

---

## 1. 环境与方法

- rustup 已核对：锁定 toolchain（rust-toolchain.toml channel），`rustup run <channel> cargo --locked`，专属 `CARGO_TARGET_DIR=/tmp/rust-target-r02-t05-review`（与执行者目录隔离）。
- 网络命令全部剥离代理：`env -u http_proxy -u https_proxy -u HTTP_PROXY -u HTTPS_PROXY -u all_proxy -u ALL_PROXY`；cargo 一律 `CARGO_NET_OFFLINE=true --locked`。
- 独立阅读：任务书 01/02/04/05/06、R02 书 §4 T05、task-catalog/acceptance-catalog 的 R02-T05/R02-A09/R02-A10、R01 PROTOCOL_SPEC、T01–T04 报告与验收、`events.rs` 全文 1443 行、`ws.rs`/`auth.rs`/`run_store.rs`（读路径与 replay 半部）、kernel `ports.rs` EventStorePort、执行者报告 §0 前驱处置表与 §7 风险台账、`artifacts/rust-tauri/R02/T05/` 全部证据。
- 对抗方法：在 `/tmp/r02t05-review-probe/` 自建探针 crate（path 依赖真实 lingxi-protocol/kernel/adapters/service），其中 `src/events_vendored.rs` 为 `events.rs` 的字节级一致副本（仅 2 行可见性改动：`fn register(`→`pub fn register(`、`fn release_hold(&self,`→`pub fn release_hold(&self,`，`diff` 核验恰好 2 行），用于把私有 hold-join 状态机暴露在确定性测试下；另用真实 `lingxi-service` 生产代码路径（`sessions.execute_for` + `EventService::subscribe` + 真实临时 SQLite）做并发施压。**未修改产品源码/测试/配置；唯一仓库写入即本文件。**

## 2. 验证命令清单与退出码（全部亲手执行）

| # | 命令（摘要） | 退出码 | 结果 |
|---|---|---|---|
| 1 | `git diff --stat e80c4b5d0 --` / `git status --short` | 0 | 8 修改 + 7 新增，全部 T05 范围内（§7） |
| 2 | `rustup run <locked> cargo fmt --all -- --check`（`--locked` 不适用，fmt） | 0 | 与执行者声明一致 |
| 3 | `cargo clippy --workspace --all-targets --locked -- -D warnings`（专属 target dir，offline，剥代理） | 0 | 0 warning；日志 `/tmp/r02t05-rev-clippy.log` |
| 4 | `cargo test --workspace --locked`（offline，剥代理） | 0 | **185 passed / 0 failed**，与声明一致（165 基线 + 20 新增 = events.rs 单测 11 + event_store_reads 3 + event_subscription 6）；日志 `/tmp/r02t05-rev-test.log` |
| 5 | `scripts/rust-tauri/r02_t05_events_matrix.sh /tmp/r02t05-rev-matrix-r{1,2,3}`（真实二进制，3 轮） | 0 ×3 | 3/3 轮 `RESULT: ALL R02-T05 BINARY-MATRIX CASES PASSED`（18/18 用例；a09 merged==durable；a10 rebuild equal；三视图交叉核对 OK） |
| 6 | `cargo test -p lingxi-service --test event_subscription --locked`（独立 5 轮） | 0 ×5 | 每轮 6 passed；无 flake |
| 7 | `cargo test -p lingxi-adapters --test event_store_reads --locked` | 0 | 3 passed（含 WAL 第二连接篡改→Corrupted 响亮失败） |
| 8 | 探针 crate `hold_join_probes`（vendored 字节一致逻辑，3 确定性探针） | 0（按设计复现缺陷） | **探针 A：register→publish[1,2]→publish[3](hold)→release_hold(2)→publish[4]，实收 [4] 而非 [3,4]，seq=3 静默丢失；探针 A2：delta 变体丢失且不计入 dropped_deltas；探针 B：见 F02** |
| 9 | 探针 crate `adversarial_extras`（6 条对抗附加：伪造 checksum 未来游标、跨主体、purge-all 后陈旧游标等） | 0 | 6/6 按预期（服务器全部拒绝/响亮失败；其中 purge-all 路径暴露 F03 词汇不一致） |
| 10 | 生产代码并发施压 `production_race_stress`（真实 lingxi-service，multi_thread） | 0 | run 2（page-capped 区间）：500 订阅中 63 个静默空洞，全部 cut==500==page cap → `next_cursor` 已发 → 文档化分页可恢复；run 4（uncapped 分批区间）：subs=200 complete=110 detached=90 **SILENT_HOLES=0**；日志 `/tmp/r02t05-race-stress-run{2,3,4}.log` |
| 11 | 生产代码重启基线施压 `restart_stall_probe` + `uncapped_window_stress` | 0 | 0/150 生产窗口（subscribe 任务总在竞态 commit 前恢复）；vendored 确定性探针 B 复现停滞（F02）；日志 `/tmp/r02t05-restart-stall.log`、`/tmp/r02t05-uncapped-stress.log` |
| 12 | 回归：T01–T04 脚本（r02_t01…r02_t04 各矩阵/探针） | 0 ×全 | 全 PASS；日志 `/tmp/r02t05-rev-reg-*.log` |
| 13 | `git diff e80c4b5d0 -- Cargo.lock` / Node/Electron 路径 / `contracts/` generated | 0 | **零 diff**（Cargo.lock、DEPENDENCY_RULES.json、desktop/、contracts/generated 均无变化） |
| 14 | 契约一致性检查（contracts 校验脚本） | 0 | 零漂移；日志 `/tmp/r02t05-rev-contracts.log` |
| 15 | ownership 检查器 | 0 | RESULT OK |
| 16 | `npm test -- tests/post-verification-audit-seal.test.ts`（封印族） | 1 | **pre-existing 红**（1 failed / 2 passed）；归因：`git diff --name-only ab4f2281..HEAD` = 127 个已提交文件，与 T05 未提交集合**零交集**；坐标滞后于已授权 R02 提交，如实记录，不提议虚报坐标/扩白名单 |
| 17 | `shasum -a 256 docs/rust-tauri/R02/R02-T05_REVIEW_R1.md`（写完本文件后） | 0 | 值见最终答复 |

## 3. Acceptance 独立判定

### R02-A09（快照与订阅无空隙；关键事件不漏不重）：**FAIL**

- 执行者交付层（不应全信，已复核）：固定调度 race 矩阵 k=0..=8（writers 先于 subscribe 生成、逐事件比对 authority DB）、真实二进制 18 用例、三视图交叉核对 —— **这些测试本身全部真实通过**（我亲手重跑 3 轮二进制 + 5 轮 cargo suite）。
- **但官方 race 测试结构性失明**：`event_subscription.rs` 中两条 A09 race 测试均为 `#[tokio::test(flavor = "current_thread")]`。`EventService::subscribe`（events.rs:901）的流程是 `register(hold)`（:978）→ `stream_events_after` await（:982）→ `release_hold(snapshot_seq)`（:1012），**durable 读完成与 release_hold 之间没有任何同步原语**。在 current_thread 下该窗口内无调度点，测试永远命中不到；唯一 multi_thread 的 WS 全链路测试从不让 subscribe 与 execute 并发竞速。即：通过的测试与缺陷存在不矛盾。
- 我的对抗反证（探针 A，vendored 字节一致逻辑，确定性）：hold 期间到达的 seq=3 在 `release_hold(cut=2)` flush 时被 `enqueue_for` 的去重分支（:530 `seq <= last_enqueued_seq` → return）丢弃——因为 hold 分支在 :543 已把 `last_enqueued_seq` 推进到 3。实收序列 [4] 而非 [3,4]，**无 detach、无 snapshot_required 信号、无 dropped_deltas 计数（探针 A2），且 `last_enqueued_seq` 已被推进使后续更正也被抑制——静默、永久**。
- 生产可达性：生产入口为 multi_thread（`#[tokio::main]`），窗口结构真实存在；施压显示 page-capped 区间 63/500 命中（可经 `next_cursor` 分页恢复），uncapped 区间 0/200（窗口窄）。窗口窄不改变「保证不成立」的判定：A09 要求的是 airtight join，任务书 02 §7 与 events.rs 模块保证 #1、`release_hold` 自身文档注释（:634-636「buffered events above it flush in order」）均被违反。

### R02-A10（缓存过期可恢复）：**PASS**

- 亲手重跑真实二进制矩阵 3 轮：purge（与 storage port 同谓词 seq < floor）后——HTTP 恢复返回 **409 cursor_expired**，`details.reason=snapshot_required` 且带新 floorSeq；WS 恢复收到 `snapshot_required` **控制帧且连接保持可用**；重建视图 == 当前 authority（a10-rebuild-diff equal，且与 inspector dump、服务页三视图一致）。
- 负例亲手探测（探针 crate adversarial_extras + 二进制矩阵）：未知 streamId（WS close 4404 / HTTP 404）、**伪造合法 checksum 的未来游标**（4409 FutureCursor——checksum 仅为完整性校验，权威判定在服务端三元检查，符合设计声明）、畸形游标（4409）、单连接重复 subscribe（4409）、跨主体页读（403）。`event_store_reads.rs` 的 purge 语义（removed=4、floor 移动、空区间/未知流 no-op）与 WAL 篡改→Corrupted 响亮失败均独立复跑通过。

## 4. 对抗变体结果汇总

| 变体 | 载体 | 结果 |
|---|---|---|
| A：hold 期间单事件 + release 后新事件 | vendored 确定性探针 | **复现 F01**：丢 seq=3，实收 [4] |
| A2：hold 期间 lossy delta | vendored 确定性探针 | **复现 F01 变体**：丢失且不计 dropped_deltas（连丢失统计都绕过） |
| B：重启基线 known_head 对齐后 pending 滞留 | vendored 确定性探针 | **复现 F02**：register(known_head=2) 置 next_seq=3 且 pending{3} 无触发者，后续 publish[4,5] 永久停驻 |
| 生产 race 施压（page-capped） | 真实 lingxi-service ×500 订阅 | 63 个空洞全部 cut==page cap → next_cursor 可恢复（公平性说明：不归入不可恢复丢失，但证明窗口在负载下频发） |
| 生产 race 施压（uncapped 分批） | 真实 lingxi-service ×200 订阅 | SILENT_HOLES=0；窗口窄 |
| 生产重启基线施压 | 真实 lingxi-service ×150 | 0/150 窗口（subscribe 恢复总先于竞态 commit） |
| purge-all 后陈旧合法游标 | adversarial_extras | 服务器按 FutureCursor 拒绝（WS 4409）而非 snapshot_required 指令（F03，响亮但词汇不一致） |
| 伪造 checksum / 篡改 checksum / 跨主体 | adversarial_extras + 二进制矩阵 | 全部拒绝；cursor checksum 确认为「防事故不防恶意」，跨主体读在 ownership 层先于游标检查被拒，**无跨主体泄漏面** |

## 5. Findings

### F01（BLOCKING）— hold-join 静默丢失 seq > cut 的已缓冲事件

- **位置**: `rust/crates/lingxi-service/src/events.rs` — :530（去重 return）、:543（hold 分支推进 `last_enqueued_seq`）、:638-653（`release_hold` flush 循环重新进入 `enqueue_for`）；触发窗口在 `EventService::subscribe` :978-:1012（register→await 读→release_hold 无同步）。
- **证据**: 探针 A/A2（vendored 字节一致副本，diff 恰好 2 行可见性；确定性复现）；生产代码施压 run 2（63/500，page-capped 可恢复区间）证明交错在负载下频发。
- **问题**: `release_hold(cut)` 的文档承诺「buffered events above it flush in order」不成立——hold 期间每个事件已把 `last_enqueued_seq` 推进（:543），flush 时 `enqueue_for` 首查 :530 即命中去重而丢弃所有 seq > cut 的 held 事件。
- **为何违反任务**: R02-A09「关键事件不漏不重」；任务书 02 §7 的 snapshot+cursor airtight join 论证（单写者 seq 是密闭性基础，但 join 的最后一环被去重逻辑掐断）；events.rs 模块保证 #1。
- **后果**: 窗口命中时订阅者永久静默丢失关键事件（含 run 终态转移），无 detach、无 snapshot_required、无丢失计数；`last_enqueued_seq` 已被推进，后续同 seq 更正亦被抑制。
- **根因**: 单一 `last_enqueued_seq` 同时承担「hold 期去重」与「live 期去重」两个语义，flush 路径没有区分「从 hold 重放」与「新到事件」。
- **同类路径**: :549 live 分支同字段推进（正常）；重启基线路径的 pending 滞留是同一「推进水位而无 drain 触发」模式（见 F02）。
- **修复要求（验收标准，非实现指定）**: flush 路径不得被 :530 去重命中——例如 release 时先把 `last_enqueued_seq` 重置为 `min(cut, 原值)`/按 cut 重锚，或为 hold flush 设旁路（绕过 seq 去重、保留 mailbox 有界语义与 detach 行为）；且必须新增能命中「durable 读完成→release_hold」窗口的 race 测试（multi_thread 或注入可控调度点/栅栏，禁止 sleep）。
- **必须重跑的测试**: events.rs 全部单测、event_subscription 套件 5 轮、二进制矩阵 3 轮、本报告探针 A/A2 应转为**反向断言**（修复后交付 [3,4]）。

### F02（MINOR）— 重启基线对齐后 pending 滞留，流永久破损且无信号

- **位置**: events.rs :606（`stream.next_seq = known_head + 1`）、:609（`pending.retain(|seq,_| *seq > known_head)`）。
- **证据**: vendored 确定性探针 B 复现（publish[3] 停驻→register(known_head=2)→next_seq=3、pending{3} 保留但无任何 drain 触发→publish[4,5] 永久停驻）；生产施压 0/150 窗口（该交错在生产上系统性难命中，故定 MINOR）。
- **问题**: retain 保留 == next_seq 的 pending 条目，而该条目的 drain 依赖「比它小的 seq 到达」，基线对齐后此条件永不再真。
- **为何违反任务**: 违反事件流「不停滞」的活性期望；下游影响超出单订阅：reorder-bound 断裂后流永久 broken，**新订阅者拿到快照后只剩沉默**，无任何控制帧信号。
- **后果**: 静默沉默（loud-failure 红线边缘）；触发条件窄（重启+恰好停在 next_seq 的 pending+新订阅者先至）。
- **根因**: 基线推进与 pending 清理不同步——retain 应同时 drain/drop == next_seq 的头部或显式标记 broken。
- **同类路径**: 与 F01 共享「水位推进后无补偿动作」模式，但根因独立，单列。
- **修复要求**: 基线对齐时同步处理 pending 头部（drain 至连续或置 broken 并发信号）；加确定性回归测试。
- **必须重跑**: events.rs 单测 + event_subscription 套件。

### F03（MINOR）— purge-all 后陈旧合法游标被判 FutureCursor（4409）而非 snapshot_required

- **位置**: subscribe 游标三元检查路径（events.rs :901 起，`stream_head` 为 None 的分支）。
- **证据**: adversarial_extras 探针 6（purge 全部事件后，持合法 checksum 的陈旧游标 → WS close 4409 FutureCursor）。
- **问题**: head 坍缩为 None 使「floor gap」检查先于/异于预期路径失败，客户端收到的是「未来游标」而非 A10 文档化的「snapshot_required + floor」指令。
- **为何违反任务**: A10 恢复流程词汇不一致；客户端按 4409 处理会走「重建连接」而非「重建快照」。
- **后果**: 响亮失败（非静默），恢复路径绕远但存在；定 MINOR。
- **修复要求**: purge-all 后 stale-cursor 显式映射到 RequiresSnapshot；补一条该形态的二进制矩阵用例。
- **必须重跑**: event_subscription 套件 + 二进制矩阵。

### F04（NON-ISSUE）— 报告 §2.5「与 envelope 无公共字段集」措辞不精确

- **位置**: R02-T05_REPORT.md §2.5。
- **证据**: 控制帧与 envelope 实际共享 `streamId` 字段名；但机制成立（envelope `deny_unknown_fields`，控制帧带 `frameKind:"control"` 且从不进 key_events——grep 核验控制帧构造器仅产出传输 JSON，无任何存储调用）。
- **结论**: 措辞问题，不影响正确性；建议后续修订措辞，不阻塞。

## 6. 架构边界与安全/数据面

- **控制帧永不入 key_events**: grep + 代码路径核验通过；控制帧为独立传输类（`frameKind:"control"`）。
- **服务器不合成终态**: 终态转移仅来自 committed events（T04 链：CommittedOutcome.events 仅 Ok 路径、publish 严格在 commit 后，sessions.rs :61 行差分核验）。
- **无第二事件权威**: EventStorePort 是唯一 seq 权威（单写者 MAX(seq)+1 在提交事务内）；hub 仅内存投递。
- **DEP-03/08/09**: DEPENDENCY_RULES.json 对 e80c4b5d0 **零 diff**；分层干净（kernel ports ← adapters impl ← service 编排）。
- **订阅授权走 T03 链**: classify_route 将 GET `/sessions/{id}/events` 归入 Scope("chat")，未知动词 LocalOnly fail-closed（auth.rs +8 核验）；WS 票证 + per-session ownership；跨主体 403 实测。
- **游标 checksum**: 完整性校验（sha256(version-tag|stream|seq)），防事故不防恶意——与报告 §7.2 披露一致；本人伪造/篡改游标实测全部被拒。
- **数据面**: 我的全部测试与探针均在 /tmp（专属 target dir、/tmp 探针 crate、mktemp home）；未触真实用户目录；无密钥入码。
- **订阅泄漏面**: mailbox 有界（128，留 1 detach 信号槽）、hold 有界、超限→detach+snapshot_required（响亮），符合红线。

## 7. 范围与回归

- 工作树 15 项全部在 T05 范围（events.rs、ports.rs、run_store.rs、lib.rs、ws.rs、auth.rs、sessions.rs、Cargo.toml(test 配置)、service_persistence.rs(+1 行)、两测试文件、两脚本、报告、artifacts）。**无无关重构、无 T06 越界、无隐藏删除**。
- 回归：T01–T04 脚本全 PASS；Cargo.lock 零 diff；Node/Electron 零变化；contracts/generated 零漂移；ownership 检查器 OK。
- 审计封印族 exit 1 为 pre-existing：归因经 `git diff --name-only ab4f2281..HEAD`（127 个已提交文件）与 T05 未提交集合零交集确认；按红线如实记录，不提议任何坐标/白名单动作。

## 8. 执行者报告一致性

- §0 前驱处置表 12 项：3 项 clippy 修复（本人 clippy -D warnings 复跑 0 warning 佐证编译面干净）+ 2 项测试缺陷修复均**真实且为加强而非放松**——writer id 改为 `5_000 + w*10 + r`（含碰撞机制注释 :167-172，与 run_store.rs :570-596 replay 吸收路径一致）、WS 测试断言强化为 snapshotSeq="6" + seqs=[4,5,6]。逐项抽查与仓库状态吻合。
- §7.1 run_id 碰撞观察：**准确**。sessions.rs :322 `run_{now_ms:016x}_{total_runs+1:06x}` 的 total_runs 读取非原子，碰撞机制真实；replay 路径 `same && attempt_present → newly_committed:false`（run_store.rs :584-590）不比对输入即吸收——移交 R03 的表述诚实。
- §7.2 checksum 域披露：准确（见 §6）。

## 9. PASS 判据逐项

| 判据 | 结果 |
|---|---|
| A09 真实 PASS（我的重跑 + 独立对抗） | **不满足**（F01 反证成立） |
| A10 真实 PASS | 满足 |
| 生产路径真实接线（storage commit→CommittedOutcome→hub→订阅推送→客户端合并，真实二进制验证） | 满足（接线真实；缺陷在 join 末环而非接线缺失） |
| 无 BLOCKING | **不满足** |
| 无测试篡改（两处前驱修复为加强） | 满足 |
| 无无法解释的事件/授权缺口 | 满足（所有缺口均有确定归因） |
| 回归绿（封印红按背景归因） | 满足 |
| 证据与候选一致 | 满足 |

## 10. VERDICT

**FAIL**。修复 F01（及建议同修 F02/F03）后需重跑：events.rs 单测、event_subscription 套件（含新增的多线程/窗口可命中 race 测试）、二进制矩阵 ≥3 轮、本报告探针 A/A2 的反向断言；复审可针对 diff 增量进行。

---

*本文件是本轮验收唯一允许的仓库写入；全部对抗探针与施压代码位于 /tmp/r02t05-review-probe/ 与 /tmp 日志，未进入仓库。*
