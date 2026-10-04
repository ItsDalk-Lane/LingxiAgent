# R05-T05 独立审查报告（REV-T05 R01）

- 审查对象：R05-T05「网络策略、超时与安全重试」（任务书 §4-T05、附录 B T05 C01–C13、附录 A 之 A09/A10、§9 NOT_APPLICABLE 规则、附录 E 记录模板）
- 审查轮次：R01（首轮独立验收）
- 基线：分支 `codex/rust-tauri-migration`，HEAD `c549ff654508ab951e2cf39cf9d309fc9c6b8656`；T01–T05 全部以未提交工作树交付
- 审查日期：2026-10-03；审查者：未参与 R05 任何实现的独立会话
- 证据目录：`artifacts/rust-tauri/R05/REVIEW-T05/`（本审查者亲跑产生，与实现者证据 `artifacts/rust-tauri/R05/T05/` 物理分离）
- 工作树一致性：`artifacts/rust-tauri/R05/T05/working-tree-manifest.txt` 56 个文件 sha256 全部与当前树匹配（`logs/reviewer-manifest-check.log`；期间本审查者重跑 golden 生成器为幂等，未改变树）

## 总体判定：PASS（离线范围；附 2 个必修项 F-01/F-02、3 个修正建议 F-03/F-04/F-05）

13 个检查点中 12 个 PASS 条目实质成立（证据链真实，本审查者全部亲跑复现）；C12 整项 NOT_RUN 经裁决胜拆腿：无效证书拒绝腿由本审查者探针**补证 PASS**，代理配置面腿 **NOT_APPLICABLE 成立**（附来源证据），私有 CA 腿是**真实能力差距**但台账理由错误须更正（F-01）。最高风险链（C01 三段位超时区分、C05/A10 不盲重发、C07 副作用不重做、C08 四类等待点取消）除复跑实现者证据外，另由 3 个审查者自写探针与 2 个 Node 探针独立亲验。LIVE 供应商验证维持任务书允许的 `BLOCKED_NOT_AUTHORIZED`，故判定限定为离线范围。

## 复核范围声明

- 亲自运行：静态门禁 4 项、聚焦/回归套件 16 组（14 组整套件 + 2 个审批取消点名测试）、golden 生成器幂等重跑、审查者探针 3 个（Rust）+ 2 个（Node）、台账抽查 8/13 条、工作树摘要 56 文件全量复算。
- 源码推断（亲读，未单独动态复现）：`dispatch.rs` 重试分类与 429/Retry-After 解析全文、`runs.rs` 驱动循环（含空闲臂 runs.rs:1198、退避臂 runs.rs:2646、预算否决 runs.rs:2626）、`compat.rs` 与 4 个 TS 模块逐行对照、`oauth.rs` client 构建链、现役 TS 侧 `core/llm-client.ts` / `core/model-operation-client.ts` / `lib/net/outbound-proxy.ts` / `desktop/src/shared/windows-system-ca.cjs`、pi-sdk OAuth 裸 fetch（`node_modules/@earendil-works/pi-ai/dist/auth/oauth/{xai,github-copilot,kimi-coding}.js`）、vendored reqwest 0.13.5 的 connect_timeout/system-proxy 行为。
- 未验证：真实供应商 LIVE 流量（未授权）；Windows/Linux 平台行为（rustls-platform-verifier 的非 macOS 路径）；全 workspace 套件未由本审查者重跑（实现者 `gates-test-workspace-nofailfast.log` 报 1142 passed / 0 failed；本审查者以 16 组聚焦/回归 + 探针覆盖 T05 及其回归面）。

## 亲跑证据（命令均真实执行，退出码亲见）

