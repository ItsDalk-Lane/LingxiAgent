# R03-T04 报告｜attempt/stream 栅栏处理迟到结果（EXECUTOR-R03-T04-E01）

- 状态：**READY_FOR_REVIEW**（执行者口径；独立复核归属总控另派）
- TASK_ID：R03-T04（ACCEPTANCE_IDS：R03-A07、R03-A08）
- TASK_BASE_SHA：`9e319d64836d5a1656908a81e23ab5edf8370515`（分支 `codex/rust-tauri-migration`，无 commit/push；工作树候选留给总控冻结）
- 执行时间：2026-09-29（证据时间见 candidate-summary.txt / 各 log）
- 环境：macOS darwin 27.0.0 arm64；rustup 锁定工具链 **1.98.1**（全部命令经 `~/.cargo/bin` rustup 代理 + `--locked`；证据链统一 `CARGO_TARGET_DIR=/tmp/r03-t04-target`）；SQLite=rusqlite 0.40.2 bundled；无网络外发、无真实供应商；npm/桌面栈零触碰；**tokio test-util feature 未引入**（确定性方案：单线程 current_thread runtime + 通道锚点 + 门控替身 + 1ms 轮询/500ms 硬上限的有界真实等待）
- 交付物三件：**结果栅栏**（ResultFence 写前核对 + audit-only stale 记账 + 端口签名升级）、**请求去重**（requestId 绑定主体+会话+规范化内容摘要的提交面幂等）、**状态属性测试**（固定 seed，重复/乱序/延迟三形态，非法状态序列即失败）

---

## 1. 冻结语义溯源（先读规格与现状，再映射——不发明交互）

阶段书 R03-T04 原文 4 步 + 总控细化（派单 §总控细化）与现状对照：

| 冻结语义 | 规格出处 | Rust R03-T04 映射 |
|---|---|---|
| **所有异步返回带 runId/attempt/generation；在写状态前核对，不只在请求发出时检查** | 阶段书怎么做 1 | 端口签名升级：`TurnProviderPort::next_turn` 返回 `ProviderTurnResult{fence, turn}`、`ToolExecutorPort::execute` 返回 `ToolExecutionResult{fence, outcome}`（kernel ports.rs 新 `ResultFence{run_id, attempt, generation}`）；driver 在**消费/persist 之前**核对 `fence.matches_ctx(ctx)`（runs.rs `fence_verdict`，两处调用点 model/tool） |
| **工具/模型返回晚于取消、会话切换或重试时，保留审计但不推进已结束任务或新任务** | 阶段书怎么做 2 | 三层栅栏：① driver 写前核对（identity 不匹配 → audit `fence_mismatch`，run 响亮 `failed.provider_error`；取消先到 → audit `cancelled_before_write` + 走取消结算，内容永不落流）；② 存储面 `record_run_events` 身份底线升级：终态 run → 拒写 + audit `run_terminal`；未开 attempt → 拒写 + audit `attempt_never_opened`；**已开但被重试超越的旧 attempt → 拒写 + audit `attempt_stale`**（T01 只查"开过"，本 Task 收紧为"当前 attempt"）；③ 全新 `StoragePort::record_stale_result`——audit-only 落 `stale_result_audit` 表（migration V2），永不写 key_events/status/messages |
| **历史重连只订阅，不重新启动模型** | 阶段书怎么做 3 | 重连路径本就是 EventService 游标续读（R02 能力，subscribe 纯读）；本 Task 用永久测试锁住：`reconnect_resubscribes_only_and_never_restarts_a_model`（cut→cursor resume 后断言 runs 数不变、provider 到达通道为空、状态不变） |
| **重复提交采用明确 requestId 去重且校验请求摘要** | 阶段书怎么做 3 | 提交面新 `SubmissionDedup`（lingxi-service/src/dedup.rs）：key=(owner_kind, owner_subject, session_id, request_id)——**绑定可信主体+会话，非全局**；值为规范化内容摘要（CRLF→LF 折叠后全量 sha256，protocol `digest_arguments`）；同 id 同摘要→幂等 replay（返回原 run_id，`replayed:true`，不重执行）；同 id 改摘要→明确 `DuplicateRequestConflict`（不复用旧执行、不新增执行）；新 id 的 admission（busy gate+分配 run id+绑定）在 key 锁内完成，并发同 id 重复提交不可能插入第二次 admission |
| **并发属性测试生成重复、乱序和延迟事件；发现非法状态序列必须失败** | 阶段书怎么做 4 | `late_result_fencing_property.rs`（真实 RunDatabase、固定 xorshift seed `0x5eed0000a07df00d`、60 轮、每轮临时库、池内交付序种子洗牌）：每笔交付后断言 durable 序列合法性（终态不翻转、拒写不动流、恰好一条 audit、非法迁移响亮拒绝） |

