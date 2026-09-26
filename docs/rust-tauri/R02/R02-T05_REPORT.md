# R02-T05｜事件顺序、快照和断线续读 — 执行报告

- 执行者：ZCode:EXECUTOR-R02-T05-r2（一次性执行代理；前执行者 exec-r1 超时中断后的恢复轮。
  不负责独立验收，不提交/推送；PASS/FAIL 判定归总控另派的独立验收）
- 状态：**READY_FOR_REVIEW**
- 日期：2026-09-26
- 任务书：`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R02_Rust独立服务、存储与事件基础.md` §4 R02-T05
  （场景 R02-A09「快照与订阅无空隙」、R02-A10「缓存过期可恢复」，均 REQUIRED）
- tested SHA：`3c7d05ca41c37019fc7a1a94beac5413c701466b`（= TASK_BASE_SHA = HEAD = 远端 HEAD）+ 本任务未提交改动（§9 全集）
- 平台：macOS 27.0 arm64（Darwin 27.0.0）；工具链 rustup 锁定 1.98.1（`rust-toolchain.toml`，经 `~/.cargo/bin`，
  Homebrew rust 未参与任何构建；本机 rustup CLI 为 1.29.1，锁定的 rustc/cargo 为 1.98.1，已核对）
- 依赖面：`rust/Cargo.lock` 零 diff（`git diff 3c7d05ca4 -- rust/Cargo.lock` 为空）。lingxi-service 的 tokio
  仅追加已锁版本的 `sync` feature flag（订阅 mailbox 的 Notify）；sha2 0.11.0 / base64 0.23.1 是 T03 已引入的
  既有依赖（cursor 校验和与 base64url 编码）。**零新增第三方依赖，零新锁条目。**

## 0. 前执行者改动处置记录（逐项）

开工基线：HEAD = TASK_BASE_SHA，工作树含 exec-r1 未提交改动（`git status` 9 个修改 + 3 个新文件）。
处置原则：逐项读代码核验 → 编译/测试实证 → 修正/保留，不盲信。

| # | 改动 | 评估结论 | 处置 |
|---|---|---|---|
| 1 | `lingxi-kernel/src/ports.rs`：新增 `EventStorePort` trait（stream_head/stream_floor/stream_events_after/purge_events_before，RPITIT，契约注释固化「读/维护半面、purge 不得静默制造空洞」） | 正确：与 T04 写半面（StoragePort）同处 kernel，无 DB 依赖（DEP-08 不破）；语义与任务书步骤 1/3 对齐 | **保留** |
| 2 | `lingxi-adapters/src/storage/run_store.rs`：`EventStorePort for RunDatabase`（同一单写者队列读 committed 行；`envelope_from_parts` 交叉校验 event_type 列 vs payload 标签，不一致=Corrupted 不猜）+ `purge_events_before`（单写事务 DELETE seq<before） | 语义正确；`stream_events_after` 的 LIMIT/ORDER 与 join 无空隙论证一致 | **保留 + 修正**：clippy `too_many_arguments`（8 参）→ 改为 `KeyEventRow` 元组单参（纯签名重构，语义零变化）；rustfmt 重排 |
| 3 | `lingxi-service/src/events.rs`（新，核心交付）：EventHub（per-stream 重排缓冲/去重/有界 mailbox/hold-join）+ EventService（订阅/快照/续读协议）+ 游标编解码 + 控制帧 | 架构正确（详见 §2）；两个 clippy 违规 | **保留 + 修正**：`SubscriptionFrame::Event(EventEnvelope)` → `Box<EventEnvelope>`（large_enum_variant）；测试里 `EventLimits::default()` 后改字段 → struct-update 语法（field_reassign_with_default） |
| 4 | `lingxi-service/src/lib.rs`：ServiceState 挂 events；`GET /lingxi/v1/sessions/{id}/events` 快照/续读页（严格 query 解析；`cursor_expired`→409+details.reason=snapshot_required）；WS 请求循环改 select!（入站帧 vs 订阅 mailbox），SubscribeEvents 处理（控制帧→快照逐帧→live；拒绝映射错误+close） | 正确：路由/错误面与 T03 既有风格一致；过期游标在 WS 是**控制帧而非 close**（连接保持以供重建后重订阅）——符合 A10「明确快照要求」 | **保留**（rustfmt 重排；Box 化跟随 #3） |
| 5 | `lingxi-service/src/ws.rs`：`WsClientRequest::SubscribeEvents{streamId, cursor?}`（deny_unknown_fields 维持） | 正确：snake_case tag=`subscribe_events`，`Cursor` 透明反序列化 | **保留** |
| 6 | `lingxi-service/src/auth.rs`：classify_route 增加 `/sessions/{id}/events` GET/HEAD → Scope("chat") | 正确：与会话读同 scope；归属在 events 服务内按 session 所有权逐请求复核；未知动词落 LocalOnly（fail closed） | **保留** |
| 7 | `lingxi-service/src/sessions.rs`：`execute_for` 增加 `events: &EventService` 参数，提交 Ok 后 `publish_committed`；单测注入真实 temp RunDatabase 的 EventService | 正确：发布严格在提交后（模块契约）；单测 publication 走生产代码 | **保留**（rustfmt） |
| 8 | `lingxi-service/tests/service_persistence.rs`：跟随签名加 `state.events()` | 正确 | **保留** |
| 9 | `lingxi-service/Cargo.toml`：tokio features +`sync`（带注释） | 正确且零锁变化（已实证） | **保留** |
| 10 | `lingxi-adapters/tests/event_store_reads.rs`（新）：head/floor/events_after 保真、purge 移 floor、event_type 篡改响亮拒绝（真实 SQLite 文件 + WAL 第二连接） | 断言有效（逐一核对：seq 界、往返保真、purge 行数/幂等/未知流、Corrupted detail） | **保留**（rustfmt） |
| 11 | `lingxi-service/tests/event_subscription.rs`（新）：A09 固定调度竞态矩阵（k=0..=8 每个提交边界）、并行订阅者、A10 过期重建、负向矩阵、WS 全链路 | **发现 2 处真实缺陷**（见下） | **保留骨架 + 修正 2 处**：(a) WS 重建步骤 `read_until_events(stream, 4)` 计数错误——purge(4) 后仅剩 seq 4,5,6 三个事件，死等第 4 个导致测试永久挂起（前执行者超时中断的直接原因之一）；改为 3 并强化断言（snapshotSeq=6、seqs=[4,5,6]）。(b) A09 写者 now_ms 撞号——两写者用相同 `5_000+r`，而 run_id=(now_ms, total_runs+1) 且计数读在并发下竞态 → run_id 碰撞被 T04 幂等吸收 → 只有 4 个事件，`merged reaches 8` 断言必败；改为 `5_000+w*10+r` 保证唯一（并加注释说明碰撞机理，见 §7 已知风险） |
| 12 | `docs/rust-tauri/R01/DEPENDENCY_RULES.json`：adapters planned→exists + DEP-09 adapters-no-service | **这是 T04 的 D5 授权更新**：T04 报告 §4.7 明确记载，但 T04 提交 dbfbc1856 实际未包含此文件（提交信息与文件集不符，注册表更新遗留为未提交状态被 exec-r1 继承）。内容与 T04 报告/REVIEW 描述逐字一致 | **保留**（非 T05 新增决策；D5 检查器实证通过：`python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py` → RESULT: OK，含 D5-reverse 与 DEP-09） |

