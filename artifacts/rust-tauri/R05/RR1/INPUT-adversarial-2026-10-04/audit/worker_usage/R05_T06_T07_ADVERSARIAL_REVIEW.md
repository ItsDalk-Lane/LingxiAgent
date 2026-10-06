# R05 T06/T07 独立对抗审查（Worker、Operations、Usage）

## 结论及边界

**本审查范围 VERDICT: FAIL。当前 T06/T07 不满足 R05 放行要求。** 已在原始生产 crate 上执行 8 条新反例测试，8 条均复现违反规格的行为；不是凭绿色测试数或实现报告推定。这里的失败测试按正确契约断言，触发的 FAIL 是缺陷证据。

- 仓库：`/workspace/scratch/9b4397d87a13/LingxiAgent`
- 被审 HEAD：`d80737b6cb9186c8a18c0f35923aac00249d45c3`
- 比较基线：`c549ff654508ab951e2cf39cf9d309fc9c6b8656`
- 工具链：Rust/cargo 1.98.1，Linux x86_64；共享环境记录 `../runtime/environment.json`。
- 主仓库只读；测试设备及报告在本目录；结束前 `git status --short`、`git diff --stat` 为空，HEAD 未变。
- 规范：2026-09-23 原任务书 R05 T06/T07/A11–A14，以及 2026-10-02 R05 专项指令正文与附录 B。专项原文在 `../source_taskbooks/灵犀/Lingxi_R05_完整执行与验收提示词_2026-10-02.md`；业务/UI 可到 R07/R08，并不豁免 R05 当前操作层的身份、预算、取消、资源授权和 usage 正确性。
- 外部网络只使用 loopback 协议替身与合成秘密。被测解析器、网关、CredentialService、OperationService、GatewayWorkerModel、QuotaManager、RunDatabase 全是本 HEAD 原产品代码；没有 mock 掉这些核心对象。
- 本目录 harness 复用原测试的建库/loopback 设备，新增测试单独过滤 `audit_`。这证明当前 Rust 公开操作/回调接口的行为；不冒称现役 Node UI 已切换，也不冒称正式二进制完整 worker 注册链已验收。

## 证据索引（8 条已复现）

| 编号 | 新反例测试 | 实際结果 | 日志 |
|---|---|---|---|
| P1 | `audit_resource_read_must_use_the_same_authorized_canonical_path` | PNG、WAV 两条路径都授权工作区内文件却读到进程 cwd 的工作区外同名文件 | `operations_probes.log` |
| P2 | `audit_video_poll_must_use_provider_job_id_and_preserve_legacy_tracker` | submit 返回 local-track/provider-vid；query 实发 `video_id=local-track` | `operations_probes.log` |
| P3 | `audit_cancelled_media_poll_must_not_download_or_publish_completed` | 已确认 `CancelledLocally`，迟到 query 仍下载媒体、登记 1 文件并返回 `Completed` | `operations_probes.log` |
| P4 | `audit_worker_failed_physical_request_must_leave_unknown_usage_row` | worker 模型 500：真实 HTTP=1，usage 台账=0 | `usage_probes.log` |
| P5 | `audit_operation_invalid_usage_must_not_persist_secret_payload` | 合法 embedding 成功，畸形 usage 数组内合成凭证全文进入持久化 `invalid_detail` | `usage_probes.log` |
| P6 | `audit_gemini_thought_tokens_are_separate_from_candidate_tokens` | 100 input + 10 candidates + 40 thoughts、total=150；当前契约算 output=10，正确总输出应为50 | `usage_probes.log` |
| P7 | `audit_rerank_non_numeric_usage_must_not_be_coerced_to_reported_zero` | 原始 `input_tokens:null/output_tokens:"7"` 被改成 0/7，并标 `Reported` | `usage_probes.log` |
| P8 | `audit_operation_deadline_includes_quota_queue_and_never_invents_http_attempt` | budget=25ms、配额占用180ms，184ms才返回；HTTP=0，仍记一行 `transport_attempts=1` | `usage_probes.log` |

复现命令（主仓库不作修改）：

```bash
source /workspace/scratch/9b4397d87a13/audit/runtime/env.sh
cargo test --manifest-path /workspace/scratch/9b4397d87a13/audit/worker_usage/Cargo.toml --test operations_audit audit_ -- --nocapture
cargo test --manifest-path /workspace/scratch/9b4397d87a13/audit/worker_usage/Cargo.toml --test usage_audit audit_ -- --nocapture
```

两条命令现都 exit 101，分别 3 failed / 5 failed。最终设备校正后的日志是 `operations_probes.log`、`usage_probes.log`；更早 `probes.log` 只含首批两个媒体测试，不用它替代完整索引。

## F-WU01 — 附件授权对象与实际读取对象不同（BLOCKING，优先修）

