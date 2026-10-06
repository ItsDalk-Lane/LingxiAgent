# R05 RR1 WP-T03 执行者报告（R1，2026-10-04）

- 执行者：R05-T03-执行者（首任，全新上下文）。范围：F06、F07、F08、F09、F10。
- 基线：分支 `codex/rust-tauri-migration`，工作树含 T01/T02 未提交修复；本包改动叠加其上（共享文件已在矩阵登记）。
- 工具链：全部命令经 `/Users/study_superior/.cargo/bin/cargo +1.98.1`（rust-toolchain.toml 锁定 1.98.1；PATH 的 Homebrew 1.93.0 未使用）。未新增依赖、未改 lock。

## 1. 旧行为反例（先红，双证据）

1. **原始探针（断言原文未动）**：`artifacts/…/INPUT-adversarial-2026-10-04/audit/protocol` 复制到 `/tmp/r05-t03-probe`（path deps 指向本树），`cargo +1.98.1 test --offline -- --nocapture --test-threads=1` → **exit 101：1 对照通过 + 12 断言失败**，其中本包 7 腿红（F06×2、F07×2、F08、F09、F10），其余 5 红属 WP-T04（PF05/PF06）。日志：`old-red-probe-on-current-tree.log`。
2. **仓库永久反例**（新文件 `rust/crates/lingxi-adapters/tests/r05_t03_rr1_replay.rs`）：修复前 **6 绿（对照/守卫）+ 18 红**（exit 101）。日志：`old-red-r05-t03-rr1-replay.log`。
   - 注：为使仓库套件可编译，先落了**惰性**的 kernel `TurnOrigin` + `AssistantTurn.origin` 字段（无任何读取者，行为不变）再跑红——红来自行为断言而非编译。

## 2. 修复摘要（详见 RR1_ISSUE_MATRIX.json 各 F-ID candidateSummary）

- **F06**（anthropic_messages.rs）：空串起始占位签名可被首个非空 `signature_delta` 赋值替换（官方 SDK 语义=赋值，非拼接）；两个不同非空签名仍是真冲突；finish 仅非空最终签名上 wire；非流式空 thinking+有效签名→独立 opaque；渲染把本族签名孤儿原位重建为 `{thinking:"",signature}`。
- **F07**（google_generative_ai.rs）：functionCall Part 留 `functionCallPart` 锚点（id+signature）于 content 原位；渲染按原顺序重放锚点调用与签名（无 id/无锚点保持登记的 content-first；锚点不匹配/重复大声失败）；同回合连续 ToolResult 合并为同一 user Content 的多 functionResponse parts。
- **F08**（openai_completions.rs + compat.rs）：渲染物化真实 `reasoning_content` 载体；缓冲响应顶层 `message.reasoning_content` 入 canonical；新增 compat 中心 `normalize_provider_payload`（`apply_for_call` 生产入口改走它；模块 dispatch `apply` 保持 golden 钉死）：无契约→剥离（不无差别注入）；require-tool-call 契约且本轮用思考→缺载体本地拒绝（incumbent 拒绝文本原文）；clear 仅 zhipu 可用。Kimi/MiMo/Zhipu 契约推导移植+单测。
- **F09**（kernel model_exchange.rs + runs.rs + 三渲染入口）：`TurnOrigin{provider,model}` 记录于 `AssistantTurn.origin`（驱动从 served_by 记录）；anthropic/google/responses(codex 共享) 在回显任何本族 opaque 前强制 origin==route，同族异 provider/同 provider 异 model/同名 modelId 异 provider/无 origin 一律 InvalidMessage；completions 无 opaque 回显目标（矩阵登记）。
- **F10**（openai_responses.rs + codex 共享）：function_call 解析期 `function_call_item` 锚点；渲染按内容原相对顺序（相邻文本段原位合并、reasoning item 原位 verbatim、锚点调用原位、无锚点交换 content-first），不再重排。

## 3. 自检（全部亲跑，命令+退出码）