前执行者未完成部分（本执行者补齐）：`scripts/rust-tauri/r02_t05_*.sh` 真实二进制矩阵脚本（不存在）、
artifacts 证据、本报告、全量门禁复跑。

## 1. 修改了什么

新增（本任务交付物）：
- `rust/crates/lingxi-service/src/events.rs`（1443 行，含 11 个单元测试）：事件订阅服务 + 快照/游标 + 流控（§2）。
- `rust/crates/lingxi-adapters/tests/event_store_reads.rs`（215 行，3 测试）：读半面 adapters 层实证。
- `rust/crates/lingxi-service/tests/event_subscription.rs`（1009 行，6 测试）：A09/A10 REQUIRED 场景 + WS 全链路。
- `scripts/rust-tauri/r02_t05_events_probe.py`（490 行，stdlib-only）：真实二进制 WS/HTTP 矩阵探针。
- `scripts/rust-tauri/r02_t05_events_matrix.sh`（164 行）：构建→启动→探针→交叉校验→优雅关停的证据脚本。
- `docs/rust-tauri/R02/R02-T05_REPORT.md`（本文件）；`artifacts/rust-tauri/R02/T05/`（证据，§6）。

修改（沿用/修正前执行者，见 §0）：`ports.rs`、`run_store.rs`、`Cargo.toml`(service)、`auth.rs`、`lib.rs`、
`sessions.rs`、`ws.rs`、`service_persistence.rs`、`DEPENDENCY_RULES.json`（T04 遗留授权更新）。

未触碰：任务书、`contracts/generated/`（门禁实证无漂移）、`.sync-audit/`、`PROGRESS.md`、
`ORCHESTRATOR_PROGRESS.json`、Node/Electron 生产入口、`lingxi-protocol`（冻结 envelope 零改动——
复用 `EventEnvelope`/`Seq`/`Cursor`/`ErrorCode::CursorExpired` 原样）。

## 2. 设计决定

1. **权威链**：T04 的「终态+关键事件同事务落库、`CommittedOutcome::events` 仅 Ok 路径存在」是唯一事实源。
   本任务只做两件事：(a) 读半面 `EventStorePort`（kernel，RPITIT，与写半面对称）；(b) 提交后发布
   （`execute_for` 在 commit Ok 后 `publish_committed`——hub 永远只见已持久事件，慢/缺席订阅者从存储追赶）。
   服务端**不合成终态**、UI 不补造终态：run 终态只来自 key_events 的 run_state_changed 行。
