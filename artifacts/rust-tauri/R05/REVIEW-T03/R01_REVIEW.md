# R05-T03 独立验收报告（R01 轮）

- 验收对象：R05-T03「协议适配实质交付」——Rust 五族 chat 协议适配器（openai-completions / anthropic-messages / google-generative-ai / openai-responses / openai-codex-responses）。
- 验收树：分支 `codex/rust-tauri-migration`，HEAD `c549ff654508ab951e2cf39cf9d309fc9c6b8656` + 未提交 T01–T03 工作树改动。
- 验收基准：任务书附录B R05-T03 C01–C12（规格附件 2026-10-02 版）。
- 验收人：独立 reviewer（本报告全部结论由本人亲自运行或亲自读码得出；实现者报告仅作线索）。
- 验收日期：2026-10-03。

## 一、核心验收结论

**有条件通过（conditional go）。**

- **实现层：go。** 五族适配器是实质交付：真实 HTTP 线级编码/解码、真实凭证头映射、错误分类共享、工具四态及扩展态诚实渲染、重试携带已确认交换。任务书 C01–C12 的**能力**全部成立，其中八条高风险链（C05/C03/C04/C06/C07/C12/C10 及 C01 golden 真实性）由本人**独立构造对抗场景复验通过**，非引自实现者自述。
- **台账层：两处不实，必须修复后可转无条件 go。** `docs/rust-tauri/R05/R05_ACCEPTANCE_LEDGER.json` 的 T03-C04 条目把任务书「相同 callId 跨会话隔离」场景替换成了取消语义（张冠李戴）；T03-C05 条目自述「nonce 证据，连跑两次」而实际证据是编译期常量单次运行。两处均为证据/台账层缺陷，**不涉及产品代码缺陷**。
- 无需修改任何产品代码；本验收未修改产品代码（`git status` 中本人的痕迹仅 `artifacts/rust-tauri/R05/REVIEW-T03/` 未跟踪目录）。

### 树-证据绑定核验（亲自复算）

台账 T03 条目 `workingTreeDigest = sha256:465eef35…916943` 声称绑定 81 个 rust/ 文件清单。本人复算：

- `shasum -a 256 artifacts/rust-tauri/R05/T03/working-tree-manifest.txt` = `465eef35…916943`，与台账条目**逐字节一致**；
- 清单内 81 个文件的 sha256 逐一与当前工作树重算比对，**81 ok / 0 bad**。

即：本人跑测试的树与实现者取证据的树是同一棵树；本人的 scratch 目录在 `artifacts/` 下，不在清单覆盖范围，不影响该绑定。

## 二、逐 C-ID 判定

验证方式标注：**[亲验]**＝本人独立构造场景/复跑；**[亲跑]**＝本人复跑实现者测试；**[读码]**＝本人通读源码确认；**[未验]**＝未覆盖。

