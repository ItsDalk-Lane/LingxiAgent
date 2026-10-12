# R02-T05 独立验收报告（REVIEW-R2）

- **Reviewer**: REVIEWER-R02-T05-R2（round 2，独立对抗验收；未参与实现与 R1 验收，不相信任何前代理声明）
- **Task**: R02-T05「事件顺序、快照和断线续读」（REQUIRED：R02-A09 / R02-A10）
- **Candidate**: HEAD(`e80c4b5d0`) + 未提交 T05 改动（含 REPAIR-R1，8 modified + 8 untracked，`git diff e80c4b5d0` 全集核对无范围外改动）
- **Base**: TASK_BASE_SHA `3c7d05ca41c37019fc7a1a94beac5413c701466b`
- **审查日期**: 2026-09-26
- **输入**: R02-T05_REVIEW_R1.md（SHA-256 `b85902a3b34f0b815376d0c5308d80f30143315a797a7c0eb0567f0848dbb1c8`，已亲算核对）、
  R02-T05_REPORT.md（含 REPAIR-R02-T05-R1 追加节）、`artifacts/rust-tauri/R02/T05/repair-r1/` 全部证据、任务书 01–06 +
  R02 §4 T05 + task-catalog/acceptance-catalog R02-T05/R02-A09/R02-A10 + 02_目标架构 §7。

## 0. 结论（VERDICT: PASS，附 1 条 MINOR 新发现）

**PASS**。R1 的四条 findings 全部真实关闭：

- **F01（BLOCKING）关闭成立**。我按「三修复精确回退法」在 /tmp 独立副本（非仓库）上重建修复前代码，
  race 测试确定性失败（`sub 0: SILENT join hole at the seam (cut=480)`，与 repair 证据
  `gates/prefix-failure-evidence.log` 逐字一致），4 个新单测同轮失败且 11 个既有单测不受影响（回退精确）；
  修复后 race 测试 10/10 轮全绿、无 flake。race 测试经审查为真实窗口命中（watcher 以
  `subscriber_stats(k)` 出现为 register(hold) 的因果调度点 + 第二 SQLite 连接使提交真正落入读窗口），
  是加强而非伪装。
- F02/F03 关闭成立（各自的修复前失败均由我独立复现）；F04 已按措辞说明处理。
- A09/A10 我亲自重跑 + 对抗全部通过；修复引入面（重锚后的重叠去重、floorSeq 缺省、F02 drain 语义、
  race flakiness）逐项对抗后无新问题。

新发现 1 条 **MINOR**（非修复引入、非 A09/A10 判定障碍，详见 F-R2-01）：
purge 与 subscribe 的 floor 读/页读之间存在 TOCTOU，部分截断落在两读之间时 resume 剪切会产生
**静默 seq 空洞**（无 directive、无信号）。我用真实 ServiceState + 产品面 purge 动态复现
（10 轮 × 240 并发 resume，累计 215 个空洞）。当前 R02 生产运行时**无任何 purge 调用方**
（grep 核验：purge 仅 port/测试/矩阵面，无 HTTP/WS 路由、无调度器），故不构成 R02 的活缺陷；
但 R03+ 接入保留策略前必须修复并加回归测试。

---

## 1. 环境与方法

- 工具链：`rust-toolchain.toml` 锁定 1.98.1；rustup 1.29.1（`~/.cargo/bin`，经代理调用解析到
  1.98.1，已亲核 `cargo --version` / `rustc --version`）。**shell 默认 PATH 解析到 Homebrew cargo
  1.93.0，所有命令显式前置 `PATH="$HOME/.cargo/bin:$PATH"`**。
- 全部 cargo 命令：`env -u http_proxy -u https_proxy -u HTTP_PROXY -u HTTPS_PROXY -u all_proxy
  -u ALL_PROXY CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=/tmp/rust-target-r02-t05-r2 --locked`（专属 target，与执行者/修复者目录隔离）。
