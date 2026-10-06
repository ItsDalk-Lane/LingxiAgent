# R05 跨任务正式闭环补充审查

## 范围与证据身份

- 源码目录：`/workspace/scratch/9b4397d87a13/LingxiAgent`。
- 冻结 HEAD：`d80737b6cb9186c8a18c0f35923aac00249d45c3`；R05 基线：`c549ff654508ab951e2cf39cf9d309fc9c6b8656`。
- 权威输入：原 R05 任务书，以及 `/workspace/scratch/9b4397d87a13/audit/source_taskbooks/灵犀/Lingxi_R05_完整执行与验收提示词_2026-10-02.md`。
- 真正运行的正式程序：`audit/runtime/target/debug/lingxi-service`；SHA-256 `c5d19bddd502762da19077c9e6f01d40ddc0bdb18b5777c6705735249bdb5861`。
- 测试只将外部模型 HTTP 端点替换为本地受控服务器；正常 `--home --config --bind` 启动、真实 loopback 认证、HTTP execute/events、RunSupervisor、Gateway、文件工具、审批、持久化、SIGTERM/重启均为仓库正式实现。未注入 ServiceDeps，未用假 Run/Tool/Storage。
- 所有文件动作仅发生在 `audit/closed_loop/` 创建的独立目录。无付费/真实供应商请求、无真实用户数据操作、无仓库源码或测试修改、无 commit/push。收尾 `git status --short` 为空。
- 本报告是独立补充审查，并不声称重跑了完整 R04/R05 阶段门禁。

**结论：R05 当前不能放行 R06。下列生产反例以及必需正式接线缺口可以直接推翻阶段完成结论。**

## CL-01：模型能力声明缺少载体，未声明支持的工具仍发送到模型

**等级：BLOCKING；已通过正式二进制复现。**

要求：原 R05-A02；专项第102–108行、R05-T01-C06（394–402行）。未声明工具/图像/目标 operation 能力应在请求前拒绝，外部计数为零。

源码：

- `rust/crates/lingxi-adapters/src/models/config.rs:32–43,180–187`：ProviderConfig/RouteBinding 无工具、图像输入等逐模型能力字段；`RouteCompatHints:208–234` 也没有这些声明。
- `rust/crates/lingxi-adapters/src/models/gateway.rs:225–237`：只检查协议族能否服务 Chat/Auxiliary/Embedding 等 operation 类，没有针对实际请求 tools/images 检查模型能力。
- `rust/crates/lingxi-adapters/src/models/provider.rs:195–226`：解析路由后立即解析凭证并发送，没有输入能力预检。
- 全 Rust 源码的 `CapabilityUnsupported` 只有错误枚举、展示和匹配，没有实际构造拒绝路径。

实际反例：配置 `text_only/text-only-no-tools`，完全没有工具能力声明；正常 binary 执行一次用户请求。模型 HTTP 替身收到 **1 次请求，tools = edit/read/write**。没有在请求前拒绝。

证据：`binary-result.json`，`binary-probe-02rlpcrh/requests.json`；脚本 `probe_binary.py`。图像同类反例由凭证网络审查代理在 `audit/credential_network/probes.log` 实跑，实际网络计数同样为1（此报告没有独立重跑图像 API 探针）。

修复及自检：建立 provider+model+operation 的可信能力数据及请求需求检查；不要用品牌或协议族替代模型能力，也不能静默去掉工具/图片。使用正式 Gateway/辅助入口分别验证未声明、明确不支持、支持三态，错误时外部请求数必须为0；覆盖同名 model 不同 provider、配置 reload、主对话/视觉/worker/媒体入口。加入明确支持的正对照，避免将全部请求一概拒绝换取通过。

## CL-02：实时规范化、最终消息与历史存储不是同一语义，只有思考的回答也被判为 final

**等级：BLOCKING；两个正式二进制反例均复现。**

要求：原 R05-T04/A07/A08；专项第138–146行，以及 R05-T08-C09（1305–1313行）要求思考/过程/MOOD/partial/final 在真实事件投影、断连、快照、持久化之间同源。

### 反例 A：think/MOOD 回到最终正文

供应商 text 包含：围栏代码里的字面 `<think>keep quoted</think>`，接着独立 `<think>PRIVATE_THINK_PROBE</think><mood>PRIVATE_MOOD_PROBE</mood>VISIBLE_FINAL_PROBE`。

实际：实时 delta 将独立 think 分为 reasoning，将可见 final 作为 text，剥离 mood；但认证 HTTP `/lingxi/v1/sessions/sess_local_alpha/events` 的 `final_message_committed.message.content` 只有一个 Text，里面保存整段原始标签文字，think 和 mood 均重新成为正文。SIGTERM 后重新启动，再读 HTTP 历史仍原样保持。

源码链：`streaming_norm.rs:838–859` 只清理 delta；`runs.rs:1431–1461` 直接采用 provider 返回的 message；`lingxi-adapters/src/storage/run_store.rs:1537–1568` 直接 canonical 序列化/clone 原 message 进入消息表及 final 事件；`lingxi-service/src/lib.rs:2656–2668` 将持久化 `cut.events` 原样返回。