| C-ID | 任务书场景 | 判定 | 方式 | 依据 |
|---|---|---|---|---|
| C01 | 每族真实编码解码 | **PASS** | [亲跑]+[读码] | goldens 3/3 亲跑通过（5 族 × forward/tool_roundtrip/error 15 个 golden 逐字节）；五族各抽 ≥1 golden 与现役 TS 实现逐字段对拍一致（见 §三-C01） |
| C02 | 外部/内部调用 ID 区分 | **PASS** | [亲跑]+[读码] | `c01_c04_same_model_id_two_families_never_cross_wire_or_credentials` 亲跑通过；外部 callId 仅作关联值原义保留，内部 `ToolCallId` 宿主铸造（`toolcatalog.rs:1568` 起 `into_tool_request` 链路） |
| C03 | 多工具乱序完成不串结果 | **PASS** | **[亲验]** | scratch `c03_reversed_completion_order_pairs_each_result_with_its_own_call_id`：交换中结果逆序（B 先 Failed、A 后 Success），四族渲染各自 id 配对正确不串号 |
| C04 | 相同 callId 跨会话隔离 | **FAIL（台账层）／能力 PASS** | **[亲验]** | 能力由 scratch `c04_same_provider_call_id_in_two_sessions_stays_isolated` 证实（两会话收相同 `toolu_SHARED`，各自续发各自真实内容，journal 各一条）；但台账条目验收场景错误 → F-01 |
| C05 | 真正读到文件再回传 | **PASS** | **[亲验]** | scratch `c05_runtime_nonce_rides_the_next_request_and_follows_changes`：stub 脚本先于 nonce 固定，两次独立 leg 的运行时 nonce 逐字节出现在各自 wire 请求，换 nonce 跟随变化；候选自证形式弱于任务书 → F-02 |
| C06 | 混合内容不被枚举丢弃 | **PASS** | **[亲验]** | scratch 双路：buffered 混合 parts（text/reasoning/unknown→opaque/text）+ tool_calls 全保序；SSE 两 tool_call 片段 index 交错独立拼装、id/参数正确。记录一项归一化 → N-03 |
| C07 | 工具结果语义端到端保真 | **PASS** | **[亲验]**+[亲跑] | scratch `c07_real_missing_file_failure_matches_wire_and_journal`：真实读缺失文件，wire 上 `tool error [...]` 开头且与 journal `ToolResultWire{status:Failed}` 重建渲染**逐字节相等**；`tool_render` 四态+truncated+resource_refs 单测亲跑通过（`lingxi-adapters --lib` 83/83） |
| C08 | 拒绝和 Future 明确回传 | **PASS** | [亲跑]+[读码] | `c08_a_policy_refusal_rides_the_next_request_as_a_structured_failure` 亲跑通过（8/8 内）；拒用走共享四态渲染路径，结构化失败结果回传不静默 |
| C09 | 名称/schema 转换无歧义 | **PASS** | [亲跑]+[读码] | `ToolDeclarationSnapshot` 按 registry 代际构建、wire name 冲突确定性 namespacing、未知名响错不猜测（`unknown_wire_names_are_protocol_violations_never_guesses` 等单测亲跑通过） |
| C10 | 协议 opaque 状态原样往返 | **PASS** | **[亲验]** | scratch `c10_opaque_roundtrip_positions_and_cross_family_isolation`：anthropic 非标位置（text 在 thinking 前）严格原位往返、跨族 opaque 不回显；记录一项归一化缺口 → N-01 |
| C11 | 服务端会话引用绑定 | **PASS** | [亲跑]+[读码] | responses 系 goldens 钉死无服务端状态形状、codex 钉死 `store:false`+`stream:true`+instructions；不支持的族明确失败而非伪造引用。跨 provider 引用绑定为 [读码] 为主 |
| C12 | 后续请求包含真实已发生历史 | **PASS** | **[亲验]**+[读码] | scratch `c12_retry_carries_real_confirmed_outcomes_cross_checked_with_journal`：读→改→读中注入可重试失败，重试请求携带全部已确认交换且与 journal 对照一致、写入不重做；`runs.rs:2236-2253` 重试臂 `attempt_seq += 1; continue` **不清空 exchange**（读码确认），失败调用不留 assistant turn，adapter 无私有重试 loop |

**结论：12 个 C-ID 中，11 条 PASS、1 条（C04）能力 PASS 但台账 FAIL。无 NOT_PROVEN。**

## 三、身份/协议/持久化真实性五条核对

### C10 — opaque 身份真实性 [亲验]

构造 anthropic 响应含 thinking+签名块且**故意用非标顺序**（text 排在 thinking 前），渲染回写后逐字节核对：签名块留在原协议位置原顺序、字节不变、不渲成正文；把 anthropic 的 opaque 块塞进 google/openai 族渲染，一律跳过不回显。同时发现 openai-responses 族对 text-在前的 assistant 内容重渲染时 opaque 位置前移（字节保留），见 N-01——裁定为非标顺序缺口，不阻断 C10 通过条件（canonical 顺序下位置严格保真、不丢、不跨族、不渲正文）。

### C01 — 协议线级真实性 [亲跑]+[读码]

- 15 个 golden 为手写多语言内容（非 ASCII call id `call_读取-γ-0004`、64 字符名、深嵌套 schema、混合四态），全部不含真实密钥形态（仅 `sk-golden-*-secret` 合成标记；error golden 还钉死了 redaction 行为）。
- 与现役 TS 实现逐字段对拍：`appendProviderApiPath`（`lib/llm/provider-client.ts:39`）、codex `resolveCodexResponsesUrl`/`extractAccountIdFromToken`/`DEFAULT_CODEX_UTILITY_INSTRUCTIONS`（`core/llm-client.ts:186-204,41-44`）与 Rust 逐字一致；anthropic `x-api-key`+`anthropic-version: 2023-06-01`、google `x-goog-api-key`+`systemInstruction`+`:generateContent` 一致。
- 凭证四形状覆盖：apiKey（Bearer / x-api-key / x-goog-api-key 按族映射）、authHeader verbatim、none、OAuth（T02 的 401 有界 refresh-retry，`provider.rs` 只有一次，非 adapter 私有 loop）。