- 独立阅读：任务书（01 通用约束、02 目标契约 §7、R02 阶段书 §4 T05、task-catalog R02-T05、
  acceptance-catalog R02-A09/A10）、R1 报告全文、执行报告全文（含修复节）、repair-r1 证据、
  `events.rs` 全文（1639 行）、`lib.rs` 事件端点/WS 循环、`ws.rs`/`auth.rs`/`sessions.rs` diff、
  kernel `ports.rs` EventStorePort、adapters `run_store.rs` 读半面、两个测试文件全部用例。
- 对抗载体：
  1. **三修复精确回退副本** `/tmp/r02t05-r2-refix/`（整棵 rust/ 拷贝，仅回退 3 处修复，diff 见 §3）；
  2. **vendored 探针 crate** `/tmp/r02t05-r2-probe/`：`events.rs` 字节级一致副本，diff 恰好 4 行
     （2 行 `crate::`→`lingxi_service::` 导入路径 + `pub` 于 `register`/`release_hold`，`diff` 输出存档），
     5 条对抗用例（P1–P5）+ 15 条原单测全绿；
  3. **purge-TOCTOU 压力探针**（真实 `ServiceState::bootstrap` + 真实产品面 purge，无 mock 核心）。
- **未修改产品源码/测试/配置/Task 实现；仓库唯一写入即本文件；/tmp 副本验证后不回灌。**

## 2. 验证命令清单与退出码（全部亲手执行）

| # | 命令（摘要） | 退出码 | 结果 |
|---|---|---|---|
| 1 | `git diff e80c4b5d0 --stat` / `git status --short` | 0 | 8 修改 + 8 未跟踪，全部 T05 范围（§8），无无关改动 |
| 2 | `cargo fmt --all -- --check` | 0 | 干净 |
| 3 | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 0 warning（日志 `/tmp/r02t05-r2-clippy.log`） |
| 4 | `cargo test --workspace --locked` | 0 | **191 passed / 0 failed**（与声称一致；日志 `/tmp/r02t05-r2-test.log`） |
| 5 | `bash scripts/rust-tauri/r02_t05_events_matrix.sh /tmp/r02t05-r2-matrix-r{1,2,3}` ×3 轮 | 0 ×3 | 每轮 **24/24 探针用例 ok**（含 6 个 f03-*）+ 三视图交叉核对 OK + 优雅关停；逐 case 复核：a10 双面（HTTP 409+floorSeq=8 / WS 控制帧）、f03 双面（409/控制帧**无 floorSeq** + 空重建 cut snapshotSeq=0）、neg-future-cursor（有效校验和 seq 99999 > head 14 → 4409）、neg-cross-principal 403 |
| 6 | `cargo test -p lingxi-service --test event_subscription --locked` ×5 轮 | 0 ×5 | 每轮 8 passed，无 flake |
| 7 | race 单测隔离 ×10 轮（`--test event_subscription a09_join_window_race`） | 0 ×10 | 10/10 绿，无 flake |
| 8 | `cargo test -p lingxi-adapters --test event_store_reads --locked` ×3 轮 | 0 ×3 | 每轮 3 passed |
| 9 | **回退法复现**：`/tmp/r02t05-r2-refix` 上 `cargo test --lib events::` | 101（预期） | **4 个新单测 FAILED**（F01×2 + F02×2）/ 11 个既有单测仍 PASS → 回退精确 |
| 10 | 回退副本上 `cargo test --test event_subscription` | 101（预期） | **race FAILED**（`sub 0: SILENT join hole at the seam (cut=480)`）+ **a10_purge_all FAILED**（`FutureCursor { seq: Seq(1), head: Seq(0) }`）；其余 6 用例 PASS → 与 repair `prefix-failure-evidence.log` 一致 |
| 11 | vendored 探针 `cargo test --offline`（P1–P5 + 15 原单测） | 0 | **20 passed / 0 failed** |
| 12 | purge-TOCTOU 压力（真实 ServiceState，10 轮 delay 0..36ms × 240 并发 resume） | 0 | **累计 215 个静默空洞**（详见 F-R2-01）——机制证实 |
| 13 | T01–T04 回归 7 脚本（smoke/boundary/dual-instance/path-priority/env-token/auth-matrix/storage-tx） | 0 ×7 | 全 PASS（日志 `/tmp/r02t05-r2-reg-*.log`） |
| 14 | `r01-t02-check-generated.sh` | 0 | 契约零漂移（56 generated + API_COMPAT_MATRIX 624 条） |
| 15 | `python3 -B r01_t01_check_ownership.py` | 0 | RESULT: OK（含 D5-reverse、DEP-09） |
| 16 | `git diff e80c4b5d0 -- rust/Cargo.lock` | 0 行 | **零 diff**；`-- desktop/ contracts/ package.json scripts/` 亦零 diff（Node/Electron/契约零改动） |
| 17 | `npm test -- --run tests/post-verification-audit-seal.test.ts` | 1 | **预存在红**（1 failed / 2 passed）；归因见 §7（含对 R1「零交集」措辞的修正） |
| 18 | `shasum -a 256 docs/rust-tauri/R02/R02-T05_REVIEW_R2.md` | 0 | 值见 §10 |