2. **无空隙 join（A09 核心）**：订阅注册（hold 态，`cut=HOLD_ALL`）→ 同流 durable 页读（单一读
   `stream_events_after`，快照与续读同一权威同一排序）→ `release_hold(snapshotSeq)`：hold 中 `<= cut` 的
   丢弃（快照已含），`> cut` 的按序投递。正确性依赖单写者 seq（提交事务内 `MAX(seq)+1`）：cut 之后提交的
   事件 seq 必 `> snapshotSeq`，之前必在 cut 内——合并视图无洞，重叠在**服务端**按 cut 去重。
   进程重启对齐：注册时以 `known_head` 为基线（仅该流零订阅者时），`<= known_head` 的发布视为重复。
3. **游标**：wire 上是协议 `Cursor` 不透明字符串；服务端解释 = canonical JSON（`{chk,q,s}`）+ base64url。
   `chk` 是 sha256(版本域标签|stream|seq) **完整性**校验（防意外损坏与盲伪造），**不是认证**——权威始终来自
   服务端三重检查：流绑定（他流游标→`stale_stream_cursor`）、seq ≤ durable head（伪造未来→`future_cursor`）、
   floor 间隙（`floor > cursor+1` → `snapshot_required`，HTTP 面 = 409 `cursor_expired` + details.reason）。
   永不空流应答、永不静默换偏移重播。
4. **流控（有界不静默）**：每订阅者有界 mailbox（默认 128，1 槽保留给 detach 信号）；溢出时文字增量
   （`model_call_delta`/`assistant_segment_delta`）可丢（计数、有界），关键事件不可丢——放不下即 detach
   （`snapshot_required` 控制帧，客户端重取快照重订阅）。并发提交的发布乱序由 per-stream 重排缓冲修复
   （默认 bound 64），超限 = 流标记 broken + 全部订阅者显式 detach（不用乱序投递掩盖发布间隙）。
   hold 同样有界（同容量），过长 cut 窗口按慢消费者 detach。
5. **帧分类（报告记录项）**：业务事件 = 冻结 `EventEnvelope` 原样（每 WS text frame 一条 canonical JSON，
   PROTOCOL_SPEC §5）；订阅层控制帧（`subscribed`/`snapshot_required`）是**独立传输类**：带
   `frameKind:"control"` + `type` 标签，与 envelope 无公共字段集（envelope 无顶层 `type`/`frameKind`），
   构造上可区分；控制流量永不写入 key_events。seq 一律十进制字符串（含控制帧内），wire 无裸 u64。
6. **快照分页**：页上限 500；截断时 `subscribed`/页带 `nextCursor`，客户端翻页并按 seq 去重有界重叠——
   边界显式、无静默跳变。R02 代表规模（每 run 2 事件）不触发分页，能力已实现并记录。
7. **R02 流命名空间**：`streamId == sessionId`（与 T04 写者一致）；API 全程 stream 键控，后续细化（按
   attempt）不改协议面。
8. **边界沿用（R01_HANDOFF）**：RR-T02-F3（payload 回退吞畸形已知事件挂 R04）——本任务**未新增**任何吞错
   回退：读半面 event_type 列 vs payload 标签不一致 = `Corrupted` 响亮失败，payload 非法 JSON 同样 Corrupted；
   RR-T02-F4（Seq 解析宽松挂 R04）——游标解码的 `q` 是严格 u64 JSON 数（非 Seq wire 解析路径），沿用现状，
   不放宽不收紧。

## 3. 关键调用链

写链：HTTP `POST /sessions/{id}/execute`（或 WS 侧等价）→ `sessions::execute_for` → `StoragePort::
record_run_started`/`commit_run_outcome`（单写者事务，seq=MAX(seq)+1）→ Ok 后 `EventService::
publish_committed(CommittedOutcome::events)` → `EventHub::publish`（去重/重排/deliver_next → 订阅者
mailbox/hold）。

读链（订阅）：WS `subscribe_events{streamId, cursor?}` → `EventService::subscribe` → 所有权（SessionStore::
get_for，同 T03 规则）→ `stream_head`（基线+future 界）→ 游标三重校验（含 `stream_floor` 间隙）→
`hub.register`（hold）→ `stream_events_after`（cut）→ `release_hold(snapshotSeq)` → `subscribed` 控制帧 +
快照 envelope 逐帧 + live（lib.rs select! 转发）。HTTP 面 `GET /sessions/{id}/events?cursor&limit` 同一
`subscribe` 权威，`events_page` 按请求 limit 重切页。

维护链：`EventService::purge_events_before` → 存储单写事务 DELETE（A10 前置的产品面；二进制矩阵经 WAL
第二连接施加同谓词）。

## 4. 测试列表（层级）