| 证据 | 命令（摘要） | 结果 |
| --- | --- | --- |
| `logs/reviewer-gates-quick.log` | `cargo fmt --all -- --check`；`xtask check-contracts`；`xtask check-boundaries` | 三项 exit 0（56 生成文件无漂移；边界规则通过） |
| `logs/reviewer-clippy.log` | `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0。**如实注明：全缓存命中**（0.21s Finished，无重新编译）；cargo 指纹保证产物对应当前源码，但非从零编译 |
| `logs/reviewer-focused.log` | 三个 r05_t05 套件 | adapters timeouts 13 passed / compat 1 passed（49 fixture）/ service timeouts 8 passed，全 exit 0 |
| `logs/reviewer-regression-stage.log` | r05_t01_model_plane / r05_t02_credentials / r05_t03_protocol_adapters / r05_t04_streaming / r04_t08_tool_matrix | 7+16+10+9+10 passed，全 exit 0 |
| `logs/reviewer-regression-c08c10.log` | cancellation_tree / cancel_terminal_race / cancel_link_inheritance / run_lifecycle / background_disconnect_recovery / event_subscription | 8+13+7+12+3+12 passed，全 exit 0 |
| `logs/reviewer-c08-approval.log` | r04_t03_approval_service `cancel_during_wait_then_late_approval_never_resurrects`；r03_t08_acceptance_matrix `combo_cancel_during_approval_wait` | 各 1 passed，exit 0 |
| `logs/reviewer-golden-idempotence.log` | 重跑 `node docs/rust-tauri/R05/r05_t05_generate_compat_goldens.mjs`（Node v24.16.0，直跑现役 TS 模块） | exit 0；49 fixture 前后 shasum diff 为空（逐字节幂等） |
| `tls-probe.log` | 生产 `dispatch::build_client()` 打自签证书 TLS 端点 + 纯 HTTP 对照 | 拒绝（`invalid peer certificate: ... not trusted: -67843`）/ 对照 200 OK；`VERDICT=tls_verification_enforced`，PROBE_EXIT=0 |
| `connect-probe.log` | 生产 `build_client_with_timeouts` + `send_with_timeouts` 打 TLS 握手挂起端点 | 402ms 跳断（≪30s 首片窗）、`upstream_unavailable` 可重试、常量 10000/30000 打印核对；`VERDICT=connect_hang_segment_distinct_and_consts_pinned`，PROBE_EXIT=0 |
| `idle-probe.log` | 真实 `ServiceState::bootstrap_with_deps`（生产 60s 空闲界限）+ 发一个 delta 后永久挂起的 double | 181.531s 结算 failed（=3×60.01s 空闲跳断 + 0.5s/1.0s 退避）、attempts=3、`failed.provider_error`；`VERDICT=idle_stream_bound_enforced_at_production_default` |
| `node-fetch-proxy-probe.log` | 设 HTTP(S)_PROXY 后内建 fetch 打直连目标 + 计数代理 | fetch 200 直连、代理 0 命中：现役不受理环境代理 |
| `node-extra-ca-probe.log` | `NODE_EXTRA_CA_CERTS=<私有CA>` 后内建 fetch 打私有 CA 签名端点 | fetch 200 OK：现役**尊重** NODE_EXTRA_CA_CERTS |
| `logs/reviewer-manifest-check.log` | 实现者 working-tree-manifest 56 文件 sha256 全量复算 | 56 matched / 0 mismatched |

## 审查者探针（自写、隔离于 `REVIEW-T05/`，不触碰产品代码）

三个 Rust 探针 crate 均为 path-dep 独立 workspace（复用 `rust/target` 缓存），只调用生产公共入口；两个 Node 探针直跑本机 Node 24 内建 fetch。

- **tls-probe**（C12 无效证书腿）：自签证书本地 TLS 端点 + 同端口形纯 HTTP 对照。生产 client 拒绝自签证书（rustls-platform-verifier 走 macOS Security 信任评估，错误码 -67843），对照证明拒绝原因是证书验证而非连通性。全仓 grep 无 `danger_accept_invalid_certs` 或等价物。
- **connect-probe**（C01 connect 腿）：本机 TUN 模式系统代理（127.0.0.1:7897）使 SYN 黑洞不可制造（`nc -z 192.0.2.1 81` 对 RFC5737 地址 0.00s「成功」——该环境事实本身反向印证了 `no_proxy()` 裁决的必要性），故以「accept 后不应答 TLS 握手」的 loopback 监听制造同段位的 connect 期挂起（vendored reqwest 0.13.5 源码确认 connect_timeout 以 tower TimeoutLayer 包住 TCP+TLS 全段）。结果：402ms 跳断（注入窗 400ms），raw client 报 `is_connect=true, is_timeout=true`，`send_with_timeouts` 归类**可重试** `upstream_unavailable`——符合 A10「可证明未接受方可重试」（TLS 握手完成前不可能有任何 HTTP 字节被处理）。同时打印核对生产常量 connect=10000 / first_byte=30000 与 R05_BASELINE §8 一致。
- **idle-probe**（C01 空闲流腿，**交付内原本无任何执行证据**，见 F-03）：double 发一个 Text delta 后 `pending()` 永久挂起；走真实 bootstrap（生产默认 60s 空闲界限、3 次尝试预算、300s 共享总预算、500ms/1s 退避）。结果：运行恰在 181.531s 结算 failed（三次尝试各 ~60.01s 跳断 + 两次退避），`run_attempts` 恰 3 行，终态 `failed.provider_error`（FailureCause 类目；wire 码不持久化进 key_events，现役测试亦如此断言）。double 完全无视 deadline，若无空闲臂运行将永久挂起——181.5s 的计时签名唯一对应 60s 预登记值。
- **node-fetch-proxy-probe / node-extra-ca-probe**（C12 代理面/私有 CA 腿的现役事实取证）：见上表。

## 13 个检查点逐条核对

| C-ID | 结论 | 运行路径上的支撑（本审查者核实） |
| --- | --- | --- |
| C01 三段位超时+总预算 | PASS（空闲流腿由审查者补证，F-03） | connect 挂起：connect-probe 402ms 跳断可重试 + 常量交叉核对；首片挂起：`first_byte_window_hit_is_terminal_no_blind_resend`（30s 段位、不可重试）复跑绿；空闲流挂起：idle-probe 生产 60s 实证（181.531s/3 attempts）；总预算 300s：发送前耗尽/首片窗内/流中段三点 + 预算否决重试（不盲睡）复跑绿，service 测试断言钉死 300_000/500/8_000/attempts=3。三段产生三种不同超时证据的要求满足 |
| C02 配额/排队/取消 | PASS | `the_backoff_sleep_holds_no_model_call_permit`（退避不占 permit）复跑绿；配额等待取消归还（cancellation_tree r03_a05）、停泊配额等待被子代理超时结算（cancel_link_inheritance）复跑绿 |
| C03 429/Retry-After/5xx | PASS | 9 测试复跑绿：delay-seconds 精确、HTTP-date 换算、垃圾 hint 忽略不捏造、5xx 有界可重试无捏造 hint、RFC 9110 双形态解析单元钉、hint 覆盖计算退避、base×2^(n-1) 封顶表（500/1000/2000/4000/8000/8000）、默认 3 次 |
| C04 重试身份对账 | PASS | `retryable_provider_failure_reopens_attempt_on_the_same_run` 等复跑绿；idle-probe 的持久层交叉印证：run 恰 1 行、run_attempts 3 行、key_events 按 attempt 归属不交叉；一调用=一网络 attempt（dispatch 无内部重试）与现役 llm-client.ts:838 一致 |
| C05/A10 接受后断连不盲重发 | PASS | `accepted_then_dropped_is_terminal_and_never_resent` / `mid_stream_transport_break_is_terminal_and_never_resent`（外部 stub 计数恰 1 次外发）复跑绿；分类亲读 dispatch.rs:155-172：仅 `is_connect()` 可重试，与现役 model-operation-client.ts:448（只 429/5xx 可重试）对齐且连接期语义更精确。connect-probe 修正台账措辞：可重试类是「连接期失败（拒绝+挂起）」而非仅「连接拒绝」（方向安全，F-05） |
| C06 部分文本重试不拼接 | PASS（措辞说明） | 流中段失败已发 delta 持久不倒带、无伪造 final（adapters 两测试 + T04 c15_a08 复跑绿）；重试携带已确认交换、失败调用自身不推 assistant turn（runs.rs:2610-2617 亲读 + C07 测试计数证明）。**如实登记**：「投影 diff」的强形式无单独单测，由不重拼接机制 + 事件持久性组合覆盖 |
| C07 已确认工具副作用不重做 | PASS | `a_retry_after_a_confirmed_tool_effect_never_re_executes_it` 复跑绿：外部计数 double 恰执行 1 次，重试 attempt 的 prior 携带已确认 ToolResult |
| C08 取消覆盖四类等待点 | PASS | 退避：`cancellation_during_backoff_settles_now_not_after_the_delay`（60s 退避中取消 <5s 四阶段结算）复跑绿；配额：cancellation_tree（r03_a05）复跑绿；流：c16_a09（断连+恰一次结算）复跑绿；审批：`cancel_during_wait_then_late_approval_never_resurrects` + `combo_cancel_during_approval_wait`（零工具执行）本审查者点名复跑绿。另终态竞态双腿（cancel_terminal_race）复跑绿 |
| C09 唯一结算 | PASS | cancel_terminal_race 13 复跑绿：claim 先于 commit 无复查间隙、太晚取消不接受、重复取消保首个原因 |
| C10 重连不重发 | PASS | background_disconnect_recovery 3 + event_subscription 12 复跑绿；cursor 续读只回放事件流，订阅面无外发触发路径（源码亲读） |
| C11 URL/资源边界 | 部分成立（F-04） | 已执行腿真实：无重定向（Policy::none）+ 3xx 不跟随 + Location 不回显（净化单测）+ 凭证按族映射只发配置端点（golden absent_headers 钉跨族泄漏）复跑/亲读核实。SSRF/附件 URL 腿实现者 observed 自报「无对应实现、归后续阶段」——现役模型路径确无 SSRF 策略层（grep 证实，仅 model-operation-resolver.ts 的 isLocalBaseUrl 本地端点判定），遗留**合理**，但折叠进 PASS 违反 §9 记录纪律 |
| C12 代理与 TLS 不降级 | 拆腿裁决（见专节） | 无效证书拒绝腿 PASS（tls-probe）；代理配置面 NOT_APPLICABLE 成立（附来源）；私有 CA 腿为真实能力差距、台账理由错误（F-01）；另发现 oauth.rs 遗漏 no_proxy()（F-02） |
| C13 compat golden | PASS | 12 模块全移植；生成器重跑 49 fixture 逐字节幂等；Rust 套件复跑绿；deepseek/zhipu/anthropic/codexResponses 四模块与 TS 源逐行对照一致（含 first-match-wins 顺序、TS throw→不可重试 InvalidMessage 逐字文案）。登记偏差（longcat/input-audio/video-url 不移植、google 族不接 compat 层、maxOutput 回退不可表达、roleplay 触发器无 Rust 源）均附源码依据且合理。「逐字节相等」措辞略过强：测试断言为 serde_json Value 结构相等（key 序不敏感），fixture 文件本身逐字节一致由生成器幂等证明（F-05） |

## C12 专项裁决（拆腿）

实现者将 C12 整项 NOT_RUN，blockingReason「配置面无现役对应物；TLS 验证保持默认开启」。按 §9 逐腿裁定：

1. **无效证书必须被拒（普遍义务）→ PASS**（审查者探针补证）：tls-probe 以生产 `dispatch::build_client()` 实证拒绝自签证书，对照纯 HTTP 正常；全仓无 `danger_accept_invalid_certs`；5 个 chat 适配器统一走 `dispatch::build_client_with_timeouts`。任务书「若确实拒绝，补一个证明测试很便宜且必须」——本审查者已补该证据（探针形式）；建议实现者固化为常驻测试。
2. **系统/手动/NO_PROXY 代理配置面 → NOT_APPLICABLE 成立（附来源证据）**：`core/llm-client.ts`、`core/model-operation-client.ts` 零 proxy 引用；node-fetch-proxy-probe 实测 Node 24 内建 fetch 不受理 HTTP(S)_PROXY（计数代理 0 命中）；`lib/net/outbound-proxy.ts` 的 `setGlobalDispatcher` 为 npm undici 拷贝（文件内注释自述内建 fetch 不读它），代理运行时只影响 MCP/bridge 面。裁决 5 的 `.no_proxy()` 与现役对齐成立。补充环境事实：本机恰运行 TUN 模式系统代理（127.0.0.1:7897，nc 对 RFC5737 文档地址 0.00s「连接成功」），若无 no_proxy()，reqwest 0.13.5 默认 `system-proxy` 特性（vendored Cargo.toml 核实）会把模型流量送进环境代理——裁决 5 的必要性在本机得到现实印证。
3. **私有 CA 显式授权 → 真实能力差距，台账理由须更正（F-01）**：node-extra-ca-probe 实测现役内建 fetch **尊重** `NODE_EXTRA_CA_CERTS`（私有 CA 签名证书 fetch 200 OK），另有 `desktop/src/shared/windows-system-ca.cjs` 的 Windows 系统 CA 合并通道（main.cjs:95 接线）。Rust 侧 rustls-platform-verifier 读平台根（Windows 系统 CA 腿由此覆盖），但**无 NODE_EXTRA_CA_CERTS 等价物**（全平台操作者附加 CA 文件）。故「无现役对应物」对该腿不成立；正确登记应为「现役有 NODE_EXTRA_CA_CERTS 通道，Rust 侧无等价物，归后续网络加固阶段」。

## 台账抽查（8/13 条）

C01/C03/C05/C07/C08/C11/C12/C13：`testNames` 逐条 grep 核实真实存在于对应测试文件且被本审查者复跑；`evidence` 路径存在于 `artifacts/rust-tauri/R05/T05/`；`exitCode=0` 与本审查者复跑一致；工作树摘要 56 文件全量复算匹配。发现的不符均入发现清单（C01 空闲流腿证据强度、C01「预登记值断言钉死 connect/first_byte」无对应断言、C05 措辞、C11 折叠、C12 理由、§28 测试计数 7→8）。未发现伪造证据或 PASS 空心化；问题集中在**措辞精确度与记录纪律**，不在行为真实性。

## 反向测试声明

凡实现者标 PASS 的条目均先疑后验：C11 拆出自报 NOT_RUN 的腿（F-04）；C12 整项 NOT_RUN 拆腿后一腿补证 PASS、一腿 NOT_APPLICABLE、一腿理由更正（F-01）；C01 空闲流腿发现零执行证据后以生产默认值探针补证（F-03）；connect 腿因本机 TUN 代理两次改换探针法（SYN 黑洞不可造 → TLS 握手挂起），未以「差不多」放行。

## 发现清单

### F-01（中，必修：更正登记，不改代码）

C12 私有 CA 腿的 NOT_RUN 理由「配置面无现役对应物」**事实错误**：现役内建 fetch 尊重 NODE_EXTRA_CA_CERTS（`node-extra-ca-probe.log`：fetchStatus 200），并有 Windows 系统 CA 合并通道。Rust 侧对该通道无等价物。要求：更正 C12 blockingReason/§25 裁决 5 的理由表述，并把「NODE_EXTRA_CA_CERTS 等价物」作为真实能力差距显式登记到后续网络加固阶段。证据：`logs/` 外 `node-extra-ca-probe.{mjs,log}`、`desktop/src/shared/windows-system-ca.cjs`。

### F-02（中，必修：一行修复或显式登记偏差）

`rust/crates/lingxi-adapters/src/models/oauth.rs:154` 的 OAuth client 只有 `redirect(Policy::none())`，**缺 `.no_proxy()`**：reqwest 默认 `system-proxy` 会读环境代理，而 token 端点请求携带 client_secret/refresh_token——与裁决 5 自己的理由（「无声暴露凭证材料」）自相矛盾，且与现役不符（pi-sdk OAuth 全部裸 `fetch()`：xai.js:44、github-copilot.js:107/147、kimi-coding.js:51；裸 fetch 不受理环境代理已由 node-fetch-proxy-probe 实证）。这是对现役的**真实行为回退**（设了环境代理的用户：CONNECT 目标主机必泄漏，企业 MITM 下请求体亦暴露）。要求：`OAuthHttp::new()` 的 builder 加 `.no_proxy()`（一行），或显式登记偏差理由。审查者不代改产品代码。

### F-03（轻，建议固化；证据已由审查者补齐）

C01 空闲流腿在交付内**零执行证据**：`DEFAULT_STREAM_IDLE_TIMEOUT_MS=60_000` 与 `with_stream_idle_timeout` 全 tests 目录无命中（仅 runs.rs 自身），T04 台账亦无。本审查者已以 idle-probe 在生产默认值下补齐（181.531s / 3 attempts / failed）。建议：用既有 `with_stream_idle_timeout` 注入缝加一个缩短窗的常驻测试，使该腿不依赖 3 分钟级探针。证据：`idle-probe.log`、`idle-probe/`。

### F-04（轻，台账纪律）

C11 标 PASS 但 observed 自报「NOT_RUN 的腿（登记，不虚标）」——按 §9 应拆腿标注或下调状态，而非折叠进 PASS。遗留本身合理（现役无对应策略层，grep 证实）。

### F-05（轻，措辞/计数修正）

- §28 称 service 套件「7 测试」，实际 8（实现者自己的 `test-r05-t05-timeouts-service.log` 亦为 8）。
- C01 observed「预登记值断言钉死（connect 10s/first_byte 30s…）」：仅 total_budget 300s/backoff 500/8000/attempts=3 有测试断言；connect/first_byte 常量无测试断言（现由 connect-probe 打印核对 + 源码/基线交叉佐证）。
- C05「连接拒绝为唯一可重试传输类」被 connect-probe 修正：连接期挂起（TLS 握手 stall）同样可重试（`is_connect()=true`），符合 A10 原则，方向安全，措辞宜精确化。
- C13「逐字节相等」：测试断言实为 serde_json Value 结构相等（key 序不敏感）；fixture 文件逐字节一致由生成器幂等重跑证明，实质无妨。

## 总体结论

**PASS（离线范围）**。12 条 PASS 的行为真实性经独立复跑与自写探针确认；C12 拆腿处置如上。必修项 F-01（登记更正）与 F-02（oauth.rs 一行 no_proxy 或显式登记）建议在合入前完成；F-03/F-04/F-05 为修正建议，不阻塞。LIVE 供应商验证维持 `BLOCKED_NOT_AUTHORIZED`；Windows/Linux 平台行为未验。

---

# fix-r1 修复轮复验（2026-10-03，同一审查者）

实现者声称 F-01—F-05 全部处理，证据 `artifacts/rust-tauri/R05/T05/fix-r1/`。本审查者逐项独立复验如下。

## fix-r1 复验结论：全部成立 — R05-T05 验收通过（离线范围）

### F-02（oauth.rs no_proxy）— 修复成立，辨别力实证

- `oauth.rs:164` 已补 `.no_proxy()`，上方文档注释准确（ruling 5 同一纪律、凭证材料、对现役裸 fetch 的回退）。
- 新回归测试 `r05_t02_oauth_flows.rs::the_credential_bearing_client_never_consults_the_ambient_proxy` 本审查者点名亲跑：1 passed（套件 20→21，全套件 21 passed exit 0）。测试结构合理：ProxyEnvGuard 清 8 个代理变量（含 NO_PROXY）后只设 HTTP_PROXY 指向计数监听器，在毒化环境下建 client（matcher 在建时捕获——reqwest client.rs `auto_sys_proxy` 源码核实），驱动真实 refresh 流，断言 token 端点恰 1 次直连 + 计数代理 0 命中。
- 辨别力声称（删掉 no_proxy 测试会失败）经两层独立核实：① 源码层——hyper-util 0.1.20 `mac::with_system` 只读 HTTPEnable/HTTPProxy/HTTPPort 与 HTTPS 对，**不导入 ExceptionsList**，且 env 优先（from_env 先跑、mac 只补空位），故毒化的 HTTP_PROXY 必然成为 matcher 条目、loopback 目标无排除项；② 实证层——本审查者自写 `proxy-discrimination-probe`（独立 crate，不触产品代码）：默认 client 在毒化环境下打 loopback 目标 → 计数代理命中 1、目标不可达（err）；`no_proxy()` 对照臂 → 直连 200、代理 0 命中。`VERDICT=test_discrimination_sound`（`proxy-discrimination-probe.log`）。

### F-01（C12 私有 CA 腿理由更正）— 成立

台账 C12 blockingReason 已更正为「私有 CA 腿 = 真实能力差距（现役有 NODE_EXTRA_CA_CERTS 通道，Rust 侧无等价物），归后续网络加固阶段」，observed 按拆腿登记（无效证书拒绝腿 PASS 附 tls-probe 证据 / 代理面 NOT_APPLICABLE 附来源 / 私有 CA 差距显式登记）；§25 裁决 5 相应重写，如实写明「原『无现役对应物』理由事实错误」。

### F-03（空闲流腿常驻测试）— 成立

新测试 `r05_t05_timeouts.rs::an_idle_stream_is_cut_at_the_injected_bound_and_settles_honestly` 经新增 `ServiceDeps.stream_idle_timeout` 缝（lib.rs:564/1117-1118 接线亲读核实）注入 200ms 界限；断言 failed/provider_error/3 attempts/重试间隔 ≥300ms/整体 <10s/每尝试 partial delta 持久。本审查者点名亲跑：1 passed，实测 0.87s（秒级声称成立）；service 套件 8→9 全绿。

### F-04（C11 拆腿）— 成立

新增 R05-T05-C11B 单独 NOT_RUN 行（SSRF 策略腿 + 附件 URL 取数授权腿），blockingReason 附现役无 SSRF 策略层的 grep 依据与 T06 范围；C11 的 expected 收窄为已执行腿。台账合计 13→14 条。

### F-05（措辞/计数）— 成立

§28 改为「9 测试」（与实际一致）；C01 observed 如实区分「有测试断言钉死（300s/500/8000/attempts=3）」与「无直接断言、经 connect-probe 打印 + 基线交叉佐证（connect 10s/first_byte 30s）」；C05 措辞更正为「连接期失败（连接拒绝 + 连接期挂起/TLS 握手 stall）可重试」；C13 更正为「结构相等（serde_json Value，key 序不敏感；拒绝文案逐字）+ fixture 文件逐字节一致由生成器幂等证明」。

### 回归与一致性（本审查者亲跑）

- `logs/reviewer-fix-r1.log`：oauth 21 / adapters timeouts 13 / compat 1 / service timeouts 9，全 exit 0；两个新测试点名复跑各 1 passed（`reviewer-fix-r1-named.log`）。
- `logs/reviewer-fix-r1-gates.log`：修复后树上 fmt / **clippy（全量重编译 3m19s，非缓存）** / check-contracts / check-boundaries 全 exit 0。
- 台账 14 条 digest 一致性：全部共享 `workingTreeDigest sha256:fce44af5…`，startedAt/finishedAt 窗口与 `fix-r1/evidence-window.txt`（03:48:20Z→04:05:35Z）一致，sourceSha=HEAD `c549ff654`。

### fix-r1 后总结论

**R05-T05 验收通过（离线范围）**。R01 的 2 个必修项均已修复并带辨别力经实证的回归钉；3 个建议项全部落实。遗留不变：LIVE 供应商验证 `BLOCKED_NOT_AUTHORIZED`；私有 CA 的 NODE_EXTRA_CA_CERTS 等价物归后续网络加固阶段（已正确登记）；Windows/Linux 平台行为未验。