**A08 绑定规则的细化落地**（总控细化原文）：「不能全局 requestId 让不同主体串用」由 key 结构保证——不同 principal kind（local_user vs device）或不同 session 使用同一 id 字符串互不可见（`request_id_namespace_is_scoped_to_principal_and_session` 实测三主体三命名空间互不串用）。

## 2. 实现与调用链（真实接线）

### 身份栅栏（kernel ports.rs — 领域规则归属层）

```
TurnProviderPort::next_turn(ctx, call, turn, input) -> ProviderTurnResult { fence, turn }
ToolExecutorPort::execute(ctx, call, request)       -> ToolExecutionResult { fence, outcome }
ResultFence { run_id, attempt, generation }
  ::of_ctx(&RunContext)          ← 健康适配器回显请求时的身份
  .matches_ctx(&RunContext)      ← 写前核对：三项全等才 admit
LateResultReason { RunTerminal, AttemptStale, AttemptNeverOpened, FenceMismatch, CancelledBeforeWrite }
  .name() → 稳定 audit 词表（run_terminal/attempt_stale/…）
StaleResultFact { reason, refused_event_types }   ← audit 载荷（只记事件类型，不记全文）
```

### 驱动面栅栏（lingxi-service/src/runs.rs，生产链路）

```
HTTP POST /lingxi/v1/sessions/{id}/execute                (lib.rs execute_session，传 ExecuteSubmission{input, request_id?})
  → SessionStore::execute_submission_for                  (sessions.rs，A08 去重见下)
  → RunSupervisor::drive_run                              (runs.rs)
      模型调用子任务返回 ProviderTurnResult：
        fence_verdict(root, &result.fence, &ctx):
          Current                                → 原有 turn 处理（persist_model_call 等）
          Stale(FenceMismatch)                   → record_stale_result(audit) + run 响亮
                                                    failed.provider_error（late_result_fenced.* 代码）
          Stale(CancelledBeforeWrite)            → record_stale_result(audit) + settle_cancellation
                                                    （完成的内容永不落流——O1 写侧竞速收口）
      工具调用子任务返回 ToolExecutionResult：
          Current                                → 原有 outcome 处理
          Stale(...)  + 取消                     → audit + 取消结算
          Stale(...)  + run 活着                 → audit + 当前 call 记 Unknown
                                                    （receipt 不可信：不盲重试、不伪成功）
      audit_late_result → port.record_stale_result（audit 失败=显式降级：tracing::error 标注，
                           栅栏本身已生效——run 自身状态写会自行暴露存储错误）
```

### 存储面栅栏（lingxi-adapters migrations V2 + run_store.rs）

- **Migration V2 `stale_result_audit`**（追加式；fingerprint 机制原样）：audit_id/run_id/session_id/attempt/generation/owner_kind/owner_subject/reason/refused_event_types/recorded_at；**无 FK**——audit 记录的是"到达的主张"，主张可以指向不存在的 run；索引 idx_stale_result_audit_run。
- `record_run_events` 升级（单写者 job 内：校验读 → 拒绝分支先以独立小事务 commit audit 行 → 返回 Conflict——拒绝事务的 rollback 不吞 audit）：
  - owner 三元组不一致 → Conflict（边界违规，不 audit——安全审计面管越权探针）；
  - run 终态 → 拒 + audit `run_terminal`（detail 保留 T01 的 "already terminal" 短语）；
  - attempt 未开 → 拒 + audit `attempt_never_opened`（保留 "never opened" 短语——T01 测试兼容）；
  - attempt 已开但非当前（`ORDER BY rowid DESC LIMIT 1` 取最近开的）→ 拒 + audit `attempt_stale`（新收紧：旧 attempt 的迟到结果不再入流）。