## 3. R1 findings 逐条关闭判定

### F01（BLOCKING）hold-join 静默丢 seq>cut 的已缓冲事件 → **关闭成立**

- **修复核实（读码）**：`release_hold`（events.rs:659–695）在 flush 前把 `last_enqueued_seq` 重锚到
  `cut`。契约核实：`EventService::subscribe` 中 `snapshot_seq = events.last().map_or(after, |e| e.seq)`
  （:1050），页读从 `after` 起升序读取 → **cut ≥ after_seq 恒成立**；重锚与 flush 同在 hub 锁内
  （:660 一次取锁），无中间观测点；held 事件只投一次（flush 后 live 首事件 seq ≥ 流 next_seq >
  max(held)），flush 仍走 `enqueue_for` 单一投递规则（mailbox 有界/delta 计数/detach 语义未旁路）。
  修复声明与代码一致，未发现旁路或第二投递路径。
- **回退法复现（我亲手）**：在 `/tmp/r02t05-r2-refix` 精确回退三修复（① 删重锚行；② 删 register 基线
  drain 循环；③ subscribe 游标校验恢复「future 界在前、无 purge-all 分支」），revert diff 仅此三处。
  结果：race 测试确定性失败 `sub 0: SILENT join hole at the seam (cut=480)`（与 repair 证据文件逐字
  一致）；4 个新单测 FAILED；**11 个既有单测全部仍 PASS**（回退没有误伤其他行为 → 三处回退精确）。
- **race 测试有效性（重点审查）**：
  - watcher 键 `hub.subscriber_stats(k)`：subscriber id 由 `next_subscriber_id` 顺序分配，测试内
    seed 阶段零订阅者，且断言 `subscription.subscriber_id() == k` 锁死映射 → stats(k) 出现当且仅当
    第 k 次 subscribe 的 `register(hold)` 恰好发生，是**因果调度点**而非时序运气；零 sleep。
  - 第二 SQLite 连接（`RunDatabase::open` 同一 runs.db，WAL 多连接）绕过状态连接单 FIFO 队列的
    「读回复先于后提交回复」结构性先手，使提交真正落入读执行窗口——这正是 R1 施压 0/200 的机理，
    修复说明对该机理的复述与代码事实一致。
  - 非 flaky：修复后 **10/10 轮隔离轮 + 5/5 轮整套**全绿；修复前（回退副本）**确定性失败**。
  - 非伪装：断言是 A09 保证本身（无缝 join 的首帧=snapshotSeq+1、连续到 final head、合并视图
    逐事件 == 权威库、有 detach 者至少 detach 前无缝），修复前失败、修复后通过，方向正确。
- **修复引入面对抗（vendored 字节级副本，diff 4 行存档）**：
  - P1（cut 高于全部 held：快照读经第二连接看到 4,5，其 hub 发布在 release 之后才到）→
    `seq <= cut` 重叠去重压制，`[6]` 唯一投递，水位正确；**快照覆盖 ≤cut 的事件零二次投递**。
  - P2（发布乱序跨越 cut：publish 6,5,4 倒序到达，next_seq 停在 4）→ 4、5 重叠压制、6 唯一投递、
    后续 7 无缝。
  - P4（double release_hold 滥用 + 幂等重放）→ publish 层 next_seq 去重兜底，恰一次投递。
  - P3（探针 A 反向断言）→ [3,4] + 水位=4。
  - 结论：重锚未破坏重叠去重、未引入重复投递、未引入旁路。

