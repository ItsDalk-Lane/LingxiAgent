# R05 独立协议与流式审查

## 结论与证据边界

**VERDICT: FAIL。R05-T03/T04 的必需契约尚未全部达成，不能据现有绿色测试放行 R06。**

固定仓库 `ItsDalk-Lane/LingxiAgent`，HEAD `d80737b6cb9186c8a18c0f35923aac00249d45c3`，R05 基线 `c549ff654508ab951e2cf39cf9d309fc9c6b8656`。本报告所有代码位置均相对于仓库根目录。仓库产品源码、测试、配置、报告未修改，结束时 `git status --short` 为空。

核对原始 `R05_模型协议、凭证、流式处理与最小完整闭环.md`，并以用户后续专项文件 `Lingxi_R05_完整执行与验收提示词_2026-10-02.md` 补充 C 检查点。实现自写的 INTERFACE_EVOLUTION 不能降低这两份规格。

独立实验项目 `audit/protocol/` 用 path dependencies 调用固定 HEAD 的真实 Rust parser、stream accumulator、renderer、compat、normalizer。复制仓库原始 Cargo.lock，核对后只有本实验包是新增 package，第三方包版本无变化；Rust 1.98.1。最终执行 13 个测试：**1 个正向对照通过，12 个应满足契约的断言失败，退出码 101，无 ignored/skipped**。完整源码 `src/lib.rs`，原始日志 `probe-output.log`。这 12 个反例归并为下列 7 项，不以测试个数冒充独立根因数量。

```bash
source /workspace/scratch/9b4397d87a13/audit/runtime/env.sh
cargo test --offline --manifest-path /workspace/scratch/9b4397d87a13/audit/protocol/Cargo.toml -- --nocapture --test-threads=1
```

真实协议服务器没有被调用。Google 并行结果的服务端影响由真实编码输出与官方契约比对得出，不能把它描述成已亲测 Google 4xx。其他代理提供的正式二进制、真实文件工具、HTTP 历史和重启反例另列来源，供总审查者独立复核。

## PF01｜Anthropic 标准 thinking 流误判签名冲突；空 thinking 的合法签名又会丢失

**级别：BLOCKING。对应 R05-A05/A06/A07，T03-C10，T04-C01/C14。**

位置：

- `rust/crates/lingxi-adapters/src/models/anthropic_messages.rs:827–841`，将 `content_block_start` 的 `signature: ""` 保存为 `Some("")`。
- 同文件 `914–932`，后续 `signature_delta` 与这个初始空串不一致就报 `InvalidMessage`。
- 同文件 `499–521`，空 `thinking` 不创建 Reasoning 块，但创建独立 signature opaque。
- 同文件 `244–277`，渲染只接受紧邻 Reasoning 的签名，独立签名当 orphan 丢弃。

真实反例：`anthropic_documented_empty_signature_placeholder_accepts_final_signature` 输入官方形式的起始 thinking 块（thinking 和 signature 都为空），随后送非空 thinking delta 和一个非空 signature delta，真实 accumulator 返回 `a thinking block carried two DIFFERENT signatures`。输入本身是官方标准流形状，不是构造未知协议。

第二个反例 `anthropic_empty_thinking_preserves_signature_on_next_request` 直接使用缓冲响应 `{type: thinking, thinking: "", signature: "opaque-sig"}` 加一次 tool_use。解析成功，继续请求却仅剩 tool_use，签名消失。