1. 单元（`events.rs` 内，11）：游标编解码往返+严拒（垃圾/错 pad/伪校验和/未知字段）、乱序发布按 seq 投递、
   seq/eventId 重复去重、重启基线中流续读、关键事件溢出 detach+显式信号、增量溢出丢弃但关键事件仍 detach、
   重排 bound 超限响亮断流、hold 缓冲与 cut 释放、guard drop 注销与发布期修剪、控制帧标记与 2^53+1 seq 字符串化、
   限位校验响亮。
2. adapters 集成（`event_store_reads.rs`，3，真实 SQLite 文件）：head/floor/events_after 保真（含 run_id/
   attempt/eventType 往返、分页 after/limit）、purge 行数/floor 移动/幂等/未知流、event_type 列篡改 = Corrupted。
3. service 集成（`event_subscription.rs`，6，真实存储+真实 hub+真实并发写者，无 mock 核心）：
   - `a09_snapshot_and_subscription_have_no_gap_at_every_commit_boundary`：**固定调度竞态矩阵**——
     current_thread 单线程 runtime，2 写者 × 2 run × 2 事件，k=0..=8 每个提交边界订阅（`stream_head` 的
     await 点推进写者、`yield_now` 步进，零 sleep），合并视图 vs 权威库逐事件断言 + 序列连续/无重/边界显式。
   - `a09_parallel_subscribers_see_identical_views`：双订阅者同视图 = 权威。
   - `a10_expired_cursor_gets_explicit_snapshot_required_and_rebuild_matches_authority`：真页游标→真 purge→
     WS/HTTP 双面显式 directive→重建=当前权威（起于 floor）。
   - `a10_negative_rejections_are_loud`：伪未来游标/畸形游标/他流游标/未知流/跨 principal。
   - `a10_duplicate_event_publication_is_deduped`：发布入口幂等重放零重投。
   - `ws_subscribe_snapshot_live_and_rejections`：T03 WS 通道全链路（ticket/bearer 升级→lingxi.wire 握手→
     订阅→快照帧→认证 execute 的 live 推送→重复订阅 close 4409→未来游标 4409→未知流 4404→过期游标控制帧且
     连接保持（session_read 应答证明）→重建 cut 起于 floor→HTTP 409 细节→跨 principal 403）。
4. 二进制矩阵（`r02_t05_events_matrix.sh` + `r02_t05_events_probe.py`，18 案例 + 独立交叉校验）：见 §5。

## 5. R02-A09 / R02-A10 逐项结果

全部命令均剥离代理变量、`--locked`、`CARGO_NET_OFFLINE=true`、专属 `CARGO_TARGET_DIR=/tmp/rust-target-r02-t05-r2`、
合成 /tmp home。证据根：`artifacts/rust-tauri/R02/T05/`。

### R02-A09 快照与订阅无空隙（REQUIRED）

| 要求 | 命令 | 退出码 | 结果 | 证据 |
|---|---|---|---|---|
| 前置：快照生成时持续产生事件 | cargo test `-p lingxi-service --test event_subscription a09_snapshot`（2 真实并发写者持续提交） | 0 | PASS（k=0..=8 全边界） | `gates/cargo-test-workspace.log` |
| 操作：竞态点订阅并合并 | 同上（固定调度：head 读 await + yield_now，零随机 sleep） | 0 | PASS | 同上 |
| 通过：关键事件不漏不重、边界明确、与权威逐事件一致 | 同上（合并视图 vs `stream_events_after` 全量逐事件相等；seq 连续无重；`snapshotSeq` 显式） | 0 | PASS | 同上 |
| 证据：固定调度竞态测试多轮 | k 矩阵 9 轮 + 并行订阅者轮 | 0 | PASS | 同上 |
| 二进制面：真实服务进程 | `bash scripts/rust-tauri/r02_t05_events_matrix.sh artifacts/rust-tauri/R02/T05` | 0 | PASS：预种子 2 run 后订阅（非空 cut，snapshotSeq≥4），竞态写者 4 run 期间 live 续读；合并 12 事件 = durable 12 逐 (seq,eventId) 相等；6 个 run 终态转换如实到达（服务端零合成） | `matrix.json`、`a09-merged-view.json`、`probe-matrix.jsonl`、`summary.txt`、`matrix-run.log`、`service.err`（全 INFO，含优雅关停） |

### R02-A10 缓存过期可恢复（REQUIRED）