反作弊证据：`split_reserved_tag_segments` 全仓只有声明、自身单测及 `r05_t04_streaming.rs:1438` 的测试调用，没有生产 history 入口调用。该测试在1427–1433行明确断言 canonical message 保存 raw 标签，再在测试内自己调用 helper，不能证明真实历史投影已经接通。T08-C09 测试仅用两段普通文本（1989–2127行），没有执行要求中的 think/MOOD/tool/partial/final 组合。

证据：`binary-result.json`，`binary-probe-02rlpcrh/events-before.json` 与 `events-after.json`；脚本 `probe_binary.py`。

### 反例 B：仅 reasoning 仍 completed.with_final

供应商只发送 OpenAI `reasoning_content="PROCESS_ONLY_REASONING"`，随后合法 `finish_reason=stop` 和 `[DONE]`，没有任何正文文本。

实际：两个 live 词汇均仅有 reasoning delta；Run 却为 `completed / completed.with_final`，HTTP `final_message_committed` 只包含 Reasoning 内容块，重启后保留这一假 final。

源码：`openai_completions.rs:712–728` 把任意非空 content（包含仅 Reasoning/Opaque）认作 ProviderTurn::Final；`runs.rs:1431–1461` 只检查 content 是否为空。协议审查代理已经检查其他协议同类路径，本报告正式二进制实证的是 OpenAI 路径。

证据：`process-only-result.json`，`binary-probe-v989acpq/events-before.json`/`events-after.json`；脚本 `probe_process_only.py`。

修复及自检：统一生产规范化结果和终态判定，不把原始协议块直接当用户 final；保留重放需要的原始/opaque 状态在受控区域，不通过破坏协议保真来清理正文。纯 reasoning、opaque-only、过程说明、混合 tools、标签跨片、围栏字面标签、真实 final、取消/断流/max_tokens 都要运行到正式 HTTP 订阅、持久化、断连续取和重启读取，比较规范化序列。无真正 final 时不能写 final_message_committed。测试必须读取真实产品接口，禁止在测试侧补投影证明产品正确。

## CL-03：整轮工具批次没有在副作用前完成 schema/ID 冲突预验证

**等级：BLOCKING；两个正式二进制反例均复现。**

要求：专项第140行要求完整模型工具轮达到协议终结条件、完整参数和 schema 验证后才产生副作用；R05-T04-C06/C09 要求完整合法批次前零副作用；C10（803–811行）明确“同ID不同参数”“冲突拒绝”。

源码：`openai_completions.rs:625–669` 遍历工具，只解析 JSON/EffectiveArguments，不对整个批次核对真实 tool schema，也不验证同一 modelCall 内 provider_call_id 冲突；`runs.rs:1491–1522` 为所有请求生成宿主 ID，随后 `1552` 开始逐个执行，直到每个独立工具的 `1679–1690` 网关准备阶段才校验其当前 schema。

### 反例 A：第2项 schema 错误，第1项已写文件

同一完整模型轮返回：

1. `call_good / write {path:"output.txt", content:"BAD_BATCH_WROTE_FILE"}`；
2. `call_bad / read {path:123}`。

实际：`output.txt` 已真实写入20字节；第一工具是 success，第二工具随后才报 `$/path: expected type ["string"], found number`；下一模型请求收到两个 tool results。不是在批次副作用发生前完成 schema 验证。

### 反例 B：同一 ID 不同参数执行了两次

同一 modelCall 返回两个 `id="dup"` 的 write，一个写 `a.txt/A`，另一个写 `b.txt/B`。

实际：两个文件均真实创建，宿主生成两个不同 ToolCallId，下一轮发送两条相同 `tool_call_id="dup"` 的结果，关联歧义且冲突未拒绝。

证据：`batch-result.json`；`batch-invalid-second-e6lgh43x/{events,requests}.json` 与 `batch-duplicate-id-hn02gf0_/{events,requests}.json`；脚本 `probe_batch.py`。协议审查代理已在各协议 parser 检查同类范围，本报告二进制实证 OpenAI。

修复及自检：在整轮批次执行前，对声明快照、每项有效参数/schema、ID唯一性/重复完成/同ID冲突作一致检查；现有逐次网关授权与取消/代次复查必须保留，预检不能替代临执行安全检查。至少覆盖合法第1写+错误第2项/错误末项、反向顺序、跨分片/跨协议、相同ID不同参数、重复完成事件；断言零工具 started、零文件变化或其他外部副作用。另设不同模型轮复用相同 provider ID 的合法正例，不能全局去重误拒；相邻相同文字不能被内容去重。这里不要求把合法工具执行后的运行期失败回滚成虚假原子事务，问题是未按明确要求完成批次准入。

## CL-04：正式二进制没有注册 worker 工具，C11 使用注入测试冒充正式完整链

**等级：BLOCKING；生产调用链静态实证，未运行不存在的正式 worker 配置入口。**

要求：专项 R05-T08-C11（1325–1333行）明确“正式服务注册受控单操作worker，模型配额=1”，完成“模型→工具→worker callback→模型→工具结果→最终回复”。

源码：