- `record_stale_result`（新 port 方法）：单事务只写 audit 行，任何 run 状态下都合法。

### 提交面去重（lingxi-service/src/dedup.rs + sessions.rs + lib.rs）

```
execute_session (lib.rs)
  request_id 形状校验（400，EndpointError::invalid_message）
  → SessionStore::execute_submission_for(&ExecuteSubmission{input, request_id?})
      归属检查（原语义）
      request_id = None  → admission()（busy gate + allocate_run_id；与 pre-T04 逐字节同路径）
      request_id = Some  → validate → DedupKey{owner_kind, owner_subject, session, id}
                           + normalized_request_digest_hex(input)
                           dedup.admit(key, digest, admission):
                             Replay    → 查 run_count → ExecuteAccepted{原 run_id, replayed:true}（不重执行）
                             Conflict  → SessionExecuteError::DuplicateRequestConflict（HTTP 409
                                         request_id_conflict，不可重试）
                             RegistryFull → 503 idempotency_registry_full（响亮，绝不静默去重失效）
                             Fresh     → admission 在 key 锁内执行；失败（Busy/RegistryFull/Storage）
                                         不留绑定；成功永久绑定 id→run
      → drive_run（无 id 路径与有 id 路径在此之后完全一致）
```

- admission 闭包是**同步**的（try_begin_run 内存门 + allocate_run_id 原子计数器），整个 admit 临界区无 await——同进程并发同 id 重复提交在锁上排队，随后 replay/conflict，**不存在半接纳可见态**（首个实现用 MutexGuard 穿 await 导致 future !Send，已重构为闭包式 admit——见 §10 复核重点）。
- `ExecuteRequest` 增可选 `requestId`（serde default + deny_unknown_fields 不变）；`ExecuteAccepted` 增 `replayed`（serde 仅 true 时序列化，wire 兼容加法）；`execute_for` 签名不变（内部委托 `ExecuteSubmission::plain`），既有调用者零改动。
- 注册表进程内存有界（cap 4096，满则 503 响亮）；跨重启语义归 R03-T07（重启后同 id 重试=全新完整校验的提交，如实文档化）。

## 3. 逐验收：预期 vs 实测

### R03-A07 旧结果不能复活任务 — **PASS（本机隔离环境，确定性替身；跨 attempt + 跨 Run 双形态）**

**形态 1（跨 attempt，`r03_a07_late_attempt1_result_vs_current_attempt_is_stale_only`）**：
- 前置：attempt1 可重试失败 → attempt2 开启并停在流读取（门控替身）；attempt1 此刻已被超越。
- 操作：经存储端口带外投递 attempt1 的迟到结果（`{run}-mc0009-done` model_call_completed）。
- 实测：`Conflict`，detail 含 "no longer the current attempt"；audit 表恰 1 行 `reason=attempt_stale, attempt={run}#a1, refused=model_call_completed`；投递前后 key_events 计数不变、messages=0；重复投递再拒再 audit（计数 2）；放行 attempt2 → run `completed.with_final`，最终消息= attempt2 答案，全流 `payload_json LIKE '%mc0009%'` 计数 0（迟到内容从未入流/成正文）。
- 证据：`a07-a08-fence-dedup.log` `R03_A07_TRACE attempt_stale` + evidence `r03_a07_attempt_stale`（late_event_leak_count="0"）。