| 要求 | 命令 | 退出码 | 结果 | 证据 |
|---|---|---|---|---|
| 前置：cursor 超出保留范围 | cargo test `a10_expired_cursor`（真页 limit=4 游标@4 → 产品面 purge(7)） | 0 | PASS（removed=6，floor=7） | `gates/cargo-test-workspace.log` |
| 操作：请求续传 | 同上 + HTTP/WS 双面 | 0 | PASS | 同上 |
| 通过：明确 snapshot_required（非空流非静默重播）；重建=当前权威 | 同上（WS=RequiresSnapshot{floor,reason:events_truncated}；HTTP=409 cursor_expired+details.reason=snapshot_required；重建起于 floor 且逐事件=当前权威库） | 0 | PASS | 同上 |
| 证据：续传与快照 diff | 二进制矩阵：purge(8) removed=7 → HTTP 409（floorSeq=8）+ WS 控制帧（连接保持，session_read 应答）→ 重建 7 事件 == 权威页 7 项 == inspector dump 保留行 | 0（脚本） | PASS | `a10-rebuild-diff.json`、`key-events-dump.json`、`matrix.json` |
| 负向：陈旧 stream 拒绝 | 未知流 WS close 4404 stream_not_found / HTTP 404；他流游标 stale_stream_cursor | 0 | PASS | `matrix.json`、cargo a10_negative |
| 负向：伪造/未来 cursor 拒绝 | **有效校验和**+seq 99999 > head 14 → invalid_message + close 4409（证明界来自 durable head 非校验和） | 0 | PASS | `probe-matrix.jsonl` neg-future-cursor |
| 负向：重复 eventId 去重 | 发布入口幂等重放零重投（cargo）+ 订阅面 last_enqueued_seq 去重（单元） | 0 | PASS | `gates/cargo-test-workspace.log` |
| 负向：跨 principal | 设备凭证（他人）读页 403（同 T03 所有权链） | 0 | PASS | `matrix.json` neg-cross-principal |

### 门禁与回归

| 门禁 | 命令 | 退出码 | 证据 |
|---|---|---|---|
| cargo fmt --check --all | 锁定 1.98.1 | 0 | `gates/fmt-check.log` |
| clippy --workspace --all-targets -D warnings | 同上 | 0 | `gates/clippy.log` |
| cargo test --workspace --locked | 同上 | 0（**185 passed / 0 failed**；T04 基线 165 + 本任务 20） | `gates/cargo-test-workspace.log` |
| 冻结契约无漂移 | `r01-t02-check-generated.sh`（schema+golden+API 矩阵） | 0 | `gates/generated-contracts-check.log` |
| D5 依赖规则 | `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0（含 D5-reverse、DEP-09） | 终端输出（本报告 §0.12） |
| T01-T04 回归 7 脚本 | smoke/boundary/dual-instance/path-priority/env-token/auth-matrix/storage-tx | 各 0 | `gates/r02_t0{1..4}_*/summary.txt` |
| 审计封印族 | `npm test -- --run tests/post-verification-audit-seal.test.ts` | 1（**1 failed / 2 passed，预存在红**：封印坐标落后于已授权 R02 提交，与 T04 开工基线同款） | `gates/audit-seal-baseline.log` |

## 6. 未验证内容与限制

- 本机单平台（macOS arm64）；未在 Linux/Windows、release 优化构建或真实打包产物上运行。
- 未做跨进程重启的 live 订阅续读（重启后订阅断开，客户端走快照重建——A10 已覆盖重建面；hub 重启基线
  对齐有单元测试覆盖，无二进制级 kill -9 中断重放矩阵，属 T06 关闭/恢复与 R03 范围）。
- 快照分页（>500 事件）能力实现但 R02 代表规模未触发二进制级长流翻页矩阵。
- sqlite3 外部连接施加 purge 谓词是维护面的等价模拟（产品 `purge_events_before` 无 HTTP 面，本就不该有）；
  adapters 层已有真实 `purge_events_before` 直测。
- 审计封印族 1 红为预存在（坐标治理），按指令如实记录，未使其变绿、未动白名单。

## 7. 已知风险

1. **run_id 并发派生碰撞（T04 面，R03 处置）**：run_id = f(墙钟 ms, total_runs+1)，计数读非原子。同毫秒
   两个并发 execute 可碰撞同一 run_id；T04 幂等设计把第二个吸收为重放（`same && attempt_present` 不比对
   input）——两个不同用户意图得到同一 run、双方都收 200。无数据损坏/半终态（存储一致性完好），但「静默合并
   两个意图」在生产语义上值得 R03（运行状态机与并发）给出唯一 run_id 派生或显式 Conflict。本任务已在
   A09 测试注释与二进制探针注释中固化该机理；探针写者串行化以规避（被测竞态=写 vs 订阅，不受影响）。
2. **游标校验和域公开**：sha256 无密钥，知情者可构造任意 (stream,seq) 的合法校验和——防意外不防恶意。
   权威防线是 durable head/floor 界与流所有权（矩阵已证伪未来游标被拒）；如需防伪造需服务端签名的游标
   （可挂 R04/R08 协议扩展讨论，不属本任务）。
3. **hold/mailbox 默认 128 帧**：极慢消费者 + 高事件速率会频繁触发 detach 重建（正确但有代价）；R05 流式
   增量接入后需用真实速率复核限位。
4. **重排 bound 64**：同流 >64 个在途乱序发布才断流；R02 单会话双写者远达不到，R03 并发扩展时复核。
5. DEPENDENCY_RULES.json 的 D5 更新为 T04 遗留未提交状态被本任务继承（内容与 T04 报告一致，检查器通过）；
   提交时归入 T04 语义更准确，验收时注意归因。

## 8. 环境与安全声明

- 全部服务/测试/探针只用 mktemp 合成 home（/tmp/lingxi-r02t05-*），未触碰真实用户目录、真实凭证、
  任何现役数据库；每运行后清理。
- 无密钥/私钥入库；游标校验和是公开域标签哈希（§7.2 已声明非认证）。
- 代理变量从全部网络命令剥离；cargo 全程 offline+locked。
- 未 commit/push/PR/tag/release；未运行任何重写既有交付 gzip 的脚本。

## 9. git status --short（完工时全文）

```
 M docs/rust-tauri/R01/DEPENDENCY_RULES.json
 M rust/crates/lingxi-adapters/src/storage/run_store.rs
 M rust/crates/lingxi-kernel/src/ports.rs
 M rust/crates/lingxi-service/Cargo.toml
 M rust/crates/lingxi-service/src/auth.rs
 M rust/crates/lingxi-service/src/lib.rs
 M rust/crates/lingxi-service/src/sessions.rs
 M rust/crates/lingxi-service/src/ws.rs
 M rust/crates/lingxi-service/tests/service_persistence.rs