### C04 — 会话身份隔离真实性 [亲验]

两个独立 seeded 会话（`sess_local_alpha`/`sess_local_beta`）从 stub 收到**相同外部 ID `toolu_SHARED`**：两会话各自续发的请求携带各自会话的真实文件内容，互不串扰；journal 各自只记自己的完成记录；内部 `ToolCallId` 各自宿主铸造互不相同。任务书「互不去重、不覆盖、不泄露；内部身份唯一」全部满足。**此能力真实，但台账 C04 条目未验收该场景（F-01）。**

### C05 — 防伪链真实性 [亲验]

模型替身（loopback HTTP stub）的脚本在 nonce 写入前固定，脚本内无任何 nonce 字样；运行时写入格式为 `NONCE-{tag}-{pid}-{nanos}-{counter}` 的随机 nonce；截获下一次 HTTP 请求，tool_result 内容与当次 nonce **逐字节相等**且角色/ID 正确（`tool_use_id` 配对）；第二次 leg 换 nonce，wire 跟随变化。预设答案/固定 done 形式在该构造下不可能通过。任务书 C05 通过条件的强形式成立。

### C12 — 持久化与重试真实性 [亲验]+[读码]

读→写→读流程中注入一次可安全重试的模型失败：重试后的请求含**全部已确认工具交换**（read 的真实内容、write 的真实结果含 resource_refs），与 journal 逐条对照一致；已确认的 write 不重做；不从原始用户输入重建。源码侧 `rust/crates/lingxi-service/src/runs.rs:2236-2253`：重试臂保留 `exchange` 不清空、失败的模型调用不产生 assistant turn、`attempt_seq` 递增走同一 run——与任务书「不在 adapter 新建私有 loop」一致（重试在 driver 层，adapter 无重试代码）。

## 四、必须修复项

### F-01：台账 T03-C04 条目验收场景张冠李戴

- **位置**：`docs/rust-tauri/R05/R05_ACCEPTANCE_LEDGER.json:1465`（`R05-T03-C04` 条目）。
- **事实**：任务书 `R05-T03-C04｜相同callId跨会话隔离` 要求「两个会话/请求返回相同外部ID，交错执行，互不去重、不覆盖、不泄露」。台账条目的 `expected` 写的是「取消即停：取消中断下一次模型请求与排队工具；已取消工具结果不回写」（取消语义），`testNames` 挂 `cancel_terminal_race::*`。取消语义不属于 T03 C01–C12 的任何一个检查点（属 R05-T05/R04 延续范畴）。
- **复查证据**：任务书 T03 全部 12 个检查点标题（规格附件附录B 第 590–700 行）无取消场景；台账 T03 其余 11 条也无一条覆盖「两会话同外部 callId 交错」。
- **后果**：任务书 C04 场景在候选交付中**无任何测试与证据**；台账「T03 12 条全 PASS」的表述在 C04 上不实（验收了错的场景）。
- **能力核实**：本人已独立构造该场景并验证**通过**（§三-C04），故此为证据/台账层缺陷，非实现层缺陷。
- **修复要求**：为 C04 补一个两会话同外部 callId 交错执行的候选测试（可直接移植本人 scratch `c04_same_provider_call_id_in_two_sessions_stays_isolated`，见 `artifacts/rust-tauri/R05/REVIEW-T03/scratch/src/main.rs`），改写 C04 条目的 expected/observed/testNames/evidence；把取消语义证据挪至其所属任务条目。顺带清理 C02 条目 expected 中混入的「跨会话同名」字样（C02 本是「外部与内部调用ID区分」）。
- **关联 C-ID**：C04（主）、C02（次要）。

### F-02：台账 T03-C05 条目自述证据形式强于实际

- **位置**：`docs/rust-tauri/R05/R05_ACCEPTANCE_LEDGER.json`（`R05-T03-C05` 条目）；被挂测试 `rust/crates/lingxi-service/tests/r05_t03_protocol_adapters.rs:648`。
- **事实**：条目 `expected` 声称「nonce 证据，连跑两次排除预置」。实际 `testNames` 只挂 `c02_anthropic_tool_roundtrip_with_the_real_file_tool`，该测试用**编译期常量** `FILE_BODY`（第 648 行）单次运行，两次「连跑」跑的是同一常量，不构成任务书要求的「运行时随机 nonce、nonce 变化复跑」证据。
- **缓解事实**：该测试的 stub 脚本先于文件写入固定、脚本内不含 `FILE_BODY`，故 wire 级防预置成立（常量确由真实 read 流出），自证强度为「中」而非「无」。
- **能力核实**：本人已用强形式独立验证**通过**（§三-C05），故为证据形式缺陷，非能力缺陷。
- **修复要求**：把 C05 证据改为运行时写入随机 nonce + 换 nonce 复跑（可参考本人 scratch `c05_runtime_nonce_rides_the_next_request_and_follows_changes`），或把条目 expected 改为与实际证据相符的表述。
- **关联 C-ID**：C05。