**形态 2（下一 Run，`r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing`，验收原文「attempt2 或下一Run已开始」字面）**：
- 前置：run1 停在流读取 → `cancel_run_for` → run1 结算 `cancelled/cancelled.requested`；run2（同会话下一 Run）`completed.with_final`（最终=run2 答案）。
- 操作：投递 run1/attempt1 的迟到结果。
- 实测：`Conflict`（"already terminal"）+ audit 恰 1 行 `run_terminal`；run1 终态不翻转、messages=0；**run2 的 key_events/messages 计数投递前后不变，run2 流中 `mc0007` 计数 0**（跨 Run 零污染）；run2 最终正文不含 run1 内容。
- 证据：`R03_A07_TRACE run_terminal` + evidence `r03_a07_run_terminal`（cross_run_leak_count="0"）。

**写前核对（阶段书怎么做 1 的活链证据）**：
- `r03_a07_driver_fences_stale_tagged_model_result_loudly`：attempt2 的调用槽收到**以 attempt1 身份栅栏标记**的 Final（延迟/竞态适配器形态）→ driver 写前核对拒写：run `failed.provider_error` 响亮、messages=0、`mc0002` 事件计数 0（被栅栏的结果零状态写入）、audit `fence_mismatch, attempt={run}#a1`。
- `r03_a07_tool_result_with_stale_fence_records_unknown_and_audits`：工具返回带外来 generation=99 → 当前 call 记 **Unknown**（payload 含 unknown 恰 1 条；stale 工具内容 `tool-content` 计数 0=不伪成功）+ audit `fence_mismatch`；run 继续并以真实 Final 完成。
- `runs.rs::fence_verdict_requires_identity_and_no_cancellation`（单测）：匹配栅栏+活 run=Current；旧 attempt=FenceMismatch；**身份匹配但取消先到=CancelledBeforeWrite**（"在写状态前核对"的写侧竞速判定逻辑）。

### R03-A08 相同 requestId 不同内容拒绝 — **PASS（幂等冲突测试）**

- `r03_a08_same_request_id_different_content_is_rejected_as_conflict`：首次提交 `client-req-0001`（内容 A）被接收并结算（runs=1）；同 id 改内容 B 重发 → `DuplicateRequestConflict{request_id, recorded_digest≠submitted_digest}`（HTTP 面 409 `request_id_conflict`）；**runs 恒为 1**——不复用旧执行、不新增执行。证据 `R03_A08_TRACE conflict` + evidence `r03_a08_conflict`。
- 幂等半边（`same_request_id_same_content_replays_without_new_execution`）：同 id 同内容（CRLF 归一后等价）→ 返回**原 run_id** + `replayed:true`，runs=1；随后改内容同 id 仍 conflict。证据 `r03_a08_replay`（same_run=true）。
- 命名空间绑定（`request_id_namespace_is_scoped_to_principal_and_session`）：同 principal 跨 session、跨 principal kind（device 同 user）同 id 各自独立接纳；owner 回放仍命中自己的绑定（runs 计数证明无串用）。证据 `r03_a08_namespace`。
- 边界（`invalid_ids_are_refused_and_plain_submissions_are_undeduplicated`）：空白/超长 id → `InvalidRequestId` 且 runs=0；无 id 重复提交 3 次=3 个 run（pre-T04 行为原样）。证据 `r03_a08_invalid_and_plain`。

### 历史重连只订阅 — **PASS**

- `reconnect_resubscribes_only_and_never_restarts_a_model`：run 结算后 subscribe(None) 取 cut（snapshot_seq>0），持 cursor 续订阅得 `mode=resume`；断言 runs 计数不变、provider 到达通道空（无新模型调用）、状态不变。证据 `reconnect_read_only`。

### 状态属性测试（怎么做 4）— **PASS**