?? artifacts/rust-tauri/R02/T05/
?? docs/rust-tauri/R02/R02-T05_REPORT.md
?? rust/crates/lingxi-adapters/tests/event_store_reads.rs
?? rust/crates/lingxi-service/src/events.rs
?? rust/crates/lingxi-service/tests/event_subscription.rs
?? scripts/rust-tauri/r02_t05_events_matrix.sh
?? scripts/rust-tauri/r02_t05_events_probe.py
```

## 10. 推荐独立验收重点

1. **A09 固定调度矩阵有效性**：`event_subscription.rs::a09_snapshot...` 是否真实竞态（写者 spawn 于订阅前、
   head await 让进度、无 sleep），断言是否逐事件对权威（防「只对长度」的弱断言）。
2. **A10 双面语义**：过期游标在 WS 是控制帧且连接保持、HTTP 是 409+机读细节；重建后与 purge 后权威逐项相等；
   负向含**有效校验和的未来游标**（防只查校验和的假防线）。
3. **流控不静默**：单元测试证关键事件溢出 detach、增量丢弃计数；`hub.publish` 无吞错路径；
   `envelope_from_parts` 不一致=Corrupted（对照 RR-T02-F3 承诺零新增吞错回退）。
4. **帧分类**：控制帧带 `frameKind:"control"` 且与 envelope 字段集不相交；key_events 无控制流量
   （`key-events-dump.json` 可复核 eventType 全为业务词汇）。
5. **门禁复跑**：fmt/clippy -D warnings/185 测试/矩阵脚本/T01-T04 回归；审计封印 1 红为预存在坐标问题。
6. §0 前执行者处置表与 §7.1 run_id 碰撞观察的事实核对（`sessions.rs` run_id 派生 + `run_store.rs` 重放条件）。

---

READY_FOR_REVIEW

---

# 修复轮 R1（REPAIR-R02-T05-R1）— 追加记录

- 修复代理：REPAIR-R02-T05-R1（未参与原实现与验收；不 commit/push）
- 日期：2026-09-26；状态：**READY_FOR_REVIEW**（针对 REVIEW-R1 全部 findings）
- 输入：`R02-T05_REVIEW_R1.md`（SHA-256 `b85902a3b34f0b815376d0c5308d80f30143315a797a7c0eb0567f0848dbb1c8`，
  VERDICT FAIL：F01 BLOCKING，F02/F03 MINOR，F04 措辞）
- tested SHA：`e80c4b5d04b7697ae0277d74cbc661f777e65a39`（HEAD）+ 本任务未提交改动（含本轮修复）；
  平台 macOS 27.0 arm64；rustup 锁定 1.98.1；全程剥代理 + `--locked` + 专属
  `CARGO_TARGET_DIR=/tmp/rust-target-r02-t05-repair`
- 证据根：`artifacts/rust-tauri/R02/T05/repair-r1/`（索引见其 `REPAIR-NOTES.md`）
- 以上原报告 §1–§10 为修复前历史，未改写；F04 处理见本节第 4 项。

## R1.1 修复内容（F01/F02/F03 + F04 说明）

1. **F01（BLOCKING，hold-join 静默丢 seq>cut 的已缓冲事件）**
   - 根因：单一 `last_enqueued_seq` 同时承担 hold 期去重与 live 期去重两个语义。hold 期间每个
     缓冲事件推进水位（`enqueue_for` hold 分支），`release_hold` 的 flush 重新进入 `enqueue_for`
     时首查 `seq <= last_enqueued_seq` 即命中 → seq > cut 的 held 事件被静默丢弃，且水位已推进
     使后续更正也被抑制（无 detach/信号/计数）。
   - 修复（`events.rs::release_hold`）：flush 前把水位**重锚到 cut**。依据：调用方契约
     cut ≥ after_seq（cut 是从 after 起的 durable 读边界），重锚后水位语义恢复为
     「≤ cut 已由快照覆盖」，flush 按序重新推进，live 续投递。正确性论证：held 事件只在此
     投递一次，release 后的 live 事件 seq ≥ 流的 next_seq > max(held)，无重复可能；重锚与
     flush 同在 hub 锁内完成，中间水位无观测点；flush 仍走 `enqueue_for` 单一投递规则
     （保留 mailbox 有界、delta 计数、key detach 语义），未引入旁路分支。选择重锚而非
     hold-flush 旁路：改动最小且可证明（旁路需要在单一投递规则上开第二个参数化路径，
     面更大、证明面更宽）。
   - 同根因路径排查（指令第 3 条）：`last_enqueued_seq` 全部写点 = overlap 分支（推进=快照覆盖，
     正确）、hold 分支（推进无投递 → 本次修复点）、live 分支（推进=已投递，正确）；读点 =
     去重检查与诊断 stats。唯一 flush 路径 = `release_hold`。无其他受影响路径。
2. **F02（MINOR，重启基线对齐后 pending 滞留、流永久停驻）**
   - 修复（`events.rs::register` 基线对齐）：`retain` 之后排空恰好落在新 `next_seq` 上的
     parked 连续链。安全性：被排空事件均已经 publish（= 已提交于本注册之前）、对齐仅在
     零订阅者流上运行（无人需要即时投递）、自此刻起的每个 durable cut 都覆盖它们
     （页截断的 cut 经 `next_cursor` 翻页覆盖）——推进水位零损失并解除停驻。
   - 同模式排查：`publish` 的重排 drain 走 `deliver_next`（真实投递），broken 流为显式
     detach——无同类「推进无补偿」残留。
3. **F03（MINOR，purge-all 后陈旧合法游标被判 FutureCursor 而非 snapshot_required）**
   - 修复（`events.rs::subscribe` 游标校验）：floor 截断检查提前到 future 界之前，并新增
     「无 floor（purge-all 坍缩）且 cursor.seq > 0 → `RequiresSnapshot{floor:None,
     reason:"events_truncated"}`」分支（同时闭合 head/floor 两次读之间 purge-all 的
     TOCTOU 窗口）。非空流上的伪造未来游标仍为 `FutureCursor`（既有负例不变）。
   - 配套（`lib.rs` HTTP 409 `cursor_expired`）：`details.floorSeq` 仅在 floor 存在时输出
     （与 WS 控制帧「无 floor 不带 floorSeq」一致；floor=Some 的既有输出逐字节不变）。
   - 二进制矩阵新增 6 个 `f03-*` 用例（beta 流 purge-all → HTTP 409 无 floorSeq + WS
     snapshot_required 控制帧（无 floorSeq、连接保持）+ 空重建 cut，snapshotSeq=0）。
4. **F04（NON-ISSUE，§2.5 措辞）**：按验收要求不改代码、不改写历史小节；在此说明：
   控制帧与 envelope 实际共享 `streamId` 字段名，机制成立的真实依据是 envelope
   `deny_unknown_fields` 且控制帧带 `frameKind:"control"`/`type` 而 envelope 无这两个顶层
   字段（控制帧构造器只产传输 JSON，从不写入 key_events）。§2.5 第 5 点中
   「与 envelope 无公共字段集」应读作「与 envelope 无公共顶层判别字段集」。

## R1.2 新增/强化测试（修复前必失败、修复后全绿）

| 测试 | 对应 | 修复前 | 修复后 |
|---|---|---|---|
| `events.rs::held_event_above_the_cut_flushes_after_the_hold_advanced_the_watermark` | 探针 A 反向断言（收 [3,4]、水位=4） | FAILED | PASS |
| `events.rs::held_delta_above_the_cut_is_delivered_not_counted_lost` | 探针 A2（delta 投递、dropped_deltas=0） | FAILED | PASS |
| `events.rs::restart_baseline_drains_the_parked_event_at_the_new_next_seq` | 探针 B 反向断言（收 [4,5]） | FAILED | PASS |
| `events.rs::restart_baseline_drains_a_parked_chain_above_the_head` | F02 链式变体 | FAILED | PASS |
| `event_subscription.rs::a09_join_window_race_under_concurrent_writer_loses_nothing` | multi_thread 生产路径 race | FAILED ×2（`SILENT join hole at the seam (cut=480)`） | PASS ×5 轮 |
| `event_subscription.rs::a10_purge_all_stale_cursor_gets_rebuild_directive_not_future_cursor` | F03 | 行为为 FutureCursor | PASS |
| 二进制探针 `f03-*` ×6 | F03 真实二进制 | — | PASS ×3 轮 |

race 测试要点（响应「能真实命中 durable 读完成→release_hold 窗口、禁 sleep」）：
multi_thread(4) + **结构化调度点**——watcher 以 `yield_now` 自旋等待 `hub.subscriber_stats(k)`
出现（该状态恰在 `register(hold)` 时刻产生），随即在**第二独立 SQLite 连接**（同一 runs.db，
WAL 多连接，`RunDatabase::open`）上执行真实 `record_run_started` 并经生产入口
`publish_committed` 发布。机理说明：状态库单 FIFO 队列会把「快照后提交」的回复排在读回复之后，
`release_hold` 有结构性先手（这也是 REVIEW 未封顶施压 0/200 的机理；本轮复现并记录），
第二连接使提交真正落入读执行窗口，窗口命中由注册事件因果触发而非时序运气。
断言不变式：无 detach 的订阅必须无缝 join（live 首帧 = snapshotSeq+1、连续到 final head、
合并视图逐事件 == 权威库）；有 detach 者至少 detach 前无缝（响亮损失不算静默丢失）。
修复前 2/2 轮命中 `sub 0: SILENT join hole at the seam (cut=480)`；修复后 5/5 轮全绿。

## R1.3 验证命令与退出码（全部亲手执行；剥代理、--locked、专属 target dir）

| 门禁 | 退出码 | 结果 | 证据 |
|---|---|---|---|
| `cargo fmt --all -- --check` | 0 | 干净 | `gates/fmt-check.log` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 0 warning | `gates/clippy.log` |
| `cargo test --workspace --locked` | 0 | **191 passed / 0 failed**（185 基线 + 6 新增） | `gates/cargo-test-workspace.log` |
| `bash scripts/rust-tauri/r02_t05_events_matrix.sh` ×3 轮（真实二进制） | 0 ×3 | 每轮 24 探针用例全过 + `RESULT: ALL R02-T05 BINARY-MATRIX CASES PASSED` + 三视图交叉核对 OK | `matrix-r{1,2,3}.log`、`matrix-r{1,2,3}/` |
| `cargo test -p lingxi-service --test event_subscription --locked` ×5 轮 | 0 ×5 | 8 passed/轮，无 flake | 终端记录 |
| 修复前失败取证（三修复按差异精确回退重建，仅用于取证后即恢复） | 预期失败 | race FAILED ×2 + 4 个新单测 FAILED | `gates/prefix-failure-evidence.log` |
| T01–T04 回归 7 脚本（smoke/boundary/dual-instance/path-priority/env-token/auth-matrix/storage-tx） | 0 ×7 | 全 PASS | `gates/reg-r02_t0*.log` |
| `scripts/rust-tauri/r01-t02-check-generated.sh` | 0 | 契约零漂移 | `gates/contracts.log` |
| `git diff e80c4b5d0 -- rust/Cargo.lock` | 空 | 零 diff | — |
| `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0 | RESULT: OK | 终端 |
| `npm test -- --run tests/post-verification-audit-seal.test.ts` | 1 | **预存在红**（坐标 `ab4f2281` 滞后于已授权 R02 提交；与本轮改动零交集；未触碰坐标/白名单） | `gates/audit-seal-baseline.log` |
| 验收者探针 crate（vendored 副本同步为修复后逻辑，可见性差 2 行核验）hold_join_probes / adversarial_extras | 0 / 0 | 探针 A/A2/B 反向断言成立（3 passed）；purge-all 分支实测 `loud RequiresSnapshot directive` | `gates/reviewer-probes-postfix.log` |