**要求：** T06-C03（大资源授权、大小有界、越权引用）；原 T06 step 3 / A11 资源交付。

**位置：**

- `rust/crates/lingxi-service/src/operations.rs:1239–1264`：`read_host_audio` 将授权返回值存为 `_scope`，却 `std::fs::read(path)`，随后才检查 25 MiB。
- 同文件 `1309–1334`：`read_image_reference` 完全同根因，20 MiB。
- `resourceaccess.rs:20–36,79–88` 明确返回的 `ResourceScope.path` 才是授权后的真实 canonical path；旧文件工具还使用 no-follow/目录句柄避免授权后链接替换。

**实证 P1：** 用户路径为合法相对文件名；传入的 cwd 下确实存在授权文件。进程 cwd 下另外放一个位于授权 root 外的同名合成秘密文件。两种 reader 最终都返回工作区外的内容，`actual_read_is_outside_workspace=true`。没有符号链接竞态或特殊权限即可触发。

**影响：** 公共附件读取接口会把错误文件交给后续图像/转写请求；授权不等于真正读取，可能外发不属于授权工作区的内容。同时当前大小检查发生于全量分配之后，文件超大时无法阻止内存耗尽。

**必须成组修：** 绑定并使用授权 canonical scope；复用已有安全打开/目录句柄纪律防止 TOCTOU；读取过程受限而非读完再拒；image/audio 两路同时修。回归相对/绝对路径、不同 cwd、目录链接及末端 symlink 替换、超限大文件、读取中增长/特殊文件、越权拒绝、正常文件不回归。不得仅把测试改成绝对路径掩盖问题。

## F-WU02 — Usage 生命周期、父子归属和查询结构不完整（BLOCKING）

**要求：** T06-C04（usage 可追到父工具）、A12（取消及 usage 可见）、T07-C01/C02/C03/C05/C09，原 T07 step 2/4（parentCall、耗时、状态，查询/导出字段）。

### 已实测的少记与虚记

- `workermodel.rs:269–285` 在 `AuxiliaryExecutor::complete` 错误时直接返回；只有 `289–310` 成功路径才记账。`models/auxiliary.rs:149–225` 在空文本/工具请求/Continue/Empty/Failed 时构造 `AuxiliaryFailure`，丢弃原结果的 usage、resolved identity、protocol、transport attempts。
- P4：模型真实返回 500，实际一条 HTTP、零台账。不是进程 crash，也不是 LIVE 环境不足；普通失败路径即可丢账。
- `operations.rs:247–269` sink 固定 `transport_attempts=1`；`embed:550–561` 把 dispatcher 的全部错误都视为物理请求已发。P8：排队过期导致 send 前拒绝、HTTP=0，仍写 attempts=1。
- 因 future drop/取消尚未结算的调用同样没有完成型 row；不得仅用“崩溃窗口登记”覆盖正常取消的对账义务。需明确区分 not-sent / sent / settled / unknown，而非所有失败都记1或全部不记。

### 源码确认的因果/字段缺口（未用运行时探针冒充验证）

- `workerrpc.rs:947` 收到真实 `ToolCallId` 却命名 `_call` 并丢弃；`:847–856,958` 另铸随机 invocation id。`workermodel.rs:143–144` 将该随机 id 作为 usage `cause_ref`；没有持久化 invocation→真实 ToolCallId/parentModelCall 的连接。
- 原测试 `tests/r05_t06_worker_model.rs:544–559` 只验证 trace.invocation 与测试 worker 自己 echo 的 request_id 相等，不能证明能从台账 JOIN 到父工具。`tests/r05_t07_usage_trace.rs:824–866` 直接调用 port 时人为把 invocation 写成父工具形状，绕过了真实 RPC 丢弃 `_call` 的路径。
- `operations.rs:184–193,247–255` 所有 operation 行固定 session/run/attempt/parent/cause=null。接口也不接收调用上下文。R06 的会话内 embedding/辅助调用无从传递真实归属；不能把所有 operation 一律当无关内部根。
- `lingxi-kernel/src/usage.rs:320–372` 的 row/query 缺开始/结束/耗时/结果状态；query 仅 owner/session/run。`storage/migrations.rs:251–274` 无耗时或请求状态字段；唯一 recorded_at 在 `run_store.rs:2863–2882` 返回时丢弃，日期筛选/导出所需日期不能从此查询结果取得。

**必须成组修：** 建立贯穿主/aux/worker/operation 的宿主调用上下文和统一结算事实，异常也携带 resolved identity/已知usage/真实HTTP尝试事实；传递并持久化真实父工具/父模型ID；operation允许合法独立根但不能强制丢弃有父任务的归属；补齐查询导出所需状态与时序字段及迁移。取消/late fence 仍防正文复活，但不能顺带删掉已发生的计费事实。