- `lib.rs:1150–1205` 构造 GatewayWorkerModel/BoundedWorkerModel，`:1376–1380` 仅暴露 getter。
- 正式 src 全仓 `register_worker_tool` 只有 `workerrpc.rs:1478` 函数定义及 `lib.rs:177` 的 pub use，没有调用；WorkerRuntime 没有正式组合根实例化调用。
- 正式 config 闭合键只有 home/workspace/providers/models（`config.rs:777–795,820–825`），无正式 worker 注册配置。正式 HTTP 工具声明实测只有 edit/read/write。
- `R05_TEST_MAP.json` 的 C11 将该项绑定到 T06 的服务注入测试，称“OFFLINE_SERVICE/BINARY（混合）”；`R05_ACCEPTANCE_LEDGER.json` C11 仍标 PASS 并称在正式 bootstrap 接线。
- 其代表测试 `r05_t06_worker_model.rs:878–930` 在884行造 worker_harness、885行造 StepsProvider，896–906行注入 turn_provider/tool_gateway/approval_gate，917行由测试手动 register_t06_worker，918–930行手工推入 ProviderTurn::ToolRequests。主模型从未走正式协议 HTTP 发送，也未执行生产二进制的 worker 注册。

修复及自检：将受控 worker 描述、授权、资源范围、进程监督与注册接到正式配置/启动路径（不新增任意不受控worker入口）。运行正常二进制，配额=1，主模型和callback均真实 HTTP，真实子进程读取并回传 nonce，继续得到最终回复；另测callback取消/超时、拒绝用途、秘密不传入、跨会话预算隔离和进程回收。移除正式注册的隔离负测应使 C11 及 gate 失败，即使 library integration 仍绿。

## CL-05：四基础工具的正式交付缺少 exec 接线

**交付缺项；应与 CL-04 一并关闭正式能力注册，而不是要求重写已实现的 R04 工具。**

原 R05 阶段 Gate 明确“四工具真实闭环”；专项第58/108行要求组装 R04 基础工具与审批。R04 第四工具是 Rust `exec_command`（不是任意新增产品功能），其实现及 `register_process_tools` 已存在 `exectools.rs:1316–1365`。

但 R05 正式 `lib.rs:1019–1021` 注释明确 `process tools stay unregistered`，`:1072–1080` 只注册文件工具。实测 wire tools 精确为 edit/read/write。必须核对并实现当前平台支持的 exec/进程监督/沙盒注册，以及真命令闭环、取消、超时、Running/StopUnconfirmed、非零退出、重启恢复；平台未实现项如实登记，不通过移除基础工具或冒领前序 library 测试消除此缺项。

## CL-06：资源验收覆盖不足，FD 计数谓词不能测到 FD

**BLOCKING 验收缺口；与 gate 审查报告合并，避免重复统计。**

专项第256行要求正常/长响应/100次以上重复取消与错误/嵌套worker/多会话，C12（1335–1343行）要求 task/连接/permit/计数器/临时文件稳态与原始采样。

实际 `r05_t08_closed_loop.rs:2226–2288` 仅2次热身+12次成功/500/replay，没有这项测试里的取消、callback、长输出、多会话或完整进程树资源采样。`:2290–2318` 只检查主 PID RSS、FD、日志数量；`R05_PERFORMANCE_RESULTS.json` 不保存原始数值（仅断言失败才打印）。其他100/200循环是分片性质测试或等待轮询，不能替代100次真实取消/错误资源负载。

更直接：`:2212–2223` 执行默认 `lsof -p PID`，却只计“行首为数字”的行；默认首列是 COMMAND，所以正常 lingxi-service 行均被过滤。且未核对 lsof 退出状态。原始谓词可一直报告0，不能证明 FD 无增长。

注意：本执行环境 PID 与 /proc/lsof 视图存在映射差异。`binary-result.json` 中 lsof 示例只证明默认 COMMAND 列与谓词不匹配，**不能作为本服务真实 FD 数或泄漏证据**；没有据此宣称服务已经泄漏。gate 代理的 `audit/gate/fd_counter_probe.json` 有同样限定。

修复及自检：修复测量器，验证目标进程身份、命令成功、原始记录存在；用受控故意保持若干 FD/连接/worker 的负对照证明计数确实升高、释放后下降，不能用解析空输出当0。按原门槛执行100次以上取消/错误和其他要求的负载，采样整个实际进程树、task/连接/permit/缓存/临时文件/FD/资源队列，保存原始带时间数据和清理期限，不能事后放宽阈值。

## 可复跑命令与范围

```bash
python /workspace/scratch/9b4397d87a13/audit/closed_loop/probe_binary.py
python /workspace/scratch/9b4397d87a13/audit/closed_loop/probe_process_only.py
python /workspace/scratch/9b4397d87a13/audit/closed_loop/probe_batch.py
```

三个脚本 exit0 表示成功完成观察并写出实际 JSON，不代表产品通过需求验收。需求预期与观察结果相反的地方已逐项列明。完整仓库测试及其他协议/凭证/媒体/usage问题由其他审查代理汇总，不能从本报告的这些复现推定其余路径全部通过。