## R1.4 R02-A09 / R02-A10 当前状态

- **R02-A09：成立（修复后）**。快照+续读合并语义（不漏不重、重叠服务端去重）由既有
  a09 固定调度矩阵（k=0..=8 逐边界、逐事件对权威）、并行订阅者同视图、二进制 a09 merged
  ==durable 三层继续钉住；本轮新增 hold-flush 确定性单测与 multi_thread race 测试把「join
  末环」补进断言面。既有用例（含重叠去重、重启基线续读、溢出 detach/delta 计数）行为不变。
- **R02-A10：维持 PASS，词汇补齐**。部分截断路径（floor>cursor+1）不变；purge-all 形态
  双面（WS 控制帧 / HTTP 409）返回 `snapshot_required/events_truncated` 且无 floor；
  重建 == 当前权威（含空流的显式空 cut）；其余负例全部复跑通过。

## R1.5 修复轮改动文件清单

相对验收轮，本轮新增/修改仅限：
`rust/crates/lingxi-service/src/events.rs`（3 处修复 + 4 单测）、
`rust/crates/lingxi-service/src/lib.rs`（floorSeq 缺省输出）、
`rust/crates/lingxi-service/tests/event_subscription.rs`（2 测试 + import）、
`scripts/rust-tauri/r02_t05_events_probe.py`（f03 用例 + execute session 参数）、
`scripts/rust-tauri/r02_t05_events_matrix.sh`（注释/汇总行）、
`docs/rust-tauri/R02/R02-T05_REPORT.md`（本节）、`artifacts/rust-tauri/R02/T05/repair-r1/`（证据）。

READY_FOR_REVIEW（REPAIR-R1）