**自检矩阵：** success、500/429、401 refresh 成功/失败、空正文仍含usage、异常工具响应含usage、流中 partial、取消前/发后、排队超时HTTP0、存储写失败、重放去重；逐条对照外部HTTP数、attempts/账行、未知状态、真正父调用JOIN、owner/session隔离、重启后查询。保留现有“先落盘后发布”正确纪律，不能为了补账提前发布成功。

## F-WU03 — Operation usage 先被数值强制转换，错误详情还可保存凭证（BLOCKING）

**要求：** T07-C05/C06/C09；原 T07未知值、隐私/查询导出契约；凭证红线还关联T02。

**位置与实证：**

- `models/operations/rerank.rs:170–200` 在严格 decoder 前调用 `parse::js_number`。P7 的 null→0、字符串"7"→7 被包装成合法数值，`models/usage.rs:293–349` 于是标为 `Reported`。复制 incumbent 的宽松 JS 规范化不能凌驾 T07 新的真实usage契约。
- 同类 `operations/embedding.rs:376–385` 的 MiniMax `total_tokens` 也先转 f64、再转回 JSON；需同扫 null/字符串/超安全整数精度。
- `operations/embedding.rs:387–391` 原样保留 usage 数组；`models/usage.rs:278–285` 用 `format!("operation usage is {usage}...")` 将整个非对象值塞进 invalid detail；`service/operations.rs:568–584,263–264` 原样落库。
- P5 使用一条合法 embedding vector 响应，其 usage 数组内包含当前 provider 的合成 Authorization 凭证；operation 成功，查询到永久 row.invalid_detail 含完整凭证。不是“未测低风险注释”；`R05_BLOCKERS.md §5` 把此项列为非阻塞携带，与可达的持久化泄漏不符。

**必须成组修：** 严格数值校验应发生在保留原始事实的边界；缺失、null、异常值不得先变成0/可信数字；处理总量和分量一致性及整数精度。invalid detail 仅保留字段路径/类型/问题性质并有界，必要时同一凭证scrub，再落库/输出；不能复制整个任意上游payload。扫描 embedding、rerank、媒体/ASR所有usage输入与失败详情。

**自检：** 对全部operation作表驱动，覆盖missing/null/false/string/negative/float/huge integer/数组/嵌套对象及合成key原文和编码变体；query/export/db诊断秘密扫描；证明合法vector仍可成功但usage标invalid且不泄漏，真实0仍是0。

## F-WU04 — Gemini 推理token包含关系错误（BLOCKING）

**要求：** T07-C07、A14及协议公式表。

**位置：** `models/usage.rs:93–105` 将 `thoughtsTokenCount` 标成已包含在 `candidatesTokenCount`；`:263–269` 直接把candidate当output；kernel `usage.rs:100–105` 原样投影。测试 `r05_t07_usage_families.rs:96–105,182–200` 反而固定断言这个错误语义。

**P6：** 合法输入 `prompt=100,candidates=10,thoughts=40,total=150`，当前表将output报10，按该表消费也不会加thoughts；应表明候选输出与思考是分离数字，总输出/计费输出为50。Google官方说明由根审查者核对：`https://ai.google.dev/gemini-api/docs/generate-content/thinking` 明确输出成本计入output及thoughts，二者分字段。

**修复：** 先确定统一 output 的真实定义，再一致修复映射、wire投影、聚合/台账/文档/测试；若总输出字段选择包含thoughts，变更归一公式，若保留candidate字段则显式标分量不包含并提供正确合计。保留unknown（thoughts缺失不能凭空补0），不要对OpenAI已包含reasoning再次相加。

**自检：** 无思考、有思考、thoughts>candidate、重复累计快照、缺失组件、total交叉校验、五族横向一致性；预期来自官方语义，不来自当前实现。

## F-WU05 — 媒体任务缺完整身份及终态fence（BLOCKING）

**要求：** T06-C01/C02/C12、原A11/A12及异步生命周期；后续R07 UI/持久化延期不豁免当前operation的正确性。

**已实证：**

1. `operations.rs:868–875` 只将 task_id→Tracking，丢弃返回的 provider_task_id；`:912–917` query 把本地task_id当provider video_id，同时 legacy_task_id/model_name=None。P2 的网络实发错误ID。底层 `models/operations/video.rs:297–323,387–441` 明明区分主provider id与legacy id，service没保留需要的数据。原TS `core/media-adapters/agnes.ts:412–435` 也明确两ID和fallback语义。
2. `query_video:892–910` 仅在发请求前看Tracking；`cancel_media_task:935–948` 只改内存值；`:821–830` 的`settle_poll`不重新核对取消，下载并移除tracking后返回Completed。P3 在query请求发出、响应延迟时取消，得到CancelledLocally后仍有 `/audit-product.mp4` 新下载及1个登记文件、迟到Completed。

