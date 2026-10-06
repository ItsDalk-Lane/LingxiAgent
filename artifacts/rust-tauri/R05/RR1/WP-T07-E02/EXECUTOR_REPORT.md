# WP-T07 R2（R05-T07-修复-r1）执行者报告 — F38 关闭 + F21 同路径 usage 拾回

日期：2026-10-06。执行者：R05-T07-修复-r1（全新上下文）。基线 HEAD d80737b6
（未前进）；工具链 rustup 代理 `/Users/study_superior/.cargo/bin/cargo` → 1.98.1
（`rust/rust-toolchain.toml` 锁定）。本报告对应独立验收 r1 登记的 F38 缺陷清单
（RR1_ISSUE_MATRIX.json F38.fact）逐项修复，外加同路径扫描发现的一处真实缺陷
（五族 parse-Err 丢已观测 usage）与两条非阻塞测试腿。

## 1. 修复内容（逐项对 F38 清单）

| 清单项 | 交付 |
|---|---|
| ① runs.rs:1256-1270 取消竞态臂落行 | select 竞态臂在 `settle_cancellation` 前经新私有入口 `persist_model_call_cancelled_in_flight` 写台账行：outcome=`cancelled`、usage unknown（ReportedUsage::Unknown）、`transport_attempts=None`（drop 后尝试数未知——绝不 0/1 冒充）、身份=dispatch-moment descriptor（回合未解析，结果侧 served_by 不存在）、started=turn 起点/settled=now。同型扫描补第二处：fence Stale+`root.is_cancelled()` 臂（回合已结算被取消 fence 拒写）——行携带回合**真实** usage/attempts/身份，outcome=`cancelled`。两处都**不写** `model_call_completed` 事件：A09/C16『取消的 call 不以伪造 completed 事件收尾』纪律保持（cancellation_tree r03_a05 与 r05_t04 c16 既有反例**零改动**通过） |
| ② workerrpc.rs:1270-1305 deadline/drop 落行 | deadline 到期分支：确定性 `await` 落行（先于 Unknown 结算返回）。run 取消 drop 整个 execute future：`CallbackAbandonGuard`（RAII）在 Drop 里把行**脱离到运行时**（`Handle::try_current`+spawn；无运行时/写失败响亮 `tracing::error`，绝不静默）。记账统一经新 trait 方法 `WorkerModelPort::abandoned(AbandonedWorkerCallback)`（自有数据；默认 no-op；`BoundedWorkerModel` 转发 inner）；生产实现 `GatewayWorkerModel::abandoned` 经既有 `LedgerWorkerCallbackTrace` 写行：outcome=`cancelled`、usage unknown、attempts=None、身份诚实 `unreported`（workermodel 既有惯例）、invocation/cb_id/parent_tool_call/purpose 全随行、model_call_id 与 settled 回调同形（aux-{slot}-{invocation}-{cb_id}） |
| ③ transport_attempts nullable 化 | `lingxi_kernel::usage::ModelCallUsageRecord.transport_attempts: Option<u32>`（Some(0)=not-sent、Some(n)=观测计数、None=drop 后未知）；migration **v7**（`model_call_usage_rr1_f38_attempts_nullable`）：SQLite 不能 ALTER 去 NOT NULL → 表重建（建新表-搬行-DROP-RENAME-索引同构重建），旧行原值保留；run_store.rs record/query/load 与 `usage_row_to_record` 可空读写（负数仍 Corrupted）。v6→v7 升级腿：`r05_t07_persistence.rs::migration_v6_to_v7_makes_attempts_nullable_and_keeps_rows`（真实 v6 库+attempts=2 行原值保留+NULL attempts 取消行 round-trip） |
| ④ 屏障控制永久测试 | `r05_t07_rr1_usage_ledger.rs` 新增三腿：`rr1_f38_driver_cancel_of_in_flight_stream_leads_a_cancelled_row_across_restart`（ParkedSseStub：请求抵达屏障→`cancel_run`→行存在/outcome=Cancelled/usage unknown/attempts=None/started+settled 有值/身份 main；**重开 RunDatabase 后仍可查**）；`rr1_f38_worker_deadline_expiry_of_in_flight_callback_leads_a_cancelled_row`（真实 r04_t07_fixture 子进程链+短 invocation deadline+回调 dispatch 屏障→生产 abandoned 落账（unreported 身份、parent_tool_call driver 铸造可 JOIN、chat 两行正常并行）、重启可查）；`rr1_f38_run_cancel_drop_of_in_flight_callback_leads_a_cancelled_row`（长 deadline 下回调 dispatch 屏障→`cancel_run`→run 结算 cancelled→**RAII 守卫脱离落账**的行有界轮询到、parent_tool_call 可 JOIN、chat 工具回合行正常、重启可查）。测试设计说明（如实）：真实 aux 链回调自带与 invocation deadline 同锚点的自身预算，两者同一瞬间竞争、aux 侧先结算为 Failed 行（同为诚实记账、attempts=1，P4 腿已钉）——deadline/drop 分支的确定性形状用 parked 模型端口钉（`complete` 永不自行结算、`abandoned` 委托生产 GatewayWorkerModel），记账全走生产件 |
| ⑤ 记录订正 | F21.remaining：删『driver 取消窗口无行=§5/§10 既有登记』失实表述，改为 R2 交付事实+订正说明；MODEL_USAGE_SEMANTICS §5（transport_attempts 三态+取消不抹账）、§7.1（v7 迁移）、§9（F38 测试映射）、§10（订正：正常取消窗口都有行，**唯崩溃窗口**无行——与冻结 HEAD 登记一致，并保留订正历史说明）重写；R05_INTERFACE_EVOLUTION 新增 §31（abandoned 接口/nullable 列/取消臂写行不写事件/五族拾回四项登记） |
| ⑥（非阻塞建议）两专门腿 | `rr1_f21_unexpected_tool_response_failure_keeps_the_reported_usage`（无工具 aux 收到工具响应：HTTP1、行1、Failed、usage 30/11 存活）；`rr1_f21_partial_usage_survives_a_failed_aux_settlement`（空正文+仅输入半 usage：Failed、input=9/output=None/Partial{missing:[output_tokens]}）。**注意**：unexpected-tool 腿落地时揭出 R1『结构上已携带 usage』声称不成立——见 §2 |