**判定：F01 真实关闭，race 测试为加强，无修复引入的新问题。**

### F02（MINOR）重启基线对齐后 pending 滞留 → **关闭成立**

- 修复核实：`register` 基线对齐（events.rs:608–631）在 `retain(seq > known_head)` 后排空恰好落在
  新 `next_seq` 上的连续 parked 链。安全性论证逐条核实：对齐仅在**零订阅者流**上运行（:597 has_subscribers
  门）；被排空事件均已经 publish（publish 严格后于 commit）→ 本注册前已提交；此后每个 durable cut
  都覆盖它们（页截断经 next_cursor 翻页覆盖）。我专项对抗「drain 是否可能把注册前未提交的事件错误
  放出」：drain 不投递任何人（零订阅者门）只是推进水位，任何未来 cut 都是存储读——不漏。
  已提交但 hub 未发布的 seq（对齐 `next_seq = known_head+1` 后迟到 publish）走 publish 层
  `seq < next_seq` 幂等去重，且其必在页读结果中（读后于注册/提交）→ cut 覆盖，无损失。
- 回退复现：`restart_baseline_drains_the_parked_event_at_the_new_next_seq`、
  `restart_baseline_drains_a_parked_chain_above_the_head` 在回退副本 FAILED、修复后 PASS（含于 191）。
- P5（vendored）：对齐按流隔离——另一流订阅者不受对齐影响，delivery 正常。

### F03（MINOR）purge-all 后陈旧合法游标被判 FutureCursor → **关闭成立**

- 修复核实：floor 截断检查（含 `None && cursor.seq > 0` 的 purge-all 分支）先于 future 界
  （events.rs:986–1027）；同时闭合 head/floor 两读之间 purge-all 的 TOCTOU（head 陈旧时
  floor=None 分支仍给出 directive）。
- 回退复现：`a10_purge_all_stale_cursor_gets_rebuild_directive_not_future_cursor` 在回退副本
  FAILED（`FutureCursor { seq: Seq(1), head: Seq(0) }`——正是 R1 记录的缺陷形态）、修复后 PASS。
- **负例未放松**（专项核对）：非空流伪造有效校验和未来游标仍 4409 FutureCursor
  （矩阵 `neg-future-cursor-rejected`：seq 99999 > head 14；`a10_negative` 同例）；畸形游标、
  跨流游标、未知流、跨主体全部维持响亮拒绝。
- floorSeq 缺省输出（lib.rs:947–955）：floor=Some 输出逐字节不变（`floorSeq="4"`/`"8"` 断言全绿），
  purge-all 双面均无 floorSeq（矩阵 f03-http/f03-ws detail 实测）→ **未破坏既有消费者断言**。

### F04（NON-ISSUE）报告 §2.5 措辞 → **按修复节说明关闭**

机制核实：key-events-dump（我 3 轮矩阵的 dump）中 key_events 的 event_type 全为业务词汇
（`run_state_changed`），无控制流量；控制帧带 `frameKind:"control"`+`type`，envelope
`deny_unknown_fields` 且无这两个顶层字段。措辞修正已记录于修复节第 4 条，不改历史小节——可接受。

## 4. Acceptance 独立判定

### R02-A09（快照与订阅无空隙；关键事件不漏不重）：**PASS**

- 固定调度竞态矩阵（k=0..=8）实读核实：真实并发写者（unique now_ms 防 run_id 碰撞被幂等吸收）、
  head await 推进调度、零 sleep、合并视图 vs 权威库**逐事件相等** + seq 连续/无重/边界显式
  （events.rs 断言非「只对长度」）。
- multi_thread race（新增）10/10 轮 + 回退法双向验证（§3 F01）。
- 真实二进制矩阵 3 轮：预种子→订阅（snapshotSeq=4）→竞态写者 live 续读→合并 12 事件逐
  (seq,eventId) == durable == inspector dump == 服务页（三视图交叉核对）。