## 五、可后续处理项（不阻断）

- **N-01（C10 非标顺序缺口）**：openai-responses 族渲染器把 opaque（encrypted reasoning item）立即入列、文本累积到最后统一入列，导致 text-在前的 assistant 内容重渲染时 opaque 位置前移（实测重渲染顺序 `["reasoning", "message:answer first"]`）。字节逐字保留、不跨族、不渲正文，canonical 顺序（reasoning 在前）下位置严格正确。建议后续在渲染层按原位置穿插。
- **N-02（观察）**：write 工具结果的 wire 文本含 `[resource: out.txt <file:///private/var/folders/.../out.txt>]`，即工作区绝对路径经 file:// URI 进入模型 payload。这是 resource_refs 的保真上送（非压平），行为正确；是否要对模型暴露绝对路径属产品策略问题，提示知晓。
- **N-03（C06 流式归一化）**：openai-completions SSE 聚合（`openai_completions.rs:834-839`）把 reasoning 与 text 各自累积后按 reasoning-在前重建，reasoning/text 之间的逐块交错顺序不保留（text↔tool_calls 的交错经本人亲验严格正确）。reasoning 模型实际 wire 顺序几乎必然 reasoning 在前，实际影响为零；建议文档补一句声明。

## 六、复跑命令清单

环境前缀：`export PATH="$HOME/.cargo/bin:$PATH"`（rustup proxy，rustc 1.98.1）。

```bash
# 静态门禁（全部 exit 0，日志 artifacts/rust-tauri/R05/REVIEW-T03/logs/）
cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings
cargo run -p xtask --locked -- check-contracts
cargo run -p xtask --locked -- check-boundaries

# 聚焦套件（全部 PASS）
cargo test --manifest-path rust/Cargo.toml -p lingxi-adapters --test r05_t03_goldens        # 3/3
cargo test --manifest-path rust/Cargo.toml -p lingxi-service  --test r05_t03_protocol_adapters # 8/8
cargo test --manifest-path rust/Cargo.toml -p lingxi-service  --test r05_t01_binary_wiring   # 2/2
cargo test --manifest-path rust/Cargo.toml -p lingxi-service  --test r05_t01_model_plane     # 7/7
cargo test --manifest-path rust/Cargo.toml -p lingxi-service  --test r05_t02_credentials     # 16/16
cargo test --manifest-path rust/Cargo.toml -p lingxi-service  --test r05_t02_oauth_flows     # 20/20
cargo test --manifest-path rust/Cargo.toml -p lingxi-service  --test r04_t08_tool_matrix     # 10/10
cargo test --manifest-path rust/Cargo.toml -p lingxi-service  --test cancel_terminal_race    # 13/13
cargo test --manifest-path rust/Cargo.toml -p lingxi-adapters --lib                          # 83/83

# 本人独立对抗验证（8/8 PASS，含 C03/C04/C05/C06×2/C07/C10/C12）
cd artifacts/rust-tauri/R05/REVIEW-T03/scratch && cargo test -- --nocapture

# 树-证据绑定复算
shasum -a 256 artifacts/rust-tauri/R05/T03/working-tree-manifest.txt   # 应等于 465eef35…916943
```

## 七、密钥处理边界

- 本验收全程**离线**：所有 provider 均为 in-test loopback HTTP stub，无真实供应商密钥、账号或流量（NOT_REAL_API）。
- 本人构造的测试仅使用合成标记（`sk-review-synthetic` 等）；golden 中仅含 `sk-golden-*-secret` 合成形态。
- 本报告与证据不含任何真实凭证材料、令牌或私钥；T02 的 OAuth/凭证存储边界由 R05-T02 验收负责，本报告不越界复述。

## 八、验证覆盖声明