## 2. 同路径扫描发现并一并修复的真实缺陷

『意外工具响应有 usage』腿首跑即红：无工具 aux 收到工具响应时 adapter 的
parse-Err（`provider requested unknown tool …`）经五族共用的
`finish → Err(error) => fail(error, retryable)` 分支构造全新
ProviderTurnResult（usage_report=Unknown）——**流上已到达的 usage 被 parse
错误丢弃**。五族（openai-completions / openai-responses / openai-codex-responses /
anthropic-messages / google-generative-ai）同型。修复：各 accumulator 新增
`observed_usage_report()`（completions=原始 usage JSON、responses/codex=terminal
聚合的 usage、anthropic/google=fold 克隆 finish+violation 严格重解码），
execute 的 finish-Err 分支 `with_usage_report(salvaged)` 附到 Failed 结果——
回合照常响亮失败，计费事实不随 parse 错误消失；解码路径与正常完成完全同一
严格解码器（`usage.rs::salvage_usage_report`），无任何放宽。

## 3. 自检（命令均经 rustup 代理 1.98.1；退出码如实）

- `cargo test --locked -p lingxi-service --test r05_t07_rr1_usage_ledger` → 13/13（含 5 新腿）
- `… --test r05_t07_persistence` → 5/5（含 v7 迁移腿）
- `… --test r05_t07_usage_trace` → 9/9（attempts 断言 Some() 机械适配）
- `… --test r05_t06_worker_model` → 10/10
- `… --test cancellation_tree` → 8/8（r03_a05 反例零改动）
- `… --test r05_t04_streaming` → 18/18（c16 反例零改动）
- `… --test r05_t03_protocol_adapters` → 12/12；`… --test r04_t07_mcp_and_workers` → 19/19；`… --test late_result_fence` → 12/12
- `cargo test --locked -p lingxi-adapters` → 25 个测试二进制全 ok（含五族 salvage 改动后的 goldens/rr1_replay/batch_terminal/usage_strict）
- `cargo test --locked -p lingxi-kernel` → 87/87
- `cargo fmt --all -- --check` → 0；`cargo clippy --workspace --all-targets --locked -- -D warnings` → 0
- `cargo test --locked -p lingxi-service --no-fail-fast` → **905 通过/1 失败**（`regression-lingxi-service-full-r2.log`，cargo exit=101 仅因该测试二进制）：唯一失败 r00_management_leaves 的 macOS ALF 环境项——非环回自地址 192.168.3.5 入站被本机防火墙拦（panic 文本自证环境归因），与 T03-T06 各轮记录同一环境项，非本包缺陷

## 4. 变异验证（隔离副本，工作树零接触）

见 `mutation-verification-r2.log`：MUT-A 删除 driver 取消臂的落行调用 →
driver 腿红（rows 0）；MUT-B 把 GatewayWorkerModel::abandoned 置空 → worker
腿红（无 Cancelled 行）。两条 F38 腿都真实钉住生产行为，非同义反复。

## 5. 如实登记的边界

- run-cancel drop 路径的弃置行是脱离到运行时的尽力落账（有专门测试腿钉住，
  见 §1④）；进程死亡本身仍属崩溃窗口（§10 唯一无行边界）。
- fence Stale 在 LIVE run（非取消，如 attempt 替换）下的迟到结果走既有
  stale_result_audit 审计路径（R03-T04 语义），不属取消窗口，未改。
- 真实 aux 链与 invocation deadline 同锚点竞争的 Failed-行形状由既有 P4 腿
  覆盖；deadline 分支的可测形状见 §1④ 说明。