- 并发订阅者同视图、发布幂等重放零重投、溢出 detach/增量丢弃计数等既有断言全部复跑通过。

### R02-A10（缓存过期可恢复）：**PASS**

- 部分截断：真页游标→产品面 purge→HTTP 409 `cursor_expired` + `details.reason=snapshot_required`
  + `floorSeq`；WS `snapshot_required` 控制帧且**连接保持**；重建 == 当前权威（7 事件 == 权威页 ==
  inspector dump，floor=8）。
- purge-all（F03 修复面）：双面 directive 无 floorSeq + 显式空重建 cut（snapshotSeq=0）——3 轮矩阵全过。
- 负面矩阵（伪造有效校验和未来游标/畸形/跨流/未知流/重复订阅/跨主体）全部响亮拒绝，词汇与 A10
  恢复路径一致。

## 5. 新发现

### F-R2-01（MINOR）— purge 与 subscribe 的 floor 读/页读 TOCTOU：部分截断可产生静默 seq 空洞

- **位置**: `rust/crates/lingxi-service/src/events.rs` `EventService::subscribe`——floor 读（:971–975）
  与页读（:1021–1024）是两次独立读，页读结果无连续性复检。
- **证据（动态，真实产品路径）**: `/tmp` 探针用真实 `ServiceState::bootstrap` + 产品面
  `purge_events_before`（同单写者队列）。10 轮（purge delay 0..36ms 扫描）× 每轮 240 个并发
  `subscribe(cursor@H-2)`：**累计 215 个静默空洞**。例（round 5，delay=20ms）：12/240 个 resume
  收到 `Started` cut，from_seq=22、首事件=24，而 cursor@22 的下一事件 23 已被 purge 删除——
  无 RequiresSnapshot、无错误、无任何信号，客户端合并视图永久缺 seq 23。
- **机理**: floor 读见旧 floor（无 directive）→ purge 把 floor 移到 cursor+1 之上 → 页读从
  cursor+1 之后开始返回 → 剪切以 `Started{mode:"resume"}` 交付且首事件 seq > cursor+1，
  服务器与客户端都无法察觉。
- **为何定 MINOR 而非 BLOCKING**:
  1. R02 生产运行时**不存在 purge 调用方**（grep 全仓核验：`purge_events_before` 仅 port 定义/实现、
     测试与二进制矩阵面；无 HTTP/WS 路由、无保留调度器）——当前产品该竞态不可触发；
  2. 非 REPAIR-R1 引入（T05 原始 subscribe 即如此；F03 修复收窄了 purge-all 形态，此为残留的
     部分截断形态）；
  3. 但一旦 R03+ 接入保留策略调用方，即成为**静默关键事件丢失**（违反任务书 02 §7 与模块保证 #2
     的红线），且我的压力显示负载下窗口不窄。
- **同类路径**: F03 已闭合 purge-all 形态；本条是同一「floor 是两次读」结构的部分截断残差。
- **修复要求（验收标准）**: resume 模式（cursor 存在）下对页读结果做连续性复检——首事件
  `seq > cursor+1`（而 cursor+1 ≤ head 已由 future 界保证已提交）→ `RequiresSnapshot`；或页读后
  重读 floor 复核。修复必须附「purge 注入于 floor 读与页读之间」的确定性回归测试（可参照 race 测试
  的第二连接/调度点方法），并在修复前把「保留策略调用方不得与订阅并发」写进 R03 交接约束。
- **必须重跑**: 全套 event_subscription + 二进制矩阵 + 本条新回归测试。

## 6. 架构边界与安全/数据面

- 控制帧不入 key_events（dump 实证 event_type 全业务词汇）；无第二事件权威（EventStorePort 与
  StoragePort 同库同单写者队列，hub 仅内存投递）；分层 kernel ports ← adapters impl ← service 编排
  （ownership 检查器 RESULT: OK，含 DEP-09/D5-reverse）。
- 订阅授权走 T03 链：`classify_route` 将 `GET /sessions/{id}/events` 归 Scope("chat")，未知动词
  LocalOnly fail-closed；WS 票证 + per-session 所有权逐请求复核；跨主体 403 实测（矩阵 + cargo）。