- **亲自运行**：§六全部命令（静态门禁 3 项、聚焦/回归套件 9 项、scratch 对抗验证 8 测试、台账 digest 复算）。
- **源码推断**：五族 adapter + dispatch/gateway/provider/tool_render/streaming 全通读；`model_exchange.rs`（786 行）与 `runs.rs` 关键臂（700–1120、2150–2330）精读；C11 跨 provider 引用绑定以读码为主。
- **未验证**：真实供应商在线调用（任务书明确 NOT_REAL_API 边界内）；非 macOS 平台行为；台账 T01/T02 条目（各有独立验收报告）。

---

# 九、fix-r1 修复轮复验（2026-10-03，R01 复审）

**复验结论：PASS —— R05-T03 转无条件通过。**

复验对象：实现者 fix-r1 修复轮（证据 `artifacts/rust-tauri/R05/T03/fix-r1/`）。复验范围限 F-01/F-02 的修复真实性与 N-01/N-02/N-03 的文档化如实性；实现层结论沿用 R01（本轮无产品代码改动，`PROGRESS_LEDGER.json` 亦声明 `product_code_changes: none`，与本人逐文件 hash 复核一致）。

## 9.1 移植测试存在且亲跑通过

- `c04_same_provider_call_id_in_two_sessions_stays_isolated`（`rust/crates/lingxi-service/tests/r05_t03_protocol_adapters.rs:870`）与 `c05_runtime_nonce_rides_the_next_request_and_follows_changes`（`:1004`，辅助 `runtime_nonce` 于 `:461`）存在。
- 本人逐行比对：两测试与本人 scratch 原版语义一致、无弱化——c04 两会话收相同 `toolu_SHARED`、各自续发各自真实文件体、每会话 journal 恰一条完成记录；c05 的 stub 脚本先于 nonce 固定、nonce 运行时生成（pid/nanos/counter）、两腿 nonce 不同、wire 逐字节等于当腿 nonce、显式断言无固定 done 替代。
- 亲跑 `--exact` 点名：**2 passed / 0 failed**（日志 `logs/test-fixr1-named-tests.log`）。

## 9.2 台账条目核对

- **C04**：expected 已改为任务书原文语义（「相同callId跨会话隔离：两个会话/请求返回相同外部ID，交错执行；互不去重、不覆盖、不泄露；内部身份唯一」）；testNames 只挂移植的 c04 测试；`cancel_terminal_race` 痕迹已清除。F-01 关闭。
- **C02**：expected 已清理为「外部与内部调用ID区分」，无「跨会话同名」残留字样。
- **C05**：expected 反映强形式（运行时随机 nonce、nonce 变化复跑跟随、固定 done 失败）；testNames 挂移植的 c05 测试，c02 常量测试保留为角色/ID 配对钉且 observed 如实标注其为编译期常量。F-02 关闭。

## 9.3 亲跑与树绑定

- 亲跑：`r05_t03_protocol_adapters` **10/10**（8→10）、`r05_t03_goldens` **3/3**（日志 `logs/test-fixr1-protocol-adapters.log`、`logs/test-fixr1-goldens.log`）。
- fix-r1 树绑定复算：12 条 T03 台账条目的 `workingTreeDigest` 全部为 `sha256:3a6df35a…dfb281`；`shasum -a 256 artifacts/rust-tauri/R05/T03/working-tree-manifest.txt` 与该值逐字节一致；清单 81 文件逐一与当前工作树重算比对 **81 ok / 0 bad**——当前树即 fix-r1 树。
- fix-r1 证据日志抽查：focused 10/10 含两移植测试、goldens 3/3、workspace 94 组合计 1100 passed / 0 failed、check-contracts（626 条无漂移）/check-boundaries/clippy/fmt 全绿。

## 9.4 N-01/N-02/N-03 文档化如实性

- `R05_INTERFACE_EVOLUTION.md` §19 三条观察项的描述与本人 R01 发现逐点相符（N-01 opaque 位置前移的字节保留/不跨族/canonical 正确三限定均在；N-03 明确 text↔tool_calls 交错严格正确）。
- N-02 已在 `PROGRESS_LEDGER.json` `followup_obligations_for_t03_plus` 登记为 T06 后续义务。
- 瑕疵（不阻断）：§18 第 333 行仍写 protocol_adapters「8 测试」，现为 10——历史段落未同步，建议顺手更正。

## 9.5 复验后逐 C-ID 终态

C01–C12 全部 **PASS**（C04 由「FAIL（台账层）」转 PASS；C05 证据形式达任务书强形式）。**R05-T03 无条件通过。**