| 命令（均 /Users/study_superior/.cargo/bin/cargo +1.98.1） | 退出码 | 结果 |
|---|---|---|
| `test --locked -p lingxi-adapters --test r05_t03_rr1_replay` | 0 | 26/26（原 18 红全绿+守卫/负测腿） |
| `test --locked -p lingxi-adapters` | 0 | 全部套件 ok（lib 111、goldens 3、compat 21、…） |
| `test --locked -p lingxi-kernel -p lingxi-protocol` | 0 | 87 + 23 |
| `test --locked -p lingxi-service --test r05_t03_protocol_adapters --test r05_t01_model_plane --test r05_t02_credentials --test r05_t04_streaming --test r05_t08_closed_loop` | 0 | 24/38/12/9/10 |
| `test --locked -p lingxi-service --no-fail-fast` | 101 | 63 套 ok + 1 失败＝既知环境项 `r00_management_leaves`（macOS 防火墙拦非回环自地址 192.168.3.5；测试自身注明 environment failure；与本包无关，修复前亦失败） |
| `clippy --locked -p lingxi-kernel -p lingxi-adapters -p lingxi-service --all-targets -- -D warnings` | 0 | 无告警 |
| `fmt --all -- --check` | 0 | clean |
| `run --locked -p xtask -- check-contracts` | 0 | 626 entries drift-free |
| `run --locked -p xtask -- check-boundaries` | 0 | PASS |
| 探针复跑（仅按接口演进补 origin 合法前置，断言原文未动） | 101 | 本包 7 腿全绿；余 5 红属 WP-T04 PF05/PF06（预期） |

日志均在本目录（green-*）。无 TS/公共 schema 变化（ContentBlock 未动；TurnOrigin 为 kernel 内部类型，API_COMPAT_MATRIX 复核一致）。

## 4. 夹具/接口演进登记（不削弱原断言）

- `ExchangeItem::AssistantTurn` 增 `origin` 字段：适配 10 处构造点（adapter 单测以匹配 route 的 origin 或 None 更新；goldens harness 以 golden 自身 route 盖章）。
- golden JSON 更新 2 份（行为修正的协议正确形状）：google tool_roundtrip（并行结果 3 Content→1 Content 多 parts，官方 Gemini 3 分组）、openai-completions tool_roundtrip（assistant 消息增 reasoning_content 载体——渲染器物化+compat 按契约处置后的正确形状）。
- 服务级 `c03` 断言更新为官方并行分组；google 单测 `thoughts_signatures_and_function_calls_parse_in_order` 增锚点断言（原断言保留）。
- 探针复跑仅补 origin 合法前置（README「重要」规则）。

## 5. 同类路径扫描

- Opaque 站点全扫：anthropic/google/completions/responses（codex 共享）四族 parse/render 全部核对；auxiliary/tool_render 无 chat 交换 opaque。
- replay 契约链：deepseek（含 v4-anthropic/v4-responses native）、kimi、mimo、zhipu（clearable）、volcengine/anthropic/openrouter/native 剥离——单测逐一钉死。
- 锚点异常路径：锚点 id 无匹配/重复 → InvalidMessage（google 与 responses 均实现；responses 侧由实现覆盖，google 侧同构）。
- anthropic/completions 保持登记的 content-first 规范化（wire matrix C06 注；completions 的 tool_calls 为消息字段，协议天然无位置）。

## 6. 剩余与移交

- F06–F10 状态 IMPLEMENTED，待全新独立审查者实测验收（矩阵已留审查字段）。
- 共享文件：kernel/model_exchange.rs（TurnOrigin）、service/runs.rs（两处 origin 记录，T04 接手 runs.rs 时需保留字段语义）；compat.rs `apply`（模块层）被 golden 钉死，生产语义在 `normalize_provider_payload`。
- 登记缺口（非缺陷）：explicit `compat.reasoningReplay` 配置覆盖与 session 级 clear 源未落地（owner 阶段 T06/T07；推导链覆盖现役四家）。
- 环境项：`r00_management_leaves` 需在允许非回环自地址入站的环境复验（与本包无关）。