- 游标 checksum 为公开域完整性校验（非认证）——服务端三元检查为权威；伪造/未来/跨流/畸形实测全拒。
- 数据面：我的全部探针/副本/日志均在 /tmp；未触真实用户目录；无密钥入库。

## 7. 范围与回归

- 工作树 16 项（8 modified + 8 untracked）逐一核对属 T05 范围：8 modified（run_store/ports/Cargo.toml/auth/lib/
  sessions/ws/service_persistence）+ events.rs、两测试文件、两脚本、报告、REVIEW_R1、artifacts/
  T05（含 repair-r1）。**无无关重构、无 T06 越界、无隐藏删除、无契约/Node 改动。**
- 回归：T01–T04 七脚本全 PASS；Cargo.lock 零 diff；desktop/、contracts/、package.json 零 diff；
  契约生成零漂移；ownership OK。
- **审计封印红（预存在）归因——含对 R1 措辞的修正**：`verified-source-sha = ab4f2281`（R01 审计
  绑定提交），HEAD 及其已提交差异（127 文件）来自已授权的 R02 T01–T04 提交；封印测试只读已提交
  状态，T05 未提交集合对其结果**无影响**。精确化：R1 称该 127 文件与「T05 未提交集合零交集」，实测
  其中有 8 个路径交集（T01–T04 本就创建/修改了 T05 现在修改的同批源文件）——R1 该措辞不精确，但
  归因结论不变：红由坐标滞后（已授权 R02 提交）造成，非 T05 所致。按红线如实记录，未提议任何
  坐标/白名单动作。

## 8. 执行者/修复者报告一致性抽查

- 191 = 165（T04 基线）+ 20（T05 原始）+ 6（修复新增 4 单测 + 2 集成）——与我实测的分套计数一致。
- 修复节声称的「修复前必失败」清单：race ×2（同款断言消息）、4 单测、F03 行为——**全部由我的独立
  回退副本复现**；matrix-r{1,2,3} 与 probe f03 用例数（24 = 18+6）一致。
- 前执行者处置表（§0）与 §7 风险台账（run_id 碰撞、checksum 非认证、限位风险）抽查与仓库状态吻合。
- R1 报告文件 SHA-256 亲算与修复节引用一致（`b85902a3…dbb1c8`）。

## 9. PASS 判据逐项

| 判据 | 结果 |
|---|---|
| A09 真实 PASS（重跑 + 独立对抗 + 回退法双向） | **满足** |
| A10 真实 PASS（双面语义 + 重建一致 + 负例不放松） | **满足** |
| 生产路径真实接线（commit→publish→hub→mailbox/WS/HTTP，真实二进制三视图一致） | 满足 |
| 无 BLOCKING（R1-F01 真实关闭且无修复引入新问题） | **满足** |
| 无测试篡改（race 测试经回退法证明「修复前必失败」，为加强非伪装；既有断言无放松） | 满足 |
| 无未解释事件/授权缺口（发现项均有确定归因；F-R2-01 已解释并给定修复路径） | 满足 |
| 回归绿（封印红按背景归因，未触碰坐标/白名单） | 满足 |
| 证据与候选一致 | 满足 |

## 10. VERDICT

**PASS**。

R02-T05 在含 REPAIR-R1 的候选上通过独立验收：R02-A09/A10 真实成立，R1 全部 findings 关闭且修复
未引入新缺陷。附带条件（不阻塞本 Task，交接 R03 的强制项）：

1. **F-R2-01（MINOR）**：在保留策略获得任何生产调用方之前，修复 subscribe 的 floor/页读 TOCTOU
   （resume 首事件连续性复检或页读后 floor 复检）并补确定性回归测试；R03 交接中明确
   「purge 调用方上线前必须先落此修复」。
2. 审计封印红为预存在坐标滞后（已授权 R02 T01–T04 提交），按流程另行封印处理，不属本 Task。

---

*本文件是本轮验收唯一允许的仓库写入；回退副本与全部对抗探针位于 /tmp（/tmp/r02t05-r2-refix、
/tmp/r02t05-r2-probe、/tmp/r02t05-r2-*.{log,jsonl}），未进入仓库、未回灌产品代码。*