官方依据：[Claude Streaming messages](https://platform.claude.com/docs/en/build-with-claude/streaming)，本次检索 ref `turn4view0`，页面行 402–418 给出空签名起始和后续 signature delta；行 170–173 说明可存在没有可见 thinking 文本而只有签名的模式。[Anthropic 官方 SDK](https://github.com/anthropics/anthropic-sdk-python/blob/main/src/anthropic/lib/streaming/_messages.py)，ref `turn1search0`，签名更新采用赋值。

根因：把流打开时的占位值当成已经完成的签名，并把协议状态的存活条件错误地绑在可见文本非空上。

修复与自检：保留签名真实状态与所属块；初始占位不是冲突证据；空可见 thinking 不得抹掉有效 opaque。覆盖初始空/缺失签名、非空签名、空 thinking、redacted thinking、带 tool_use 的下一轮、字节拆片，以及实际冲突的负例。**不是要求把任意多条 signature_delta 字符串拼接；该猜测经官方 SDK 核查已排除。** 正向对照 `positive_controls_normal_text_final_and_signature_without_placeholder_work` 证明省略初始 signature 的旧测试形状能通过，因此现有夹具绕过了真实空占位形状。

## PF02｜Google functionCall 签名被主动丢弃，并行工具结果的 Content 分组错误

**级别：BLOCKING。对应 R05-A05/A06，T03-C03/C06/C10，T04-C01/C08/C14。**

位置：

- `rust/crates/lingxi-adapters/src/models/google_generative_ai.rs:488–523`，functionCall 解析后直接 `continue`，绕过 `548–556` 的 thoughtSignature 保存。
- 同文件 `323–334`，继续请求只重建 name/args/id，无法恢复同一 Part 的签名。
- 同文件 `341–369`，每个 ToolResult 都创建一条单独的 user Content，没有按其所属 model 工具回合合并 parts。

反例 `google_function_call_signature_survives_buffered_and_streamed_round_trip` 同时调用缓冲 parser 和生产使用的 GenerateStreamAccumulator。原输入 functionCall Part 具有 `thoughtSignature: "signed-function-part"`；两条路径输出相同，但下一次请求的 Part 都丢了 thoughtSignature。正常解码 text/thought Part 的签名不能证明 functionCall Part 已覆盖。

反例 `google_parallel_results_form_one_reply_turn` 同回合两个带独立 id 的 read 请求，真实重建结果是 4 条 Content：user、model(two calls)、user(one result)、user(one result)。官方并行形状应是 3 条：user、model(two calls)、user(two result parts)。

官方依据：[Thought signatures](https://ai.google.dev/gemini-api/docs/generate-content/thought-signatures)，ref `turn10search0`，要求把签名放回收到它的原始 Part；[Gemini 3 guide](https://ai.google.dev/gemini-api/docs/generate-content/gemini-3)，ref `turn10search3`，并行示例明确把多个 FunctionResponse 放在下一条 Content 的同一个 parts 数组。

根因：内部工具请求只保留 name/args/id，丢失协议 Part 身份和原位置；工具结果逐条渲染时又丢失共同回合边界。

修复与自检：保持完整有序 Part/工具关联及必要签名；按照原工具回合合并 results。覆盖单工具、同名双工具、异名双工具、顺序及逆序结果、部分工具失败/取消、第一 functionCall 带签名、连续多个工具步骤均带不同签名、empty text 的独立签名。对流与非流都做“原响应→canonical→真实工具结果→下一次 wire”的结构级比较。

## PF03｜OpenAI-compatible 思考状态在发送层被删，DeepSeek 等后续工具请求缺 reasoning_content

**级别：BLOCKING。对应 R05-A05/A06，T03-C06/C10/C12，协议兼容与 T04-C14。**

位置：

- `rust/crates/lingxi-adapters/src/models/openai_completions.rs:141–164`，assistant wire 结构没有 reasoning carrier。
- 同文件 `395–406`，render_chat_request 明确忽略所有 Reasoning/Opaque。
- 同文件 `866–871`，流式 reasoning_content 进入 ContentBlock::Reasoning；`203–211` 的缓冲 ResponseMessage 则连顶层 reasoning_content 字段都未声明。
- `rust/crates/lingxi-adapters/src/models/compat.rs:35–42` 明示省略 central reasoning-replay validation；`1090–1103` 又为 deepseek-reasoner 开 thinking，未补回刚丢掉的推理状态。

反例 `deepseek_stream_reasoning_replays_through_actual_renderer_and_compat`：先让真实 ChatStreamAccumulator 收 reasoning_content 和工具调用；确认 canonical 里仍有原始 reasoning；再走生产相同的 render_chat_request→apply_for_call。结果请求 `thinking.type=enabled`，但 assistant 工具历史的 reasoning_content 缺失，访问为 null。不是本地 fail-closed；损坏 wire 仍可外发。

现役源码依据：`core/provider-compat/deepseek.ts` 文件头及 `core/provider-compat/reasoning-content-replay.ts` 明确要求工具回合重放真实 reasoning_content，不能造空占位。类似供应商需要按现役支持矩阵扫全，不能只增加 DeepSeek 特判。

根因：迁移时以为通用 ChatCompletions 输入永远不能带 reasoning，从 canonical 到 wire 的公共投影先把供应商兼容层需要的输入删除；compat golden 直接喂已加工 payload，没验证这个前后连接点。

修复与自检：从真实响应保留 carrier 身份和内容，按实际 provider/model/replay policy 重放，缺少必需状态时请求前明确失败；不能伪造空 reasoning。覆盖 DeepSeek、Kimi、MiMo、Zhipu 等现役已声明 replay 契约的路由；Chat/Utility、thinking开关、纯文本/带工具、流/非流、跨provider切换。每条测试必须经过真实 renderer + compat，不只单测 compat 输入样例。

## PF04｜opaque 的 provider 字段实际装协议族，换供应商后仍回传旧供应商签名

**级别：BLOCKING。对应 R05-A06，T03-C10/C11。**

位置：

- `rust/crates/lingxi-kernel/src/model_exchange.rs:253–261`，AssistantTurn 没有保存 served provider/model 的来源身份。
- `anthropic_messages.rs:514–520` 等 parser 将 Opaque.provider 写成 FAMILY（如 anthropic-messages）。
- `anthropic_messages.rs:249–280`、`google_generative_ai.rs:214–270`、`openai_responses.rs:242–250` 只按 FAMILY 放行 opaque，不核对实际 provider/model。
- `models/provider.rs:200` 后的 next_turn_for_operation 每次重新解析当前路由；服务循环继续发送 prior exchange，因此同族路由变更是可到达路径。

反例 `provider_opaque_state_never_replays_into_another_provider_of_same_family`：生成 provider A 所属 thinking/signature 历史，将模型路由指定 provider B/other-model，真实 Anthropic renderer 返回成功并把 `provider-a-sig` 放进 B 的请求。

根因：将“协议格式兼容”误当成“协议状态来源授权兼容”。已有 cross-family 测试只能拒绝不同 FAMILY，不能验证不同 provider 使用同 FAMILY 的情况。

修复与自检：交换记录和 opaque 保存产生它们的 provider/model/必要会话绑定，发送前按允许规则验证。无法安全继续时明确中断或采用已定义转换，不把 A 签名发 B。覆盖同族异provider、同provider异model、不同协议族、配置热更新、重试/取消、同名 modelId、历史重放。需逐族扫描，不仅修 Anthropic 单处。

## PF05｜工具轮准入只检查局部形状，缺整批身份、schema 和内容块完成验证

**级别：BLOCKING。对应 R05-A05/A07/A08，专项 §R05-T04，T04-C05/C06/C09/C10/C11。**

位置：

- 四族 parser 分别在 `openai_completions.rs:618–669`、`anthropic_messages.rs:524–567`、`openai_responses.rs:565–608`、`google_generative_ai.rs:488–522` 逐个 push 请求，没有整批 provider call id 一致性检查；Codex 共享 Responses parser。
- `rust/crates/lingxi-service/src/runs.rs:1498–1519` 给每条请求新铸 host ID；`1552` 起逐工具准入执行，不能靠 host ID 唯一性解决重复 provider ID。
- `anthropic_messages.rs:1073–1120` finish 只检查 message_stop_seen，不检查所有已打开 block 都在 closed 集中，也不要求有效 stop_reason。
- `openai_completions.rs:967–973` 只检查 DONE；`678–717` 对缺失/未知 finish_reason 落入常规路径。

独立反例：

1. `duplicate_provider_ids_with_conflicting_arguments_reject_whole_batch` 在四族输入两个同 id="same"、path 各为 a/b 的请求，全部解析成功并返回两个 ToolRequests。
2. `anthropic_open_tool_block_cannot_close_as_a_completed_batch` 送工具 JSON 后省去 content_block_stop/message_delta，只给 message_stop，真实 finish 仍返回可执行 ToolRequests。
3. `openai_done_without_finish_reason_does_not_make_final` 仅文本 delta + DONE，没有 choice 正常停止原因，仍产生 Final。

跨审查者正式二进制证据：`audit/closed_loop/batch-result.json`。闭环审查者证明，同轮合法 write + read(path=123) 时真实文件先被写，之后才报告第二个 schema violation；两个 provider id='dup' 的 write 又实际创建了两个文件，下一请求含两个同名 tool_call_id='dup'。这些是实际生产 bootstrap/ToolGateway/文件工具/DB 路径，HTTP 替身只替代外部模型。

根因：单个参数 JSON 可解析、host ID 唯一、看到传输终结标记，这些局部事实被误当成“整个模型工具轮已经完整且无歧义”。模型调用边界与工具执行边界之间没有完整批次准入凭据。

修复与自检：在副作用前对整轮合法终结、所有 blocks 的完成状态、工具名称和 schema、provider id 冲突进行完整校验；同ID异参数拒绝；同调用完成重发不重复执行；普通同字文本不得内容去重。按协议区分 sentinel、content block close、正常 stop，未知关键/矛盾终态明确失败。覆盖合法第1工具+非法第2工具、重复ID同参/异参、双index同ID、缺block stop、缺正常stop、长度截断、错误事件、同字连续delta、正常多工具阳性对照。失败整批的真实文件写入计数必须为0。

## PF06｜过程内容被判为最终答复；实时阶段与历史规范化各行其是

**级别：BLOCKING。对应 R05-A08/A15/A16，原T04步骤3–4，专项T04-C11/C13以及T08消息/历史要求。**

位置：

- 五族 parse 以“content非空且无tool请求”直接 Final：completions `718–730`，Anthropic `626–636`，Google `626–636`，Responses `624–636`（Codex共用）。
- `runs.rs:1431–1462` 只检查 message.content 是否空，随后 CompletedWithFinal；不会发现内容只有 Reasoning/Opaque。
- `streaming_norm.rs:888–909` 未收到语义阶段或终结，就为所有文字发 FinalAnswer。
- `lingxi-protocol/src/wire.rs:142–156` 已有 Unresolved，并明确不能默默猜成 final_answer；实现文档 D5“text=final”与此及原任务书冲突。
- `streaming_norm.rs:853–860` 从实时事件剥离 mood；最终原始 Text 却不经同一规范化器。`split_reserved_tag_segments` 在生产 Rust 路径没有调用者。
- `rust/crates/lingxi-service/tests/r05_t04_streaming.rs:1427–1461` 断言 DB 保留 raw mood，然后测试自己调用 helper(raw)，并未测试实际历史入口。

反例：`thinking_only_and_opaque_only_do_not_form_final_answers` 为五族分别提供完整正常结束、但只有 Reasoning/签名Opaque 的响应，五族全返回 Final；`unclassified_live_text_is_not_persisted_as_final_answer` 仅喂“我将查看文件”第一段就得到三个 FinalAnswer 事件。

正式二进制交叉验证：

- `audit/closed_loop/process-only-result.json`：只有 reasoning_content 的正常 stop+DONE，run 为 completed.with_final，HTTP final_message_committed 只有 Reasoning；重启保持。
- `audit/closed_loop/binary-result.json` 与对应 before/after HTTP events：真实输入含独立 think/mood，实时分开推理/正文并剥mood，最终历史却一个 raw Text 包含两个标签及其内容，重启仍然如此。存储落点 `rust/crates/lingxi-adapters/src/storage/run_store.rs:1545–1568`，HTTP直接返回 `rust/crates/lingxi-service/src/lib.rs:2663–2668`。

根因：把“有内容”“一次模型请求结束”“用户任务完成”混为一谈；实时事件和持久化消息使用两种不同投影。

修复与自检：统一规范化阶段判定，在可信终结和可用答案存在后才形成 final；仅过程/仅opaque使用真实process/no_final终态；未定文本保留Unresolved/已定义的过程阶段；原始协议状态可独立保留，但正文/推理/MOOD历史投影必须和实时同源。测试真正HTTP历史读取和重启，不在测试端手工补清洗。覆盖过程→工具→答案、仅reasoning、仅opaque、仅mood、跨片标签、字面代码标签、final流中断、取消、重启。不能只修前端正则。

## PF07｜Responses/Codex 下一请求改变混合内容顺序

**级别：BLOCKING（明确必需契约未完成）。对应 R05-A06，T03-C06/C10。**

位置：`rust/crates/lingxi-adapters/src/models/openai_responses.rs:225–270`。文本先累计，reasoning opaque 立即插入 items，文本在遍历结束后才插入；工具又统一附在其后。Codex `render_codex_request` 使用同一个 render_input_items。

反例 `responses_text_reasoning_and_tools_keep_original_relative_order` 输入 message(text)→reasoning→function_call。下一请求变成 reasoning→message(text)→function_call。原签名字节保留并不能满足“原位置与相对顺序保留”。

仓库自己也在 `docs/rust-tauri/R05/R05_INTERFACE_EVOLUTION.md:354–357` 将 N-01 记为留给T05的观察项；固定HEAD的实现仍未修复，不能把已记账误当已完成。

修复与自检：使用有序交换表示保留内容、工具与opaque的原关联/相对顺序，分别覆盖Responses与Codex。至少测text→reasoning→tool、reasoning→text→tool、两段reasoning/文本交错、多个工具穿插，以及纯文本/标准reasoning-first阳性对照。签名必须保持字节/结构保真，不能通过重新排成单一固定顺序让样例通过。

## 收口自检矩阵与放行要求

| 组 | 必须保留的失败回归 | 必须同时通过的阳性对照 |
|---|---|---|
| Anthropic 状态 | 空起始签名、空thinking有效签名、真正冲突 | 官方正常流、普通无thinking、redacted thinking |
| Google 工具交换 | functionCall签名、并行结果分组、逆序/同名工具 | 单工具普通回传、text/thought签名 |
| Compatible replay | renderer→compat完整链缺reasoning | 正确carrier重放、off/utility策略 |
| 来源绑定 | 同族异provider/异model opaque泄漏 | 同源允许重放、不需要opaque的切换 |
| 批次准入 | 同ID冲突、任一schema错误、缺block close/stop | 两个合法独立工具各执行一次 |
| 消息语义 | process-only假final、raw标签历史、未定文本 | 真final、正常代码示例、实时/重启相同 |
| 混合顺序 | Responses/Codex重排 | 标准顺序与多种合法交错 |

全部回归需绑定修复后的candidate、工具链、lockfile、平台和执行日志；对整批/取消/历史问题必须验证生产入口和真实文件计数，不能只靠纯parser新单测。再跑受影响的完整 R05 T03/T04/T05/T08 套件及阶段 Gate，由新独立验收者重新核查；不能把标准套件仍绿当成上述反例已经修复。初始审查中关于“Anthropic任意签名片应拼接”的猜测已排除，本报告未将其计入问题。