`late_result_fencing_property.rs`（真实 RunDatabase，seed `0x5eed0000a07df00d`，60 轮；每轮：run 启动→随机开 attempt2→池={当前 attempt 合法事件×2、超期 attempt 迟到结果×2（延迟+重复）、幽灵 attempt#7、结算（completed/cancelled 随机）、结算重复、冲突结算、非法迁移×2}→种子洗牌乱序交付）：
- **每笔交付后**断言：终态永不翻转；拒绝交付 key_events 计数不变；stale 类拒绝恰 +1 行 audit；非 stale 类拒绝零 audit（settlement 冲突归 T01 域）；messages 恒 0；非法迁移必拒。
- **非空泛化断言**：totals `current_ok=36, stale_refused=23, ghost_refused=26, terminal_refused=167, settle_first=60（=每轮恰一次首结算）, settle_replay=10, settle_conflict=110, illegal_rejected=120（=每轮 2 笔全拒）`——每一类都被真实行使。
- 证据：`a07-property.log`（逐轮 R03_T04_PROPERTY 行 + totals）+ evidence `r03_t04_fencing_property`。

### 任务书「怎么做」1–4 对照

1. 异步返回带 runId/attempt/generation + 写前核对 ✔（端口载体 + driver 双调用点 + 单测/集成双层证据）
2. 迟到结果保留审计不推进 ✔（audit-only stale 三 reason + driver 两 reason；不推进已结束/新任务由 A07 双形态与属性测试证明）
3. 重连只订阅 ✔（测试锁定）；requestId 去重+摘要校验 ✔（A08 全组）
4. 属性测试重复/乱序/延迟三形态、非法状态序列必失败 ✔（重复=同类交付二次+结算重复；乱序=种子洗牌含结算居中；延迟=结算后投递/attempt2 后投 attempt1）

## 4. 修改文件清单

修改（10）：`rust/crates/lingxi-kernel/src/ports.rs`（ResultFence 载体+端口签名+record_stale_result+LateResultReason/StaleResultFact+2 单测）、`lingxi-adapters/src/storage/migrations.rs`（V2 表+迁移测试适配）、`lingxi-adapters/src/storage/run_store.rs`（record_run_events 栅栏化+record_stale_result+current_attempt_of/insert_stale_audit/event_type_summary）、`lingxi-adapters/tests/migration_idempotency.rs`（版本常量改 supported_version()）、`lingxi-service/src/{lib.rs,runs.rs,sessions.rs}`、`lingxi-service/tests/{cancellation_tree,run_lifecycle,session_serialization}.rs`（端口适配）。
新增（4）：`lingxi-service/src/dedup.rs`（生产代码+6 单测）、`lingxi-service/tests/late_result_fence.rs`（A07+重连，5 测试）、`lingxi-service/tests/request_dedup.rs`（A08，4 测试）、`lingxi-adapters/tests/late_result_fencing_property.rs`（属性，1 测试）。
`rust/Cargo.lock` 零变化（`90111c4b…`=R02_HANDOFF）；三个 crate 的 Cargo.toml 零变化（sha256 见 candidate-summary.txt——无新依赖，摘要复用 protocol 既有 sha2/canon）。逐文件 SHA-256 见 candidate-summary.txt。**task_supervisor.rs 零触碰**（T03 审查 R1-D1 按派单条件递延——本 Task 的栅栏不需要改动该文件；见 §9）。

### 既有测试夹具改动（逐处+理由；无断言删除/放宽/改永真，无 skipped）

1. **端口签名适配**（run_lifecycle/cancellation_tree/session_serialization 各 2 处 impl + import）：替身返回值包一层 `ProviderTurnResult::of_ctx(&ctx_at_issue, …)`/`ToolExecutionResult::of_ctx(...)`——回显请求时身份，即健康适配器的默认行为；断言原文全部未动。
2. **migrations.rs 内部测试 3 处**：`apply_then_verify_is_idempotent` 期望值从硬编码 `[1]` 改为 `(1..=supported_version())`；`tampered_version_row_is_rejected` 的 UPDATE 加 `WHERE version = 1`（两行 receipt 后全表置 0 会撞主键而非走防篡改检查——保持测试意图即"单行篡改被检出"）；`newer_database_is_rejected_not_downgraded` 的"未来版本"从硬编码 2 改 `supported_version()+1`。均为**加法迁移后的等价适配**，防篡改/幂等/拒降级断言语义不变。
3. **migration_idempotency.rs 1 处**：版本断言从硬编码 "1" 改 `supported_version()`（同一意图：版本不得移动）。
4. sessions.rs 内部测试 FakePort：补 `record_stale_result` 最小实现（trait 满足编译；耐久行为由真实 adapter 测试覆盖）。

## 5. 验证命令与退出码（全部经 rustup 1.98.1 + `--locked`；verify-stage R03 未注册，未运行未伪造）

| 命令 | 退出码 | 结果摘要 |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | 无 diff |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 0 error/warning |
| `cargo test --workspace --locked` | 0 | **539 passed / 0 failed / 52 suites ok**（T03 为 520/0；+19 = late_result_fence 5 + request_dedup 4 + property 1 + dedup:: 6 + kernel fence 2 + runs::fence_verdict 1；含 R02 全量回归） |
| `cargo run -p xtask -- check-contracts` | 0 | API_COMPAT_MATRIX 626 entries 零漂移（生成物全部 drift-free） |
| `cargo run -p xtask -- check-boundaries` | 0 | DEP-07/08/D5 全 PASS |

定向过滤器（全部命中>0，filters.log）：late_result_fence 5、request_dedup 4、late_result_fencing_property 1；name 过滤 r03_a07=4、r03_a08=1、result_fence=1、late_result_reason=1、fence_verdict=1、dedup::=6、runs::=5、sessions::=8；回归子集 run_lifecycle 12、cancellation_tree 8、session_serialization 5、execute_concurrency 3、event_subscription 12、service_persistence 2、run_finalize_property 2、storage_transactions 7、migration_idempotency 2、lingxi-kernel lib 25（T03 为 23，+2 fence 单测）。证据重跑统一 `--test-threads=1`（共享 evidence JSON 的读改写需串行——并行首轮曾丢 r03_a08_conflict 键，串行重跑后 10 键齐全）。

## 6. 证据位置

`artifacts/rust-tauri/R03/T04-E01/`：`commands.log`（四命令+退出码）、`workspace-test.log`（539/0 原始输出 + EXIT_CODE=0）、`a07-a08-fence-dedup.log`（A07 四场景 + A08 四场景 TRACE + --test-threads=1 证据重跑 + dedup 单测）、`a07-property.log`（逐轮 R03_T04_PROPERTY + totals）、`filters.log`（13 组 suite 计数 + 8 组 name 过滤）、`r03-t04-evidence.json`（10 组机器断言：a07 attempt_stale/run_terminal/fence_mismatch_model/fence_mismatch_tool、a08 conflict/replay/namespace/invalid_and_plain、property totals、reconnect_read_only）、`candidate-summary.txt`（基线/HEAD+修改 10+新增 4 逐文件 sha256+Cargo.lock 与三个 Cargo.toml 零变化）。复跑后无残留进程、无 `/tmp/lingxi-r03t04-*` 遗留。

## 7. 测试替身与最小接口边界

- 允许侧：`FencedProvider`（脚本步进 + 可选门控停"流读取" + 可选**错标身份栅栏**——延迟/竞态适配器投递的忠实模型）、`DeferredDeliveryProvider`（捕获 attempt1 ctx、在 attempt2 槽返回旧身份标记的 Final）、`FencedTool`/`ForeignGenTool`（外来 generation=99 的错标）只在测试选定时刻产生外部响应或错误身份标记，经 `ServiceDeps` 注入**真实**组合根——真实 execute/cancel 入口、真实 SessionSupervisor/QuotaManager/RunSupervisor/取消树/TaskSupervisor、真实内核状态机与唯一 finalize、真实 RunDatabase 单写者事务（含栅栏 audit 事务与 V2 迁移）、真实事件发布与游标续读。替身不写状态、不落库、不 finalize、不参与身份 mint；属性测试直接驱动存储端口（与 T01 run_finalize_property 同层）。
- 取消原语侧：无替身模拟的"取消"；等待全部为 1ms 轮询+500ms 硬上限的真实有界等待。

## 8. 候选输入摘要

基线 `9e319d648…`（=HEAD，无 commit）；修改 10 + 新增 4 路径，逐文件 SHA-256 与说明见 candidate-summary.txt；`rust/Cargo.lock` 零变化（`90111c4b…`）。要点 digest：kernel ports `7341bbac…`、run_store `a18b3061…`、migrations `0432e17b…`、runs.rs `a6c1249d…`、sessions.rs `f6a94948…`、lib.rs `fb08aaec…`、dedup.rs（新）`31ad5e8e…`、late_result_fence.rs（新）`a35ff341…`、request_dedup.rs（新）`0eed4984…`、late_result_fencing_property.rs（新）`3cda07a2…`（以 candidate-summary.txt 为准）。

## 9. 未验证项 / 边界（如实）

- **跨重启去重**：SubmissionDedup 是进程内存有界表（cap 4096，满载 503 响亮）；重启后同 id 重试=全新提交（如实文档化，dedup.rs 模块头）。跨重启幂等/恢复分类归 **R03-T07**。
- **取消传输入口**：A07 经服务面 `cancel_run_for`/存储端口带外投递驱动（沿 T02/T03 先例；WS/HTTP 传输路由归 R06/R07）。
- **generation 语义**：RunContext.generation 当前恒为 1（execute_for 传入）；栅栏三重核对中的 generation 腿在 R04 工具目录代次接入后获得真实语义，本 Task 已按三重全等实现并有单测（other_gen=FenceMismatch）。
- **CancelledBeforeWrite 的多线程竞态窗口**：单线程 runtime 下 driver select 与写前核对之间无 yield 点，该分支不可确定性注入；逻辑由单测覆盖，行为由 biased-select 语义保证（取消优先）。T03 审查 O1 的写侧窗口就此收口（完成内容不再落流），Accepted-vs-AlreadyTerminal 的表面竞态仍为 O1 原样（无状态损坏）。
- **audit 表增长**：durable audit 表按到达记账（运维保留策略未设；本阶段为诊断事实，与 key_events 同性质）。
- **R1-D1 递延确认**：本 Task 未触碰 `task_supervisor.rs`（栅栏不需要），按派单条件 D1（drain 到期 abort 死代码）继续递延至下一个触碰该文件的任务。
- 无跨平台（本机 arm64 macOS）；npm/桌面栈未触碰；真实供应商/网络流归 R05（A07 用门控替身模拟流读取等待与延迟投递）。

## 10. 独立复核重点建议

1. **audit 不被回滚吞掉**：run_store `record_run_events` 拒绝分支先以独立 `with_write_txn` commit audit 行再返回 Err（单写者队列串行化校验读与 audit 写）；首轮实现把 audit 放进拒绝事务被 rollback 吞掉（实测 audit_count=0 抓出），复核该结构而非仅看测试绿。
2. **"当前 attempt"判定**：`current_attempt_of` 用 `ORDER BY rowid DESC LIMIT 1`（单写者 ⇒ rowid=开序）；T01 的 `retryable_provider_failure_reopens_attempt_on_the_same_run` 仍绿证明合法路径（事件在 attempt 当期写入）不受收紧影响。
3. **admit 闭包的串行化声明**：dedup `admit` 在锁内执行 admission（busy gate+原子分配，全同步无 await）；若未来 admission 引入 await，该结构必须重审（MutexGuard 跨 await 会 !Send——首个实现即因此被 Handler 边界拒绝，已重构）。
4. **A07 泄漏断言口径**：`payload_json LIKE '%mc0009%'` 计数=0（迟到内容从未入流）与 messages=0、最终正文含 attempt2/run2 答案——三者合成"不追加到当前正文/成功状态"的跨 attempt 断言。
5. **迁移测试适配非弱化**：§4 第 2/3 条的三处改动在加法迁移（V1→V2）下等价保持原断言语义（版本不动/单行篡改被检出/更新库被拒）。
6. **539/0 与套件计数对照**：T03 520 + 新增 19 逐项核对（§5）；R02 回归链全部原样绿。