**同根因源码确认：**

- image/video共享同一个仅由裸task_id作键的map（`:173–177,284–288`），没有种类、provider、model、endpoint/generation或provider任务ID。
- image query `:761–767` 和 video `:887–891` 重解当前绑定；模型配置reload后老job可能发去新provider。此项为源码确认，未另加reload动态探针。
- 多个并发poll也没有任务级settling互斥/最终回执缓存，修复时必须一并验证一次交付。

**修复：** 宿主持有不可混淆任务记录，保存操作kind、host tracker、provider job/legacy ID、提交时的provider/model路由身份和合法凭证引用、generation、状态/取消信号、最终回执；不得将凭证明文复制到task。旧任务不能改绑新模型。查询/下载/落盘各边界接取消并以终态fence提交，失败清理部分产物；取消与完成线性化；无远程取消支持时继续诚实说local-only。

**自检：** 两ID不同、provider主query失败触发正确legacy一次、同裸ID跨provider/跨image-video、不兼容操作query、reload前后旧job、并发双poll、取消在HTTP/下载/写文件/完成提交前后；外部请求、产物数、终态、回执四方一致。

## F-WU06 — 操作总预算漏掉排队；macOS系统语音另绕过预算/取消（BLOCKING）

**要求：** T05-C01/C02（总预算含排队/刷新），T06-C12与A12（辅助/媒体生命周期），原T06 step 2；system-speech为本阶段已声明实现的操作，不是未来UI功能。

**已复现 P8：** `operations.rs:427–430` 配额wait不接deadline，`:546–547`等方法先resolve再admit，deadline只交给后面的HTTP。25ms调用等了184ms，直到占用180ms的permit释放才发现过期。当前默认lane wait_timeout为30000ms，所以不是仅25ms的计时误差；配额拥堵可把短期限拖到约30秒/层。HTTP0却attempts1并入F-WU02。

**system-speech 源码确认，未在本Linux实测macOS：**

- `operations.rs:965–969` 在获取统一permit之前分支调用system_speech。
- `:1060–1064` 将参数命名 `_deadline_unix_ms` 并完全不用；`:1113–1114` 恒等120s。
- `:1102–1112` spawn后的Child既未kill_on_drop，也未纳入现有进程组/取消RAII；调用future被取消/丢弃时没有终止say的清理路径。仅`:1120–1147`等固定120s timeout时才做TERM/KILL。
- 该分支无usage记录，失败/取消也无统一操作结算。取消后进程/临时产物风险是明确源码路径，平台现象须在macOS验证，不能拿Linux上的“不支持”拒绝冒充通过。

**修复：** 共享绝对deadline覆盖resolve/刷新、quota、HTTP、解析/资源下载/进程等待；上限取剩余预算而非每段重置。system-speech在统一宿主预算下运行，并使用已有受监督进程和drop/cancel清理契约，验证终止/回收及部分文件清理。不要复制另一套cancel系统。

**自检：** 排队前已过期、队列拥堵、刷新挂起、授权撤销、请求中取消、下载中取消、返回前过期；macOS真实say或合规进程替身加真实平台验证，确认调用结束进程树/permit/临时文件均可解释。与网络代理已发现的error-body无deadline问题同组扫全分段，但无需重复其证据。

## 应保留的正确实现与不误报范围

- 主循环在model turn解决后释放model permit（`runs.rs:1341`），已可避免父请求持permit等worker造成的明显全局1死锁。
- WorkerHost的独立purpose白名单、单invocation预算键、RPC异步等待、RAII计数回收都实际存在；不因发现usage失败就说这些完全未实现。
- 主loop `runs.rs:3118–3137` 保持usage先写库、再完成事件落库/发布；修复应扩展完整性，不能拆掉这一写序。
- 媒体任务完整业务持久化/UI属于R07；当前发现是R05明确交付的操作接口自身，不要求现在迁移所有Node业务。
- 未运行真实付费供应商、macOS say、Windows；没有用静态代码阅读声称这些平台实测PASS。R05已有LIVE延期不会解释这里的loopback可复现缺陷。

## 最终建议

按以上6个共同根因建立一次修复矩阵，所有相关路径同时处理；更新原测试中错误的Gemini预期，但每个新预期必须由正确契约或真实外部计数支持。将本目录8条反例重建为仓库永久测试并纳入明确C-ID映射，不能只放证据文件。R05原通过台账中对应A/C应降回待修复/待重验；不能继续把operation隐私缺口列为非阻塞。修复后新独立审查者重新跑T06/T07全部C、共享T05预算边界及阶段Gate，完整记录当前candidate，不仅看本8条是否变绿。
